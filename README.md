# VIBDL — Vibecoded Image Board Downloader Library

Personal desktop app for browsing imageboards, downloading selected originals with their metadata, and organizing them in a searchable local library. Gelbooru first; more boards later via the provider interface.

Personal use only — not distributed. Full plan: [docs/PLAN.md](docs/PLAN.md).

## Stack

Tauri 2 (Rust core) · SvelteKit + TypeScript UI · SQLite (coming in M4)

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
src-tauri/src/
  commands.rs           Tauri IPC surface
  error.rs              redacted error type
  net/                  HTTP client, rate limiter, retry/backoff, secret redaction
  browse/               gallery queries, rating filter, thumb:// protocol + disk cache
  providers/            ImageboardProvider trait + shared types
    gelbooru/           Gelbooru dapi client + tolerant parser + fixtures
  settings/             settings.json + OS keychain credentials
docs/PLAN.md            feasibility report & roadmap
```

## Status

- [x] M0 — API fields/auth verified live; TOS reviewed (personal use)
- [x] M1 — Skeleton, settings, keychain credentials, rate-limited client, Gelbooru provider
- [x] M2 — Online browser (thumbnail grid, infinite scroll, thumbnail disk cache)
- [ ] M3 — Post detail + categorized tags
- [ ] M4 — Single download + SQLite + sidecars
- [ ] M5 — Local library (MVP)

## Safety rules baked in

- API key lives only in the OS credential store; `api_key`/`user_id` are redacted from every log and error.
- ≥ 500 ms between API requests (default 1 s), backoff on 429/5xx, hard stop on 401/403.
- Only general-rated content is shown until you opt in.
