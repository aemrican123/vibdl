//! Local thumbnails for the offline library, stored in
//! `<root>/.vibdl/thumbs/<provider>-<md5>.jpg`. Generated on first view from
//! the original (still images), or copied from the online thumbnail cache
//! (videos/animations), so the library works fully offline.

use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;

use crate::error::{AppError, AppResult};
use crate::storage;

pub const THUMB_PX: u32 = 360;
const STILL_EXTS: [&str; 5] = ["jpg", "jpeg", "png", "gif", "webp"];

pub fn thumb_path(root: &Path, provider: &str, md5: &str) -> PathBuf {
    root.join(".vibdl").join("thumbs").join(format!("{provider}-{md5}.jpg"))
}

pub fn is_still(ext: &str) -> bool {
    STILL_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn mime_for(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

fn generate(original: &Path, out: &Path) -> AppResult<()> {
    let img = image::ImageReader::open(original)?
        .with_guessed_format()?
        .decode()
        .map_err(|e| AppError::Parse(format!("can't decode image: {e}")))?;
    let small = img.thumbnail(THUMB_PX, THUMB_PX).to_rgb8();
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = out.with_extension("tmp");
    {
        let file = std::fs::File::create(&tmp)?;
        let mut w = std::io::BufWriter::new(file);
        image::DynamicImage::ImageRgb8(small)
            .write_with_encoder(JpegEncoder::new_with_quality(&mut w, 85))
            .map_err(|e| AppError::Parse(format!("can't encode thumbnail: {e}")))?;
    }
    std::fs::rename(&tmp, out)?;
    Ok(())
}

/// Path to a thumbnail for a library item, creating it if needed.
/// `online_cache` is the browse thumbnail file for this post, if any.
/// Blocking: call from `spawn_blocking`.
pub fn ensure(
    root: &Path,
    provider: &str,
    md5: &str,
    local_path: &str,
    ext: &str,
    online_cache: Option<&Path>,
) -> AppResult<Option<PathBuf>> {
    let out = thumb_path(root, provider, md5);
    if out.is_file() {
        return Ok(Some(out));
    }
    let original = storage::resolve_inside(root, Path::new(local_path))?;
    if is_still(ext) && original.is_file() {
        match generate(&original, &out) {
            Ok(()) => return Ok(Some(out)),
            Err(e) => tracing::warn!(%md5, "thumbnail generation failed: {e}"),
        }
    }
    if let Some(cached) = online_cache.filter(|p| p.is_file()) {
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::copy(cached, &out)?;
        return Ok(Some(out));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD5: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn generates_and_reuses_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = format!("danbooru/01/{MD5}.png");
        let orig = root.join(&rel);
        std::fs::create_dir_all(orig.parent().unwrap()).unwrap();
        image::RgbaImage::from_pixel(1200, 600, image::Rgba([200, 30, 30, 255])).save(&orig).unwrap();

        let t = ensure(root, "danbooru", MD5, &rel, "png", None).unwrap().unwrap();
        let thumb = image::open(&t).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (360, 180), "keeps aspect ratio");
        let mtime = std::fs::metadata(&t).unwrap().modified().unwrap();
        ensure(root, "danbooru", MD5, &rel, "png", None).unwrap();
        assert_eq!(std::fs::metadata(&t).unwrap().modified().unwrap(), mtime, "not regenerated");
    }

    #[test]
    fn videos_use_online_cache_copy_or_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = format!("danbooru/01/{MD5}.mp4");
        assert!(ensure(root, "danbooru", MD5, &rel, "mp4", None).unwrap().is_none());
        let cached = root.join("cached.thumb");
        std::fs::write(&cached, [0xFF, 0xD8, 0xFF]).unwrap();
        let t = ensure(root, "danbooru", MD5, &rel, "mp4", Some(&cached)).unwrap().unwrap();
        assert_eq!(std::fs::read(t).unwrap(), vec![0xFF, 0xD8, 0xFF]);
    }

    #[test]
    fn refuses_paths_outside_library() {
        let dir = tempfile::tempdir().unwrap();
        assert!(ensure(dir.path(), "danbooru", MD5, "../evil.png", "png", None).is_err());
    }
}
