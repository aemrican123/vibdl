//! Pure parsing of Gelbooru dapi JSON. No network; unit-tested with fixtures.

use std::collections::HashMap;

use serde_json::Value;

use super::{post_url, MAX_RESULT_DEPTH, PROVIDER_ID};
use crate::error::{AppError, AppResult};
use crate::providers::{Page, RemotePost, TagCategory, TagInfo};

pub fn page_within_depth(page: u32, limit: u32) -> bool {
    (page as u64 + 1) * limit as u64 <= MAX_RESULT_DEPTH as u64
}

/// Gelbooru wraps lists as `{"@attributes": {...}, "<key>": [...]}`; a single
/// result may come back as an object instead of an array, and an empty result
/// may omit the key entirely or be an empty body.
fn items<'a>(body: &'a Value, key: &str) -> AppResult<Vec<&'a Value>> {
    match body {
        Value::Null => Ok(vec![]),
        Value::Object(map) => match map.get(key) {
            None | Some(Value::Null) => Ok(vec![]),
            Some(Value::Array(a)) => Ok(a.iter().collect()),
            Some(v @ Value::Object(_)) => Ok(vec![v]),
            Some(other) => Err(AppError::Parse(format!("'{key}' has unexpected type: {other}"))),
        },
        Value::Array(a) => Ok(a.iter().collect()),
        other => Err(AppError::Parse(format!(
            "expected object, got {}",
            other.to_string().chars().take(80).collect::<String>()
        ))),
    }
}

fn attr_u64(body: &Value, name: &str) -> Option<u64> {
    body.get("@attributes").and_then(|a| a.get(name)).and_then(as_u64)
}

fn as_u64(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn get_u32(v: &Value, key: &str) -> Option<u32> {
    v.get(key).and_then(as_u64).and_then(|n| u32::try_from(n).ok())
}

fn get_str(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Gelbooru has been seen returning HTML-escaped tag text; decode the common
/// entities defensively. (UNVERIFIED — confirm against a real response.)
pub fn unescape_html(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn extension_of(name: &str) -> Option<String> {
    let name = name.split(['?', '#']).next()?;
    let file = name.rsplit('/').next()?;
    let (_, ext) = file.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    (!ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .then_some(ext)
}

pub fn post(v: &Value) -> Option<RemotePost> {
    let remote_id = get_str(v, "id")?;
    let file_url = get_str(v, "file_url");
    let file_ext = get_str(v, "image")
        .and_then(|i| extension_of(&i))
        .or_else(|| file_url.as_deref().and_then(extension_of));
    let tags = get_str(v, "tags")
        .map(|t| t.split_whitespace().map(unescape_html).collect())
        .unwrap_or_default();

    Some(RemotePost {
        provider: PROVIDER_ID,
        post_url: post_url(&remote_id),
        remote_id,
        md5: get_str(v, "md5").map(|m| m.to_ascii_lowercase()),
        rating: get_str(v, "rating"),
        width: get_u32(v, "width"),
        height: get_u32(v, "height"),
        file_url,
        file_ext,
        sample_url: get_str(v, "sample_url"),
        preview_url: get_str(v, "preview_url"),
        preview_width: get_u32(v, "preview_width"),
        preview_height: get_u32(v, "preview_height"),
        source: get_str(v, "source"),
        created_at_raw: get_str(v, "created_at"),
        tags,
        raw: v.clone(),
    })
}

pub fn posts_page(body: &Value, page: u32, limit: u32) -> AppResult<Page<RemotePost>> {
    let raw_items = items(body, "post")?;
    let posts: Vec<RemotePost> = raw_items.iter().filter_map(|v| post(v)).collect();
    if posts.len() < raw_items.len() {
        tracing::warn!(skipped = raw_items.len() - posts.len(), "skipped posts without an id");
    }

    let total = attr_u64(body, "count");
    let offset = attr_u64(body, "offset").unwrap_or(page as u64 * limit as u64);
    let next_in_depth = page_within_depth(page + 1, limit);
    let more_remote = match total {
        Some(t) => offset + (raw_items.len() as u64) < t,
        None => raw_items.len() as u32 >= limit,
    };

    Ok(Page {
        items: posts,
        page,
        limit,
        total,
        has_more: more_remote && next_in_depth,
        depth_limited: more_remote && !next_in_depth,
    })
}

pub fn category_from_type(t: u64) -> TagCategory {
    match t {
        0 => TagCategory::General,
        1 => TagCategory::Artist,
        2 => TagCategory::Deprecated,
        3 => TagCategory::Copyright,
        4 => TagCategory::Character,
        5 => TagCategory::Meta,
        _ => TagCategory::Unknown,
    }
}

pub fn tags(body: &Value) -> AppResult<Vec<TagInfo>> {
    Ok(items(body, "tag")?
        .into_iter()
        .filter_map(|v| {
            let name = unescape_html(&get_str(v, "name")?);
            let category = v
                .get("type")
                .and_then(as_u64)
                .map(category_from_type)
                .unwrap_or(TagCategory::Unknown);
            Some(TagInfo {
                name,
                category,
                count: v.get("count").and_then(as_u64),
            })
        })
        .collect())
}

/// One result per requested name, in request order. Names the board didn't
/// return are `Unknown` — never guessed from the name.
pub fn merge_requested(requested: &[String], found: Vec<TagInfo>) -> Vec<TagInfo> {
    let by_name: HashMap<String, TagInfo> = found
        .into_iter()
        .map(|t| (t.name.to_lowercase(), t))
        .collect();
    requested
        .iter()
        .map(|name| {
            by_name.get(&name.to_lowercase()).cloned().unwrap_or(TagInfo {
                name: name.clone(),
                category: TagCategory::Unknown,
                count: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // SYNTHETIC fixtures shaped after open-source clients. Replace with real
    // captures (credentials removed) during Milestone 0.
    const POSTS: &str = include_str!("fixtures/posts_synthetic.json");
    const SINGLE: &str = include_str!("fixtures/post_single_object_synthetic.json");
    const TAGS: &str = include_str!("fixtures/tags_synthetic.json");

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn parses_posts_page() {
        let page = posts_page(&json(POSTS), 0, 2).unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.total, Some(57));
        assert!(page.has_more);
        assert!(!page.depth_limited);

        let p = &page.items[0];
        assert_eq!(p.remote_id, "1000001");
        assert_eq!(p.md5.as_deref(), Some("0123456789abcdef0123456789abcdef"));
        assert_eq!(p.file_ext.as_deref(), Some("png"));
        assert_eq!((p.width, p.height), (Some(2048), Some(1536)));
        assert_eq!(p.rating.as_deref(), Some("general"));
        assert_eq!(p.post_url, "https://gelbooru.com/index.php?page=post&s=view&id=1000001");
        assert!(p.tags.contains(&"example_artist".to_string()));
        assert!(p.tags.contains(&"jeanne_d'arc".to_string()), "html entities decoded");
        assert_eq!(p.source, None, "empty source becomes None");

        // numeric fields given as strings still parse
        assert_eq!(page.items[1].width, Some(800));
        assert_eq!(page.items[1].file_ext.as_deref(), Some("jpg"));
    }

    #[test]
    fn single_object_and_empty_results() {
        let page = posts_page(&json(SINGLE), 0, 1).unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(!page.has_more);

        let empty = json(r#"{"@attributes":{"limit":100,"offset":0,"count":0}}"#);
        let page = posts_page(&empty, 0, 100).unwrap();
        assert!(page.items.is_empty());
        assert!(!page.has_more);

        assert!(posts_page(&Value::Null, 0, 100).unwrap().items.is_empty());
    }

    #[test]
    fn depth_cap() {
        assert!(page_within_depth(0, 100));
        assert!(page_within_depth(199, 100));
        assert!(!page_within_depth(200, 100));
        let body = json(r#"{"@attributes":{"limit":100,"offset":19900,"count":90000},"post":[]}"#);
        let page = posts_page(&body, 199, 100).unwrap();
        assert!(!page.has_more);
        assert!(page.depth_limited);
    }

    #[test]
    fn parses_and_merges_tags() {
        let found = tags(&json(TAGS)).unwrap();
        let requested: Vec<String> = [
            "example_artist",
            "character_a",
            "example_series",
            "long_hair",
            "highres",
            "old_tag",
            "not_returned",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let merged = merge_requested(&requested, found);
        let cats: Vec<TagCategory> = merged.iter().map(|t| t.category).collect();
        assert_eq!(
            cats,
            vec![
                TagCategory::Artist,
                TagCategory::Character,
                TagCategory::Copyright,
                TagCategory::General,
                TagCategory::Meta,
                TagCategory::Deprecated,
                TagCategory::Unknown,
            ]
        );
        assert_eq!(merged[3].count, Some(1_500_000));
    }

    #[test]
    fn extensions() {
        assert_eq!(extension_of("abc.PNG").as_deref(), Some("png"));
        assert_eq!(extension_of("https://img/x/y/abc.webm?123").as_deref(), Some("webm"));
        assert_eq!(extension_of("noext"), None);
    }
}
