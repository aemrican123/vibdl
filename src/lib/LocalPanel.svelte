<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    api,
    errorText,
    isVideoExt,
    libraryFileUrl,
    libraryThumbUrl,
    libraryVideoUrl,
    type CategorizedTags,
    type LibraryDetails,
    type LibraryItem,
  } from "$lib/api";

  let {
    item,
    onclose,
    onfilter,
    ondeleted,
    onview,
  }: {
    item: LibraryItem;
    onclose: () => void;
    onfilter: (term: string, add: boolean) => void;
    ondeleted?: (item: LibraryItem) => void;
    onview?: () => void;
  } = $props();

  let confirmDelete = $state(false);
  let working = $state(false);
  let note = $state<{ kind: "ok" | "err"; text: string } | null>(null);

  async function refreshMeta() {
    working = true;
    note = null;
    try {
      const d = await api.libraryRefresh(item.provider, item.remote_id);
      if (d) details = d;
      note = { kind: "ok", text: "Metadata refreshed from the board." };
    } catch (e) {
      note = { kind: "err", text: errorText(e) };
    } finally {
      working = false;
    }
  }

  async function doDelete() {
    working = true;
    note = null;
    try {
      await api.libraryDelete(item.provider, item.remote_id);
      ondeleted?.(item);
    } catch (e) {
      note = { kind: "err", text: errorText(e) };
      working = false;
    }
  }

  let details = $state<LibraryDetails | null>(null);
  let error = $state<string | null>(null);
  let fullLoaded = $state(false);
  let fullFailed = $state(false);
  let showFields = $state(false);

  const STILL = ["jpg", "jpeg", "png", "gif", "webp"];
  const isStill = $derived(STILL.includes(item.file_ext.toLowerCase()));

  let current = "";
  $effect(() => {
    const key = `${item.provider}:${item.remote_id}`;
    current = key;
    details = null;
    error = null;
    fullLoaded = false;
    fullFailed = false;
    confirmDelete = false;
    working = false;
    note = null;
    api
      .libraryDetails(item.provider, item.remote_id)
      .then((d) => {
        if (current === key) details = d;
      })
      .catch((e) => {
        if (current === key) error = errorText(e);
      });
  });

  const GROUPS: { key: keyof CategorizedTags; label: string; prefix: string }[] = [
    { key: "artist", label: "Artist", prefix: "artist:" },
    { key: "character", label: "Character", prefix: "char:" },
    { key: "copyright", label: "Copyright", prefix: "copy:" },
    { key: "general", label: "General", prefix: "" },
    { key: "meta", label: "Meta", prefix: "meta:" },
    { key: "deprecated", label: "Deprecated", prefix: "" },
    { key: "unknown", label: "Uncategorized", prefix: "" },
  ];

  const fmtSize = (b: number) =>
    b >= 1048576 ? `${(b / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`;
  const fmtDate = (s: string) => {
    const d = new Date(s);
    return isNaN(d.getTime()) ? s : d.toLocaleString();
  };
  const open = (reveal: boolean) =>
    api.openLocal(item.local_path, reveal).catch((e) => (error = errorText(e)));
</script>

<aside class="panel">
  <div class="head">
    <h3>{item.provider} #{item.remote_id}</h3>
    <button class="ghost small" onclick={onclose} aria-label="Close">✕</button>
  </div>

  <div class="preview">
    {#if onview}
      <button class="viewbtn" onclick={onview} title="Open full screen (Enter)" aria-label="Open full screen">⤢</button>
    {/if}
    {#if isVideoExt(item.file_ext)}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video controls loop preload="metadata" poster={libraryThumbUrl(item)} src={libraryVideoUrl(item)}></video>
    {:else if isStill && !fullFailed}
      {#if !fullLoaded}<img class="under" src={libraryThumbUrl(item)} alt="" />{/if}
      <img
        class:hidden={!fullLoaded}
        src={libraryFileUrl(item)}
        alt={`${item.provider} ${item.remote_id}`}
        onload={() => (fullLoaded = true)}
        ondblclick={onview}
        onerror={() => (fullFailed = true)}
      />
    {:else}
      <img src={libraryThumbUrl(item)} alt={`${item.provider} ${item.remote_id}`} />
    {/if}
  </div>
  {#if fullFailed}<p class="err small">The original file couldn't be loaded. Was it moved or deleted?</p>{/if}

  <div class="actions">
    <button onclick={() => open(false)}>Open file</button>
    <button class="ghost" onclick={() => open(true)}>Show in folder</button>
    <button class="ghost" onclick={() => openUrl(item.post_url)}>Open post</button>
  </div>
  <div class="actions">
    <button class="ghost small" disabled={working} onclick={refreshMeta} title="Re-download tags and info from the board (the image isn't touched)">
      Refresh metadata
    </button>
    {#if !confirmDelete}
      <button class="ghost small danger" disabled={working} onclick={() => (confirmDelete = true)}>Delete…</button>
    {/if}
  </div>
  {#if confirmDelete}
    <div class="confirm">
      <p>Move this image, its metadata file and thumbnail to the <strong>Recycle Bin</strong> and remove it from the library?</p>
      <div class="actions">
        <button class="danger-solid" disabled={working} onclick={doDelete}>Move to Recycle Bin</button>
        <button class="ghost" disabled={working} onclick={() => (confirmDelete = false)}>Cancel</button>
      </div>
    </div>
  {/if}
  {#if note}<p class={`${note.kind} small`}>{note.text}</p>{/if}

  <dl>
    <dt>Resolution</dt><dd>{item.width ?? "?"} × {item.height ?? "?"}</dd>
    <dt>File</dt><dd>{item.file_ext.toUpperCase()} · {fmtSize(item.file_size)}</dd>
    <dt>Rating</dt><dd>{item.rating ?? "?"}</dd>
    <dt>Board</dt><dd>{item.provider}</dd>
    <dt>Downloaded</dt><dd>{fmtDate(item.downloaded_at)}</dd>
    {#if details?.created_at}<dt>Posted</dt><dd>{details.created_at}</dd>{/if}
    <dt>MD5</dt><dd class="mono">{item.md5}</dd>
    <dt>Path</dt><dd class="mono">{item.local_path}</dd>
    {#if details?.source}
      <dt>Source</dt>
      <dd>
        {#each details.source.split(/\s+/).filter(Boolean) as s}
          {#if s.startsWith("http")}
            <button class="link" onclick={() => openUrl(s)}>{s}</button>
          {:else}<span>{s}</span>{/if}
        {/each}
      </dd>
    {/if}
  </dl>

  {#if error}<p class="err">{error}</p>{/if}

  {#if details}
    {#each GROUPS as g}
      {@const list = details.tags[g.key]}
      {#if list.length}
        <section class="group">
          <h4>{g.label} <span class="dim">{list.length}</span></h4>
          <div class="tags">
            {#each list as t}
              <button
                class={`tag ${g.key}`}
                title="Click: filter by this tag · Shift+click: add to filter"
                onclick={(e) => onfilter(g.prefix + t, e.shiftKey)}>{t.replaceAll("_", " ")}</button
              >
            {/each}
          </div>
        </section>
      {/if}
    {/each}

    <button class="ghost small" onclick={() => (showFields = !showFields)}>
      {showFields ? "Hide" : "Show"} all saved fields ({Object.keys(details.fields ?? {}).length})
    </button>
    {#if showFields}
      <table class="fields">
        <tbody>
          {#each Object.entries(details.fields ?? {}).sort(([a], [b]) => a.localeCompare(b)) as [k, v]}
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
  h3 { margin: 0; font-size: 1rem; text-transform: capitalize; }
  .preview {
    position: relative; margin: 12px 0 6px; min-height: 160px; max-height: 520px;
    display: flex; align-items: center; justify-content: center;
    background: #111216; border-radius: 6px; overflow: hidden;
  }
  .preview img { max-width: 100%; max-height: 520px; object-fit: contain; display: block; }
  .preview .viewbtn {
    position: absolute; right: 6px; top: 6px; z-index: 2; padding: 2px 8px; font-size: 1rem;
    background: rgba(0, 0, 0, 0.6); border: 1px solid #444; border-radius: 5px; color: #eee; cursor: pointer;
  }
  .preview video { width: 100%; max-height: 520px; background: #000; display: block; }
  .preview img.under { position: absolute; inset: 0; margin: auto; filter: blur(2px); opacity: 0.6; }
  .preview img.hidden { visibility: hidden; position: absolute; }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; margin: 10px 0 4px; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; font-size: 0.86rem; margin: 12px 0; }
  dt { color: #9a9ba3; }
  dd { margin: 0; min-width: 0; overflow-wrap: anywhere; display: flex; flex-direction: column; gap: 2px; }
  .mono { font-family: ui-monospace, Consolas, monospace; font-size: 0.76rem; }
  .dim { color: #7d7e87; }
  .small { font-size: 0.8rem; }
  .err { color: #f07b7b; font-size: 0.85rem; }
  .ok { color: #6fd39a; }
  .confirm { background: #2a1d1f; border: 1px solid #5a2a2a; border-radius: 6px; padding: 8px 10px; margin: 6px 0; font-size: 0.85rem; }
  .confirm p { margin: 0 0 6px; }
  button.danger { color: #f07b7b; border-color: #5a2a2a; }
  button.danger-solid { background: #b8433f; border-color: #b8433f; }
  button:disabled { opacity: 0.45; cursor: default; }
  .group { margin: 12px 0; }
  h4 { margin: 0 0 6px; font-size: 0.82rem; text-transform: uppercase; letter-spacing: 0.05em; color: #b5b6bf; }
  .tags { display: flex; flex-wrap: wrap; gap: 4px; }
  .tag {
    border: none; border-radius: 4px; padding: 2px 7px; font: inherit; font-size: 0.8rem;
    cursor: pointer; background: #26272e; color: #d6d7dc;
  }
  .tag:hover { filter: brightness(1.3); }
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
  button.link {
    background: none; border: none; padding: 0; color: #8fa0ff; text-align: left; cursor: pointer;
    font-size: 0.82rem; overflow-wrap: anywhere;
  }
</style>
