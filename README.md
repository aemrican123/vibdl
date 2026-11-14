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

## Install as a normal app (Windows)

```powershell
npm run tauri build
```

This produces `src-tauri\target\release\bundle\nsis\VIBDL_<version>_x64-setup.exe`. Run it. It installs for your user only (no admin) and adds VIBDL to the Start menu. The installed app shares settings, library and queue with `npm run tauri dev`.

Desktop shortcut (run once after installing):

```powershell
$exe = Get-ChildItem "$env:LOCALAPPDATA\VIBDL" -Filter *.exe | Where-Object { $_.Name -notmatch '^uninstall' } | Select-Object -First 1
$lnk = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path ([Environment]::GetFolderPath('Desktop')) 'VIBDL.lnk'))
$lnk.TargetPath = $exe.FullName; $lnk.WorkingDirectory = $exe.DirectoryName; $lnk.IconLocation = $exe.FullName; $lnk.Save()
```

App icon source: `src-tauri/app-icon.png` (regenerate sizes with `npm run tauri icon src-tauri/app-icon.png`).

## Layout

```
src/                    UI (SvelteKit, SPA mode)
  lib/api.ts            typed wrappers for Rust commands
  lib/Browse.svelte     online gallery
  lib/Settings.svelte   setup & settings
  lib/Library.svelte    offline library (+ LocalPanel.svelte, LibraryTools.svelte)
  lib/Downloads.svelte  download queue
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
  library/              offline query language, search, facets, local thumbnails, manage (delete/similar/check/export)
  queue/                download queue (sequential worker, retry, cancel)
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
- [x] M7 — Library management: delete (Recycle Bin), refresh metadata, similar-image finder (dHash), library check, export with readable names
- [x] M6 — Download queue: multi-select in Browse, downloaded badges, Downloads tab (sequential, retry, cancel). Enabled per board (Danbooru only; Gelbooru never)
- [x] M5 — Offline Library tab: filters (tags, artist/char/copy, rating, board, type), facets, sort, local thumbnails — **MVP complete**

## Keyboard shortcuts

`/` focus search · `Enter` or double-click open the full-screen viewer (there: `←`/`→` navigate, wheel zoom, drag pan, double-click or `F` fit / `1` 100 %, `+`/`-` zoom, `Esc` close) · `↑`/`↓` + `Tab`/`Enter` pick a tag suggestion · `Esc` close panel / leave select mode · `←`/`→` previous/next item while a panel is open · Ctrl/Shift+click multi-select (Browse)

## Library filter syntax

`long_hair` has tag · `-monochrome` lacks tag · `artist:name` / `char:name` / `copy:name` / `meta:name` tag in a category · `rating:g,s` · `board:danbooru` · `type:png`. All terms are combined with AND. Your Settings rating filter always applies.

## Safety rules baked in

- API key lives only in the OS credential store; `api_key`/`user_id` are redacted from every log and error.
- ≥ 500 ms between API requests (default 1 s), backoff on 429/5xx, hard stop on 401/403.
- Only general-rated content is shown until you opt in.
