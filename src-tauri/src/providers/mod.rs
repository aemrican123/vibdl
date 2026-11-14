//! Imageboard provider abstraction. Each board lives in its own submodule and
//! maps its API onto these shared types. Adding a board = implementing
//! `ImageboardProvider` and listing it in `PROVIDERS`.

pub mod danbooru;
pub mod gelbooru;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TagCategory {
    General,
    Artist,
    Copyright,
    Character,
    Meta,
    Deprecated,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TagInfo {
    pub name: String,
    pub category: TagCategory,
    /// Number of posts using the tag on the remote board, when reported.
    pub count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    /// Tag query in the board's own syntax (rating filter already applied).
    pub tags: String,
    /// Zero-based page index.
    pub page: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub limit: u32,
    /// Total matches reported by the board, if any.
    pub total: Option<u64>,
    pub has_more: bool,
    /// True when the board's maximum search depth stops further paging.
    pub depth_limited: bool,
}

/// A post as returned by a remote board, normalized. `raw` keeps the original
/// object so provider-specific data is never lost.
#[derive(Debug, Clone, Serialize)]
pub struct RemotePost {
    pub provider: &'static str,
    pub remote_id: String,
    pub post_url: String,
    pub md5: Option<String>,
    /// One of general / sensitive / questionable / explicit when known.
    pub rating: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub file_url: Option<String>,
    pub file_ext: Option<String>,
    pub file_size: Option<u64>,
    pub sample_url: Option<String>,
    pub preview_url: Option<String>,
    pub preview_width: Option<u32>,
    pub preview_height: Option<u32>,
    pub source: Option<String>,
    pub created_at_raw: Option<String>,
    /// Flat tag list.
    pub tags: Vec<String>,
    /// Categories when the board includes them with the post (Danbooru);
    /// otherwise `None` and `categorize_tags` is used.
    pub categorized: Option<Vec<TagInfo>>,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub max_page_size: u32,
    /// Max number of results reachable by paging (`page * limit`), if capped.
    pub max_result_depth: Option<u32>,
    /// Whether tag categories require a separate lookup.
    pub needs_tag_lookup: bool,
    /// Max tags per search for the current account, if limited.
    pub max_tags: Option<u32>,
}

/// Static description of a supported board, for the provider picker.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// Whether an account (user + API key) is required to search.
    pub requires_account: bool,
    /// Label for the account field ("Username", "User ID").
    pub account_label: &'static str,
    /// Where the user finds their API key.
    pub key_help: &'static str,
    /// Known limitation shown in the picker, if any.
    pub warning: Option<&'static str>,
    /// Whether the multi-select download queue may be used with this board.
    /// Off unless the board's terms have been reviewed and allow it.
    pub allows_batch: bool,
}

pub const PROVIDERS: [ProviderInfo; 3] = [
    ProviderInfo {
        id: danbooru::PROVIDER_ID,
        name: "Danbooru",
        requires_account: false,
        account_label: "Username",
        key_help: "Optional. Danbooru → My Account → API Key. Works without an account (2-tag searches).",
        warning: None,
        allows_batch: true,
    },
    ProviderInfo {
        id: gelbooru::GELBOORU.id,
        name: "Gelbooru",
        requires_account: true,
        account_label: "User ID",
        key_help: "Gelbooru → My Account → Options (the &api_key=…&user_id=… line).",
        warning: Some(
            "Gelbooru's hotlink protection blocks thumbnails, previews and downloads for apps. Search and metadata only.",
        ),
        // Gelbooru TOS forbids automated retrieval; never batch (docs/PLAN.md).
        allows_batch: false,
    },
    ProviderInfo {
        id: gelbooru::RULE34.id,
        name: "Rule34",
        requires_account: true,
        account_label: "User ID",
        key_help: "rule34.xxx → My Account → Options (API access credentials).",
        warning: Some("Untested: image access and tag categories not yet verified."),
        allows_batch: false,
    },
];

pub fn provider_info(id: &str) -> Option<&'static ProviderInfo> {
    PROVIDERS.iter().find(|p| p.id == id)
}

pub const ALL_RATINGS: [&str; 4] = ["general", "sensitive", "questionable", "explicit"];

/// Default rating filter (Gelbooru-style): exclude every disallowed rating.
pub fn exclusion_query(user_tags: &str, allowed: &[String]) -> String {
    let mut parts: Vec<String> = user_tags.split_whitespace().map(str::to_string).collect();
    for r in ALL_RATINGS {
        if !allowed.iter().any(|a| a == r) {
            parts.push(format!("-rating:{r}"));
        }
    }
    parts.join(" ")
}

#[async_trait]
pub trait ImageboardProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    /// Combine the user's tags with the allowed-ratings filter in this
    /// board's syntax.
    fn build_query(&self, user_tags: &str, allowed_ratings: &[String]) -> String {
        exclusion_query(user_tags, allowed_ratings)
    }
    async fn search(&self, query: &SearchQuery) -> AppResult<Page<RemotePost>>;
    async fn get_post(&self, remote_id: &str) -> AppResult<Option<RemotePost>>;
    /// Returns one entry per requested name; unknown names get `Unknown`.
    async fn categorize_tags(&self, names: &[String]) -> AppResult<Vec<TagInfo>>;
    fn post_url(&self, remote_id: &str) -> String;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn exclusion_query_excludes_disallowed() {
        assert_eq!(
            exclusion_query("  long_hair   blue_eyes ", &s(&["general"])),
            "long_hair blue_eyes -rating:sensitive -rating:questionable -rating:explicit"
        );
        assert_eq!(exclusion_query("", &s(&ALL_RATINGS)), "");
    }

    #[test]
    fn provider_list_is_consistent() {
        assert!(provider_info("danbooru").is_some());
        assert!(provider_info("gelbooru").is_some_and(|p| p.warning.is_some()));
        assert!(provider_info("nope").is_none());
    }
}
