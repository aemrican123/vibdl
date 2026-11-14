//! User settings, stored as JSON in the OS app-config folder. Secrets (API
//! keys) are NOT stored here — see `credentials`.

pub mod credentials;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// User has self-attested they are 18+ (shown on first run).
    pub age_confirmed: bool,
    /// Where downloaded originals + sidecars go. `None` until chosen.
    pub library_dir: Option<PathBuf>,
    pub thumb_cache_mb: u32,
    /// Ratings shown in browse/library. Default: general only.
    pub allowed_ratings: Vec<String>,
    /// Gelbooru user ID (not secret; the API key lives in the keychain).
    pub gelbooru_user_id: Option<String>,
    /// Minimum gap between API requests (applies after restart).
    pub api_min_interval_ms: u64,
    pub download_concurrency: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            age_confirmed: false,
            library_dir: None,
            thumb_cache_mb: 500,
            allowed_ratings: vec!["general".into()],
            gelbooru_user_id: None,
            api_min_interval_ms: 1000,
            download_concurrency: 1,
        }
    }
}

impl Settings {
    /// Clamp values to safe ranges so a hand-edited file can't make VIBDL
    /// hammer the API.
    pub fn sanitized(mut self) -> Self {
        self.api_min_interval_ms = self.api_min_interval_ms.max(500);
        self.download_concurrency = self.download_concurrency.clamp(1, 3);
        self.thumb_cache_mb = self.thumb_cache_mb.clamp(50, 10_000);
        self.allowed_ratings.retain(|r| !r.trim().is_empty());
        self.allowed_ratings.dedup();
        self.gelbooru_user_id = self
            .gelbooru_user_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        self
    }
}

pub fn load(path: &Path) -> AppResult<Settings> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Settings>(&text)
            .map(Settings::sanitized)
            .map_err(|e| AppError::Settings(format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e.into()),
    }
}

/// Write atomically: temp file then rename, so a crash can't truncate it.
pub fn save(path: &Path, settings: &Settings) -> AppResult<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(settings)
        .map_err(|e| AppError::Settings(e.to_string()))?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let s = load(&dir.path().join("nope.json")).unwrap();
        assert_eq!(s, Settings::default());
        assert_eq!(s.allowed_ratings, vec!["general".to_string()]);
    }

    #[test]
    fn roundtrip_and_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("settings.json");
        let s = Settings {
            age_confirmed: true,
            gelbooru_user_id: Some("12345".into()),
            ..Default::default()
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap(), s);

        // Older/partial files fill in defaults; unsafe values are clamped.
        fs::write(&path, r#"{"age_confirmed":true,"api_min_interval_ms":10,"download_concurrency":50}"#).unwrap();
        let s = load(&path).unwrap();
        assert!(s.age_confirmed);
        assert_eq!(s.api_min_interval_ms, 500);
        assert_eq!(s.download_concurrency, 3);
        assert_eq!(s.thumb_cache_mb, 500);
    }

    #[test]
    fn settings_never_contain_api_key_field() {
        let text = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!text.contains("api_key"));
    }
}
