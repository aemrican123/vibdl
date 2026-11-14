//! Tauri IPC commands. Thin wrappers: real logic lives in the modules.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::browse::{self, BrowsePage, PostCache, PostDetails, thumbs::Thumbs};
use crate::db::LocalEntry;
use crate::downloads::{self, Downloader, Library, Outcome, RebuildReport};
use crate::error::{AppError, AppResult};
use crate::library;
use crate::queue::{self, Job, JobState, Queue};
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
    pub downloader: Downloader,
    pub default_library_dir: PathBuf,
    /// Opened lazily; reopened if the library folder setting changes.
    pub library: Mutex<Option<Arc<Library>>>,
    /// Limits concurrent local thumbnail generation (CPU/RAM heavy).
    pub thumb_gen: tokio::sync::Semaphore,
    pub queue: Queue,
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

    fn library_root(&self) -> PathBuf {
        self.snapshot().library_dir.unwrap_or_else(|| self.default_library_dir.clone())
    }

    pub fn library(&self) -> AppResult<Arc<Library>> {
        let root = self.library_root();
        let mut guard = self.library.lock().expect("library lock poisoned");
        if let Some(lib) = guard.as_ref().filter(|l| l.root == root) {
            return Ok(lib.clone());
        }
        let lib = Arc::new(Library::open(&root)?);
        *guard = Some(lib.clone());
        Ok(lib)
    }

    /// Post from recent results, or fetched from the board.
    async fn post(&self, p: &dyn ImageboardProvider, id: &str) -> AppResult<crate::providers::RemotePost> {
        if let Some(post) = self.posts.get(p.id(), id) {
            return Ok(post);
        }
        let post = p
            .get_post(id)
            .await?
            .ok_or_else(|| AppError::Parse(format!("post {id} not found")))?;
        self.posts.put(&post);
        Ok(post)
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
    let post = state.post(p.as_ref(), &id).await?;
    if !browse::rating_allowed(&post, &settings.allowed_ratings) {
        return Err(AppError::Settings("This post's rating is hidden by your settings.".into()));
    }
    let local = state
        .library()
        .and_then(|lib| lib.db.find(post.provider, &post.remote_id, post.md5.as_deref()))
        .unwrap_or_else(|e| {
            tracing::warn!("library unavailable: {e}");
            None
        });
    let (tags, tag_err, looked_up) = metadata::categorize_post(p.as_ref(), &state.tag_cache, &post).await;
    if let Some(e) = &tag_err {
        tracing::warn!(post = %id, "tag lookup failed: {e}");
    }
    let mut details = browse::to_details(post, tags, tag_err, looked_up, &state.thumbs);
    details.local = local;
    Ok(details)
}

#[derive(Serialize)]
pub struct LibraryInfo {
    pub root: String,
    pub is_default: bool,
    pub post_count: u64,
}

#[tauri::command]
pub fn library_info(state: State<'_, AppState>) -> AppResult<LibraryInfo> {
    let lib = state.library()?;
    Ok(LibraryInfo {
        root: lib.root.display().to_string(),
        is_default: state.snapshot().library_dir.is_none(),
        post_count: lib.db.post_count()?,
    })
}

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub provider: String,
    pub id: String,
    pub bytes: u64,
    pub total: Option<u64>,
}

/// Download one post's original into the library. Emits `download-progress`.
#[tauri::command]
pub async fn download_post(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: String,
    id: String,
) -> AppResult<Outcome> {
    let settings = state.require_age()?;
    let p = state.provider(&provider)?;
    let post = state.post(p.as_ref(), &id).await?;
    if !browse::rating_allowed(&post, &settings.allowed_ratings) {
        return Err(AppError::Settings("This post's rating is hidden by your settings.".into()));
    }
    let lib = state.library()?;
    let (tags, _, _) = metadata::categorize_post(p.as_ref(), &state.tag_cache, &post).await;
    let (prov, pid) = (provider.clone(), id.clone());
    let never = std::sync::atomic::AtomicBool::new(false);
    state
        .downloader
        .download(&lib, &post, tags, &never, move |bytes, total| {
            let _ = app.emit(
                "download-progress",
                DownloadProgress { provider: prov.clone(), id: pid.clone(), bytes, total },
            );
        })
        .await
}

#[tauri::command]
pub fn local_status(
    state: State<'_, AppState>,
    provider: String,
    id: String,
    md5: Option<String>,
) -> AppResult<Option<LocalEntry>> {
    state.library()?.db.find(&provider, &id, md5.as_deref())
}

/// Open a library file with its default app, or show it in Explorer.
/// Only paths inside the library are accepted.
#[tauri::command]
pub fn open_local(app: AppHandle, state: State<'_, AppState>, local_path: String, reveal: bool) -> AppResult<()> {
    let lib = state.library()?;
    let abs = crate::storage::resolve_inside(&lib.root, std::path::Path::new(&local_path))?;
    if !abs.exists() {
        return Err(AppError::Settings("That file is no longer in the library folder.".into()));
    }
    let result = if reveal {
        app.opener().reveal_item_in_dir(&abs)
    } else {
        app.opener().open_path(abs.display().to_string(), None::<&str>)
    };
    result.map_err(|e| AppError::Settings(format!("couldn't open: {e}")))
}

#[tauri::command]
pub fn open_library_folder(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let lib = state.library()?;
    app.opener()
        .open_path(lib.root.display().to_string(), None::<&str>)
        .map_err(|e| AppError::Settings(format!("couldn't open: {e}")))
}

#[tauri::command]
pub fn library_search(
    state: State<'_, AppState>,
    q: String,
    sort: library::Sort,
    offset: u32,
) -> AppResult<library::SearchResult> {
    let settings = state.require_age()?;
    library::search(&state.library()?.db, &q, sort, offset, &settings.allowed_ratings)
}

#[tauri::command]
pub fn library_facets(state: State<'_, AppState>, q: String) -> AppResult<library::Facets> {
    let settings = state.require_age()?;
    library::facets(&state.library()?.db, &q, &settings.allowed_ratings)
}

#[tauri::command]
pub fn library_details(
    state: State<'_, AppState>,
    provider: String,
    remote_id: String,
) -> AppResult<Option<library::LibraryDetails>> {
    let settings = state.require_age()?;
    let d = library::details(&state.library()?.db, &provider, &remote_id)?;
    Ok(d.filter(|d| {
        d.item.rating.as_ref().is_some_and(|r| settings.allowed_ratings.contains(r))
    }))
}

/// Serve `library` URI requests: `t-<provider>-<md5>` (thumbnail, generated
/// on demand) or `f-<provider>-<md5>` (original file). Only files that are
/// in the index are served.
pub async fn serve_library(state: &AppState, key: &str) -> AppResult<Option<(Vec<u8>, &'static str)>> {
    let Some((kind, rest)) = key.split_once('-') else { return Ok(None) };
    let Some((provider, md5)) = rest.split_once('-') else { return Ok(None) };
    if !crate::storage::is_md5(md5) || providers::provider_info(provider).is_none() {
        return Ok(None);
    }
    let lib = state.library()?;
    let Some(entry) = lib.db.find(provider, "", Some(md5))? else { return Ok(None) };
    let ext = entry.local_path.rsplit('.').next().unwrap_or("").to_string();
    match kind {
        "t" => {
            let _permit = state.thumb_gen.acquire().await.expect("semaphore closed");
            let cache = state.thumbs.dir().join(format!("{provider}-{md5}.thumb"));
            let root = lib.root.clone();
            let (p, m, lp) = (provider.to_string(), md5.to_string(), entry.local_path.clone());
            let path = tauri::async_runtime::spawn_blocking(move || {
                library::thumbs::ensure(&root, &p, &m, &lp, &ext, Some(&cache))
            })
            .await
            .map_err(|e| AppError::Settings(format!("thumbnail task failed: {e}")))??;
            match path {
                Some(p) => Ok(Some((tokio::fs::read(p).await?, "image/jpeg"))),
                None => Ok(None),
            }
        }
        "f" => {
            let abs = crate::storage::resolve_inside(&lib.root, std::path::Path::new(&entry.local_path))?;
            match tokio::fs::read(&abs).await {
                Ok(bytes) => Ok(Some((bytes, library::thumbs::mime_for(&ext)))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            }
        }
        _ => Ok(None),
    }
}

/// Which of these posts are already in the library (same post or same file).
#[derive(serde::Deserialize)]
pub struct StatusQuery {
    pub id: String,
    pub md5: Option<String>,
}

#[tauri::command]
pub fn local_status_many(
    state: State<'_, AppState>,
    provider: String,
    items: Vec<StatusQuery>,
) -> AppResult<Vec<String>> {
    let lib = state.library()?;
    let mut have = Vec::new();
    for it in items.iter().take(1000) {
        if lib.db.find(&provider, &it.id, it.md5.as_deref())?.is_some() {
            have.push(it.id.clone());
        }
    }
    Ok(have)
}

#[derive(serde::Deserialize)]
pub struct QueueItem {
    pub id: String,
    pub thumb_key: Option<String>,
}

/// Queue selected posts for download. Only boards whose terms allow it.
#[tauri::command]
pub fn queue_add(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: String,
    items: Vec<QueueItem>,
) -> AppResult<usize> {
    state.require_age()?;
    let meta = info(&provider)?;
    if !meta.allows_batch {
        return Err(AppError::Rejected(format!(
            "multi-download is disabled for {} (its terms don't allow automated retrieval, or haven't been reviewed).",
            meta.name
        )));
    }
    let items: Vec<(String, Option<String>)> = items.into_iter().map(|i| (i.id, i.thumb_key)).collect();
    let n = state.queue.add(&provider, &items);
    emit_queue(&app, &state.queue);
    Ok(n)
}

#[tauri::command]
pub fn queue_list(state: State<'_, AppState>) -> Vec<Job> {
    state.queue.snapshot()
}

#[tauri::command]
pub fn queue_cancel(app: AppHandle, state: State<'_, AppState>, key: Option<String>) -> usize {
    let n = match key {
        Some(k) => usize::from(state.queue.cancel(&k)),
        None => state.queue.cancel_all(),
    };
    emit_queue(&app, &state.queue);
    n
}

#[tauri::command]
pub fn queue_retry_failed(app: AppHandle, state: State<'_, AppState>) -> usize {
    let n = state.queue.retry_failed();
    emit_queue(&app, &state.queue);
    n
}

#[tauri::command]
pub fn queue_clear_finished(app: AppHandle, state: State<'_, AppState>) {
    state.queue.clear_finished();
    emit_queue(&app, &state.queue);
}

fn emit_queue(app: &AppHandle, q: &Queue) {
    let _ = app.emit("queue-updated", q.snapshot());
}

#[derive(Clone, Serialize)]
struct QueueProgress {
    key: String,
    bytes: u64,
    total: Option<u64>,
}

async fn run_job(app: &AppHandle, state: &AppState, job: &Job, cancel: &std::sync::atomic::AtomicBool) -> AppResult<Outcome> {
    let settings = state.require_age()?;
    let p = state.provider(&job.provider)?;
    let post = state.post(p.as_ref(), &job.id).await?;
    if !browse::rating_allowed(&post, &settings.allowed_ratings) {
        return Err(AppError::Settings("rating hidden by your settings".into()));
    }
    let lib = state.library()?;
    let (tags, _, _) = metadata::categorize_post(p.as_ref(), &state.tag_cache, &post).await;
    let (key, prov, pid) = (job.key.clone(), job.provider.clone(), job.id.clone());
    let app2 = app.clone();
    state
        .downloader
        .download(&lib, &post, tags, cancel, move |bytes, total| {
            let _ = app2.emit("queue-progress", QueueProgress { key: key.clone(), bytes, total });
            let _ = app2.emit(
                "download-progress",
                DownloadProgress { provider: prov.clone(), id: pid.clone(), bytes, total },
            );
        })
        .await
}

/// Background worker: processes queued jobs one at a time, forever.
pub async fn queue_worker(app: AppHandle) {
    loop {
        let state = app.state::<AppState>();
        let Some((job, cancel)) = state.queue.next() else {
            state.queue.wait().await;
            continue;
        };
        emit_queue(&app, &state.queue);
        let result = run_job(&app, &state, &job, &cancel).await;
        match result {
            Ok(Outcome::Downloaded { entry, bytes }) => state.queue.update(&job.key, |j| {
                j.state = JobState::Done;
                j.bytes = bytes;
                j.local_path = Some(entry.local_path);
            }),
            Ok(Outcome::Duplicate { entry }) => state.queue.update(&job.key, |j| {
                j.state = JobState::Duplicate;
                j.local_path = Some(entry.local_path);
            }),
            Err(AppError::Cancelled) => state.queue.update(&job.key, |j| j.state = JobState::Cancelled),
            Err(e) if queue::is_retryable(&e) && job.attempts < queue::MAX_ATTEMPTS && !cancel.load(std::sync::atomic::Ordering::Relaxed) => {
                let wait = std::time::Duration::from_secs(5 * job.attempts as u64);
                tracing::warn!(job = %job.key, attempt = job.attempts, "download failed, retrying in {}s: {e}", wait.as_secs());
                state.queue.update(&job.key, |j| {
                    j.state = JobState::Queued;
                    j.error = Some(format!("retrying: {e}"));
                });
                emit_queue(&app, &state.queue);
                tokio::time::sleep(wait).await;
            }
            Err(e) => {
                tracing::warn!(job = %job.key, "download failed: {e}");
                state.queue.update(&job.key, |j| {
                    j.state = JobState::Failed;
                    j.error = Some(e.to_string());
                });
            }
        }
        emit_queue(&app, &state.queue);
    }
}

/// Rebuild the SQLite index from sidecar files.
#[tauri::command]
pub async fn rebuild_library(state: State<'_, AppState>) -> AppResult<RebuildReport> {
    let lib = state.library()?;
    tauri::async_runtime::spawn_blocking(move || downloads::rebuild(&lib))
        .await
        .map_err(|e| AppError::Settings(format!("rebuild failed: {e}")))
}
