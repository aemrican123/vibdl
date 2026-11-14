# VIBDL — Feasibility Report & Development Plan (v0.1, planning only)

Prepared 2026-11-14. **No code has been written. Implementation waits for your explicit approval.**

## Evidence legend

| Tag | Meaning |
|---|---|
| **[DOC]** | Confirmed through official Gelbooru documentation |
| **[LIVE]** | Confirmed through a live test |
| **[2ND]** | Consistent across several independent open-source Gelbooru clients, but not checked against the official docs |
| **[UNVERIFIED]** | Not yet verified |
| **[BLOCKED]** | Blocked by an access requirement |
| **[POLICY?]** | Potentially restricted by terms or policy |

### Important limitation of this investigation

I could **not** open any gelbooru.com page from this research environment. The web-fetch tool rejected the domain because it's not on the allowlist, and the browser pane refused it for safety reasons (the site hosts adult content). That means:

- **Nothing below is tagged [DOC] or [LIVE].** I didn't read the official API page, the API wiki page (id=18780), or `tos.php` directly.
- The API facts come from the source code and READMEs of several independent, maintained Gelbooru client libraries (Rust, Python, Go, Node, Haskell). Most of them link back to the official wiki page 18780. Where they agree, I mark the fact **[2ND]**. Where they disagree, I say so.
- **The Terms of Service were not read at all.** Every legal or contractual conclusion is therefore **[POLICY?]** until you (or I, with access) read `tos.php` itself. §0 below gives a 15-minute verification procedure that closes this gap. **I recommend making it Milestone 0, a hard gate before any code.**

---

## Milestone 0 results (2026-11-14)

- **[LIVE]** Authenticated dapi post search works with `api_key` + `user_id` (test: `tags=rating:general`, `limit=1`). It reported 3,859,549 general-rated posts.
- **[LIVE]** The post object has exactly these 29 fields: `change, created_at, creator_id, directory, file_url, has_children, has_comments, has_notes, height, id, image, md5, owner, parent_id, post_locked, preview_height, preview_url, preview_width, rating, sample, sample_height, sample_url, sample_width, score, source, status, tags, title, width`. This matches the [2ND] list in Part 3.
- **[LIVE]** There is **no file-size field**. VIBDL records the size at download time.
- **[LIVE]** The `rating:general` filter works, which confirms the 4-level vocabulary (general/sensitive/questionable/explicit).
- **[USER-VERIFIED]** The owner reviewed `tos.php` and judged personal use acceptable. VIBDL is personal use only and won't be distributed.
- Still [UNVERIFIED]: the behavior without a key, the tag endpoint's `type` values against a real response, the `created_at` format, and whether thumbnails need a Referer. These get checked as M2/M3 exercise them.

---

## Part 1 — Feasibility verdict

**Technically feasible:** yes, with high confidence. Gelbooru exposes a JSON/XML "dapi" that third-party clients have used for years. Through it you can search posts by tags, get preview, sample and original-file URLs, and look up tag categories. [2ND]

**Contractually permissible:** **unknown. I couldn't verify it.** Three warning signs:

1. At least one open-source Gelbooru downloader (booru-dl) carries a README warning that it is *"probably against Gelbooru's TOS."* [2ND, not authoritative]
2. Current client libraries disagree on authentication. One says credentials are **required** (HTTP 401 without them). Others say they are optional but unauthenticated use is throttled and limited to 2 tags per query. Either way, Gelbooru is actively gating API use. [2ND, conflicting]
3. Your own prompt flags anti-scraping and anti-indexing language as a concern. That fits what large boorus typically publish, but I haven't confirmed the exact wording.

**Verdict:** VIBDL as designed is a user-driven client that browses on demand, downloads only what you explicitly pick, and doesn't crawl. That is the **lowest-risk shape** this kind of tool can take, and I believe it's buildable. But I recommend these **modifications** until the TOS is read:

- Single-image download is in the MVP. **Batch downloading stays out of the MVP** and is gated on the TOS review, and ideally on written permission from Gelbooru staff if the TOS is restrictive.
- Each user uses **their own** Gelbooru account, API key and user ID. VIBDL never ships a shared or embedded key.
- VIBDL sends an honest, identifying `User-Agent` and enforces conservative client-side rate limits. It never tries to get around 401/403/429 responses, Cloudflare challenges or search-depth caps.
- **Public distribution** (publishing builds) is a separate decision, made after the TOS review. Private, personal use comes first.

---

## Part 2 — Gelbooru prerequisite checklist

| # | Question | Finding | Status |
|---|---|---|---|
| A1 | Can a third-party desktop app use the official API? | Yes, technically. Endpoint: `https://gelbooru.com/index.php?page=dapi&s=post&q=index&json=1` | [2ND] / permission [POLICY?] |
| A2 | Does it need an account, key or cookies? | `api_key` + `user_id` query params, taken from the user's account options page. Libraries conflict: "required (401)" vs "optional but throttled, 2-tag limit". No cookies needed. | [2ND, conflicting] → **plan as REQUIRED** |
| A3 | Gallery without downloading originals? | Yes. Each post returns `preview_url` (thumbnail) and `sample_url` (medium) separately from `file_url` (original). | [2ND] |
| A4 | Use thumbnails or previews? | Yes: `preview_url`, `preview_width`, `preview_height`. | [2ND] |
| A5 | Pagination, lazy loading, caching? | Yes. `limit` (max 100 per request) and `pid` (page index); the response carries `@attributes {limit, offset, count}`. **Deep-pagination cap:** results beyond about `limit × pid = 20,000` aren't served. | [2ND] |
| A6 | Search by tags, artists, characters, ratings? | Yes. Artists and characters are ordinary tags in the `tags` param. Ratings use `rating:<value>` meta-tags. The site's AND/OR/NOT tag syntax is supported. | [2ND] |
| A7 | Preview and metadata before download? | Yes. All metadata is in the search response; a single post comes from `&id=<post_id>`. | [2ND] |
| A8 | Restrictions and rate limits? | 100 posts per request; ~20k search-depth cap; throttling for unauthenticated (or non-supporter) users; one source says Patreon supporters are not rate-limited. **No published numeric rate limit found.** | [2ND] / numeric limit [UNVERIFIED] |
| A9 | Browsing within TOS? | Couldn't read TOS. | **[BLOCKED] + [POLICY?]** |
| B1–B4 | Original file, original URL, original format, no recompression? | `file_url` points to the original; `image` holds the filename with its extension. Streaming the bytes to disk preserves format and quality exactly (no decoding or re-encoding). | [2ND] |
| B5 | Controlled queue | Client-side design; no API obstacle. | Design (post-MVP) |
| B6 | Duplicate detection before download | `md5` field per post → check the local DB before fetching; verify the hash after download. | [2ND] |
| B7 | Failed or interrupted downloads | Client-side: `.part` file + retry with backoff. HTTP Range resume depends on the CDN. | Range support [UNVERIFIED] |
| B8 | Preserve post URL and ID | `id` is returned; post URL = `https://gelbooru.com/index.php?page=post&s=view&id=<id>` | [2ND] |
| B9 | Metadata alongside image | Yes. See Part 3. | [2ND] |
| — | Thumbnail hotlinking (Referer or Cloudflare requirements on the CDN) | Unknown | [UNVERIFIED] |
| — | Distributing VIBDL publicly | Depends on TOS | [POLICY?] |
| — | Batch downloading | Depends on TOS; it's the likeliest prohibited feature | [POLICY?] |

---

## Part 3 — Metadata capability report

### Post fields (as reported by multiple client libraries) [2ND]

`id`, `created_at`, `score`, `width`, `height`, `md5`, `directory`, `image` (filename + extension), `change` (last-modified stamp), `owner`, `creator_id`, `parent_id`, `rating`, `sample`, `sample_width`, `sample_height`, `sample_url`, `preview_width`, `preview_height`, `preview_url`, `file_url`, `source`, `tags`, `title`, `status`, `has_notes`, `has_comments`, `has_children`, `post_locked`.

| You need | Available? | How |
|---|---|---|
| Post ID | ✅ | `id` |
| Artist / Character / Copyright / General / Metadata tags | ⚠️ **Not directly** | See "Tag categories" below |
| Rating | ✅ | `rating`. **Store the raw string.** Older libraries model 3 values (safe/questionable/explicit); newer ones use "general". The exact current vocabulary is [UNVERIFIED]. |
| Source URL | ✅ | `source` (may be empty or contain several URLs) |
| Original image URL | ✅ | `file_url` |
| Width / height | ✅ | `width`, `height` |
| File extension | ✅ | Derived from `image` / `file_url` |
| **File size** | ❌ **Not in the post object** in any library I found | Take it from the `Content-Length` header during download, or stat the file afterward |
| Creation / upload date | ✅ | `created_at`. The format is [UNVERIFIED], so parse it defensively and keep the raw value. |
| Hashes / identifiers | ✅ | `md5`; also `parent_id`, `change` |
| Download date | Local | Recorded by VIBDL |

### Tag categories: the key finding

**Your instinct was right: artists are *not* a dedicated field.** A post's `tags` is a single space-separated string with no category information. [2ND]

Categories come from a **second endpoint**: `page=dapi&s=tag&q=index&json=1`, queried with `names=<tag1 tag2 …>` (batch), `name=`, or `name_pattern=` (SQL-like `%`/`_`). Each tag returns `id`, `name`, `count`, `type`, where `type` is: [2ND]

| type | Category |
|---|---|
| 0 | General |
| 1 | **Artist** |
| 2 | Deprecated |
| 3 | Copyright |
| 4 | Character |
| 5 | Metadata |

**Handling strategy:**

1. On viewing details or downloading, split `tags`, then look up all **unknown** tags in **one batched** `names=` request.
2. Cache tag→type in a local `tags` table with a `fetched_at` timestamp. Popular tags are shared across posts, so the cache cuts API calls sharply over time. Refresh stale entries (e.g. older than 30 days) lazily.
3. If a lookup fails, store the tag as `category = unknown` and retry later. **Never guess a category from the tag's name.**
4. A post can have zero, one or several artist tags. The schema treats "artist" as a many-to-many relationship, not a single column.
5. Type 2 (deprecated) is kept and shown, but excluded from default filters.

Example: post tags `example_artist character_a character_b example_series long_hair blue_eyes night` + one batched lookup → Artist `example_artist`; Characters `character_a`, `character_b`; Copyright `example_series`; General `long_hair`, `blue_eyes`, `night`. This is fully achievable.

---

## Part 4 — Recommended technology stack

**Recommendation: Tauri 2 (Rust core) + Svelte/SvelteKit or React (TypeScript) UI + SQLite.**

| Need | Why Tauri over Electron |
|---|---|
| Low memory | Uses the OS webview (WebView2 on Windows) instead of bundling Chromium. Idle RAM and install size are typically a fraction of Electron's. |
| Fast downloads and hashing | Network, disk I/O and MD5 run in native Rust (`reqwest` + `tokio`), off the UI thread |
| Secure credentials | `keyring` crate → Windows Credential Manager / macOS Keychain / Secret Service |
| Local files offline | A custom URI protocol (`vibdl://`) serves cached thumbnails and library files to the UI without exposing the filesystem |
| Extensibility | Rust traits give each imageboard a clean provider boundary |

Trade-off: Rust has a steeper learning curve than an all-JavaScript Electron app. Since you're "vibecoding" with me, I'll write the Rust and keep the frontend in familiar TypeScript. *(Alternatives I considered: **Electron**, the easiest but heaviest; **.NET/Avalonia** or **Qt**, native and fast but with a less modern UI tooling story.)*

**Core crates:** `reqwest` (HTTP, streaming), `tokio`, `serde`, `rusqlite` or `sqlx` (SQLite, with FTS5 optional), `md-5`, `keyring`, `governor` (rate limiting), `tracing` (logging with secrets redacted). **UI:** virtualized grid (e.g. `svelte-virtual` / `react-virtuoso`) and IntersectionObserver lazy loading.

### Storage design

```
<LibraryRoot>/
  gelbooru/
    ab/                          # first 2 chars of md5 → keeps folders small
      ab12…ef.png                # original bytes, untouched
      ab12…ef.vibdl.json         # sidecar: full raw API post + categorized tags + download info
  .vibdl/
    library.sqlite               # index (rebuildable from sidecars)
    thumbs/                      # LRU-capped online thumbnail cache (e.g. 500 MB)
```

- **SQLite is the fast index; sidecar JSON is the source of truth for recovery and portability.** If the DB is lost or corrupted, "Rebuild library from sidecars" restores everything. Sidecars also survive moving the folder to another machine or tool.
- Filenames use the md5 (stable, collision-free, makes dedupe trivial). The original Gelbooru filename is kept in metadata. An optional "export with readable names" comes later.

### Schema sketch

```
sources(id, key 'gelbooru', display_name)
posts(id PK, source_id, remote_post_id, md5 UNIQUE, rating_raw, width, height,
      file_ext, file_size, source_url, post_url, file_url, created_at_raw,
      created_at, downloaded_at, local_path, raw_json)          -- UNIQUE(source_id, remote_post_id)
tags(id PK, source_id, name, category ENUM(general,artist,copyright,character,meta,deprecated,unknown),
     remote_count, fetched_at)                                 -- UNIQUE(source_id, name)
post_tags(post_id, tag_id)                                     -- indexed both ways
```

Combined filtering ("artist X AND character Y, NOT tag Z, sort by resolution") becomes indexed joins plus `ORDER BY width*height`.

### Component architecture

```
┌──────────────── UI (TypeScript) ────────────────┐
│ OnlineBrowser │ PostDetail │ Library │ Settings │ DownloadsPanel │
└────────────────────┬──────────── Tauri IPC (commands/events) ───┘
┌────────────────────┴──────────── Rust core ──────────────────────┐
│ 1 providers/      ImageboardProvider trait → gelbooru/ impl      │
│ 2 browse/         search sessions, paging, thumbnail cache       │
│ 3 downloads/      queue, workers, retry, .part → verify → rename │
│ 4 metadata/       tag categorization, tag-type cache, normalize  │
│ 5 storage/        file layout, sidecar read/write, integrity     │
│ 6 db/             migrations, repositories, query builder        │
│ 7 library/        search/filter/sort, duplicates, rebuild        │
│ 8 settings/       config file + keyring credentials + rating gate│
│   net/            shared HTTP client, UA, rate limiter, backoff  │
└──────────────────────────────────────────────────────────────────┘
```

### Provider abstraction (pseudocode)

```rust
trait ImageboardProvider {
    fn id(&self) -> &'static str;                       // "gelbooru"
    fn capabilities(&self) -> Capabilities;             // max_page_size, max_depth, needs_tag_lookup, ratings[]
    async fn search(&self, q: &SearchQuery, page: u32) -> Result<Page<RemotePost>>;
    async fn get_post(&self, remote_id: &str) -> Result<RemotePost>;
    async fn categorize_tags(&self, names: &[String]) -> Result<Vec<TagInfo>>; // Danbooru-style boards can return these inline
    fn post_url(&self, remote_id: &str) -> String;
}
// RemotePost = normalized fields + `raw: serde_json::Value` (never lose provider-specific data)
```

To add Danbooru or Rule34 later: implement the trait in `providers/<name>/`, register it, and map its ratings and tag categories to the shared enums. Nothing else changes.

---

## Part 5 — MVP feature specification

**The MVP proves browse → inspect → download one → organize → find again.**

1. **First run:** an 18+ age confirmation (self-attested), choice of library folder, and entry of Gelbooru user ID + API key. The key is stored in the OS keychain. A "Test connection" button makes one `limit=1` call.
2. **Online search:** a tag query box (Gelbooru syntax passed through) and a rating filter (default **general only**; others opt-in in Settings).
3. **Thumbnail grid:** `preview_url` only, 42–100 posts per page, infinite scroll, a virtualized grid, an on-disk thumbnail LRU cache, and a clear "end of results / depth limit reached" message.
4. **Post detail:** a `sample_url` preview (never the original), with all metadata and tags grouped by category through a batched tag lookup.
5. **Download one image:** stream `file_url` → `.part` → MD5 verify against the API `md5` → atomic rename → write sidecar → insert into SQLite. Duplicates (md5 already present) are skipped with a notice.
6. **Local library:** an offline grid of downloaded items with tag search (AND / NOT), **artist filter**, character filter, sort by download date / resolution, a detail pane with all metadata, "Open file", "Open folder" and "Open Gelbooru post".
7. **Settings:** download folder, thumbnail cache size, rating filters, credentials (masked, never logged).
8. **Networking guarantees:** a global rate limiter (default ≤ 1 API request/s, 1 download at a time), honest `User-Agent: VIBDL/0.1 (+contact)`, exponential backoff on 429/5xx, and a hard stop with a clear message on 401/403. No retries around blocks.

**Explicitly NOT in the MVP:** batch/queued multi-download, other boards, favorites sync, auto-tag-subscriptions, video handling beyond storing the file, AI tagging, cloud anything.

---

## Part 6 — Development roadmap

### Milestone 0 — Verification gate (you + me, no code)
- **Objective:** turn [2ND]/[POLICY?] into [DOC]/[LIVE].
- **Tasks:** see §0 procedure below. Read `tos.php` and paste the relevant clauses to me. Run 4 read-only API calls in your own browser.
- **Risk:** the TOS prohibits API clients or downloading → we pivot (permission request, or reduce scope to a pure local library manager).
- **Acceptance:** a field list confirmed from a real response; the auth behavior known; the TOS clauses reviewed; a go/no-go decision recorded.

### Milestone 1 — Skeleton + Gelbooru client
- **Features:** Tauri project, settings file, keychain credentials, shared HTTP client with rate limiter and redacting logger, `GelbooruProvider` (search, get_post, categorize_tags).
- **Prereqs:** M0 go; Rust + Node toolchains on your PC (C:\coding\vibdl).
- **Risks:** response format drift. Mitigate with tolerant parsing and keeping `raw`.
- **Deliverable:** the app launches and "Test connection" works.
- **Acceptance / test:** unit tests parse **recorded fixture JSON** (captured once in M0, so tests never hit the network); one manual live call; grep logs to confirm no key appears.

### Milestone 2 — Online browser
- **Features:** search box, rating filter, virtualized thumbnail grid, infinite scroll, thumbnail disk cache, age gate.
- **Risks:** CDN hotlink rules (Referer/Cloudflare); memory with large grids.
- **Acceptance:** scroll 500 results with steady memory (< ~300 MB app total); zero `file_url` requests in the network log; the cache hit rate is visible on revisit.

### Milestone 3 — Post detail + categorized metadata
- **Features:** sample preview, metadata panel, batched tag lookup, tag-type cache.
- **Acceptance:** for 10 hand-checked posts, categories match the Gelbooru website's sidebar colors; a second view of the same post makes 0 tag API calls.

### Milestone 4 — Single download + persistence
- **Features:** stream download, md5 verification, sidecar, SQLite insert, duplicate skip.
- **Acceptance / test:** the downloaded file's md5 equals the API `md5` and its bytes match exactly (no recompression); killing the app mid-download leaves no corrupt final file; a re-download is detected as a duplicate; deleting the DB and running "rebuild" restores the record.

### Milestone 5 — Local library (MVP COMPLETE)
- **Features:** offline grid, tag/artist/character filters, combined filters, sorts, open file/folder/post.
- **Acceptance:** works with networking disabled; a 3-filter query on a seeded 10k-row DB returns in < 100 ms.

### Post-MVP (each gated on review)
- **M6 Download queue:** multi-select, concurrency (default 1–2), pause/resume/retry, progress panel. **Gated on the TOS allowing it.**
- **M7 Library management:** delete (file + record), metadata refresh, duplicate review screen, export with readable names, DB backup.
- **M8 Second provider (e.g. Danbooru):** proves the abstraction; Danbooru returns tag categories inline.
- **M9 Polish:** keyboard navigation, saved searches, perceptual-hash duplicate detection (catches re-encodes that md5 misses).

---

## Part 7 — Risks & decisions

| Area | Risk | Mitigation |
|---|---|---|
| **TOS / legal** | Automated downloading or third-party clients may be prohibited | M0 gate; no batch in MVP; ask Gelbooru staff for permission if ambiguous; don't publish builds until cleared |
| **Auth** | Key required, or unauthenticated requests are crippled | Treat the key as required; per-user keys only |
| **Rate limits** | Unpublished numbers → risk of throttling or account action | Conservative defaults, backoff, respect for `Retry-After`, no parallel API bursts |
| **Depth cap** | ~20k results per query | Show "refine your search"; **don't** add workarounds that page around the cap |
| **Hotlinking / Cloudflare** | Thumbnails may need Referer or fail through challenges | Verify in M0; if a challenge appears, stop and surface it. Never solve or bypass challenges. |
| **Bandwidth** | Originals are often 2–20 MB+ | Originals only on explicit action; thumbnails cached; sample previews not cached long-term |
| **Adult content** | Explicit material; minors | 18+ gate, default "general" only, per-rating toggles, optional thumbnail blur, filter applies to library too, quick "safe mode" toggle |
| **Security** | API key leakage | OS keychain; redact `api_key` in all logs and error messages; never in sidecars or DB |
| **DB integrity** | Corruption / out-of-sync files | WAL mode, transactions, sidecars as source of truth, rebuild + integrity scan |
| **Format drift** | Gelbooru changes fields or date format | Tolerant parsing; keep raw JSON; fixture-based tests |
| **Rating vocabulary** | 3 vs 4 rating levels | Store raw; map through a provider table |
| **Scale** | 100k+ items | Indexed join tables, virtualized UI, md5-sharded folders |
| **Future boards** | Different APIs (Danbooru JSON, Rule34 dapi clone) | Provider trait + `raw` field + capability flags |

**Decisions you need to make:** (1) Svelte or React for the UI (I lean Svelte for lightness); (2) whether you're OK making M0 a hard gate; (3) whether VIBDL is for personal use only or intended for public release.

---

## §0 — Minimal verification procedure (Milestone 0)

Do this in **your own browser, logged in**, using your own key. Each step is one read-only request. Don't download any original files.

1. **Read the TOS:** `https://gelbooru.com/tos.php`. Copy any clauses about bots, automated access, scraping, downloading, API use, indexing, or redistribution.
2. **Read the API docs:** `https://gelbooru.com/index.php?page=wiki&s=view&id=18780` and `…?page=help&topic=dapi`. Note any stated rate limits or usage rules.
3. **Auth test (no key):** `https://gelbooru.com/index.php?page=dapi&s=post&q=index&json=1&limit=1&tags=rating:general` → note whether you get 401/403, data, or a restricted result.
4. **Auth test (with key):** append `&api_key=YOUR_KEY&user_id=YOUR_ID` → save the JSON. **Remove the key before sharing it with me.**
5. **Tag lookup:** `https://gelbooru.com/index.php?page=dapi&s=tag&q=index&json=1&names=<3 tags from that post>&api_key=…&user_id=…` → confirm the `type` values.
6. **Thumbnail:** open that post's `preview_url` directly in a new tab → does it load without a Referer?

Paste the results to me (with the keys removed) and I'll upgrade every [2ND] marker to [LIVE] or correct it.

---

## Sources (secondary — official pages were unreachable from this environment)

- booru-rs Gelbooru client (credentials required, 401): https://docs.rs/booru-rs/latest/src/booru_rs/client/gelbooru.rs.html
- gelbooru-api crate (tag type mapping, tag endpoint params, post struct): https://docs.rs/crate/gelbooru-api/0.4.0/source/src/api.rs
- booru-dl (limit 1–100, 20k depth cap, TOS warning): https://docs.rs/crate/booru-dl/latest/source/src/api.rs · https://awesome.ecosyste.ms/projects/github.com%2Fwsh032%2Fbooru-dl
- Go `boorus/gelbooru` (post fields incl. md5, preview_url, status; tag `type` field): https://beta.pkg.go.dev/github.com/GreatValueCreamSoda/boorus/gelbooru
- gelbooru-mcp (optional key, throttling, 2-tag limit, `id`/`cid` params): https://glama.ai/mcp/servers/yzw53k18n9 · https://glama.ai/mcp/servers/citronlegacy/gelbooru-mcp/tools/search_posts
- pygelbooru (key optional, access limited without it; `name_pattern`): https://pypi.org/project/pygelbooru/0.4.1/
- Official (to verify in M0): https://gelbooru.com/tos.php · https://gelbooru.com/index.php?page=wiki&s=view&id=18780 · https://gelbooru.com/index.php?page=help&topic=dapi
