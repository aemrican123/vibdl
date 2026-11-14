//! Gelbooru provider (dapi). Field names and limits come from open-source
//! clients and are marked UNVERIFIED in docs/PLAN.md until checked against a
//! real response (Milestone 0). Parsing is deliberately tolerant.

mod parse;

use std::sync::Arc;

use async_trait::async_trait;

use super::{Capabilities, ImageboardProvider, Page, RemotePost, SearchQuery, TagInfo};
use crate::error::AppResult;
use crate::net::ApiClient;

pub const PROVIDER_ID: &str = "gelbooru";
pub const API_URL: &str = "https://gelbooru.com/index.php";
pub const MAX_PAGE_SIZE: u32 = 100;
/// Results beyond roughly this many (page * limit) are not served.
pub const MAX_RESULT_DEPTH: u32 = 20_000;
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
    client: Arc<ApiClient>,
    creds: Credentials,
}

impl GelbooruProvider {
    pub fn new(client: Arc<ApiClient>, creds: Credentials) -> Self {
        Self { client, creds }
    }

    fn base_query(&self, endpoint: &str) -> Vec<(&'static str, String)> {
        vec![
            ("page", "dapi".into()),
            ("s", endpoint.into()),
            ("q", "index".into()),
            ("json", "1".into()),
            ("api_key", self.creds.api_key.clone()),
            ("user_id", self.creds.user_id.clone()),
        ]
    }
}

pub fn post_url(remote_id: &str) -> String {
    format!("{API_URL}?page=post&s=view&id={remote_id}")
}

#[async_trait]
impl ImageboardProvider for GelbooruProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_page_size: MAX_PAGE_SIZE,
            max_result_depth: Some(MAX_RESULT_DEPTH),
            needs_tag_lookup: true,
        }
    }

    async fn search(&self, query: &SearchQuery) -> AppResult<Page<RemotePost>> {
        let limit = query.limit.clamp(1, MAX_PAGE_SIZE);
        // Don't ask for pages the board won't serve.
        if !parse::page_within_depth(query.page, limit) {
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
        let body = self.client.get_json(API_URL, &q).await?;
        parse::posts_page(&body, query.page, limit)
    }

    async fn get_post(&self, remote_id: &str) -> AppResult<Option<RemotePost>> {
        let mut q = self.base_query("post");
        q.push(("id", remote_id.to_string()));
        let body = self.client.get_json(API_URL, &q).await?;
        Ok(parse::posts_page(&body, 0, 1)?.items.into_iter().next())
    }

    async fn categorize_tags(&self, names: &[String]) -> AppResult<Vec<TagInfo>> {
        let mut found = Vec::new();
        for chunk in names.chunks(TAG_BATCH) {
            let mut q = self.base_query("tag");
            q.push(("names", chunk.join(" ")));
            q.push(("limit", chunk.len().to_string()));
            let body = self.client.get_json(API_URL, &q).await?;
            found.extend(parse::tags(&body)?);
        }
        Ok(parse::merge_requested(names, found))
    }

    fn post_url(&self, remote_id: &str) -> String {
        post_url(remote_id)
    }
}
