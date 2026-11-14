<script lang="ts">
  import { onDestroy } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    api,
    errorText,
    thumbUrl,
    type BrowsePost,
    type CategorizedTags,
    type DownloadProgress,
    type LocalEntry,
    type PostDetails,
  } from "$lib/api";

  let {
    post,
    onclose,
    ontag,
  }: { post: BrowsePost; onclose: () => void; ontag: (tag: string, add: boolean) => void } = $props();

  let details = $state<PostDetails | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let sampleFailed = $state(false);
  let sampleLoaded = $state(false);
  let showFields = $state(false);
  let local = $state<LocalEntry | null>(null);
  let downloading = $state(false);
  let progress = $state<{ bytes: number; total: number | null } | null>(null);
  let dlMessage = $state<{ kind: "ok" | "err"; text: string } | null>(null);

  // Progress events from the Rust downloader.
  let unlisten: UnlistenFn | null = null;
  listen<DownloadProgress>("download-progress", (e) => {
    if (e.payload.id === post.id && e.payload.provider === post.provider) {
      progress = { bytes: e.payload.bytes, total: e.payload.total };
    }
  }).then((u) => (unlisten = u));
  onDestroy(() => unlisten?.());

  async function download() {
    const target = post;
    downloading = true;
    dlMessage = null;
    progress = null;
    try {
      const out = await api.downloadPost(target.provider, target.id);
      if (target.id !== post.id) return;
      local = out.entry;
      dlMessage =
        out.status === "downloaded"
          ? { kind: "ok", text: `Saved original (${fmtSize(out.bytes)}).` }
          : {
              kind: "ok",
              text:
                out.entry.provider === target.provider && out.entry.remote_id === target.id
                  ? "Already in your library."
                  : `Same file already in your library (from ${out.entry.provider} #${out.entry.remote_id}).`,
            };
    } catch (e) {
      if (target.id === post.id) dlMessage = { kind: "err", text: errorText(e) };
    } finally {
      if (target.id === post.id) downloading = false;
    }
  }

  const openLocal = (reveal: boolean) =>
    local &&
    api.openLocal(local.local_path, reveal).catch((e) => (dlMessage = { kind: "err", text: errorText(e) }));

  // Reload whenever a different post is selected; ignore stale responses.
  let current = "";
  $effect(() => {
    const id = post.id;
    current = id;
    details = null;
    error = null;
    sampleFailed = false;
    sampleLoaded = false;
    local = null;
    downloading = false;
    progress = null;
    dlMessage = null;
    loading = true;
    api
      .postDetails(post.provider, id)
      .then((d) => {
        if (current === id) {
          details = d;
          local = d.local;
        }
      })
      .catch((e) => {
        if (current === id) error = errorText(e);
      })
      .finally(() => {
        if (current === id) loading = false;
      });
  });

  const GROUPS: { key: keyof CategorizedTags; label: string }[] = [
    { key: "artist", label: "Artist" },
    { key: "character", label: "Character" },
    { key: "copyright", label: "Copyright" },
    { key: "general", label: "General" },
    { key: "meta", label: "Meta" },
    { key: "deprecated", label: "Deprecated" },
    { key: "unknown", label: "Uncategorized" },
  ];

  const fmtSize = (b: number) =>
    b >= 1048576 ? `${(b / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`;
  const res = (w: number | null, h: number | null) => (w && h ? `${w} × ${h}` : "?");
  const megapixels = (w: number | null, h: number | null) =>
    w && h ? `${((w * h) / 1_000_000).toFixed(1)} MP` : "";
</script>

<aside class="panel">
  <div class="head">
    <h3>Post #{post.id}</h3>
    <button class="ghost small" onclick={onclose} aria-label="Close">✕</button>
  </div>

  <div class="preview">
    {#if details?.sample_key && !sampleFailed}
      <!-- Thumbnail shows instantly underneath while the sample loads. -->
      {#if post.thumb_key && !sampleLoaded}
        <img class="under" src={thumbUrl(post.thumb_key)} alt="" />
      {/if}
      <img
        class:hidden={!sampleLoaded}
        src={thumbUrl(details.sample_key)}
        alt={`Preview of post ${post.id}`}
        onload={() => (sampleLoaded = true)}
        onerror={() => (sampleFailed = true)}
      />
    {:else if post.thumb_key}
      <img src={thumbUrl(post.thumb_key)} alt={`Thumbnail of post ${post.id}`} />
    {:else}
      <span class="none">no preview</span>
    {/if}
  </div>
  {#if details && !details.sample_key}
    <p class="hint">No preview image for this file type/size.</p>
  {:else if sampleFailed}
    <p class="hint">Sample failed to load; showing nothing. Check the terminal log.</p>
  {/if}

  <div class="actions">
    {#if local}
      <button onclick={() => openLocal(false)}>Open file</button>
      <button class="ghost" onclick={() => openLocal(true)}>Show in folder</button>
    {:else}
      <button disabled={downloading || loading || !!error} onclick={download}>
        {downloading ? "Downloading…" : "Download original"}
      </button>
    {/if}
    <button class="ghost" onclick={() => openUrl(post.post_url)}>Open on site</button>
  </div>
  {#if downloading && progress}
    <div class="bar" title={`${fmtSize(progress.bytes)}${progress.total ? ` of ${fmtSize(progress.total)}` : ""}`}>
      <div style:width={progress.total ? `${Math.min(100, (progress.bytes / progress.total) * 100)}%` : "100%"}
           class:indeterminate={!progress.total}></div>
    </div>
  {/if}
  {#if local && !dlMessage}
    <p class="ok small">In your library · {local.local_path}</p>
  {/if}
  {#if dlMessage}<p class={`${dlMessage.kind} small`}>{dlMessage.text}</p>{/if}

  {#if loading}
    <p class="hint">Loading details…</p>
  {:else if error}
    <p class="err">{error}</p>
  {:else if details}
    <dl>
      <dt>Resolution</dt><dd>{res(details.width, details.height)} <span class="dim">{megapixels(details.width, details.height)}</span></dd>
      <dt>Format</dt><dd>{details.file_ext?.toUpperCase() ?? "?"}{details.file_size ? ` · ${fmtSize(details.file_size)}` : ""}</dd>
      <dt>Rating</dt><dd>{details.rating ?? "?"}</dd>
      <dt>Score</dt><dd>{details.score ?? "?"}</dd>
      <dt>Posted</dt><dd>{details.created_at ?? "?"}</dd>
      <dt>MD5</dt><dd class="mono">{details.md5 ?? "?"}</dd>
      {#if details.parent_id}<dt>Parent</dt><dd>#{details.parent_id}</dd>{/if}
      {#if details.has_children}<dt>Children</dt><dd>yes</dd>{/if}
      {#if details.status && details.status !== "active"}<dt>Status</dt><dd>{details.status}</dd>{/if}
      <dt>Source</dt>
      <dd>
        {#each details.sources as s}
          {#if s.startsWith("http")}
            <button class="link" onclick={() => openUrl(s)}>{s}</button>
          {:else}
            <span>{s}</span>
          {/if}
        {:else}
          <span class="dim">none</span>
        {/each}
      </dd>
    </dl>

    {#if details.tag_lookup_error}
      <p class="err small">Couldn't categorize some tags: {details.tag_lookup_error}</p>
    {/if}

    {#each GROUPS as g}
      {@const list = details.tags[g.key]}
      {#if list.length}
        <section class="group">
          <h4>{g.label} <span class="dim">{list.length}</span></h4>
          <div class="tags">
            {#each list as t}
              <button
                class={`tag ${g.key}`}
                title="Click: search this tag · Shift+click: add to search"
                onclick={(e) => ontag(t, e.shiftKey)}>{t.replaceAll("_", " ")}</button
              >
            {/each}
          </div>
        </section>
      {/if}
    {/each}
    <p class="dim small">
      {details.tags_looked_up === 0 ? "Tag categories from cache." : `${details.tags_looked_up} tags looked up.`}
    </p>

    <button class="ghost small" onclick={() => (showFields = !showFields)}>
      {showFields ? "Hide" : "Show"} all fields ({Object.keys(details.fields).length})
    </button>
    {#if showFields}
      <table class="fields">
        <tbody>
          {#each Object.entries(details.fields).sort(([a], [b]) => a.localeCompare(b)) as [k, v]}
            <tr><th>{k}</th><td>{typeof v === "string" ? v : JSON.stringify(v)}</td></tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {/if}
</aside>

<style>
  .panel {
    width: 380px; flex-shrink: 0; overflow-y: auto; padding: 14px 16px 24px;
    border-left: 1px solid #2c2d35; background: #1a1b20;
  }
  .head { display: flex; justify-content: space-between; align-items: center; }
  h3 { margin: 0; font-size: 1rem; }
  .preview {
    position: relative; margin: 12px 0 6px; min-height: 160px; max-height: 480px;
    display: flex; align-items: center; justify-content: center;
    background: #111216; border-radius: 6px; overflow: hidden;
  }
  .preview img { max-width: 100%; max-height: 480px; object-fit: contain; display: block; }
  .preview img.under { position: absolute; inset: 0; margin: auto; filter: blur(2px); opacity: 0.6; }
  .preview img.hidden { visibility: hidden; position: absolute; }
  .none { color: #6b6c75; font-size: 0.85rem; }
  .actions { display: flex; gap: 8px; margin: 10px 0 4px; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; font-size: 0.86rem; margin: 12px 0; }
  dt { color: #9a9ba3; }
  dd { margin: 0; min-width: 0; overflow-wrap: anywhere; display: flex; flex-direction: column; gap: 2px; }
  .mono { font-family: ui-monospace, Consolas, monospace; font-size: 0.78rem; }
  .dim { color: #7d7e87; }
  .small { font-size: 0.8rem; }
  .hint { color: #7d7e87; font-size: 0.8rem; margin: 4px 0; }
  .err { color: #f07b7b; font-size: 0.85rem; }
  .ok { color: #6fd39a; }
  .bar { height: 6px; background: #26272e; border-radius: 3px; overflow: hidden; margin: 6px 0; }
  .bar > div { height: 100%; background: #4b5bd6; transition: width 0.2s; }
  .bar > div.indeterminate { opacity: 0.5; }
  .group { margin: 12px 0; }
  h4 { margin: 0 0 6px; font-size: 0.82rem; text-transform: uppercase; letter-spacing: 0.05em; color: #b5b6bf; }
  .tags { display: flex; flex-wrap: wrap; gap: 4px; }
  .tag {
    border: none; border-radius: 4px; padding: 2px 7px; font: inherit; font-size: 0.8rem;
    cursor: pointer; background: #26272e; color: #d6d7dc;
  }
  .tag:hover { filter: brightness(1.3); }
  /* Colors roughly follow Gelbooru's tag colors. */
  .tag.artist { background: #3b2323; color: #ff8a8a; }
  .tag.character { background: #1f3324; color: #7fdc8f; }
  .tag.copyright { background: #2f2340; color: #d19bff; }
  .tag.meta { background: #3a2f1c; color: #ffc46b; }
  .tag.deprecated { background: #2a2a2a; color: #8d8d8d; text-decoration: line-through; }
  .tag.unknown { background: #26272e; color: #a9aab3; font-style: italic; }
  .fields { width: 100%; margin-top: 8px; border-collapse: collapse; font-size: 0.75rem; }
  .fields th { text-align: left; color: #9a9ba3; font-weight: normal; padding: 2px 8px 2px 0; vertical-align: top; }
  .fields td { overflow-wrap: anywhere; padding: 2px 0; }
  button {
    padding: 7px 12px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: white;
    font: inherit; font-size: 0.88rem; cursor: pointer;
  }
  button.ghost { background: transparent; border-color: #4a4b55; color: #c9c9cf; }
  button.small { padding: 2px 8px; font-size: 0.8rem; }
  button:disabled { opacity: 0.45; cursor: default; }
  button.link {
    background: none; border: none; padding: 0; color: #8fa0ff; text-align: left; cursor: pointer;
    font-size: 0.82rem; overflow-wrap: anywhere;
  }
</style>
