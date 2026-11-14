//! Pure parsing of Danbooru JSON. No network; unit-tested with fixtures.

use serde_json::Value;

use super::{post_url, MAX_PAGE_NUMBER, PROVIDER_ID};
use crate::error::{AppError, AppResult};
use crate::providers::{Page, RemotePost, TagCategory, TagInfo, ALL_RATINGS};

const VIDEO_EXTS: [&str; 6] = ["mp4", "webm", "zip", "swf", "mov", "mkv"];

fn rating_letter(r: &str) -> Option<char> {
    match r {
        "general" => Some('g'),
        "sensitive" => Some('s'),
        "questionable" => Some('q'),
        "explicit" => Some('e'),
        _ => None,
    }
}

pub fn rating_word(letter: &str) -> Option<String> {
    Some(
        match letter.trim().to_ascii_lowercase().as_str() {
            "g" | "general" => "general",
            "s" | "sensitive" => "sensitive",
            "q" | "questionable" => "questionable",
            "e" | "explicit" => "explicit",
            _ => return None,
        }
        .to_string(),
    )
}

/// User tags + `rating:g,s` (one tag) for the allowed ratings. All allowed →
/// no rating tag at all.
pub fn build_query(user_tags: &str, allowed: &[String]) -> String {
    let mut parts: Vec<String> = user_tags.split_whitespace().map(str::to_string).collect();
    let letters: String = ALL_RATINGS
        .iter()
        .filter(|r| allowed.iter().any(|a| a == *r))
        .filter_map(|r| rating_letter(r))
        .map(String::from)
        .collect::<Vec<_>>()
        .join(",");
    if letters.split(',').count() < ALL_RATINGS.len() {
        // Empty allowed list can't happen (settings enforce ≥1), but stay safe.
        parts.push(format!("rating:{}", if letters.is_empty() { "g".into() } else { letters }));
    }
    parts.join(" ")
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

fn ext_of(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let (_, ext) = path.rsplit('/').next()?.rsplit_once('.')?;
    Some(ext.to_ascii_lowercase())
}

fn split(v: &Value, key: &str) -> Vec<String> {
    get_str(v, key)
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

pub fn post(v: &Value) -> Option<RemotePost> {
    if !v.is_object() {
        return None;
    }
    let remote_id = get_str(v, "id")?;
    let file_url = get_str(v, "file_url");
    let file_ext = get_str(v, "file_ext")
        .map(|e| e.to_ascii_lowercase())
        .or_else(|| file_url.as_deref().and_then(ext_of));
    // large_file_url is a still image for pictures, but can be a video for
    // animations; only keep it as a preview if it's not a video.
    let sample_url = get_str(v, "large_file_url")
        .filter(|u| ext_of(u).is_none_or(|e| !VIDEO_EXTS.contains(&e.as_str())));

    let mut categorized = Vec::new();
    for (key, cat) in [
        ("tag_string_artist", TagCategory::Artist),
        ("tag_string_character", TagCategory::Character),
        ("tag_string_copyright", TagCategory::Copyright),
        ("tag_string_general", TagCategory::General),
        ("tag_string_meta", TagCategory::Meta),
    ] {
        categorized.extend(split(v, key).into_iter().map(|name| TagInfo { name, category: cat, count: None }));
    }
    let tags = split(v, "tag_string");

    Some(RemotePost {
        provider: PROVIDER_ID,
        post_url: post_url(&remote_id),
        remote_id,
        md5: get_str(v, "md5").map(|m| m.to_ascii_lowercase()),
        rating: get_str(v, "rating").and_then(|r| rating_word(&r)),
        width: get_u32(v, "image_width"),
        height: get_u32(v, "image_height"),
        file_url,
        file_ext,
        file_size: v.get("file_size").and_then(as_u64),
        sample_url,
        preview_url: get_str(v, "preview_file_url"),
        preview_width: None,
        preview_height: None,
        source: get_str(v, "source"),
        created_at_raw: get_str(v, "created_at"),
        categorized: (!categorized.is_empty()).then_some(categorized),
        tags,
        raw: v.clone(),
    })
}

pub fn posts_page(body: &Value, page: u32, limit: u32) -> AppResult<Page<RemotePost>> {
    let raw = match body {
        Value::Array(a) => a,
        Value::Null => &Vec::new(),
        Value::Object(o) if o.contains_key("posts") => match &o["posts"] {
            Value::Array(a) => a,
            _ => return Err(AppError::Parse("'posts' is not a list".into())),
        },
        other => {
            let msg = other
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unexpected response");
            return Err(AppError::Rejected(msg.chars().take(200).collect()));
        }
    };
    let items: Vec<RemotePost> = raw.iter().filter_map(post).collect();
    let full = raw.len() as u32 >= limit;
    let next_ok = page + 2 <= MAX_PAGE_NUMBER; // next page (1-based) = page + 2
    Ok(Page {
        items,
        page,
        limit,
        total: None,
        has_more: full && next_ok,
        depth_limited: full && !next_ok,
    })
}

fn category(c: u64) -> TagCategory {
    match c {
        0 => TagCategory::General,
        1 => TagCategory::Artist,
        3 => TagCategory::Copyright,
        4 => TagCategory::Character,
        5 => TagCategory::Meta,
        _ => TagCategory::Unknown,
    }
}

pub fn tags(body: &Value) -> AppResult<Vec<TagInfo>> {
    let Value::Array(list) = body else {
        return Ok(vec![]);
    };
    Ok(list
        .iter()
        .filter_map(|t| {
            Some(TagInfo {
                name: get_str(t, "name")?,
                category: t.get("category").and_then(as_u64).map(category).unwrap_or(TagCategory::Unknown),
                count: t.get("post_count").and_then(as_u64),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // SYNTHETIC fixture shaped after open-source clients; replace with a real
    // capture (no credentials) once verified live.
    const POSTS: &str = include_str!("fixtures/posts_synthetic.json");

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn query_uses_single_rating_tag() {
        assert_eq!(build_query("cat_ears", &s(&["general"])), "cat_ears rating:g");
        assert_eq!(build_query(" a  b ", &s(&["general", "sensitive"])), "a b rating:g,s");
        assert_eq!(build_query("a", &s(&ALL_RATINGS)), "a");
        assert_eq!(build_query("", &s(&["explicit", "general"])), "rating:g,e");
    }

    #[test]
    fn parses_posts_with_inline_categories() {
        let body: Value = serde_json::from_str(POSTS).unwrap();
        let page = posts_page(&body, 0, 3).unwrap();
        assert_eq!(page.items.len(), 2, "post without id is skipped");
        assert!(page.has_more);

        let p = &page.items[0];
        assert_eq!(p.provider, "danbooru");
        assert_eq!(p.post_url, "https://danbooru.donmai.us/posts/7000001");
        assert_eq!(p.rating.as_deref(), Some("sensitive"));
        assert_eq!((p.width, p.height), (Some(2894), Some(4093)));
        assert_eq!(p.file_size, Some(5_123_456));
        assert_eq!(p.file_ext.as_deref(), Some("png"));
        assert_eq!(p.md5.as_deref(), Some("334e19279933437764c2864a2e7cc903"));
        assert!(p.sample_url.as_deref().unwrap().contains("/sample/"));
        let cats = p.categorized.as_ref().unwrap();
        let artists: Vec<&str> = cats.iter().filter(|t| t.category == TagCategory::Artist).map(|t| t.name.as_str()).collect();
        assert_eq!(artists, vec!["example_artist"]);
        assert!(cats.iter().any(|t| t.name == "highres" && t.category == TagCategory::Meta));
        assert_eq!(p.tags.len(), 6);

        // Video: large_file_url is an mp4 → no sample preview.
        let v = &page.items[1];
        assert_eq!(v.file_ext.as_deref(), Some("mp4"));
        assert!(v.sample_url.is_none());
        assert_eq!(v.rating.as_deref(), Some("general"));
    }

    #[test]
    fn error_object_becomes_rejected() {
        let body: Value =
            serde_json::from_str(r#"{"success":false,"message":"You cannot search for more than 2 tags at a time."}"#).unwrap();
        match posts_page(&body, 0, 60) {
            Err(AppError::Rejected(m)) => assert!(m.contains("2 tags")),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn page_cap() {
        let body: Value = serde_json::from_str(POSTS).unwrap();
        let last = posts_page(&body, MAX_PAGE_NUMBER - 1, 3).unwrap();
        assert!(!last.has_more);
        assert!(last.depth_limited);
    }

    #[test]
    fn parses_tags() {
        let body: Value = serde_json::from_str(
            r#"[{"id":1,"name":"example_artist","post_count":10,"category":1},{"id":2,"name":"solo","post_count":"99","category":0}]"#,
        )
        .unwrap();
        let t = tags(&body).unwrap();
        assert_eq!(t[0].category, TagCategory::Artist);
        assert_eq!(t[1].count, Some(99));
    }
}
