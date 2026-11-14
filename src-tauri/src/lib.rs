//! VIBDL — Vibecoded Image Board Downloader Library.
//!
//! Module map (see docs/PLAN.md):
//!   providers/  imageboard integrations (Gelbooru first)
//!   browse/     online gallery queries + `thumb` URI scheme / disk cache
//!   net/        shared HTTP client, rate limiting, redaction
//!   settings/   settings file + OS-keychain credentials
//!   commands    Tauri IPC surface
//! Coming in later milestones: metadata/, downloads/, storage/, db/, library/

pub mod browse;
pub mod commands;
pub mod error;
pub mod net;
pub mod providers;
pub mod settings;

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
        // UI loads thumbnails as thumb://localhost/<key> (http://thumb.localhost/<key>
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
                        .header(header::CACHE_CONTROL, "max-age=86400")
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
            let thumbs = Thumbs::new(thumb_dir, loaded.thumb_cache_mb, net::build_http()?)?;
            let (dir, cap) = (thumbs.dir().to_path_buf(), thumbs.cap_bytes());
            tauri::async_runtime::spawn_blocking(move || thumbs::sweep(&dir, cap));

            tracing::info!(path = %settings_path.display(), "settings loaded");
            app.manage(AppState {
                settings_path,
                settings: Mutex::new(loaded),
                api: Arc::new(api),
                thumbs: Arc::new(thumbs),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::gelbooru_credential_status,
            commands::set_gelbooru_credentials,
            commands::clear_gelbooru_credentials,
            commands::test_gelbooru_connection,
            commands::browse_search,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
