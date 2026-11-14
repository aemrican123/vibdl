//! On-disk library layout and sidecar files.
//!
//! ```text
//! <root>/<provider>/<md5[0..2]>/<md5>.<ext>          original, byte-for-byte
//! <root>/<provider>/<md5[0..2]>/<md5>.vibdl.json     sidecar (source of truth)
//! <root>/.vibdl/library.sqlite                       index (rebuildable)
//! ```

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::metadata::CategorizedTags;

pub const SIDECAR_SUFFIX: &str = ".vibdl.json";
pub const SIDECAR_VERSION: u32 = 1;

pub fn is_md5(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn is_safe_segment(s: &str) -> bool {
    !s.is_empty() && s.len() <= 16 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Relative path (from the library root) for an original file.
pub fn relative_media_path(provider: &str, md5: &str, ext: &str) -> AppResult<PathBuf> {
    let ext = ext.to_ascii_lowercase();
    if !is_safe_segment(provider) || !is_md5(md5) || !is_safe_segment(&ext) || ext.len() > 5 {
        return Err(AppError::Parse("unsafe file name components".into()));
    }
    Ok(PathBuf::from(provider).join(&md5[..2]).join(format!("{md5}.{ext}")))
}

pub fn sidecar_path_for(media: &Path) -> PathBuf {
    let stem = media.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
    media.with_file_name(format!("{stem}{SIDECAR_SUFFIX}"))
}

/// Resolve `rel` under `root`, refusing anything that could escape it.
pub fn resolve_inside(root: &Path, rel: &Path) -> AppResult<PathBuf> {
    if rel.is_absolute() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(AppError::Settings("path is outside the library".into()));
    }
    Ok(root.join(rel))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    pub vibdl_sidecar: u32,
    pub provider: String,
    pub remote_id: String,
    pub post_url: String,
    pub md5: String,
    pub rating: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub file_ext: String,
    pub file_size: u64,
    pub source: Option<String>,
    pub file_url: Option<String>,
    pub created_at: Option<String>,
    /// RFC 3339, UTC.
    pub downloaded_at: String,
    pub tags: CategorizedTags,
    /// Everything the board returned for the post.
    pub raw: Value,
}

/// Atomic write: temp file then rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn write_sidecar(media_abs: &Path, sidecar: &Sidecar) -> AppResult<PathBuf> {
    let path = sidecar_path_for(media_abs);
    let text = serde_json::to_vec_pretty(sidecar).map_err(|e| AppError::Parse(e.to_string()))?;
    write_atomic(&path, &text)?;
    Ok(path)
}

pub fn read_sidecar(path: &Path) -> AppResult<Sidecar> {
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| AppError::Parse(format!("{}: {e}", path.display())))
}

/// All sidecar files under `root/<provider>/<xx>/` (skips `.vibdl/`).
pub fn find_sidecars(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(providers) = fs::read_dir(root) else {
        return out;
    };
    for p in providers.flatten() {
        let pp = p.path();
        if !pp.is_dir() || p.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        for shard in fs::read_dir(&pp).into_iter().flatten().flatten() {
            if !shard.path().is_dir() {
                continue;
            }
            for f in fs::read_dir(shard.path()).into_iter().flatten().flatten() {
                if f.file_name().to_string_lossy().ends_with(SIDECAR_SUFFIX) {
                    out.push(f.path());
                }
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD5: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn media_paths_are_sharded_and_safe() {
        assert_eq!(
            relative_media_path("danbooru", MD5, "PNG").unwrap(),
            PathBuf::from("danbooru").join("01").join(format!("{MD5}.png"))
        );
        assert!(relative_media_path("../x", MD5, "png").is_err());
        assert!(relative_media_path("danbooru", "nothex", "png").is_err());
        assert!(relative_media_path("danbooru", MD5, "p/ng").is_err());
    }

    #[test]
    fn sidecar_sits_next_to_media() {
        let m = PathBuf::from("lib").join("danbooru").join("01").join(format!("{MD5}.png"));
        assert_eq!(sidecar_path_for(&m).file_name().unwrap().to_str().unwrap(), format!("{MD5}.vibdl.json"));
    }

    #[test]
    fn resolve_refuses_escapes() {
        let root = Path::new("root");
        assert!(resolve_inside(root, Path::new("danbooru/01/a.png")).is_ok());
        assert!(resolve_inside(root, Path::new("../secret")).is_err());
        assert!(resolve_inside(root, Path::new("/etc/passwd")).is_err());
    }
}
