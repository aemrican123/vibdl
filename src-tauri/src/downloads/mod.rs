//! Downloading originals into the local library.
//!
//! Flow: duplicate check → stream to `.vibdl/tmp/*.part` while hashing →
//! verify MD5 → atomic rename into place → sidecar → DB row. A crash or
//! dropped connection leaves at most a `.part` file, never a broken original.

use std::path::{Path, PathBuf};
use std::time::Duration;

use md5::{Digest, Md5};
use reqwest::header;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

use crate::db::{Db, LocalEntry};
use crate::error::{AppError, AppResult};
use crate::metadata::CategorizedTags;
use crate::net::RateLimiter;
use crate::providers::RemotePost;
use crate::storage::{self, Sidecar};

pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const PROGRESS_EVERY: u64 = 256 * 1024;

/// An opened library folder: files on disk + its SQLite index.
pub struct Library {
    pub root: PathBuf,
    pub db: Db,
}

impl Library {
    pub fn open(root: &Path) -> AppResult<Self> {
        std::fs::create_dir_all(root)?;
        let db = Db::open(&root.join(".vibdl").join("library.sqlite"))?;
        Ok(Self { root: root.to_path_buf(), db })
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join(".vibdl").join("tmp")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Outcome {
    Downloaded { entry: LocalEntry, bytes: u64 },
    Duplicate { entry: LocalEntry },
}

pub struct Downloader {
    http: reqwest::Client,
    limiter: RateLimiter,
    /// One download at a time (concurrency setting arrives with the queue).
    permit: Semaphore,
    allow_http: bool,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn rel_string(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

impl Downloader {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            limiter: RateLimiter::new(Duration::from_secs(1)),
            permit: Semaphore::new(1),
            allow_http: false,
        }
    }

    #[cfg(test)]
    fn for_tests(http: reqwest::Client) -> Self {
        Self { allow_http: true, limiter: RateLimiter::new(Duration::ZERO), ..Self::new(http) }
    }

    pub async fn download(
        &self,
        lib: &Library,
        post: &RemotePost,
        tags: CategorizedTags,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> AppResult<Outcome> {
        if let Some(entry) = lib.db.find(post.provider, &post.remote_id, post.md5.as_deref())? {
            return Ok(Outcome::Duplicate { entry });
        }
        let url = post
            .file_url
            .as_deref()
            .ok_or_else(|| AppError::Rejected("this post's original file isn't available (restricted post)".into()))?;
        if !(url.starts_with("https://") || (self.allow_http && url.starts_with("http://"))) {
            return Err(AppError::Rejected("refusing non-HTTPS download".into()));
        }
        let ext = post
            .file_ext
            .clone()
            .ok_or_else(|| AppError::Parse("unknown file type".into()))?;

        let _permit = self.permit.acquire().await.expect("semaphore closed");
        self.limiter.acquire().await;

        let resp = self.http.get(url).send().await?;
        let status = resp.status();
        if status.is_redirection() {
            return Err(AppError::Rejected(
                "the image server refused the download (redirect). This board blocks app access to files.".into(),
            ));
        }
        if !status.is_success() {
            return Err(AppError::Http(status.as_u16()));
        }
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if content_type.starts_with("text/html") {
            return Err(AppError::Rejected("the server returned a web page instead of the file".into()));
        }
        let total = resp.content_length();
        if total.is_some_and(|t| t > MAX_FILE_BYTES) {
            return Err(AppError::Rejected("file is larger than 2 GB".into()));
        }

        std::fs::create_dir_all(lib.tmp_dir())?;
        let part = lib.tmp_dir().join(format!("{}-{}.part", post.provider, post.remote_id));
        let result = self.stream_to(resp, &part, total, &mut progress).await;
        let (bytes, md5) = match result {
            Ok(v) => v,
            Err(e) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(e);
            }
        };

        if let Some(expected) = post.md5.as_deref() {
            if expected != md5 {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(AppError::Parse(format!(
                    "downloaded file failed the MD5 check (expected {expected}, got {md5})"
                )));
            }
        } else if let Some(entry) = lib.db.find(post.provider, &post.remote_id, Some(&md5))? {
            let _ = tokio::fs::remove_file(&part).await;
            return Ok(Outcome::Duplicate { entry });
        }

        let rel = storage::relative_media_path(post.provider, &md5, &ext)?;
        let abs = lib.root.join(&rel);
        if let Some(dir) = abs.parent() {
            std::fs::create_dir_all(dir)?;
        }
        tokio::fs::rename(&part, &abs).await?;

        let sidecar = Sidecar {
            vibdl_sidecar: storage::SIDECAR_VERSION,
            provider: post.provider.into(),
            remote_id: post.remote_id.clone(),
            post_url: post.post_url.clone(),
            md5: md5.clone(),
            rating: post.rating.clone(),
            width: post.width,
            height: post.height,
            file_ext: ext.to_ascii_lowercase(),
            file_size: bytes,
            source: post.source.clone(),
            file_url: post.file_url.clone(),
            created_at: post.created_at_raw.clone(),
            downloaded_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            tags,
            raw: post.raw.clone(),
        };
        storage::write_sidecar(&abs, &sidecar)?;
        let local_path = rel_string(&rel);
        lib.db.upsert(&sidecar, &local_path)?;
        tracing::info!(provider = post.provider, id = %post.remote_id, bytes, "downloaded");

        Ok(Outcome::Downloaded {
            entry: LocalEntry {
                provider: sidecar.provider,
                remote_id: sidecar.remote_id,
                md5,
                local_path,
                downloaded_at: sidecar.downloaded_at,
            },
            bytes,
        })
    }

    async fn stream_to(
        &self,
        mut resp: reqwest::Response,
        part: &Path,
        total: Option<u64>,
        progress: &mut impl FnMut(u64, Option<u64>),
    ) -> AppResult<(u64, String)> {
        let mut file = tokio::fs::File::create(part).await?;
        let mut hasher = Md5::new();
        let mut written: u64 = 0;
        let mut last_report: u64 = 0;
        progress(0, total);
        while let Some(chunk) = resp.chunk().await? {
            written += chunk.len() as u64;
            if written > MAX_FILE_BYTES {
                return Err(AppError::Rejected("file is larger than 2 GB".into()));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            if written - last_report >= PROGRESS_EVERY {
                last_report = written;
                progress(written, total);
            }
        }
        if total.is_some_and(|t| t != written) {
            return Err(AppError::Network("connection ended before the file was complete".into()));
        }
        file.flush().await?;
        file.sync_all().await?;
        progress(written, total);
        Ok((written, hex(&hasher.finalize())))
    }
}

#[derive(Debug, Default, Serialize)]
pub struct RebuildReport {
    pub sidecars: usize,
    pub imported: usize,
    pub missing_media: usize,
    pub errors: Vec<String>,
}

/// Re-create the index from sidecar files (e.g. after deleting the DB).
pub fn rebuild(lib: &Library) -> RebuildReport {
    let mut report = RebuildReport::default();
    for path in storage::find_sidecars(&lib.root) {
        report.sidecars += 1;
        let result = (|| -> AppResult<bool> {
            let s = storage::read_sidecar(&path)?;
            let rel = storage::relative_media_path(&s.provider, &s.md5, &s.file_ext)?;
            if !lib.root.join(&rel).is_file() {
                return Ok(false);
            }
            lib.db.upsert(&s, &rel_string(&rel))?;
            Ok(true)
        })();
        match result {
            Ok(true) => report.imported += 1,
            Ok(false) => report.missing_media += 1,
            Err(e) => {
                if report.errors.len() < 20 {
                    report.errors.push(e.to_string());
                }
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const BODY: &[u8] = b"\x89PNG\r\n\x1a\nfake image bytes for testing";

    fn md5_of(b: &[u8]) -> String {
        hex(&Md5::digest(b))
    }

    /// Minimal HTTP server: `/ok`, `/short` (closes early), `/redirect`, `/html`.
    async fn server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let resp: Vec<u8> = match path.as_str() {
                        "/ok" => [
                            format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", BODY.len()).into_bytes(),
                            BODY.to_vec(),
                        ].concat(),
                        "/short" => [
                            format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", BODY.len() + 100).into_bytes(),
                            BODY.to_vec(),
                        ].concat(),
                        "/redirect" => b"HTTP/1.1 302 Found\r\nLocation: https://example/hotlink\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                        _ => b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 6\r\nConnection: close\r\n\r\n<html>".to_vec(),
                    };
                    let _ = sock.write_all(&resp).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn post(url: &str, md5: Option<String>) -> RemotePost {
        RemotePost {
            provider: "danbooru",
            remote_id: "42".into(),
            post_url: "https://danbooru.donmai.us/posts/42".into(),
            md5,
            rating: Some("general".into()),
            width: Some(10),
            height: Some(10),
            file_url: Some(url.into()),
            file_ext: Some("png".into()),
            file_size: None,
            sample_url: None,
            preview_url: None,
            preview_width: None,
            preview_height: None,
            source: None,
            created_at_raw: None,
            tags: vec!["long_hair".into()],
            categorized: None,
            raw: serde_json::json!({"id": 42}),
        }
    }

    fn client() -> reqwest::Client {
        crate::net::build_http_no_redirect().unwrap()
    }

    fn tags() -> CategorizedTags {
        CategorizedTags { general: vec!["long_hair".into()], ..Default::default() }
    }

    fn leftover_parts(lib: &Library) -> usize {
        std::fs::read_dir(lib.tmp_dir()).map(|d| d.count()).unwrap_or(0)
    }

    #[tokio::test]
    async fn downloads_verifies_writes_sidecar_and_dedupes() {
        let base = server().await;
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let dl = Downloader::for_tests(client());
        let p = post(&format!("{base}/ok"), Some(md5_of(BODY)));

        let mut reports = 0;
        let out = dl.download(&lib, &p, tags(), |_, _| reports += 1).await.unwrap();
        let Outcome::Downloaded { entry, bytes } = out else { panic!("expected download") };
        assert_eq!(bytes, BODY.len() as u64);
        assert!(reports >= 2);

        let media = lib.root.join(&entry.local_path);
        assert_eq!(std::fs::read(&media).unwrap(), BODY, "bytes are untouched");
        let sc = storage::read_sidecar(&storage::sidecar_path_for(&media)).unwrap();
        assert_eq!(sc.md5, md5_of(BODY));
        assert_eq!(sc.file_size, BODY.len() as u64);
        assert_eq!(lib.db.post_count().unwrap(), 1);
        assert_eq!(leftover_parts(&lib), 0);

        // Second time: duplicate, no network needed.
        let mut p2 = p.clone();
        p2.file_url = Some("http://127.0.0.1:9/unreachable".into());
        assert!(matches!(dl.download(&lib, &p2, tags(), |_, _| {}).await.unwrap(), Outcome::Duplicate { .. }));

        // Rebuild from sidecars after losing the DB.
        drop(lib);
        std::fs::remove_dir_all(dir.path().join(".vibdl")).unwrap();
        let lib = Library::open(dir.path()).unwrap();
        assert_eq!(lib.db.post_count().unwrap(), 0);
        let r = rebuild(&lib);
        assert_eq!((r.sidecars, r.imported, r.missing_media), (1, 1, 0));
        assert_eq!(lib.db.post_count().unwrap(), 1);
    }

    #[tokio::test]
    async fn md5_mismatch_leaves_nothing_behind() {
        let base = server().await;
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let dl = Downloader::for_tests(client());
        let p = post(&format!("{base}/ok"), Some("ffffffffffffffffffffffffffffffff".into()));
        let err = dl.download(&lib, &p, tags(), |_, _| {}).await.unwrap_err();
        assert!(err.to_string().contains("MD5"));
        assert_eq!(leftover_parts(&lib), 0);
        assert_eq!(lib.db.post_count().unwrap(), 0);
        assert!(storage::find_sidecars(&lib.root).is_empty());
    }

    #[tokio::test]
    async fn interrupted_download_leaves_no_final_file() {
        let base = server().await;
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let dl = Downloader::for_tests(client());
        let p = post(&format!("{base}/short"), Some(md5_of(BODY)));
        assert!(dl.download(&lib, &p, tags(), |_, _| {}).await.is_err());
        assert_eq!(leftover_parts(&lib), 0);
        assert!(!lib.root.join("danbooru").exists());
    }

    #[tokio::test]
    async fn refuses_redirects_html_and_plain_http() {
        let base = server().await;
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let dl = Downloader::for_tests(client());
        for path in ["/redirect", "/html"] {
            let e = dl.download(&lib, &post(&format!("{base}{path}"), None), tags(), |_, _| {}).await.unwrap_err();
            assert!(matches!(e, AppError::Rejected(_)), "{path}: {e}");
        }
        let strict = Downloader::new(client());
        let e = strict.download(&lib, &post(&format!("{base}/ok"), None), tags(), |_, _| {}).await.unwrap_err();
        assert!(e.to_string().contains("non-HTTPS"));
    }

    #[test]
    fn rebuild_counts_missing_media() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let s = crate::db::tests::sidecar("danbooru", "1", "0123456789abcdef0123456789abcdef");
        let media = lib.root.join(storage::relative_media_path("danbooru", &s.md5, "png").unwrap());
        storage::write_sidecar(&media, &s).unwrap();
        let r = rebuild(&lib);
        assert_eq!((r.sidecars, r.imported, r.missing_media), (1, 0, 1));
    }
}
