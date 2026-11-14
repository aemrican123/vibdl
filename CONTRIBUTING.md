# Contributing

Thanks for wanting to help. Bug reports, fixes and new boards are all welcome.

## Before you start

- For anything bigger than a small fix, open an issue first so we can agree on the approach.
- VIBDL only uses each board's public API, within its terms. Pull requests that get around rate limits, logins, hotlink protection, Cloudflare checks or other access controls won't be merged. That includes faking a browser `Referer` or User-Agent.
- Never commit API keys, real account IDs, or captured responses that contain them. Test fixtures must be small and synthetic.

## Setup

See **Build from source** in the [README](README.md). Before you open a pull request, run:

```powershell
cd src-tauri; cargo fmt; cargo clippy --all-targets; cargo test; cd ..
npm run check
```

CI runs the same checks on Windows.

## Adding a board

1. Reuse an engine (`danbooru`, `moebooru`, `gelbooru`) with a new `Site`, or implement `ImageboardProvider`.
2. Add a `ProviderInfo` entry to `PROVIDERS` in `src-tauri/src/providers/mod.rs`.
3. Add a parser test with a synthetic fixture.
4. Read the board's terms. Set `allows_batch: true` only if they allow automated downloads, and link the terms in your pull request.
5. Check that thumbnails and originals load without any header tricks. If the board blocks apps, it can't be added.

## Licensing of contributions

Unless you say otherwise, any contribution you submit is dual-licensed under MIT OR Apache-2.0, like the rest of the project, with no extra terms.
