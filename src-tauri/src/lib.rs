//! VIBDL — Vibecoded Image Board Downloader Library.
//!
//! Module map (see docs/PLAN.md):
//!   providers/  imageboard integrations (Gelbooru first)
//!   net/        shared HTTP client, rate limiting, redaction
//!   settings/   settings file + OS-keychain credentials
//!   commands    Tauri IPC surface
//! Coming in later milestones: browse/, metadata/, downloads/, storage/, db/, library/

pub mod commands;
pub mod error;
pub mod net;
pub mod providers;
pub mod settings;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::Manager;

use commands::AppState;

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
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let settings_path = config_dir.join("settings.json");
            let loaded = settings::load(&settings_path).unwrap_or_else(|e| {
                tracing::error!("could not read settings, using defaults: {e}");
                settings::Settings::default()
            });
            let api = net::ApiClient::new(Duration::from_millis(loaded.api_min_interval_ms))?;
            tracing::info!(path = %settings_path.display(), "settings loaded");
            app.manage(AppState {
                settings_path,
                settings: Mutex::new(loaded),
                api: Arc::new(api),
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
