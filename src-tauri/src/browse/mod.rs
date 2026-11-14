//! Online browsing: builds rating-safe queries, slims posts down for the
//! gallery, and serves thumbnails through the `thumb` URI scheme.

pub mod thumbs;

use serde::Serialize;

use crate::providers::{Page, RemotePost};
use thumbs::Thumbs;

/// Posts per gallery page.
pub const PAGE_SIZE: u32 = 60;

pub const ALL_RATINGS: [&str; 4] = ["general", "sensitive", "questionable", "explicit"];

/// Map older/abbreviated rating spellings onto Gelbooru's current four.
pub fn normalize_rating(r: &str) -> String {
    match r.trim().to_ascii_lowercase().as_str() {
        "g" | "safe" | "s" => "general".into(),
        "q" => "questionable".into(),
        "e" => "explicit".into(),
        other => other.into(),
    }
}

/// Append an exclusion for every rating the user hasn't allowed, so the
/// board never even returns them.
pub fn build_query(user_tags: &str, allowed: &[String]) -> String {
    let mut parts: Vec<String> = user_tags.split_whitespace().map(str::to_string).collect();
    for r in ALL_RATINGS {
        if !allowed.iter().any(|a| a == r) {
            parts.push(format!("-rating:{r}"));
        }
    }
    parts.join(" ")
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowsePost {
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
pub fn to_browse_page(page: Page<RemotePost>, allowed: &[String], thumbs: &Thumbs) -> BrowsePage {
    let mut hidden = 0;
    let posts = page
        .items
        .into_iter()
        .filter(|p| {
            let ok = p
                .rating
                .as_deref()
                .map(normalize_rating)
                .is_some_and(|r| allowed.contains(&r));
            if !ok {
                hidden += 1;
            }
            ok
        })
        .map(|p| {
            let thumb_key = match (&p.md5, &p.preview_url) {
                (Some(md5), Some(url)) => thumbs.register(p.provider, md5, url),
                _ => None,
            };
            BrowsePost {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn query_excludes_disallowed_ratings() {
        assert_eq!(
            build_query("  long_hair   blue_eyes ", &s(&["general"])),
            "long_hair blue_eyes -rating:sensitive -rating:questionable -rating:explicit"
        );
        assert_eq!(build_query("", &s(&ALL_RATINGS)), "");
        assert_eq!(
            build_query("cat", &s(&["general", "sensitive", "questionable"])),
            "cat -rating:explicit"
        );
    }

    #[test]
    fn normalizes_ratings() {
        assert_eq!(normalize_rating("Safe"), "general");
        assert_eq!(normalize_rating("e"), "explicit");
        assert_eq!(normalize_rating("sensitive"), "sensitive");
    }
}
