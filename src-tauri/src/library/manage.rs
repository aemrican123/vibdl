//! Library management: delete, similar-image detection, integrity check,
//! and export with readable file names. All of it is blocking file work:
//! call from `spawn_blocking`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{LibraryItem, thumbs};
use crate::db::LocalEntry;
use crate::downloads::Library;
use crate::error::{AppError, AppResult};
use crate::storage;

// ---------------------------------------------------------------- delete

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteMode {
    /// Move files to the OS Recycle Bin (what the UI uses).
    RecycleBin,
    /// Remove files permanently (tests only).
    #[cfg(test)]
    Permanent,
}

#[derive(Debug, Default, Serialize)]
pub struct DeleteReport {
    pub removed_from_index: bool,
    pub files_removed: usize,
}

fn remove_file(path: &Path, mode: DeleteMode) -> AppResult<bool> {
    if !path.exists() {
        return Ok(false);
    }
    match mode {
        DeleteMode::RecycleBin => {
            trash::delete(path).map_err(|e| AppError::Settings(format!("couldn't move to Recycle Bin: {e}")))?
        }
        #[cfg(test)]
        DeleteMode::Permanent => std::fs::remove_file(path)?,
    }
    Ok(true)
}

/// Delete an item: original + sidecar + local thumbnail + index row. If the
/// same file is shared by no other entry (it never is: md5 is unique), the
/// files go; the index row goes last so a failure leaves it findable.
pub fn delete(lib: &Library, entry: &LocalEntry, mode: DeleteMode) -> AppResult<DeleteReport> {
    let media = storage::resolve_inside(&lib.root, Path::new(&entry.local_path))?;
    let mut report = DeleteReport::default();
    for path in [
        media.clone(),
        storage::sidecar_path_for(&media),
        thumbs::thumb_path(&lib.root, &entry.provider, &entry.md5),
    ] {
        if remove_file(&path, mode)? {
            report.files_removed += 1;
        }
    }
    report.removed_from_index = lib.db.delete_post(&entry.provider, &entry.remote_id)?;
    Ok(report)
}

// ---------------------------------------------------------------- similar

/// 64-bit difference hash of an image (9×8 grayscale, compare neighbours).
pub fn dhash(path: &Path) -> AppResult<u64> {
    let img = image::open(path).map_err(|e| AppError::Parse(format!("can't read thumbnail: {e}")))?;
    let small = image::imageops::resize(&img.to_luma8(), 9, 8, image::imageops::FilterType::Triangle);
    let mut h = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            let left = small.get_pixel(x, y)[0];
            let right = small.get_pixel(x + 1, y)[0];
            h = (h << 1) | u64::from(left > right);
        }
    }
    Ok(h)
}

/// Bits that differ; ≤ `SIMILAR_MAX_DISTANCE` counts as "looks the same".
pub fn distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

pub const SIMILAR_MAX_DISTANCE: u32 = 6;

/// Group indices whose hashes are within `max` bits of each other
/// (single-link clustering via union-find).
pub fn group_similar(hashes: &[u64], max: u32) -> Vec<Vec<usize>> {
    let n = hashes.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if distance(hashes[i], hashes[j]) <= max {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    groups.into_values().filter(|g| g.len() > 1).collect()
}

#[derive(Debug, Default, Serialize)]
pub struct SimilarReport {
    pub groups: Vec<Vec<LibraryItem>>,
    pub scanned: usize,
    /// Items without a usable thumbnail (e.g. videos never browsed).
    pub skipped: usize,
}

/// Find visually similar items. Hashes are cached in the DB, so only new
/// items are hashed on later runs.
pub fn similar(lib: &Library, online_thumb_dir: &Path, allowed: &[String]) -> AppResult<SimilarReport> {
    let mut known = lib.db.phashes()?;
    let entries = lib.db.all_entries()?;
    let mut report = SimilarReport::default();
    let mut keyed: Vec<(String, u64)> = Vec::new();
    for e in &entries {
        let key = format!("{}:{}", e.provider, e.remote_id);
        let hash = match known.remove(&key) {
            Some(h) => h,
            None => {
                let ext = e.local_path.rsplit('.').next().unwrap_or("");
                let cache = online_thumb_dir.join(format!("{}-{}.thumb", e.provider, e.md5));
                let thumb = thumbs::ensure(&lib.root, &e.provider, &e.md5, &e.local_path, ext, Some(&cache))?;
                let Some(h) = thumb.and_then(|t| dhash(&t).ok()) else {
                    report.skipped += 1;
                    continue;
                };
                lib.db.set_phash(&e.provider, &e.remote_id, h)?;
                h
            }
        };
        keyed.push((key, hash));
    }
    report.scanned = keyed.len();

    let hashes: Vec<u64> = keyed.iter().map(|k| k.1).collect();
    let items = items_by_key(lib, allowed)?;
    for g in group_similar(&hashes, SIMILAR_MAX_DISTANCE) {
        let mut members: Vec<LibraryItem> = g.iter().filter_map(|&i| items.get(&keyed[i].0).cloned()).collect();
        if members.len() > 1 {
            // Biggest (usually best) version first.
            members.sort_by_key(|m| std::cmp::Reverse(m.width.unwrap_or(0) as u64 * m.height.unwrap_or(0) as u64));
            report.groups.push(members);
        }
    }
    Ok(report)
}

fn items_by_key(lib: &Library, allowed: &[String]) -> AppResult<std::collections::HashMap<String, LibraryItem>> {
    let mut map = std::collections::HashMap::new();
    let mut offset = 0;
    loop {
        let page = super::search(&lib.db, "", super::Sort::Oldest, offset, allowed)?;
        if page.items.is_empty() {
            break;
        }
        offset += page.items.len() as u32;
        for it in page.items {
            map.insert(format!("{}:{}", it.provider, it.remote_id), it);
        }
    }
    Ok(map)
}

// ---------------------------------------------------------------- check

#[derive(Debug, Default, Serialize)]
pub struct CheckReport {
    pub total: usize,
    /// Index entries whose original file is gone.
    pub missing: Vec<LocalEntry>,
}

pub fn check(lib: &Library) -> AppResult<CheckReport> {
    let entries = lib.db.all_entries()?;
    let mut r = CheckReport { total: entries.len(), ..Default::default() };
    for e in entries {
        let ok = storage::resolve_inside(&lib.root, Path::new(&e.local_path))
            .map(|p| p.is_file())
            .unwrap_or(false);
        if !ok {
            r.missing.push(e);
        }
    }
    Ok(r)
}

/// Drop index entries whose files are gone (their sidecars too, if any are
/// left behind — otherwise "Rebuild index" would bring them back).
pub fn remove_missing(lib: &Library, mode: DeleteMode) -> AppResult<usize> {
    let report = check(lib)?;
    let mut n = 0;
    for e in &report.missing {
        if let Ok(media) = storage::resolve_inside(&lib.root, Path::new(&e.local_path)) {
            let _ = remove_file(&storage::sidecar_path_for(&media), mode);
        }
        let _ = remove_file(&thumbs::thumb_path(&lib.root, &e.provider, &e.md5), mode);
        if lib.db.delete_post(&e.provider, &e.remote_id)? {
            n += 1;
        }
    }
    Ok(n)
}

// ---------------------------------------------------------------- export

pub const DEFAULT_PATTERN: &str = "{artist} - {copyright} - {board} {id}";

#[derive(Debug, Default, Serialize)]
pub struct ExportReport {
    pub copied: usize,
    pub skipped_existing: usize,
    pub errors: Vec<String>,
    pub dest: String,
}

/// Make a string safe as a Windows/macOS/Linux file name component.
pub fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if trimmed.is_empty() {
        "untitled".into()
    } else if RESERVED.contains(&trimmed.to_ascii_uppercase().as_str()) {
        format!("_{trimmed}")
    } else {
        trimmed
    }
}

fn join_names(list: &[String], max: usize) -> String {
    if list.is_empty() {
        return "unknown".into();
    }
    let mut names: Vec<String> = list.iter().take(max).map(|n| n.replace('_', " ")).collect();
    if list.len() > max {
        names.push(format!("+{}", list.len() - max));
    }
    names.join(", ")
}

/// File name (without extension) from the pattern.
pub fn render_name(pattern: &str, d: &super::LibraryDetails) -> String {
    let mut out = pattern.to_string();
    for (token, value) in [
        ("{artist}", join_names(&d.tags.artist, 2)),
        ("{character}", join_names(&d.tags.character, 2)),
        ("{copyright}", join_names(&d.tags.copyright, 1)),
        ("{board}", d.item.provider.clone()),
        ("{id}", d.item.remote_id.clone()),
        ("{md5}", d.item.md5.clone()),
        ("{rating}", d.item.rating.clone().unwrap_or_default()),
    ] {
        out = out.replace(token, &value.chars().take(80).collect::<String>());
    }
    let mut name = sanitize(&out);
    if name.chars().count() > 150 {
        name = name.chars().take(150).collect::<String>().trim_end().to_string();
    }
    name
}

/// Copy the items matching `q` into `dest` with readable names. Never
/// overwrites: an existing file with the same name and size is skipped,
/// otherwise a " (2)" suffix is added.
pub fn export(lib: &Library, q: &str, allowed: &[String], dest: &Path, pattern: &str) -> AppResult<ExportReport> {
    if !dest.is_absolute() {
        return Err(AppError::Settings("Use a full folder path for the export destination.".into()));
    }
    if dest.starts_with(lib.root.join(".vibdl")) {
        return Err(AppError::Settings("Choose a folder outside VIBDL's internal .vibdl folder.".into()));
    }
    std::fs::create_dir_all(dest)?;
    let pattern = if pattern.trim().is_empty() { DEFAULT_PATTERN } else { pattern };
    let mut report = ExportReport { dest: dest.display().to_string(), ..Default::default() };
    let mut used: HashSet<PathBuf> = HashSet::new();
    let mut offset = 0;
    loop {
        let page = super::search(&lib.db, q, super::Sort::Oldest, offset, allowed)?;
        if page.items.is_empty() {
            break;
        }
        offset += page.items.len() as u32;
        for it in page.items {
            let result = (|| -> AppResult<bool> {
                let d = super::details(&lib.db, &it.provider, &it.remote_id)?
                    .ok_or_else(|| AppError::Parse("item vanished".into()))?;
                let src = storage::resolve_inside(&lib.root, Path::new(&it.local_path))?;
                let base = render_name(pattern, &d);
                let mut n = 1;
                loop {
                    let name = if n == 1 { format!("{base}.{}", it.file_ext) } else { format!("{base} ({n}).{}", it.file_ext) };
                    let target = dest.join(name);
                    if used.contains(&target) {
                        n += 1;
                        continue;
                    }
                    if target.exists() {
                        if target.metadata()?.len() == it.file_size {
                            used.insert(target);
                            return Ok(false);
                        }
                        n += 1;
                        continue;
                    }
                    std::fs::copy(&src, &target)?;
                    used.insert(target);
                    return Ok(true);
                }
            })();
            match result {
                Ok(true) => report.copied += 1,
                Ok(false) => report.skipped_existing += 1,
                Err(e) if report.errors.len() < 20 => report.errors.push(format!("{} #{}: {e}", it.provider, it.remote_id)),
                Err(_) => {}
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::sidecar;
    use crate::metadata::CategorizedTags;

    fn all() -> Vec<String> {
        crate::providers::ALL_RATINGS.iter().map(|s| s.to_string()).collect()
    }

    /// Library with real files: two near-identical images and one different.
    fn lib_with_images() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let gradient = |w: u32, h: u32, invert: bool| {
            image::RgbImage::from_fn(w, h, |x, _| {
                let v = (x * 255 / w) as u8;
                let v = if invert { 255 - v } else { v };
                image::Rgb([v, v, v])
            })
        };
        for (id, md5n, img) in [
            ("1", 1u8, gradient(800, 600, false)),
            ("2", 2u8, gradient(400, 300, false)), // resized copy of #1
            ("3", 3u8, gradient(800, 600, true)),  // different
        ] {
            let md5 = format!("{:032x}", md5n);
            let mut s = sidecar("danbooru", id, &md5);
            s.width = Some(img.width());
            s.height = Some(img.height());
            s.tags = CategorizedTags {
                artist: vec!["some_artist".into()],
                copyright: vec!["a/b:series".into()],
                ..Default::default()
            };
            let rel = storage::relative_media_path("danbooru", &md5, "png").unwrap();
            let abs = lib.root.join(&rel);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            img.save(&abs).unwrap();
            s.file_size = std::fs::metadata(&abs).unwrap().len();
            storage::write_sidecar(&abs, &s).unwrap();
            lib.db.upsert(&s, &rel.to_string_lossy().replace('\\', "/")).unwrap();
        }
        (dir, lib)
    }

    #[test]
    fn hashing_and_grouping() {
        assert_eq!(distance(0b1010, 0b0110), 2);
        let groups = group_similar(&[0, 1, u64::MAX, 3, u64::MAX - 1], 2);
        assert_eq!(groups, vec![vec![0, 1, 3], vec![2, 4]]);
    }

    #[test]
    fn finds_resized_copies_and_caches_hashes() {
        let (_d, lib) = lib_with_images();
        let r = similar(&lib, Path::new("/nonexistent"), &all()).unwrap();
        assert_eq!(r.scanned, 3);
        assert_eq!(r.groups.len(), 1);
        let ids: Vec<&str> = r.groups[0].iter().map(|i| i.remote_id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2"], "largest first");
        assert_eq!(lib.db.phashes().unwrap().len(), 3, "hashes cached");
    }

    #[test]
    fn delete_removes_files_and_entry() {
        let (_d, lib) = lib_with_images();
        let e = lib.db.find("danbooru", "1", None).unwrap().unwrap();
        thumbs::ensure(&lib.root, "danbooru", &e.md5, &e.local_path, "png", None).unwrap();
        let r = delete(&lib, &e, DeleteMode::Permanent).unwrap();
        assert!(r.removed_from_index);
        assert_eq!(r.files_removed, 3);
        assert!(!lib.root.join(&e.local_path).exists());
        assert_eq!(lib.db.post_count().unwrap(), 2);
        assert_eq!(crate::downloads::rebuild(&lib).imported, 2, "sidecar gone too");
    }

    #[test]
    fn check_and_remove_missing() {
        let (_d, lib) = lib_with_images();
        let e = lib.db.find("danbooru", "2", None).unwrap().unwrap();
        std::fs::remove_file(lib.root.join(&e.local_path)).unwrap();
        let c = check(&lib).unwrap();
        assert_eq!((c.total, c.missing.len()), (3, 1));
        assert_eq!(remove_missing(&lib, DeleteMode::Permanent).unwrap(), 1);
        assert!(check(&lib).unwrap().missing.is_empty());
        assert_eq!(crate::downloads::rebuild(&lib).sidecars, 2, "orphan sidecar removed");
    }

    #[test]
    fn export_names_are_readable_safe_and_never_overwrite() {
        let (_d, lib) = lib_with_images();
        let out = tempfile::tempdir().unwrap();
        let r = export(&lib, "", &all(), out.path(), "").unwrap();
        assert_eq!((r.copied, r.skipped_existing), (3, 0), "{:?}", r.errors);
        let mut names: Vec<String> = std::fs::read_dir(out.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names[0], "some artist - a_b_series - danbooru 1.png");
        // Running again skips identical files instead of duplicating them.
        let r2 = export(&lib, "", &all(), out.path(), "").unwrap();
        assert_eq!((r2.copied, r2.skipped_existing), (0, 3));
        // Same name for different items gets a suffix.
        let out2 = tempfile::tempdir().unwrap();
        export(&lib, "", &all(), out2.path(), "{artist}").unwrap();
        assert!(out2.path().join("some artist (2).png").exists());
        assert!(export(&lib, "", &all(), Path::new("relative"), "").is_err());
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize(r#"a<b>c:d"e/f\g|h?i*j"#), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize("  name... "), "name");
        assert_eq!(sanitize("con"), "_con");
        assert_eq!(sanitize(""), "untitled");
    }
}
