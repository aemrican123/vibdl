//! Thumbnail + sample-preview fetching. The UI loads `thumb` URLs; this module
//! only fetches URLs that came from an API response (registered by key),
//! never arbitrary URLs from the page.
//!
//! - `<provider>-<md5>`    thumbnail, cached on disk (size-capped)
//! - `<provider>-<md5>-s`  sample preview, memory only (webview caches it)

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use std::time::Duration;

use reqwest::{StatusCode, header};
use tokio::sync::Semaphore;
use tokio::time::{Instant, sleep};

use crate::error::{AppError, AppResult};
use crate::net::RateLimiter;

const MAX_THUMB_BYTES: usize = 5 * 1024 * 1024;
const MAX_SAMPLE_BYTES: usize = 25 * 1024 * 1024;
const SAMPLE_SUFFIX: &str = "-s";
const MAX_REGISTERED: usize = 50_000;
const SWEEP_EVERY_WRITES: u32 = 50;
/// The image CDN rate-limits (HTTP 429) bursts, so stay under it:
/// at most 3 in flight and one new request every 200 ms (≤ 5/s). A 429
/// still triggers a shared cooldown.
const PARALLEL_FETCHES: usize = 3;
const MIN_INTERVAL: Duration = Duration::from_millis(200);
const DEFAULT_COOLDOWN: Duration = Duration::from_secs(10);
const MAX_COOLDOWN: Duration = Duration::from_secs(60);
const MAX_ATTEMPTS: u32 = 3;

pub struct Thumbs {
    dir: PathBuf,
    cap_bytes: u64,
    http: reqwest::Client,
    registry: Mutex<HashMap<String, String>>,
    permits: Semaphore,
    sample_permits: Semaphore,
    limiter: RateLimiter,
    /// When the CDN says "slow down", every fetch waits until this instant.
    cooldown_until: Mutex<Option<Instant>>,
    /// Keys whose URL returned a web page instead of an image this session.
    /// Not retried (the page is the same every time).
    missing: Mutex<HashSet<String>>,
    writes: AtomicU32,
}

/// Key format: `<provider>-<32 lowercase hex md5>`. Safe as a filename and as
/// a URL path segment.
pub fn make_key(provider: &str, md5: &str) -> Option<String> {
    let md5 = md5.to_ascii_lowercase();
    let key = format!("{provider}-{md5}");
    is_valid_key(&key).then_some(key)
}

pub fn sample_key(provider: &str, md5: &str) -> Option<String> {
    make_key(provider, md5).map(|k| k + SAMPLE_SUFFIX)
}

pub fn is_valid_key(key: &str) -> bool {
    let key = key.strip_suffix(SAMPLE_SUFFIX).unwrap_or(key);
    let Some((provider, md5)) = key.split_once('-') else {
        return false;
    };
    !provider.is_empty()
        && provider.chars().all(|c| c.is_ascii_lowercase())
        && md5.len() == 32
        && md5.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

pub fn sniff_mime(bytes: &[u8]) -> &'static str {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'G', b'I', b'F', b'8', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        [_, _, _, _, b'f', b't', b'y', b'p', b'a', b'v', b'i', b'f', ..] => "image/avif",
        _ => "application/octet-stream",
    }
}

impl Thumbs {
    pub fn new(dir: PathBuf, cap_mb: u32, http: reqwest::Client) -> AppResult<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            cap_bytes: cap_mb as u64 * 1024 * 1024,
            http,
            registry: Mutex::new(HashMap::new()),
            permits: Semaphore::new(PARALLEL_FETCHES),
            sample_permits: Semaphore::new(1),
            limiter: RateLimiter::new(MIN_INTERVAL),
            cooldown_until: Mutex::new(None),
            missing: Mutex::new(HashSet::new()),
            writes: AtomicU32::new(0),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn cap_bytes(&self) -> u64 {
        self.cap_bytes
    }

    /// Remember which thumbnail URL a key maps to. Returns the key.
    pub fn register(&self, provider: &str, md5: &str, url: &str) -> Option<String> {
        self.insert(make_key(provider, md5)?, url)
    }

    /// Remember a sample-preview URL. Returns the key.
    pub fn register_sample(&self, provider: &str, md5: &str, url: &str) -> Option<String> {
        self.insert(sample_key(provider, md5)?, url)
    }

    fn insert(&self, key: String, url: &str) -> Option<String> {
        if !url.starts_with("https://") {
            return None;
        }
        let mut reg = self.registry.lock().expect("thumb registry poisoned");
        if reg.len() >= MAX_REGISTERED {
            reg.clear();
        }
        reg.insert(key.clone(), url.to_string());
        Some(key)
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.thumb"))
    }

    fn lookup(&self, key: &str) -> Option<String> {
        self.registry.lock().expect("thumb registry poisoned").get(key).cloned()
    }

    fn start_cooldown(&self, wait: Duration) {
        let until = Instant::now() + wait.min(MAX_COOLDOWN);
        let mut c = self.cooldown_until.lock().expect("cooldown poisoned");
        if c.is_none_or(|cur| cur < until) {
            *c = Some(until);
        }
    }

    async fn wait_cooldown(&self) {
        loop {
            let until = *self.cooldown_until.lock().expect("cooldown poisoned");
            match until {
                Some(u) if u > Instant::now() => sleep(u - Instant::now()).await,
                _ => return,
            }
        }
    }

    /// Polite fetch: spaced requests, shared cooldown on 429/503, a few
    /// retries. Never retries around 401/403. `Ok(None)` = the server sent a
    /// non-image (e.g. an HTML page) — deterministic, so not retried.
    async fn fetch_image(&self, url: &str, max_bytes: usize) -> AppResult<Option<(Vec<u8>, &'static str)>> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            self.wait_cooldown().await;
            self.limiter.acquire().await;
            let resp = self.http.get(url).send().await?;
            let status = resp.status();

            if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
                let wait = retry_after(&resp).unwrap_or(DEFAULT_COOLDOWN * attempt);
                self.start_cooldown(wait);
                tracing::info!(status = status.as_u16(), wait_s = wait.as_secs(), attempt, "image CDN asked us to slow down");
                if attempt < MAX_ATTEMPTS {
                    continue;
                }
                return Err(AppError::RateLimited);
            }
            if status.is_redirection() {
                let location = resp
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("-")
                    .to_string();
                tracing::warn!(
                    status = status.as_u16(),
                    location = %crate::net::redact(&location),
                    url = %crate::net::redact(url),
                    "image server redirected instead of serving the image; marking as no preview"
                );
                return Ok(None);
            }
            if !status.is_success() {
                return Err(AppError::Http(status.as_u16()));
            }

            let content_type = resp
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-")
                .to_string();
            let bytes = resp.bytes().await?;
            if bytes.len() > max_bytes {
                return Err(AppError::Parse("image too large for preview".into()));
            }
            let mime = sniff_mime(&bytes);
            if mime.starts_with("image/") {
                return Ok(Some((bytes.to_vec(), mime)));
            }

            tracing::warn!(
                %content_type,
                len = bytes.len(),
                title = %html_title(&bytes).unwrap_or_else(|| describe_prefix(&bytes)),
                url = %crate::net::redact(url),
                "image server returned a web page instead of an image; marking as no preview"
            );
            return Ok(None);
        }
    }

    fn mark_missing(&self, key: &str) {
        self.missing.lock().expect("missing set poisoned").insert(key.to_string());
    }

    fn is_missing(&self, key: &str) -> bool {
        self.missing.lock().expect("missing set poisoned").contains(key)
    }

    /// Thumbnails: cached bytes, or fetch + cache. Samples: fetch, no disk.
    /// `Ok(None)` = unknown/invalid key.
    pub async fn get(&self, key: &str) -> AppResult<Option<(Vec<u8>, &'static str)>> {
        if !is_valid_key(key) || self.is_missing(key) {
            return Ok(None);
        }
        if key.ends_with(SAMPLE_SUFFIX) {
            let Some(url) = self.lookup(key) else {
                return Ok(None);
            };
            let _permit = self.sample_permits.acquire().await.expect("semaphore closed");
            let got = self.fetch_image(&url, MAX_SAMPLE_BYTES).await?;
            if got.is_none() {
                self.mark_missing(key);
            }
            return Ok(got);
        }

        let path = self.path_for(key);
        if let Ok(bytes) = tokio::fs::read(&path).await {
            let mime = sniff_mime(&bytes);
            return Ok(Some((bytes, mime)));
        }

        let Some(url) = self.lookup(key) else {
            return Ok(None);
        };

        let _permit = self.permits.acquire().await.expect("semaphore closed");
        let Some((bytes, mime)) = self.fetch_image(&url, MAX_THUMB_BYTES).await? else {
            self.mark_missing(key);
            return Ok(None);
        };

        let tmp = path.with_extension("part");
        tokio::fs::write(&tmp, &bytes).await?;
        tokio::fs::rename(&tmp, &path).await?;

        if self.writes.fetch_add(1, Ordering::Relaxed) % SWEEP_EVERY_WRITES == SWEEP_EVERY_WRITES - 1 {
            let (dir, cap) = (self.dir.clone(), self.cap_bytes);
            tokio::task::spawn_blocking(move || sweep(&dir, cap));
        }
        Ok(Some((bytes, mime)))
    }
}

fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    resp.headers()
        .get(header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// `<title>` of an HTML page, for diagnostics.
pub fn html_title(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(64 * 1024)]);
    let lower = text.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = start + lower[start..].find('>')? + 1;
    let close = open_end + lower[open_end..].find("</title>")?;
    let title: String = text[open_end..close].split_whitespace().collect::<Vec<_>>().join(" ");
    Some(title.chars().take(120).collect())
}

/// First bytes of a response as printable text, for diagnostics.
pub fn describe_prefix(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(48)
        .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' })
        .collect()
}

/// Delete oldest cached thumbnails until the folder fits under `cap_bytes`.
/// Returns the number of files removed.
pub fn sweep(dir: &Path, cap_bytes: u64) -> u64 {
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((meta.modified().ok()?, meta.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= cap_bytes {
        return 0;
    }
    files.sort_by_key(|f| f.0); // oldest first
    let mut removed = 0;
    for (_, len, path) in files {
        if total <= cap_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
            removed += 1;
        }
    }
    tracing::debug!(removed, "thumbnail cache swept");
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_strict() {
        assert_eq!(
            make_key("gelbooru", "0123456789ABCDEF0123456789abcdef").as_deref(),
            Some("gelbooru-0123456789abcdef0123456789abcdef")
        );
        assert!(make_key("gelbooru", "short").is_none());
        assert!(!is_valid_key("../etc-0123456789abcdef0123456789abcdef"));
        assert!(!is_valid_key("gelbooru-0123456789abcdef0123456789abcdeg"));
        assert!(!is_valid_key("gelbooru"));
        assert_eq!(
            sample_key("gelbooru", "0123456789abcdef0123456789abcdef").as_deref(),
            Some("gelbooru-0123456789abcdef0123456789abcdef-s")
        );
        assert!(is_valid_key("gelbooru-0123456789abcdef0123456789abcdef-s"));
        assert!(!is_valid_key("gelbooru-0123456789abcdef0123456789abcdef-x"));
    }

    #[test]
    fn sniffs_images() {
        assert_eq!(sniff_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), "image/jpeg");
        assert_eq!(sniff_mime(b"\x89PNG\r\n"), "image/png");
        assert_eq!(sniff_mime(b"RIFF1234WEBPVP8"), "image/webp");
        assert_eq!(sniff_mime(b"\x00\x00\x00\x1cftypavif"), "image/avif");
        assert_eq!(sniff_mime(b"<html>"), "application/octet-stream");
        assert_eq!(describe_prefix(b"<html>\n<x"), "<html>.<x");
        assert_eq!(
            html_title(b"<!DOCTYPE html><html><head><meta x><TITLE lang=en>\n  Page   Not Found </title>").as_deref(),
            Some("Page Not Found")
        );
        assert_eq!(html_title(b"no title here"), None);
    }

    #[test]
    fn register_rejects_non_https() {
        let dir = tempfile::tempdir().unwrap();
        let t = Thumbs::new(dir.path().into(), 50, reqwest::Client::new()).unwrap();
        let md5 = "0123456789abcdef0123456789abcdef";
        assert!(t.register("gelbooru", md5, "http://x/a.jpg").is_none());
        assert!(t.register("gelbooru", md5, "https://x/a.jpg").is_some());
    }

    #[tokio::test]
    async fn unknown_key_is_none_and_cached_file_is_served() {
        let dir = tempfile::tempdir().unwrap();
        let t = Thumbs::new(dir.path().into(), 50, reqwest::Client::new()).unwrap();
        let key = "gelbooru-0123456789abcdef0123456789abcdef";
        assert!(t.get(key).await.unwrap().is_none());
        std::fs::write(dir.path().join(format!("{key}.thumb")), [0xFF, 0xD8, 0xFF, 0x00]).unwrap();
        let (bytes, mime) = t.get(key).await.unwrap().unwrap();
        assert_eq!(mime, "image/jpeg");
        assert_eq!(bytes.len(), 4);
        assert!(t.get("not-a-key").await.unwrap().is_none());
    }

    #[test]
    fn sweep_removes_oldest_until_under_cap() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            std::fs::write(dir.path().join(format!("f{i}.thumb")), vec![0u8; 1000]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        let removed = sweep(dir.path(), 2500);
        assert_eq!(removed, 3);
        assert!(!dir.path().join("f0.thumb").exists());
        assert!(dir.path().join("f4.thumb").exists());
        assert_eq!(sweep(dir.path(), 2500), 0);
    }
}
