//! Imageboard provider abstraction. Each board lives in its own submodule and
//! maps its API onto these shared types. Adding a board = implementing
//! `ImageboardProvider`; nothing else in the app should need to change.

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
    /// Raw tag query in the board's own syntax.
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
    /// Board's own rating string, stored as-is (vocabularies differ).
    pub rating: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub file_url: Option<String>,
    pub file_ext: Option<String>,
    pub sample_url: Option<String>,
    pub preview_url: Option<String>,
    pub preview_width: Option<u32>,
    pub preview_height: Option<u32>,
    pub source: Option<String>,
    pub created_at_raw: Option<String>,
    /// Flat tag list; categories come from `categorize_tags`.
    pub tags: Vec<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub max_page_size: u32,
    /// Max number of results reachable by paging (`page * limit`), if capped.
    pub max_result_depth: Option<u32>,
    /// Whether tag categories require a separate lookup.
    pub needs_tag_lookup: bool,
}

#[async_trait]
pub trait ImageboardProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    async fn search(&self, query: &SearchQuery) -> AppResult<Page<RemotePost>>;
    async fn get_post(&self, remote_id: &str) -> AppResult<Option<RemotePost>>;
    /// Returns one entry per requested name; unknown names get `Unknown`.
    async fn categorize_tags(&self, names: &[String]) -> AppResult<Vec<TagInfo>>;
    fn post_url(&self, remote_id: &str) -> String;
}
