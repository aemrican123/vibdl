//! Tauri IPC commands. Thin wrappers: real logic lives in the modules.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::State;

use crate::browse::{self, BrowsePage, thumbs::Thumbs};
use crate::error::{AppError, AppResult};
use crate::net::ApiClient;
use crate::providers::gelbooru::{self, Credentials, GelbooruProvider};
use crate::providers::{ImageboardProvider, SearchQuery};
use crate::settings::{self, credentials, Settings};

pub struct AppState {
    pub settings_path: PathBuf,
    pub settings: Mutex<Settings>,
    pub api: Arc<ApiClient>,
    pub thumbs: Arc<Thumbs>,
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

    fn gelbooru(&self) -> AppResult<GelbooruProvider> {
        let user_id = self
            .snapshot()
            .gelbooru_user_id
            .ok_or_else(|| AppError::MissingCredentials("Gelbooru".into()))?;
        let api_key = credentials::get_api_key(gelbooru::PROVIDER_ID)?
            .ok_or_else(|| AppError::MissingCredentials("Gelbooru".into()))?;
        Ok(GelbooruProvider::new(self.api.clone(), Credentials { user_id, api_key }))
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

#[derive(Serialize)]
pub struct CredentialStatus {
    pub has_user_id: bool,
    pub has_api_key: bool,
}

#[tauri::command]
pub fn gelbooru_credential_status(state: State<'_, AppState>) -> AppResult<CredentialStatus> {
    Ok(CredentialStatus {
        has_user_id: state.snapshot().gelbooru_user_id.is_some(),
        has_api_key: credentials::get_api_key(gelbooru::PROVIDER_ID)?.is_some(),
    })
}

#[tauri::command]
pub fn set_gelbooru_credentials(
    state: State<'_, AppState>,
    user_id: String,
    api_key: String,
) -> AppResult<CredentialStatus> {
    let user_id = user_id.trim().to_string();
    let api_key = api_key.trim().to_string();
    if user_id.is_empty() || api_key.is_empty() {
        return Err(AppError::Settings("Both user ID and API key are required.".into()));
    }
    credentials::set_api_key(gelbooru::PROVIDER_ID, &api_key)?;
    state.update(|s| s.gelbooru_user_id = Some(user_id))?;
    tracing::info!("Gelbooru credentials saved");
    gelbooru_credential_status(state)
}

#[tauri::command]
pub fn clear_gelbooru_credentials(state: State<'_, AppState>) -> AppResult<()> {
    credentials::delete_api_key(gelbooru::PROVIDER_ID)?;
    state.update(|s| s.gelbooru_user_id = None)?;
    tracing::info!("Gelbooru credentials cleared");
    Ok(())
}

#[derive(Serialize)]
pub struct ConnectionTest {
    pub total_general_posts: Option<u64>,
    pub sample_post_id: Option<String>,
    pub sample_fields: Vec<String>,
}

/// One read-only request: a single general-rated post.
#[tauri::command]
pub async fn test_gelbooru_connection(state: State<'_, AppState>) -> AppResult<ConnectionTest> {
    let provider = state.gelbooru()?;
    let page = provider
        .search(&SearchQuery { tags: "rating:general".into(), page: 0, limit: 1 })
        .await?;
    let first = page.items.first();
    let mut fields: Vec<String> = first
        .and_then(|p| p.raw.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    fields.sort();
    Ok(ConnectionTest {
        total_general_posts: page.total,
        sample_post_id: first.map(|p| p.remote_id.clone()),
        sample_fields: fields,
    })
}

/// One page of Gelbooru results for the online browser. Ratings the user
/// hasn't allowed are excluded in the query and filtered again here.
#[tauri::command]
pub async fn browse_search(
    state: State<'_, AppState>,
    tags: String,
    page: u32,
) -> AppResult<BrowsePage> {
    let settings = state.snapshot();
    if !settings.age_confirmed {
        return Err(AppError::Settings("Confirm your age in Settings first.".into()));
    }
    let provider = state.gelbooru()?;
    let query = browse::build_query(&tags, &settings.allowed_ratings);
    let result = provider
        .search(&SearchQuery { tags: query, page, limit: browse::PAGE_SIZE })
        .await?;
    Ok(browse::to_browse_page(result, &settings.allowed_ratings, &state.thumbs))
}
