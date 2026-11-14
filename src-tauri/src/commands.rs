//! Tauri IPC commands. Thin wrappers: real logic lives in the modules.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::State;

use crate::browse::{self, BrowsePage, PostCache, PostDetails, thumbs::Thumbs};
use crate::error::{AppError, AppResult};
use crate::metadata::{self, TagCache};
use crate::net::ApiClient;
use crate::providers::danbooru::{self, DanbooruProvider};
use crate::providers::gelbooru::{self, GelbooruProvider};
use crate::providers::{self, ImageboardProvider, ProviderInfo, SearchQuery};
use crate::settings::{self, credentials, Settings};

pub struct AppState {
    pub settings_path: PathBuf,
    pub settings: Mutex<Settings>,
    pub api: Arc<ApiClient>,
    pub thumbs: Arc<Thumbs>,
    pub posts: PostCache,
    pub tag_cache: TagCache,
}

fn info(id: &str) -> AppResult<&'static ProviderInfo> {
    providers::provider_info(id).ok_or_else(|| AppError::Settings(format!("Unknown imageboard '{id}'.")))
}

impl AppState {
    fn snapshot(&self) -> Settings {
        self.settings.lock().expect("settings lock poisoned").clone()
    }

    fn update(&self, f: impl FnOnce(&mut Settings)) -> AppResult<Settings> {
        let mut guard = self.settings.lock().expect("settings lock poisoned");
        let mut next = guard.clone();
        f(&mut next);
        let next = next.sanitized();
        settings::save(&self.settings_path, &next)?;
        *guard = next.clone();
        Ok(next)
    }

    /// Build a provider client with whatever credentials are saved for it.
    fn provider(&self, id: &str) -> AppResult<Box<dyn ImageboardProvider>> {
        let meta = info(id)?;
        let account = self.snapshot().accounts.get(id).cloned();
        let key = credentials::get_api_key(id)?;
        let both = account.zip(key);
        if meta.requires_account && both.is_none() {
            return Err(AppError::MissingCredentials(meta.name.into()));
        }
        Ok(match id {
            danbooru::PROVIDER_ID => Box::new(DanbooruProvider::new(
                self.api.clone(),
                both.map(|(login, api_key)| danbooru::Credentials { login, api_key }),
            )),
            _ => {
                let site = if id == gelbooru::RULE34.id { &gelbooru::RULE34 } else { &gelbooru::GELBOORU };
                let (user_id, api_key) = both.expect("checked above");
                Box::new(GelbooruProvider::new(site, self.api.clone(), gelbooru::Credentials { user_id, api_key }))
            }
        })
    }

    fn require_age(&self) -> AppResult<Settings> {
        let s = self.snapshot();
        if !s.age_confirmed {
            return Err(AppError::Settings("Confirm your age in Settings first.".into()));
        }
        Ok(s)
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.snapshot()
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> AppResult<Settings> {
    state.update(|s| *s = settings)
}

#[tauri::command]
pub fn list_providers() -> Vec<ProviderInfo> {
    providers::PROVIDERS.to_vec()
}

#[derive(Serialize)]
pub struct CredentialStatus {
    pub provider: String,
    pub account: Option<String>,
    pub has_api_key: bool,
    pub requires_account: bool,
    /// Ready to search with this board.
    pub ready: bool,
}

fn status(state: &AppState, provider: &str) -> AppResult<CredentialStatus> {
    let meta = info(provider)?;
    let account = state.snapshot().accounts.get(provider).cloned();
    let has_api_key = credentials::get_api_key(provider)?.is_some();
    Ok(CredentialStatus {
        provider: provider.into(),
        ready: !meta.requires_account || (account.is_some() && has_api_key),
        account,
        has_api_key,
        requires_account: meta.requires_account,
    })
}

#[tauri::command]
pub fn credential_status(state: State<'_, AppState>, provider: String) -> AppResult<CredentialStatus> {
    status(&state, &provider)
}

#[tauri::command]
pub fn set_credentials(
    state: State<'_, AppState>,
    provider: String,
    account: String,
    api_key: String,
) -> AppResult<CredentialStatus> {
    info(&provider)?;
    let account = account.trim().to_string();
    let api_key = api_key.trim().to_string();
    if account.is_empty() || api_key.is_empty() {
        return Err(AppError::Settings("Both the account and the API key are required.".into()));
    }
    credentials::set_api_key(&provider, &api_key)?;
    state.update(|s| {
        s.accounts.insert(provider.clone(), account);
    })?;
    tracing::info!(%provider, "credentials saved");
    status(&state, &provider)
}

#[tauri::command]
pub fn clear_credentials(state: State<'_, AppState>, provider: String) -> AppResult<CredentialStatus> {
    info(&provider)?;
    credentials::delete_api_key(&provider)?;
    state.update(|s| {
        s.accounts.remove(&provider);
    })?;
    tracing::info!(%provider, "credentials cleared");
    status(&state, &provider)
}

#[derive(Serialize)]
pub struct ConnectionTest {
    pub provider: String,
    pub authenticated: bool,
    pub total_general_posts: Option<u64>,
    pub sample_post_id: Option<String>,
    pub sample_fields: Vec<String>,
}

/// One read-only request: a single general-rated post.
#[tauri::command]
pub async fn test_connection(state: State<'_, AppState>, provider: String) -> AppResult<ConnectionTest> {
    let st = status(&state, &provider)?;
    let p = state.provider(&provider)?;
    let page = p
        .search(&SearchQuery { tags: p.build_query("", &["general".into()]), page: 0, limit: 1 })
        .await?;
    let first = page.items.first();
    let mut fields: Vec<String> = first
        .and_then(|p| p.raw.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    fields.sort();
    Ok(ConnectionTest {
        provider,
        authenticated: st.account.is_some() && st.has_api_key,
        total_general_posts: page.total,
        sample_post_id: first.map(|p| p.remote_id.clone()),
        sample_fields: fields,
    })
}

/// One page of results from the active board. Ratings the user hasn't
/// allowed are excluded in the query and filtered again here.
#[tauri::command]
pub async fn browse_search(state: State<'_, AppState>, tags: String, page: u32) -> AppResult<BrowsePage> {
    let settings = state.require_age()?;
    let provider = state.provider(&settings.active_provider)?;
    let query = provider.build_query(&tags, &settings.allowed_ratings);
    let result = provider
        .search(&SearchQuery { tags: query, page, limit: browse::PAGE_SIZE })
        .await?;
    Ok(browse::to_browse_page(result, &settings.allowed_ratings, &state.thumbs, &state.posts))
}

/// Full metadata + categorized tags for one post. Uses the post from recent
/// search results when available, inline categories when the board sends
/// them, and the tag cache otherwise.
#[tauri::command]
pub async fn post_details(state: State<'_, AppState>, provider: String, id: String) -> AppResult<PostDetails> {
    let settings = state.require_age()?;
    let p = state.provider(&provider)?;
    let post = match state.posts.get(p.id(), &id) {
        Some(post) => post,
        None => {
            let post = p
                .get_post(&id)
                .await?
                .ok_or_else(|| AppError::Parse(format!("post {id} not found")))?;
            state.posts.put(&post);
            post
        }
    };
    if !browse::rating_allowed(&post, &settings.allowed_ratings) {
        return Err(AppError::Settings("This post's rating is hidden by your settings.".into()));
    }
    let (tags, tag_err, looked_up) = metadata::categorize_post(p.as_ref(), &state.tag_cache, &post).await;
    if let Some(e) = &tag_err {
        tracing::warn!(post = %id, "tag lookup failed: {e}");
    }
    Ok(browse::to_details(post, tags, tag_err, looked_up, &state.thumbs))
}
