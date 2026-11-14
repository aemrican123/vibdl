//! VIBDL — Vibecoded Image Board Downloader Library.
//!
//! Module map (see docs/PLAN.md):
//!   providers/  imageboard integrations (Danbooru, Gelbooru, Rule34)
//!   browse/     online gallery queries + `thumb` URI scheme / disk cache
//!   net/        shared HTTP client, rate limiting, redaction
//!   settings/   settings file + OS-keychain credentials
//!   commands    Tauri IPC surface
//!   metadata/   tag categorization + tag-type cache
//!   downloads/  original downloads (verify, atomic move) + index rebuild
//!   storage/    library folder layout + sidecar JSON
//!   db/         SQLite index of the library
//!   library/    offline search/filter/facets + local thumbnails
//!   queue/      multi-select download queue (sequential, retry, cancel)

pub mod browse;
pub mod commands;
pub mod db;
pub mod downloads;
pub mod error;
pub mod library;
pub mod metadata;
pub mod net;
pub mod providers;
pub mod queue;
pub mod settings;
pub mod storage;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::http::{Response, header};
use tauri::Manager;

use browse::thumbs::{self, Thumbs};
use commands::AppState;

fn empty(status: u16) -> Response<Vec<u8>> {
    Response::builder().status(status).body(Vec::new()).expect("static response")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vibdl_lib=info,warn".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // UI loads thumbnails and sample previews as thumb://localhost/<key> (http://thumb.localhost/<key>
        // on Windows); the key must have been registered from an API response.
        .register_asynchronous_uri_scheme_protocol("thumb", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let key = request.uri().path().trim_start_matches('/').to_string();
            tauri::async_runtime::spawn(async move {
                let thumbs = app.state::<AppState>().thumbs.clone();
                let resp = match thumbs.get(&key).await {
                    Ok(Some((bytes, mime))) => Response::builder()
                        .status(200)
                        .header(header::CONTENT_TYPE, mime)
                        // Keys are content hashes, so the bytes never change.
                        .header(header::CACHE_CONTROL, "max-age=604800, immutable")
                        .body(bytes)
                        .unwrap_or_else(|_| empty(500)),
                    Ok(None) => empty(404),
                    Err(e) => {
                        tracing::warn!(%key, "thumbnail failed: {e}");
                        empty(502)
                    }
                };
                responder.respond(resp);
            });
        })
        // Offline library images: library://localhost/t-<provider>-<md5> (thumb)
        // and .../f-<provider>-<md5> (original).
        .register_asynchronous_uri_scheme_protocol("library", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let key = request.uri().path().trim_start_matches('/').to_string();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                let resp = match commands::serve_library(&state, &key).await {
                    Ok(Some((bytes, mime))) => Response::builder()
                        .status(200)
                        .header(header::CONTENT_TYPE, mime)
                        .header(header::CACHE_CONTROL, "max-age=604800, immutable")
                        .body(bytes)
                        .unwrap_or_else(|_| empty(500)),
                    Ok(None) => empty(404),
                    Err(e) => {
                        tracing::warn!(%key, "library file failed: {e}");
                        empty(500)
                    }
                };
                responder.respond(resp);
            });
        })
        // Video streaming with Range support: media://localhost/<key>.
        .register_asynchronous_uri_scheme_protocol("media", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let key = request.uri().path().trim_start_matches('/').to_string();
            let range = browse::media::parse_range(
                request.headers().get(header::RANGE).and_then(|v| v.to_str().ok()),
            );
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                let resp = match commands::serve_media(&state, &key, range).await {
                    Ok(Some(m)) => {
                        let mut b = Response::builder()
                            .status(m.status)
                            .header(header::CONTENT_TYPE, m.content_type)
                            .header(header::ACCEPT_RANGES, "bytes")
                            .header(header::CACHE_CONTROL, "no-store");
                        if let Some(cr) = m.content_range {
                            b = b.header(header::CONTENT_RANGE, cr);
                        }
                        b.body(m.bytes).unwrap_or_else(|_| empty(500))
                    }
                    Ok(None) => empty(404),
                    Err(e) => {
                        tracing::warn!(%key, "video stream failed: {e}");
                        empty(502)
                    }
                };
                responder.respond(resp);
            });
        })
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let settings_path = config_dir.join("settings.json");
            let loaded = settings::load(&settings_path).unwrap_or_else(|e| {
                tracing::error!("could not read settings, using defaults: {e}");
                settings::Settings::default()
            });
            let api = net::ApiClient::new(Duration::from_millis(loaded.api_min_interval_ms))?;

            let thumb_dir = app.path().app_cache_dir()?.join("thumbs");
            let thumbs = Thumbs::new(thumb_dir, loaded.thumb_cache_mb, net::build_http_no_redirect()?)?;
            let (dir, cap) = (thumbs.dir().to_path_buf(), thumbs.cap_bytes());
            tauri::async_runtime::spawn_blocking(move || thumbs::sweep(&dir, cap));

            let default_library_dir = app
                .path()
                .picture_dir()
                .map(|p| p.join("VIBDL"))
                .or_else(|_| app.path().app_data_dir().map(|p| p.join("library")))?;

            tracing::info!(path = %settings_path.display(), "settings loaded");
            app.manage(AppState {
                settings_path,
                settings: Mutex::new(loaded),
                api: Arc::new(api),
                thumbs: Arc::new(thumbs),
                posts: browse::PostCache::default(),
                tag_cache: metadata::TagCache::default(),
                downloader: downloads::Downloader::new(net::build_http_no_redirect()?),
                default_library_dir,
                library: Mutex::new(None),
                thumb_gen: tokio::sync::Semaphore::new(2),
                queue: queue::Queue::persistent(config_dir.join("queue.json")),
                suggest_cache: Mutex::new(Default::default()),
            });
            tauri::async_runtime::spawn(commands::queue_worker(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::list_providers,
            commands::credential_status,
            commands::set_credentials,
            commands::clear_credentials,
            commands::test_connection,
            commands::browse_search,
            commands::post_details,
            commands::library_info,
            commands::download_post,
            commands::local_status,
            commands::open_local,
            commands::open_library_folder,
            commands::rebuild_library,
            commands::library_search,
            commands::library_facets,
            commands::library_details,
            commands::local_status_many,
            commands::suggest_tags,
            commands::library_suggest,
            commands::library_delete,
            commands::library_refresh,
            commands::library_similar,
            commands::library_check,
            commands::library_remove_missing,
            commands::library_export,
            commands::library_keys,
            commands::pick_folder,
            commands::library_delete_many,
            commands::library_export_selected,
            commands::library_refresh_many,
            commands::queue_add,
            commands::queue_list,
            commands::queue_cancel,
            commands::queue_retry_failed,
            commands::queue_clear_finished,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
