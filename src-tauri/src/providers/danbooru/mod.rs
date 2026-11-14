//! Danbooru (danbooru.donmai.us). Posts carry their tag categories inline
//! (`tag_string_artist`, …), so no extra lookup is needed for them.
//! Field names come from open-source clients; verify live (docs/PLAN.md).

mod parse;

use std::sync::Arc;

use async_trait::async_trait;

use super::{Capabilities, ImageboardProvider, Page, RemotePost, SearchQuery, TagInfo};
use crate::error::{AppError, AppResult};
use crate::net::ApiClient;

pub const PROVIDER_ID: &str = "danbooru";
pub const BASE_URL: &str = "https://danbooru.donmai.us";
pub const MAX_PAGE_SIZE: u32 = 200;
/// Danbooru serves numbered pages up to 1000 for regular accounts.
pub const MAX_PAGE_NUMBER: u32 = 1000;
/// Free accounts may search at most this many tags.
pub const FREE_TAG_LIMIT: u32 = 2;
const TAG_BATCH: usize = 100;

#[derive(Clone)]
pub struct Credentials {
    pub login: String,
    pub api_key: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials(REDACTED)")
    }
}

pub struct DanbooruProvider {
    client: Arc<ApiClient>,
    creds: Option<Credentials>,
}

impl DanbooruProvider {
    pub fn new(client: Arc<ApiClient>, creds: Option<Credentials>) -> Self {
        Self { client, creds }
    }

    fn auth(&self) -> Vec<(&'static str, String)> {
        match &self.creds {
            Some(c) => vec![("login", c.login.clone()), ("api_key", c.api_key.clone())],
            None => vec![],
        }
    }
}

pub fn post_url(remote_id: &str) -> String {
    format!("{BASE_URL}/posts/{remote_id}")
}

#[async_trait]
impl ImageboardProvider for DanbooruProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_page_size: MAX_PAGE_SIZE,
            max_result_depth: Some(MAX_PAGE_NUMBER * MAX_PAGE_SIZE),
            needs_tag_lookup: false,
            max_tags: Some(FREE_TAG_LIMIT),
        }
    }

    /// Danbooru accepts a comma list (`rating:g,s`) as a single tag, which
    /// matters with the 2-tag limit.
    fn build_query(&self, user_tags: &str, allowed_ratings: &[String]) -> String {
        parse::build_query(user_tags, allowed_ratings)
    }

    async fn search(&self, query: &SearchQuery) -> AppResult<Page<RemotePost>> {
        let limit = query.limit.clamp(1, MAX_PAGE_SIZE);
        let page_number = query.page + 1; // Danbooru pages are 1-based
        if page_number > MAX_PAGE_NUMBER {
            return Ok(Page {
                items: vec![],
                page: query.page,
                limit,
                total: None,
                has_more: false,
                depth_limited: true,
            });
        }
        let mut q = self.auth();
        q.push(("limit", limit.to_string()));
        q.push(("page", page_number.to_string()));
        if !query.tags.trim().is_empty() {
            q.push(("tags", query.tags.trim().to_string()));
        }
        let body = self.client.get_json(&format!("{BASE_URL}/posts.json"), &q).await?;
        parse::posts_page(&body, query.page, limit)
    }

    async fn get_post(&self, remote_id: &str) -> AppResult<Option<RemotePost>> {
        if !remote_id.chars().all(|c| c.is_ascii_digit()) || remote_id.is_empty() {
            return Ok(None);
        }
        let q = self.auth();
        match self
            .client
            .get_json(&format!("{BASE_URL}/posts/{remote_id}.json"), &q)
            .await
        {
            Ok(body) => Ok(parse::post(&body)),
            Err(AppError::Http(404)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn categorize_tags(&self, names: &[String]) -> AppResult<Vec<TagInfo>> {
        let mut found = Vec::new();
        for chunk in names.chunks(TAG_BATCH) {
            let mut q = self.auth();
            q.push(("search[name_comma]", chunk.join(",")));
            q.push(("limit", chunk.len().to_string()));
            let body = self.client.get_json(&format!("{BASE_URL}/tags.json"), &q).await?;
            found.extend(parse::tags(&body)?);
        }
        Ok(crate::providers::gelbooru::merge_requested(names, found))
    }

    fn post_url(&self, remote_id: &str) -> String {
        post_url(remote_id)
    }

    async fn suggest_tags(&self, prefix: &str, limit: u32) -> AppResult<Vec<TagInfo>> {
        let mut q = self.auth();
        q.push(("search[name_matches]", format!("{prefix}*")));
        q.push(("search[order]", "count".into()));
        q.push(("search[hide_empty]", "yes".into()));
        q.push(("limit", limit.min(50).to_string()));
        let body = self.client.get_json(&format!("{BASE_URL}/tags.json"), &q).await?;
        Ok(super::filter_suggestions(prefix, parse::tags(&body)?, limit as usize))
    }
}
