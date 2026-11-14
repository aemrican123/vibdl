//! Imageboard provider abstraction. Each board lives in its own submodule and
//! maps its API onto these shared types. Adding a board = implementing
//! `ImageboardProvider` and listing it in `PROVIDERS`.

pub mod danbooru;
pub mod gelbooru;
pub mod moebooru;

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
    /// Whether VIBDL sends an account to this board at all (if not, Settings
    /// hides the account fields).
    pub uses_account: bool,
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

/// Boards in the picker. Removed (2026-11-14) because they block app access:
/// Gelbooru (hotlink protection), Konachan and Safebooru/donmai (HTTP 403).
/// Their code stays, and their ids stay in `RETIRED` so library items from
/// them keep working.
pub const PROVIDERS: [ProviderInfo; 4] = [
    ProviderInfo {
        id: danbooru::PROVIDER_ID,
        name: "Danbooru",
        requires_account: false,
        uses_account: true,
        account_label: "Username",
        key_help: "Optional. Danbooru → My Account → API Key. Works without an account (2-tag searches).",
        warning: None,
        allows_batch: true,
    },
    ProviderInfo {
        id: moebooru::YANDERE.id,
        name: "yande.re",
        requires_account: false,
        uses_account: false,
        account_label: "Username",
        key_help: "No account needed.",
        warning: None,
        allows_batch: true,
    },
    ProviderInfo {
        id: gelbooru::SAFEBOORU_ORG.id,
        name: "Safebooru.org",
        requires_account: false,
        uses_account: false,
        account_label: "User ID",
        key_help: "No account needed.",
        warning: None,
        allows_batch: true,
    },
    ProviderInfo {
        id: gelbooru::RULE34.id,
        name: "Rule34",
        requires_account: true,
        uses_account: true,
        account_label: "User ID",
        key_help: "rule34.xxx → My Account → Options (API access credentials).",
        warning: Some("Adult content board. Needs your own API key."),
        allows_batch: true,
    },
];

pub fn provider_info(id: &str) -> Option<&'static ProviderInfo> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// Board ids that may appear in an existing library but aren't selectable.
pub const RETIRED: [&str; 3] = ["gelbooru", "konachan", "safebooru"];

/// Any board id that can appear in the library (current or retired).
pub fn is_known_board(id: &str) -> bool {
    provider_info(id).is_some() || RETIRED.contains(&id)
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
    /// Tags starting with `prefix`, most-used first (search suggestions).
    async fn suggest_tags(&self, prefix: &str, limit: u32) -> AppResult<Vec<TagInfo>> {
        let _ = (prefix, limit);
        Ok(vec![])
    }
}

/// Normalize the word being typed into a suggestion prefix: lowercase, no
/// leading `-`/`~`, no spaces. `None` = don't suggest.
pub fn suggestion_prefix(raw: &str) -> Option<String> {
    let p = raw.trim().trim_start_matches(['-', '~']).to_lowercase();
    let ok = p.chars().count() >= 2 && p.len() <= 64 && !p.contains(char::is_whitespace) && !p.contains('*');
    ok.then_some(p)
}

/// Keep only real prefix matches (board wildcards are looser), best first.
pub fn filter_suggestions(prefix: &str, mut tags: Vec<TagInfo>, limit: usize) -> Vec<TagInfo> {
    tags.retain(|t| t.name.to_lowercase().starts_with(prefix));
    tags.sort_by(|a, b| b.count.unwrap_or(0).cmp(&a.count.unwrap_or(0)).then(a.name.cmp(&b.name)));
    tags.dedup_by(|a, b| a.name == b.name);
    tags.truncate(limit);
    tags
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
    fn suggestion_prefixes_and_filtering() {
        assert_eq!(suggestion_prefix(" -Long").as_deref(), Some("long"));
        assert_eq!(suggestion_prefix("l"), None);
        assert_eq!(suggestion_prefix("a*b"), None);
        let t = |n: &str, c: u64| TagInfo { name: n.into(), category: TagCategory::General, count: Some(c) };
        let out = filter_suggestions("long_h", vec![t("long_hair", 9), t("longxhair", 99), t("long_hair_x", 3), t("long_hair", 9)], 10);
        let names: Vec<&str> = out.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["long_hair", "long_hair_x"]);
    }

    #[test]
    fn provider_list_is_consistent() {
        assert!(provider_info("danbooru").is_some());
        assert!(provider_info("gelbooru").is_none(), "retired");
        assert!(is_known_board("gelbooru") && is_known_board("danbooru") && !is_known_board("nope"));
        assert!(provider_info("nope").is_none());
        let mut ids: Vec<&str> = PROVIDERS.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PROVIDERS.len(), "ids are unique");
        for p in PROVIDERS {
            // Ids are used in thumbnail keys and folder names.
            assert!(
                p.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) && p.id.len() <= 16,
                "{}",
                p.id
            );
            assert!(crate::browse::thumbs::make_key(p.id, "0123456789abcdef0123456789abcdef").is_some(), "{}", p.id);
        }
    }
}
