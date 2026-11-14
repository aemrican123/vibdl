//! Moebooru-engine boards: yande.re and Konachan. Uses the v2 post API
//! (`api_version=2&include_tags=1`), which returns tag types alongside the
//! posts, so no per-tag lookups are needed. UNTESTED against the live sites
//! (fixtures are synthetic); parsing is tolerant and also accepts the v1
//! plain-array response.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::{Capabilities, ImageboardProvider, Page, RemotePost, SearchQuery, TagCategory, TagInfo};
use crate::error::{AppError, AppResult};
use crate::net::ApiClient;

#[derive(Debug)]
pub struct Site {
    pub id: &'static str,
    pub base: &'static str,
}

pub const YANDERE: Site = Site { id: "yandere", base: "https://yande.re" };
pub const KONACHAN: Site = Site { id: "konachan", base: "https://konachan.com" };

pub const MAX_PAGE_SIZE: u32 = 100;

pub struct MoebooruProvider {
    site: &'static Site,
    client: Arc<ApiClient>,
}

impl MoebooruProvider {
    pub fn new(site: &'static Site, client: Arc<ApiClient>) -> Self {
        Self { site, client }
    }
}

pub fn post_url(site: &Site, id: &str) -> String {
    format!("{}/post/show/{id}", site.base)
}

/// Moebooru has three ratings: s(afe), q(uestionable), e(xplicit).
/// "safe" maps to our "general"; "sensitive" has no equivalent.
fn rating_word(r: &str) -> Option<String> {
    Some(
        match r.trim().to_ascii_lowercase().as_str() {
            "s" | "safe" => "general",
            "q" | "questionable" => "questionable",
            "e" | "explicit" => "explicit",
            _ => return None,
        }
        .to_string(),
    )
}

pub fn build_query(user_tags: &str, allowed: &[String]) -> String {
    let mut parts: Vec<String> = user_tags.split_whitespace().map(str::to_string).collect();
    for (letter, ours) in [("s", "general"), ("q", "questionable"), ("e", "explicit")] {
        if !allowed.iter().any(|a| a == ours) {
            parts.push(format!("-rating:{letter}"));
        }
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

fn get_u32(v: &Value, k: &str) -> Option<u32> {
    v.get(k).and_then(as_u64).and_then(|n| u32::try_from(n).ok())
}

fn get_str(v: &Value, k: &str) -> Option<String> {
    match v.get(k)? {
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

/// Tag type as a word ("artist") or a number (1). "circle" (an artist
/// group) counts as artist; "faults" (image problems) as meta.
pub fn category(v: &Value) -> TagCategory {
    let word = match v {
        Value::String(s) => s.to_ascii_lowercase(),
        Value::Number(n) => match n.as_u64() {
            Some(0) => "general".into(),
            Some(1) => "artist".into(),
            Some(3) => "copyright".into(),
            Some(4) => "character".into(),
            Some(5) => "circle".into(),
            Some(6) => "faults".into(),
            _ => String::new(),
        },
        _ => String::new(),
    };
    match word.as_str() {
        "general" => TagCategory::General,
        "artist" | "circle" => TagCategory::Artist,
        "copyright" => TagCategory::Copyright,
        "character" => TagCategory::Character,
        "faults" | "meta" => TagCategory::Meta,
        _ => TagCategory::Unknown,
    }
}

pub fn post(site: &Site, v: &Value, types: &HashMap<String, TagCategory>) -> Option<RemotePost> {
    let remote_id = get_str(v, "id")?;
    let file_url = get_str(v, "file_url");
    let tags: Vec<String> = get_str(v, "tags")
        .map(|t| t.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    let categorized = (!types.is_empty()).then(|| {
        tags.iter()
            .map(|t| TagInfo {
                name: t.clone(),
                category: types.get(t).copied().unwrap_or(TagCategory::Unknown),
                count: None,
            })
            .collect()
    });
    // created_at is a unix timestamp on Moebooru; keep it readable.
    let created_at_raw = match v.get("created_at") {
        Some(Value::Number(n)) => n
            .as_i64()
            .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
            .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        _ => get_str(v, "created_at"),
    };
    Some(RemotePost {
        provider: site.id,
        post_url: post_url(site, &remote_id),
        remote_id,
        md5: get_str(v, "md5").map(|m| m.to_ascii_lowercase()),
        rating: get_str(v, "rating").and_then(|r| rating_word(&r)),
        width: get_u32(v, "width"),
        height: get_u32(v, "height"),
        file_ext: get_str(v, "file_ext")
            .map(|e| e.to_ascii_lowercase())
            .or_else(|| file_url.as_deref().and_then(ext_of)),
        file_size: v.get("file_size").and_then(as_u64),
        file_url,
        // jpeg_url is a full-size JPEG for PNG originals; sample_url is smaller.
        sample_url: get_str(v, "sample_url").or_else(|| get_str(v, "jpeg_url")),
        preview_url: get_str(v, "preview_url"),
        preview_width: get_u32(v, "actual_preview_width").or_else(|| get_u32(v, "preview_width")),
        preview_height: get_u32(v, "actual_preview_height").or_else(|| get_u32(v, "preview_height")),
        source: get_str(v, "source"),
        created_at_raw,
        tags,
        categorized,
        raw: v.clone(),
    })
}

/// Accepts the v2 object (`{posts, tags}`) or a v1 plain array.
pub fn posts_page(site: &Site, body: &Value, page: u32, limit: u32) -> AppResult<Page<RemotePost>> {
    let (list, types): (&[Value], HashMap<String, TagCategory>) = match body {
        Value::Null => (&[], HashMap::new()),
        Value::Array(a) => (a.as_slice(), HashMap::new()),
        Value::Object(o) => {
            let list = match o.get("posts") {
                Some(Value::Array(a)) => a.as_slice(),
                None | Some(Value::Null) => &[],
                Some(_) => return Err(AppError::Parse("'posts' is not a list".into())),
            };
            let types = o
                .get("tags")
                .and_then(Value::as_object)
                .map(|m| m.iter().map(|(k, v)| (k.clone(), category(v))).collect())
                .unwrap_or_default();
            (list, types)
        }
        _ => return Err(AppError::Parse("unexpected response".into())),
    };
    let items: Vec<RemotePost> = list.iter().filter_map(|v| post(site, v, &types)).collect();
    Ok(Page {
        items,
        page,
        limit,
        total: None,
        has_more: list.len() as u32 >= limit,
        depth_limited: false,
    })
}

pub fn tags(body: &Value) -> Vec<TagInfo> {
    body.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    Some(TagInfo {
                        name: get_str(t, "name")?,
                        category: t.get("type").map(category).unwrap_or(TagCategory::Unknown),
                        count: t.get("count").and_then(as_u64),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl ImageboardProvider for MoebooruProvider {
    fn id(&self) -> &'static str {
        self.site.id
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { max_page_size: MAX_PAGE_SIZE, max_result_depth: None, needs_tag_lookup: false, max_tags: Some(6) }
    }

    fn build_query(&self, user_tags: &str, allowed: &[String]) -> String {
        build_query(user_tags, allowed)
    }

    async fn search(&self, q: &SearchQuery) -> AppResult<Page<RemotePost>> {
        let limit = q.limit.clamp(1, MAX_PAGE_SIZE);
        let mut params = vec![
            ("api_version", "2".to_string()),
            ("include_tags", "1".to_string()),
            ("limit", limit.to_string()),
            ("page", (q.page + 1).to_string()),
        ];
        if !q.tags.trim().is_empty() {
            params.push(("tags", q.tags.trim().to_string()));
        }
        let body = self.client.get_json(&format!("{}/post.json", self.site.base), &params).await?;
        posts_page(self.site, &body, q.page, limit)
    }

    async fn get_post(&self, id: &str) -> AppResult<Option<RemotePost>> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            return Ok(None);
        }
        let params = vec![
            ("api_version", "2".to_string()),
            ("include_tags", "1".to_string()),
            ("tags", format!("id:{id}")),
            ("limit", "1".to_string()),
        ];
        let body = self.client.get_json(&format!("{}/post.json", self.site.base), &params).await?;
        Ok(posts_page(self.site, &body, 0, 1)?.items.into_iter().next())
    }

    /// Only used if a response came without tag types: no guessing.
    async fn categorize_tags(&self, names: &[String]) -> AppResult<Vec<TagInfo>> {
        Ok(names
            .iter()
            .map(|n| TagInfo { name: n.clone(), category: TagCategory::Unknown, count: None })
            .collect())
    }

    fn post_url(&self, id: &str) -> String {
        post_url(self.site, id)
    }

    async fn suggest_tags(&self, prefix: &str, limit: u32) -> AppResult<Vec<TagInfo>> {
        let params = vec![
            ("name", format!("{prefix}*")),
            ("order", "count".to_string()),
            ("limit", (limit * 2).min(50).to_string()),
        ];
        let body = self.client.get_json(&format!("{}/tag.json", self.site.base), &params).await?;
        Ok(super::filter_suggestions(prefix, tags(&body), limit as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SYNTHETIC, shaped after Moebooru client libraries.
    const V2: &str = include_str!("fixtures/posts_v2_synthetic.json");

    fn all() -> Vec<String> {
        crate::providers::ALL_RATINGS.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_v2_with_inline_tag_types() {
        let body: Value = serde_json::from_str(V2).unwrap();
        let page = posts_page(&YANDERE, &body, 0, 2).unwrap();
        assert_eq!(page.items.len(), 2);
        assert!(page.has_more);
        let p = &page.items[0];
        assert_eq!(p.provider, "yandere");
        assert_eq!(p.post_url, "https://yande.re/post/show/900001");
        assert_eq!(p.rating.as_deref(), Some("general"), "moebooru 's' = safe = general");
        assert_eq!(p.md5.as_deref(), Some("abcdefabcdefabcdefabcdefabcdefab"));
        assert_eq!((p.width, p.height, p.file_size), (Some(3000), Some(2000), Some(4_000_000)));
        assert_eq!(p.file_ext.as_deref(), Some("png"));
        assert_eq!(p.created_at_raw.as_deref(), Some("2026-11-13T00:00:00Z"));
        let cats = p.categorized.as_ref().unwrap();
        let cat = |n: &str| cats.iter().find(|t| t.name == n).unwrap().category;
        assert_eq!(cat("some_artist"), TagCategory::Artist);
        assert_eq!(cat("a_circle"), TagCategory::Artist);
        assert_eq!(cat("some_series"), TagCategory::Copyright);
        assert_eq!(cat("char_a"), TagCategory::Character);
        assert_eq!(cat("jpeg_artifacts"), TagCategory::Meta);
        assert_eq!(cat("thighhighs"), TagCategory::General);
        assert_eq!(page.items[1].rating.as_deref(), Some("explicit"));
    }

    #[test]
    fn accepts_v1_array_without_types() {
        let body: Value = serde_json::from_str(r#"[{"id":5,"tags":"a b","rating":"q","md5":"x","file_url":"https://f/x.jpg"}]"#).unwrap();
        let page = posts_page(&KONACHAN, &body, 0, 60).unwrap();
        assert_eq!(page.items[0].post_url, "https://konachan.com/post/show/5");
        assert!(page.items[0].categorized.is_none());
        assert!(!page.has_more);
    }

    #[test]
    fn rating_query_and_tag_types() {
        assert_eq!(build_query("cat", &["general".into()]), "cat -rating:q -rating:e");
        assert_eq!(build_query("", &all()), "");
        assert_eq!(category(&serde_json::json!(4)), TagCategory::Character);
        let t = tags(&serde_json::json!([{"name":"long_hair","count":5,"type":0}]));
        assert_eq!((t[0].category, t[0].count), (TagCategory::General, Some(5)));
    }
}
