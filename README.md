# VIBDL

**Vibecoded Image Board Downloader Library.** A desktop app for browsing imageboards, saving the posts you like together with their tags, and keeping them in a searchable offline library.

Windows · Tauri 2 (Rust) · SvelteKit · SQLite · MIT OR Apache-2.0

---

## What it does

- **Browse** a board's newest posts as soon as the app opens. Search with tags, get tag suggestions while you type, and preview images and MP4/WebM videos in a full-screen viewer with zoom and pan.
- **Download** the original file of one post, or select many posts and send them to the download queue. Every file is checked against the board's MD5 hash before it is kept.
- **Keep the metadata.** Each file gets a small JSON sidecar with its tags (sorted into artist, character, copyright, general and meta), rating, source and post link.
- **Library** works fully offline. Filter by tag, category, rating, board or file type, sort the results, and see tag counts per category. Select many items with Shift or Ctrl and delete, export or refresh them in bulk.
- **Library tools**: find visually similar images, check for missing or changed files, export a selection to another folder, and rebuild the index from the sidecars.
- **Saved searches** and a download queue that survives restarts.

VIBDL is a client for each board's public API. It does not scrape web pages, and it does not get around rate limits, logins, hotlink protection or any other access control.

## Supported boards

| Board | Account | Notes |
|---|---|---|
| [Danbooru](https://danbooru.donmai.us) | Optional | Default board. Without an account, searches are limited to 2 tags. Add your username and API key in Settings to raise the limit. |
| [yande.re](https://yande.re) | No | |
| [Safebooru.org](https://safebooru.org) | No | Safe-for-work images only. |
| [Rule34](https://rule34.xxx) | Required | Adult content. Needs your own user ID and API key from rule34.xxx → My Account → Options. |

All four allow multi-download through the queue.

### Boards that were removed

These were tried and taken out because they block third-party apps:

- **Gelbooru**: its hotlink protection blocks images for apps. VIBDL will not fake browser headers to get around it.
- **Konachan** and **Safebooru (donmai)**: they answer API requests from apps with HTTP 403.

If you downloaded posts from these boards with an older version, those items still show up and work in your Library.

## Install

1. Open the [Releases](https://github.com/aemrican123/vibdl/releases) page and download `VIBDL_<version>_x64-setup.exe`.
2. Run it. It installs for your Windows user only, so no admin rights are needed, and adds VIBDL to the Start menu and the desktop.
3. Windows SmartScreen may warn you because the installer isn't code-signed. Click **More info → Run anyway** if you trust the release.

The app needs the WebView2 runtime. Windows 10 and 11 already have it, and the installer adds it if it's missing.

### Upgrading from 0.3.x

Version 0.4.0 changed the app's internal ID, so Windows treats it as a new app. On first start it copies your settings and download queue from the old version. Your library folder and saved API keys are not affected. Afterwards you can uninstall the old "VIBDL" entry from Settings → Apps.

## First run

1. **Settings** opens first. Confirm you are 18 or older. This is needed because some boards host adult content.
2. Pick a board. If it needs an account, enter your user name or ID and API key, then click **Test connection**.
3. Choose which ratings to show. Only **general** is on by default.
4. Choose your library folder. The default is `Pictures\VIBDL`.
5. Go to **Browse**.

## Using it

### Browse

Type tags in the search box. Suggestions come from the current board: use ↑ ↓ to pick one and Tab or Enter to accept it. Click a post to see its details. Shift-click or Ctrl-click to select several posts and send them all to the queue.

### Downloads

The queue downloads one file at a time, waits between requests, and retries when something fails. You can cancel a job, retry failed ones, and clear finished ones. If you close the app, the queue picks up where it left off next time.

### Library search syntax

Terms are separated by spaces, and an item has to match all of them.

| Term | Meaning |
|---|---|
| `long_hair` | has this tag (any category) |
| `-monochrome` | doesn't have this tag |
| `artist:name` | tag in a category (also `character:` / `char:`, `copyright:` / `copy:`, `general:`, `meta:`) |
| `rating:g,s` | rating is one of these (letters or full words) |
| `board:danbooru` | came from this board (also `provider:`) |
| `type:png` | file type (also `ext:`) |

### Keyboard shortcuts

| Where | Key | Action |
|---|---|---|
| Browse / Library | `/` | focus the search box |
| Browse / Library | `Enter` | open the selected post in the viewer |
| Browse / Library | `←` `→` | previous / next post |
| Browse / Library | `Esc` | close the panel or clear the selection |
| Library | `Ctrl+A` | select all |
| Library | `Delete` | delete the selected items (asks first) |
| Viewer | `F` or `0` | fit to window |
| Viewer | `1` | actual size |
| Viewer | `+` `-`, mouse wheel | zoom |
| Viewer | drag | pan |
| Viewer | `Esc` | close |

Deleted files go to the Recycle Bin, not straight to permanent deletion.

## Where your data lives

| What | Where |
|---|---|
| Library | `<library folder>\<board>\<first 2 chars of md5>\<md5>.<ext>` plus a `<md5>.vibdl.json` sidecar |
| Library index | `<library folder>\.vibdl\library.sqlite`. It can always be rebuilt from the sidecars. |
| Settings, queue | `%APPDATA%\io.github.aemrican123.vibdl\` |
| Thumbnail cache | `%LOCALAPPDATA%\io.github.aemrican123.vibdl\` (size limit set in Settings) |
| Log | `%LOCALAPPDATA%\io.github.aemrican123.vibdl\logs\vibdl.log`. Use Settings → Open log folder. API keys and user IDs are removed from the log. |
| API keys | Windows Credential Manager (service `vibdl`). They are never written to files. |

## Privacy

- No telemetry, analytics, ads or update checks.
- The app only connects to the board you are using. Requests identify themselves as `VIBDL/<version> (+https://github.com/aemrican123/vibdl)`.
- No API keys ship with the app. You bring your own, and they stay in your OS keychain.

## Build from source

You need [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) 20 or newer, and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for Windows (the WebView2 runtime and the MSVC build tools).

```powershell
git clone https://github.com/aemrican123/vibdl.git
cd vibdl
npm ci
npm run tauri dev       # run in development mode
npm run tauri build     # build the installer
```

The installer ends up in `src-tauri\target\release\bundle\nsis\`.

If PowerShell blocks `npm` with an execution-policy error, run `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` once.

Checks (CI runs these too):

```powershell
cd src-tauri; cargo test; cargo clippy --all-targets; cd ..
npm run check
```

### Project layout

```
src/                     UI (SvelteKit, SPA mode, Svelte 5)
  lib/api.ts             typed wrappers for the Rust commands
  lib/Browse.svelte      online gallery
  lib/Library.svelte     offline library (+ LocalPanel, LibraryTools)
  lib/Downloads.svelte   download queue
  lib/Viewer.svelte      full-screen viewer
  lib/Settings.svelte    setup and settings
src-tauri/src/
  commands.rs            the commands the UI can call
  net/                   HTTP client, rate limiter, retry and backoff, secret redaction
  providers/             board integrations behind one ImageboardProvider trait
    danbooru/            Danbooru engine
    moebooru/            Moebooru engine (yande.re)
    gelbooru/            Gelbooru-style engine (Safebooru.org, Rule34)
  browse/                gallery queries, thumb:// and media:// protocols, disk cache
  downloads/             verified downloads and index rebuild
  storage/               library layout and sidecars
  db/                    SQLite index and migrations
  library/               offline query language, facets, local thumbnails, bulk tools
  queue/                 persistent download queue
  settings/              settings file and OS-keychain credentials
docs/PLAN.md             original feasibility study and design notes
```

### Adding a board

Implement `ImageboardProvider` (or reuse one of the three engines with a new `Site`), add a `ProviderInfo` entry in `providers/mod.rs`, add a parser test with a small synthetic fixture, and check that board's API terms before you set `allows_batch: true`. Please don't add boards that block apps.

## Disclaimer

VIBDL is not affiliated with or endorsed by any of the boards it supports. You are responsible for following each board's terms of service and the copyright of the artwork you download. Please support the artists. Some supported boards host adult content, so the app is meant for adults (18+).

## License

Dual-licensed under either of

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option. Contributions are accepted under the same terms (see [CONTRIBUTING.md](CONTRIBUTING.md)). Third-party licenses are listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

© 2026 aemrican123
