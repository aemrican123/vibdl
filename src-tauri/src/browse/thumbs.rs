//! Thumbnail fetching + on-disk cache. The UI loads `thumb` URLs; this module
//! only fetches preview URLs that came from an API response (registered by
//! key), never arbitrary URLs from the page.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use tokio::sync::Semaphore;

use crate::error::{AppError, AppResult};

const MAX_THUMB_BYTES: usize = 5 * 1024 * 1024;
const MAX_REGISTERED: usize = 50_000;
const SWEEP_EVERY_WRITES: u32 = 50;
const PARALLEL_FETCHES: usize = 4;

pub struct Thumbs {
    dir: PathBuf,
    cap_bytes: u64,
    http: reqwest::Client,
    registry: Mutex<HashMap<String, String>>,
    permits: Semaphore,
    writes: AtomicU32,
}

/// Key format: `<provider>-<32 lowercase hex md5>`. Safe as a filename and as
/// a URL path segment.
pub fn make_key(provider: &str, md5: &str) -> Option<String> {
    let md5 = md5.to_ascii_lowercase();
    let key = format!("{provider}-{md5}");
    is_valid_key(&key).then_some(key)
}

pub fn is_valid_key(key: &str) -> bool {
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
            writes: AtomicU32::new(0),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn cap_bytes(&self) -> u64 {
        self.cap_bytes
    }

    /// Remember which preview URL a key maps to. Returns the key.
    pub fn register(&self, provider: &str, md5: &str, url: &str) -> Option<String> {
        if !url.starts_with("https://") {
            return None;
        }
        let key = make_key(provider, md5)?;
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

    /// Cached bytes, or fetch + cache. `Ok(None)` = unknown/invalid key.
    pub async fn get(&self, key: &str) -> AppResult<Option<(Vec<u8>, &'static str)>> {
        if !is_valid_key(key) {
            return Ok(None);
        }
        let path = self.path_for(key);
        if let Ok(bytes) = tokio::fs::read(&path).await {
            let mime = sniff_mime(&bytes);
            return Ok(Some((bytes, mime)));
        }

        let Some(url) = self
            .registry
            .lock()
            .expect("thumb registry poisoned")
            .get(key)
            .cloned()
        else {
            return Ok(None);
        };

        let _permit = self.permits.acquire().await.expect("semaphore closed");
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(AppError::Http(status.as_u16()));
        }
        let bytes = resp.bytes().await?;
        if bytes.len() > MAX_THUMB_BYTES {
            return Err(AppError::Parse("thumbnail too large".into()));
        }
        let mime = sniff_mime(&bytes);
        if !mime.starts_with("image/") {
            return Err(AppError::Parse("thumbnail is not an image".into()));
        }

        let tmp = path.with_extension("part");
        tokio::fs::write(&tmp, &bytes).await?;
        tokio::fs::rename(&tmp, &path).await?;

        if self.writes.fetch_add(1, Ordering::Relaxed) % SWEEP_EVERY_WRITES == SWEEP_EVERY_WRITES - 1 {
            let (dir, cap) = (self.dir.clone(), self.cap_bytes);
            tokio::task::spawn_blocking(move || sweep(&dir, cap));
        }
        Ok(Some((bytes.to_vec(), mime)))
    }
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
    }

    #[test]
    fn sniffs_images() {
        assert_eq!(sniff_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), "image/jpeg");
        assert_eq!(sniff_mime(b"\x89PNG\r\n"), "image/png");
        assert_eq!(sniff_mime(b"RIFF1234WEBPVP8"), "image/webp");
        assert_eq!(sniff_mime(b"<html>"), "application/octet-stream");
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
