//! Video playback through the `media` URI scheme, with HTTP Range support so
//! the player can stream and seek without loading whole files into memory.
//!
//! - remote: proxies a range of a registered board video URL (same polite
//!   client as thumbnails: our User-Agent, no redirects)
//! - local: reads a range of a library file

use std::io::SeekFrom;
use std::path::Path;

use reqwest::{StatusCode, header};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::error::{AppError, AppResult};

/// Max bytes returned per request; the player asks for the next range.
pub const CHUNK: u64 = 4 * 1024 * 1024;
/// If a server ignores Range, don't buffer more than this.
const MAX_UNRANGED: u64 = 64 * 1024 * 1024;

pub const VIDEO_EXTS: [&str; 2] = ["mp4", "webm"];

pub fn is_video(ext: &str) -> bool {
    VIDEO_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn mime(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "webm" => "video/webm",
        _ => "video/mp4",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: u64,
    pub end: Option<u64>,
}

/// Parse `bytes=START-` / `bytes=START-END`. Suffix ranges (`bytes=-N`) and
/// multi-ranges aren't used by video players; treat them as "from 0".
pub fn parse_range(header: Option<&str>) -> Range {
    let spec = header.and_then(|h| h.trim().strip_prefix("bytes=")).unwrap_or("0-");
    let first = spec.split(',').next().unwrap_or("0-");
    let (a, b) = first.split_once('-').unwrap_or(("0", ""));
    match a.trim().parse::<u64>() {
        Ok(start) => Range { start, end: b.trim().parse::<u64>().ok().filter(|e| *e >= start) },
        Err(_) => Range { start: 0, end: None },
    }
}

/// Clamp a requested range to at most `CHUNK` bytes and the file size.
pub fn clamp(range: Range, total: u64) -> Option<(u64, u64)> {
    if total == 0 || range.start >= total {
        return None;
    }
    let max_end = range.start + CHUNK - 1;
    let end = range.end.unwrap_or(max_end).min(max_end).min(total - 1);
    Some((range.start, end))
}

#[derive(Debug)]
pub struct MediaResponse {
    pub status: u16,
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub content_range: Option<String>,
}

pub async fn local(path: &Path, ext: &str, range: Range) -> AppResult<MediaResponse> {
    let mut file = tokio::fs::File::open(path).await?;
    let total = file.metadata().await?.len();
    let Some((start, end)) = clamp(range, total) else {
        return Ok(MediaResponse {
            status: 416,
            bytes: vec![],
            content_type: mime(ext).into(),
            content_range: Some(format!("bytes */{total}")),
        });
    };
    file.seek(SeekFrom::Start(start)).await?;
    let mut buf = vec![0u8; (end - start + 1) as usize];
    file.read_exact(&mut buf).await?;
    Ok(MediaResponse {
        status: 206,
        bytes: buf,
        content_type: mime(ext).into(),
        content_range: Some(format!("bytes {start}-{end}/{total}")),
    })
}

pub async fn remote(http: &reqwest::Client, url: &str, ext: &str, range: Range) -> AppResult<MediaResponse> {
    let end = range.end.unwrap_or(range.start + CHUNK - 1).min(range.start + CHUNK - 1);
    let resp = http
        .get(url)
        .header(header::RANGE, format!("bytes={}-{end}", range.start))
        .send()
        .await?;
    let status = resp.status();
    if status.is_redirection() {
        return Err(AppError::Rejected("the video server refused the request (redirect)".into()));
    }
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|ct| ct.starts_with("video/"))
        .unwrap_or(mime(ext))
        .to_string();
    match status {
        StatusCode::PARTIAL_CONTENT => {
            let content_range = resp
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let bytes = resp.bytes().await?.to_vec();
            Ok(MediaResponse { status: 206, bytes, content_type, content_range })
        }
        StatusCode::OK => {
            // Server ignored Range: buffer (bounded) and slice it ourselves.
            if resp.content_length().is_some_and(|l| l > MAX_UNRANGED) {
                return Err(AppError::Rejected("video too large to preview without range support".into()));
            }
            let all = resp.bytes().await?;
            let total = all.len() as u64;
            let Some((s, e)) = clamp(range, total) else {
                return Ok(MediaResponse {
                    status: 416,
                    bytes: vec![],
                    content_type,
                    content_range: Some(format!("bytes */{total}")),
                });
            };
            Ok(MediaResponse {
                status: 206,
                bytes: all[s as usize..=e as usize].to_vec(),
                content_type,
                content_range: Some(format!("bytes {s}-{e}/{total}")),
            })
        }
        StatusCode::RANGE_NOT_SATISFIABLE => Ok(MediaResponse {
            status: 416,
            bytes: vec![],
            content_type,
            content_range: None,
        }),
        s => Err(AppError::Http(s.as_u16())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges() {
        assert_eq!(parse_range(Some("bytes=0-")), Range { start: 0, end: None });
        assert_eq!(parse_range(Some("bytes=100-199")), Range { start: 100, end: Some(199) });
        assert_eq!(parse_range(Some("bytes=-500")), Range { start: 0, end: None });
        assert_eq!(parse_range(Some("bytes=10-5")), Range { start: 10, end: None });
        assert_eq!(parse_range(None), Range { start: 0, end: None });
    }

    #[test]
    fn clamps_to_chunk_and_size() {
        assert_eq!(clamp(Range { start: 0, end: None }, 100), Some((0, 99)));
        assert_eq!(clamp(Range { start: 0, end: None }, 10 * CHUNK), Some((0, CHUNK - 1)));
        assert_eq!(clamp(Range { start: 50, end: Some(60) }, 100), Some((50, 60)));
        assert_eq!(clamp(Range { start: 100, end: None }, 100), None);
    }

    #[tokio::test]
    async fn serves_local_slices() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("v.mp4");
        std::fs::write(&p, (0u8..=199).collect::<Vec<u8>>()).unwrap();
        let r = local(&p, "mp4", Range { start: 10, end: Some(19) }).await.unwrap();
        assert_eq!(r.status, 206);
        assert_eq!(r.bytes, (10u8..=19).collect::<Vec<u8>>());
        assert_eq!(r.content_range.as_deref(), Some("bytes 10-19/200"));
        let r = local(&p, "mp4", Range { start: 500, end: None }).await.unwrap();
        assert_eq!(r.status, 416);
    }
}
