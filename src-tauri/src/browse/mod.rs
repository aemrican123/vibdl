//! Online browsing: builds rating-safe queries, slims posts down for the
//! gallery, and serves thumbnails through the `thumb` URI scheme.

pub mod media;
pub mod thumbs;

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;

use crate::metadata::CategorizedTags;
use crate::providers::{Page, RemotePost};
use thumbs::Thumbs;

const MAX_CACHED_POSTS: usize = 3_000;
/// Without a sample, show the original as the preview only if it's this small.
const MAX_ORIGINAL_AS_PREVIEW_PX: u32 = 1500;

/// Posts seen in recent search results, so opening details needs no extra
/// post request.
#[derive(Default)]
pub struct PostCache {
    map: Mutex<HashMap<String, RemotePost>>,
}

impl PostCache {
    fn key(provider: &str, id: &str) -> String {
        format!("{provider}:{id}")
    }

    pub fn put(&self, post: &RemotePost) {
        let mut map = self.map.lock().expect("post cache poisoned");
        if map.len() >= MAX_CACHED_POSTS {
            map.clear();
        }
        map.insert(Self::key(post.provider, &post.remote_id), post.clone());
    }

    pub fn get(&self, provider: &str, id: &str) -> Option<RemotePost> {
        self.map.lock().expect("post cache poisoned").get(&Self::key(provider, id)).cloned()
    }
}

/// Posts per gallery page.
pub const PAGE_SIZE: u32 = 60;

/// Defensive: map abbreviated/legacy rating spellings onto the four words
/// (providers already normalize).
pub fn normalize_rating(r: &str) -> String {
    match r.trim().to_ascii_lowercase().as_str() {
        "g" | "safe" => "general".into(),
        "s" => "sensitive".into(),
        "q" => "questionable".into(),
        "e" => "explicit".into(),
        other => other.into(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowsePost {
    pub provider: &'static str,
    pub id: String,
    pub md5: Option<String>,
    pub rating: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub preview_width: Option<u32>,
    pub preview_height: Option<u32>,
    pub file_ext: Option<String>,
    /// Key for the `thumb` URI scheme; `None` if the post has no preview.
    pub thumb_key: Option<String>,
    pub post_url: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowsePage {
    pub posts: Vec<BrowsePost>,
    pub page: u32,
    pub total: Option<u64>,
    pub has_more: bool,
    pub depth_limited: bool,
    /// Posts dropped client-side because their rating isn't allowed
    /// (should normally be 0 thanks to the query exclusions).
    pub hidden_by_rating: usize,
}

/// Slim a provider page down for the UI, enforce the rating filter a second
/// time, and register preview URLs so only API-supplied URLs are fetchable.
pub fn rating_allowed(post: &RemotePost, allowed: &[String]) -> bool {
    post.rating
        .as_deref()
        .map(normalize_rating)
        .is_some_and(|r| allowed.contains(&r))
}

pub fn to_browse_page(
    page: Page<RemotePost>,
    allowed: &[String],
    thumbs: &Thumbs,
    cache: &PostCache,
) -> BrowsePage {
    let mut hidden = 0;
    let posts = page
        .items
        .into_iter()
        .filter(|p| {
            let ok = rating_allowed(p, allowed);
            if !ok {
                hidden += 1;
            }
            ok
        })
        .map(|p| {
            cache.put(&p);
            let thumb_key = match (&p.md5, &p.preview_url) {
                (Some(md5), Some(url)) => thumbs.register(p.provider, md5, url),
                _ => None,
            };
            BrowsePost {
                provider: p.provider,
                id: p.remote_id,
                md5: p.md5,
                rating: p.rating.as_deref().map(normalize_rating),
                width: p.width,
                height: p.height,
                preview_width: p.preview_width,
                preview_height: p.preview_height,
                file_ext: p.file_ext,
                thumb_key,
                post_url: p.post_url,
                tags: p.tags,
            }
        })
        .collect();

    BrowsePage {
        posts,
        page: page.page,
        total: page.total,
        has_more: page.has_more,
        depth_limited: page.depth_limited,
        hidden_by_rating: hidden,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PostDetails {
    pub provider: &'static str,
    pub id: String,
    pub post_url: String,
    pub md5: Option<String>,
    pub rating: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub file_ext: Option<String>,
    pub file_url: Option<String>,
    pub file_size: Option<u64>,
    pub sources: Vec<String>,
    pub created_at: Option<String>,
    pub score: Option<i64>,
    pub parent_id: Option<String>,
    pub has_children: bool,
    pub status: Option<String>,
    pub thumb_key: Option<String>,
    /// Key for the sample preview, if one can be shown without the original.
    pub sample_key: Option<String>,
    /// True when the "sample" is actually the (small) original file.
    pub preview_is_original: bool,
    /// Key for streaming the video (mp4/webm) via the `media` scheme.
    pub video_key: Option<String>,
    /// Key for viewing the full original image (still images only; fetched
    /// only when the user asks for it in the viewer, never saved).
    pub original_key: Option<String>,
    pub tags: CategorizedTags,
    pub tag_lookup_error: Option<String>,
    /// How many tags needed an API lookup (0 = all from cache).
    pub tags_looked_up: usize,
    /// Every field the board returned, for the "all fields" view.
    pub fields: Value,
    /// Set when this post (or the same file) is already in the library.
    pub local: Option<crate::db::LocalEntry>,
}

const VIDEO_EXTS: [&str; 6] = ["webm", "mp4", "mov", "mkv", "swf", "zip"];

/// Pick the preview image URL: the sample if there is one, else the original
/// only when it's a small still image. Returns (url, is_original).
pub fn preview_source(p: &RemotePost) -> Option<(&str, bool)> {
    if let Some(s) = p.sample_url.as_deref() {
        return Some((s, false));
    }
    let is_video = p.file_ext.as_deref().is_some_and(|e| VIDEO_EXTS.contains(&e));
    let small = matches!((p.width, p.height), (Some(w), Some(h)) if w.max(h) <= MAX_ORIGINAL_AS_PREVIEW_PX);
    match (&p.file_url, is_video, small) {
        (Some(f), false, true) => Some((f.as_str(), true)),
        _ => None,
    }
}

fn raw_i64(raw: &Value, key: &str) -> Option<i64> {
    match raw.get(key)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn raw_bool(raw: &Value, key: &str) -> bool {
    match raw.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true") || s == "1",
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        _ => false,
    }
}

fn raw_string(raw: &Value, key: &str) -> Option<String> {
    match raw.get(key)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn to_details(
    p: RemotePost,
    tags: CategorizedTags,
    tag_lookup_error: Option<String>,
    tags_looked_up: usize,
    thumbs: &Thumbs,
) -> PostDetails {
    let (sample_key, preview_is_original) = match (p.md5.as_deref(), preview_source(&p)) {
        (Some(md5), Some((url, orig))) => (thumbs.register_sample(p.provider, md5, url), orig),
        _ => (None, false),
    };
    let video_key = match (p.md5.as_deref(), p.file_url.as_deref(), p.file_ext.as_deref()) {
        (Some(md5), Some(url), Some(ext)) if media::is_video(ext) => thumbs.register_video(p.provider, md5, url),
        _ => None,
    };
    let original_key = match (p.md5.as_deref(), p.file_url.as_deref(), p.file_ext.as_deref()) {
        (Some(md5), Some(url), Some(ext)) if crate::library::thumbs::is_still(ext) => {
            thumbs.register_original(p.provider, md5, url)
        }
        _ => None,
    };
    let thumb_key = match (p.md5.as_deref(), p.preview_url.as_deref()) {
        (Some(md5), Some(url)) => thumbs.register(p.provider, md5, url),
        _ => None,
    };
    PostDetails {
        sources: p.source.as_deref().map(|s| s.split_whitespace().map(str::to_string).collect()).unwrap_or_default(),
        score: raw_i64(&p.raw, "score"),
        parent_id: raw_string(&p.raw, "parent_id").filter(|s| s != "0"),
        has_children: raw_bool(&p.raw, "has_children"),
        status: raw_string(&p.raw, "status"),
        rating: p.rating.as_deref().map(normalize_rating),
        provider: p.provider,
        id: p.remote_id,
        post_url: p.post_url,
        md5: p.md5,
        width: p.width,
        height: p.height,
        file_ext: p.file_ext,
        file_url: p.file_url,
        file_size: p.file_size,
        created_at: p.created_at_raw,
        thumb_key,
        sample_key,
        preview_is_original,
        video_key,
        original_key,
        tags,
        tag_lookup_error,
        tags_looked_up,
        fields: p.raw,
        local: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(sample: Option<&str>, ext: &str, w: u32) -> RemotePost {
        RemotePost {
            provider: "gelbooru",
            remote_id: "1".into(),
            post_url: "u".into(),
            md5: Some("0123456789abcdef0123456789abcdef".into()),
            rating: Some("general".into()),
            width: Some(w),
            height: Some(w),
            file_url: Some(format!("https://img/x.{ext}")),
            file_ext: Some(ext.into()),
            file_size: None,
            sample_url: sample.map(str::to_string),
            preview_url: Some("https://img/t.jpg".into()),
            preview_width: None,
            preview_height: None,
            source: Some("https://a https://b".into()),
            created_at_raw: None,
            tags: vec![],
            categorized: None,
            raw: serde_json::json!({"score": "7", "parent_id": 0, "has_children": "false", "status": "active"}),
        }
    }

    #[test]
    fn preview_never_uses_large_originals() {
        assert_eq!(preview_source(&post(Some("https://img/s.jpg"), "png", 4000)), Some(("https://img/s.jpg", false)));
        assert_eq!(preview_source(&post(None, "png", 800)), Some(("https://img/x.png", true)));
        assert_eq!(preview_source(&post(None, "png", 4000)), None);
        assert_eq!(preview_source(&post(None, "webm", 640)), None);
    }

    #[test]
    fn details_extracts_extra_fields() {
        let dir = tempfile::tempdir().unwrap();
        let t = Thumbs::new(dir.path().into(), 50, reqwest::Client::new()).unwrap();
        let d = to_details(post(Some("https://img/s.jpg"), "png", 4000), CategorizedTags::default(), None, 0, &t);
        assert_eq!(d.score, Some(7));
        assert_eq!(d.parent_id, None);
        assert!(!d.has_children);
        assert_eq!(d.sources, vec!["https://a", "https://b"]);
        assert!(d.sample_key.as_deref().unwrap().ends_with("-s"));
        assert!(d.thumb_key.is_some());
        assert!(d.original_key.as_deref().unwrap().ends_with("-o"));
        assert!(d.video_key.is_none());
    }

    #[test]
    fn normalizes_ratings() {
        assert_eq!(normalize_rating("Safe"), "general");
        assert_eq!(normalize_rating("s"), "sensitive");
        assert_eq!(normalize_rating("e"), "explicit");
        assert_eq!(normalize_rating("sensitive"), "sensitive");
    }
}
