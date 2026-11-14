//! Gelbooru-engine boards (dapi): Gelbooru itself, Rule34 and Safebooru.org. Field names
//! were verified live for Gelbooru (docs/PLAN.md, Milestone 0); Rule34 is
//! untested. Parsing is deliberately tolerant.

mod parse;

pub use parse::merge_requested;

use std::sync::Arc;

use async_trait::async_trait;

use super::{Capabilities, ImageboardProvider, Page, RemotePost, SearchQuery, TagInfo};
use crate::error::AppResult;
use crate::net::ApiClient;

/// One Gelbooru-engine site.
#[derive(Debug)]
pub struct Site {
    pub id: &'static str,
    pub api_url: &'static str,
    /// Base for post page links, e.g. `https://gelbooru.com/index.php`.
    pub site_url: &'static str,
    /// Results beyond roughly this many (page * limit) are not served.
    pub max_result_depth: u32,
    /// (our rating, the site's rating word). Gelbooru has four ratings;
    /// older Gelbooru-engine sites (Rule34, Safebooru.org) have three, with
    /// "safe" = our "general" and nothing for "sensitive".
    pub ratings: &'static [(&'static str, &'static str)],
}

const FOUR_RATINGS: &[(&str, &str)] =
    &[("general", "general"), ("sensitive", "sensitive"), ("questionable", "questionable"), ("explicit", "explicit")];
const THREE_RATINGS: &[(&str, &str)] = &[("general", "safe"), ("questionable", "questionable"), ("explicit", "explicit")];

/// Exclude each of the site's ratings the user hasn't allowed.
pub fn build_query(site: &Site, user_tags: &str, allowed: &[String]) -> String {
    let mut parts: Vec<String> = user_tags.split_whitespace().map(str::to_string).collect();
    for (ours, theirs) in site.ratings {
        if !allowed.iter().any(|a| a == ours) {
            parts.push(format!("-rating:{theirs}"));
        }
    }
    parts.join(" ")
}

pub const GELBOORU: Site = Site {
    id: "gelbooru",
    api_url: "https://gelbooru.com/index.php",
    site_url: "https://gelbooru.com/index.php",
    max_result_depth: 20_000,
    ratings: FOUR_RATINGS,
};

pub const RULE34: Site = Site {
    id: "rule34",
    api_url: "https://api.rule34.xxx/index.php",
    site_url: "https://rule34.xxx/index.php",
    max_result_depth: 200_000,
    ratings: THREE_RATINGS,
};

pub const SAFEBOORU_ORG: Site = Site {
    id: "safebooruorg",
    api_url: "https://safebooru.org/index.php",
    site_url: "https://safebooru.org/index.php",
    max_result_depth: 200_000,
    ratings: THREE_RATINGS,
};

pub const MAX_PAGE_SIZE: u32 = 100;
/// Tags per batched lookup request.
const TAG_BATCH: usize = 100;

#[derive(Clone)]
pub struct Credentials {
    pub user_id: String,
    pub api_key: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("user_id", &"REDACTED")
            .field("api_key", &"REDACTED")
            .finish()
    }
}

pub struct GelbooruProvider {
    site: &'static Site,
    client: Arc<ApiClient>,
    /// Required by Gelbooru/Rule34; Safebooru.org works without.
    creds: Option<Credentials>,
}

impl GelbooruProvider {
    pub fn new(site: &'static Site, client: Arc<ApiClient>, creds: Option<Credentials>) -> Self {
        Self { site, client, creds }
    }

    fn base_query(&self, endpoint: &str) -> Vec<(&'static str, String)> {
        let mut q = vec![
            ("page", "dapi".into()),
            ("s", endpoint.into()),
            ("q", "index".into()),
            ("json", "1".into()),
        ];
        if let Some(c) = &self.creds {
            q.push(("api_key", c.api_key.clone()));
            q.push(("user_id", c.user_id.clone()));
        }
        q
    }
}

pub fn post_url(site: &Site, remote_id: &str) -> String {
    format!("{}?page=post&s=view&id={remote_id}", site.site_url)
}

#[async_trait]
impl ImageboardProvider for GelbooruProvider {
    fn id(&self) -> &'static str {
        self.site.id
    }

    fn build_query(&self, user_tags: &str, allowed: &[String]) -> String {
        build_query(self.site, user_tags, allowed)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_page_size: MAX_PAGE_SIZE,
            max_result_depth: Some(self.site.max_result_depth),
            needs_tag_lookup: true,
            max_tags: None,
        }
    }

    async fn search(&self, query: &SearchQuery) -> AppResult<Page<RemotePost>> {
        let limit = query.limit.clamp(1, MAX_PAGE_SIZE);
        // Don't ask for pages the board won't serve.
        if !parse::page_within_depth(self.site, query.page, limit) {
            return Ok(Page {
                items: vec![],
                page: query.page,
                limit,
                total: None,
                has_more: false,
                depth_limited: true,
            });
        }
        let mut q = self.base_query("post");
        q.push(("limit", limit.to_string()));
        q.push(("pid", query.page.to_string()));
        if !query.tags.trim().is_empty() {
            q.push(("tags", query.tags.trim().to_string()));
        }
        let body = self.client.get_json(self.site.api_url, &q).await?;
        parse::posts_page(self.site, &body, query.page, limit)
    }

    async fn get_post(&self, remote_id: &str) -> AppResult<Option<RemotePost>> {
        let mut q = self.base_query("post");
        q.push(("id", remote_id.to_string()));
        let body = self.client.get_json(self.site.api_url, &q).await?;
        Ok(parse::posts_page(self.site, &body, 0, 1)?.items.into_iter().next())
    }

    async fn categorize_tags(&self, names: &[String]) -> AppResult<Vec<TagInfo>> {
        let mut found = Vec::new();
        for chunk in names.chunks(TAG_BATCH) {
            let mut q = self.base_query("tag");
            q.push(("names", chunk.join(" ")));
            q.push(("limit", chunk.len().to_string()));
            let body = self.client.get_json(self.site.api_url, &q).await?;
            found.extend(parse::tags(&body)?);
        }
        Ok(parse::merge_requested(names, found))
    }

    fn post_url(&self, remote_id: &str) -> String {
        post_url(self.site, remote_id)
    }

    async fn suggest_tags(&self, prefix: &str, limit: u32) -> AppResult<Vec<TagInfo>> {
        let mut q = self.base_query("tag");
        // `%` = any run of characters; `_` matches one char, so results
        // are re-filtered to exact prefixes.
        q.push(("name_pattern", format!("{prefix}%")));
        q.push(("orderby", "count".into()));
        q.push(("order", "DESC".into()));
        q.push(("limit", (limit * 2).min(50).to_string()));
        let body = self.client.get_json(self.site.api_url, &q).await?;
        Ok(super::filter_suggestions(prefix, parse::tags(&body)?, limit as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rating_words_per_site() {
        let general = vec!["general".to_string()];
        assert_eq!(build_query(&RULE34, "cat", &general), "cat -rating:questionable -rating:explicit");
        assert_eq!(
            build_query(&GELBOORU, "", &general),
            "-rating:sensitive -rating:questionable -rating:explicit"
        );
        let all: Vec<String> = crate::providers::ALL_RATINGS.iter().map(|s| s.to_string()).collect();
        assert_eq!(build_query(&SAFEBOORU_ORG, "x", &all), "x");
        let explicit_only = vec!["explicit".to_string()];
        assert_eq!(build_query(&RULE34, "", &explicit_only), "-rating:safe -rating:questionable");
    }
}
