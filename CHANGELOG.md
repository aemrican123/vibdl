# Changelog

All notable changes to VIBDL. Versions follow [Semantic Versioning](https://semver.org).

## [0.4.0] - 2026-11-14

First public release.

### Changed
- Licensed under MIT OR Apache-2.0.
- New app identifier `io.github.aemrican123.vibdl`. Settings and the download queue are copied over from the old version on first start.
- Multi-download is now allowed on yande.re, Safebooru.org and Rule34 after reviewing their terms.
- The User-Agent now links to the project page.
- A strict Content Security Policy is applied to the app window.

### Removed
- Gelbooru, Konachan and Safebooru (donmai) from the board picker, because they block third-party apps. Library items from them keep working.

## [0.3.x]

Private builds:
- More boards: yande.re, Safebooru.org and Rule34, with each board's own rating vocabulary.
- Folder picker, NSIS installer and app icon.
- Library multi-select with bulk delete, export and refresh.
- Saved searches, a persistent download queue, and Browse opening with the newest posts.
- Full-screen viewer with zoom, pan and video.
- Virtualized grids, database indexes and caching.
- Tag autocomplete and MP4/WebM preview.

## [0.1.0]

Private MVP: Danbooru browsing, MD5-checked downloads with JSON sidecars, SQLite index, offline Library with filters and facets, and the download queue.
