# VIBDL — Vibecoded Image Board Downloader Library

Personal desktop app for browsing imageboards, downloading selected originals with their metadata, and organizing them in a searchable local library. Boards: **Danbooru** (default), Gelbooru (search/metadata only: its hotlink protection blocks images for apps), Rule34 (untested). Pick one in Settings.

Personal use only — not distributed. Full plan: [docs/PLAN.md](docs/PLAN.md).

## Stack

Tauri 2 (Rust core) · SvelteKit + TypeScript UI · SQLite

## Develop

```powershell
npm install
npm run tauri dev               # run the app
cd src-tauri; cargo test        # Rust unit tests
npm run check                   # Svelte/TS type check
```

## Layout

```
src/                    UI (SvelteKit, SPA mode)
  lib/api.ts            typed wrappers for Rust commands
  lib/Browse.svelte     online gallery
  lib/Settings.svelte   setup & settings
  lib/Library.svelte    offline library (+ LocalPanel.svelte)
src-tauri/src/
  commands.rs           Tauri IPC surface
  error.rs              redacted error type
  net/                  HTTP client, rate limiter, retry/backoff, secret redaction
  browse/               gallery queries, rating filter, thumb:// protocol + disk cache
  providers/            ImageboardProvider trait + shared types
    danbooru/           Danbooru client (inline tag categories) + parser + fixtures
    gelbooru/           Gelbooru-engine client (Gelbooru, Rule34) + parser + fixtures
  metadata/             tag categorization + cache
  downloads/            original downloads + index rebuild
  storage/              library folder layout + sidecar JSON
  db/                   SQLite index
  library/              offline query language, search, facets, local thumbnails
  settings/             settings.json + OS keychain credentials
docs/PLAN.md            feasibility report & roadmap
```

## Status

- [x] M0 — API fields/auth verified live; TOS reviewed (personal use)
- [x] M1 — Skeleton, settings, keychain credentials, rate-limited client, Gelbooru provider
- [x] M2 — Online browser (thumbnail grid, infinite scroll, thumbnail disk cache)
- [x] M3 — Post detail + categorized tags + sample preview
- [x] Provider picker (Danbooru / Gelbooru / Rule34); Gelbooru images blocked by hotlink protection
- [x] M4 — Single download (MD5-verified, atomic) + SQLite index + sidecars + rebuild
- [x] M5 — Offline Library tab: filters (tags, artist/char/copy, rating, board, type), facets, sort, local thumbnails — **MVP complete**

## Library filter syntax

`long_hair` has tag · `-monochrome` lacks tag · `artist:name` / `char:name` / `copy:name` / `meta:name` tag in a category · `rating:g,s` · `board:danbooru` · `type:png`. All terms are combined with AND. Your Settings rating filter always applies.

## Safety rules baked in

- API key lives only in the OS credential store; `api_key`/`user_id` are redacted from every log and error.
- ≥ 500 ms between API requests (default 1 s), backoff on 429/5xx, hard stop on 401/403.
- Only general-rated content is shown until you opt in.
