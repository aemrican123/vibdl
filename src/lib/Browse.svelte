<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import PostPanel from "$lib/PostPanel.svelte";
  import TagInput from "$lib/TagInput.svelte";
  import VirtualGrid from "$lib/VirtualGrid.svelte";
  import SavedSearches from "$lib/SavedSearches.svelte";
  import Viewer, { type ViewerMedia } from "$lib/Viewer.svelte";
  import { isTyping, loadPref, savePref } from "$lib/prefs";
  import { api, errorText, mediaUrl, thumbUrl, type BrowsePost, type Job } from "$lib/api";

  let {
    provider,
    providerName = "",
    allowsBatch = false,
    active = true,
  }: { provider: string; providerName?: string; allowsBatch?: boolean; active?: boolean } = $props();

  let grid = $state<{ reveal: (i: number) => void; toTop: () => void } | null>(null);
  let searchBox = $state<{ focus: () => void } | null>(null);

  let query = $state("");
  let activeQuery = $state<string | null>(null);
  let posts = $state<BrowsePost[]>([]);
  let nextPage = $state(0);
  let hasMore = $state(false);
  let depthLimited = $state(false);
  let total = $state<number | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let selected = $state<BrowsePost | null>(null);
  // Per-post thumbnail retry count; > MAX_THUMB_RETRIES = give up.
  let thumbRetries = $state<Record<string, number>>({});
  const MAX_THUMB_RETRIES = 2;

  // Multi-select for the download queue.
  let selectMode = $state(false);
  let picked = $state<Set<string>>(new Set());
  let lastPicked: number | null = null;
  // Posts already in the library, and ones currently queued.
  let have = $state<Set<string>>(new Set());
  let queued = $state<Set<string>>(new Set());
  let queueMsg = $state<{ kind: "ok" | "err"; text: string } | null>(null);

  // Bumped on every new search so late responses from an old search are ignored.
  let generation = 0;

  function search(e?: Event) {
    e?.preventDefault();
    generation++;
    activeQuery = query.trim();
    posts = [];
    nextPage = 0;
    hasMore = true;
    depthLimited = false;
    total = null;
    selected = null;
    error = null;
    thumbRetries = {};
    picked = new Set();
    lastPicked = null;
    savePref(`browse.query.${provider}`, activeQuery);
    grid?.toTop();
    loadMore();
  }

  // Open with the last search for this board, or the newest posts.
  onMount(() => {
    query = loadPref<string | null>(`browse.query.${provider}`, null) ?? "";
    search();
  });

  // Full-screen viewer.
  let viewerIndex = $state<number | null>(null);

  function openViewer(i: number) {
    if (i < 0 || i >= posts.length) return;
    selected = posts[i];
    viewerIndex = i;
  }

  async function resolveMedia(i: number): Promise<ViewerMedia | null> {
    const p = posts[i];
    if (!p) return null;
    const thumb = p.thumb_key ? thumbUrl(p.thumb_key) : null;
    const title = `${p.provider} #${p.id}`;
    const subtitle = `${fmtRes(p)}${p.file_ext ? " · " + p.file_ext.toUpperCase() : ""}`;
    try {
      const d = await api.postDetails(p.provider, p.id);
      if (d.video_key) return { kind: "video", src: mediaUrl(d.video_key), placeholder: thumb, title, subtitle };
      if (d.sample_key)
        return {
          kind: "image",
          src: thumbUrl(d.sample_key),
          placeholder: thumb,
          originalSrc: d.original_key && !d.preview_is_original ? thumbUrl(d.original_key) : null,
          title,
          subtitle,
        };
    } catch {
      /* fall back to the thumbnail */
    }
    return thumb ? { kind: "image", src: thumb, title, subtitle } : null;
  }

  function viewerMove(i: number) {
    viewerIndex = i;
    selected = posts[i];
    grid?.reveal(i);
    if (i >= posts.length - 5) loadMore();
  }

  // Keyboard: "/" search · Esc close/exit · ←/→ previous/next post.
  function onKey(e: KeyboardEvent) {
    if (!active || viewerIndex !== null || isTyping(e)) return;
    if (e.key === "Enter" && selected) {
      e.preventDefault();
      openViewer(posts.findIndex((p) => p.id === selected!.id));
      return;
    }
    if (e.key === "/") {
      e.preventDefault();
      searchBox?.focus();
    } else if (e.key === "Escape") {
      if (selected) selected = null;
      else if (selectMode) {
        selectMode = false;
        clearPicked();
      }
    } else if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && selected) {
      e.preventDefault();
      const i = posts.findIndex((p) => p.id === selected!.id);
      const j = e.key === "ArrowRight" ? i + 1 : i - 1;
      if (j >= 0 && j < posts.length) {
        selected = posts[j];
        grid?.reveal(j);
      }
    }
  }

  async function loadMore() {
    if (loading || !hasMore || activeQuery === null) return;
    const gen = generation;
    loading = true;
    try {
      const page = await api.browseSearch(activeQuery, nextPage);
      if (gen !== generation) return;
      // New uploads shift pages while you scroll; skip posts we already have.
      const seen = new Set(posts.map((p) => p.id));
      const fresh = page.posts.filter((p) => !seen.has(p.id));
      posts = [...posts, ...fresh];
      total = page.total;
      hasMore = page.has_more;
      depthLimited = page.depth_limited;
      nextPage = page.page + 1;
      markDownloaded(fresh);
    } catch (err) {
      if (gen !== generation) return;
      error = errorText(err);
      hasMore = false;
    } finally {
      if (gen === generation) loading = false;
    }
  }

  async function markDownloaded(list: BrowsePost[]) {
    if (!list.length) return;
    try {
      const ids = await api.localStatusMany(provider, list.map((p) => ({ id: p.id, md5: p.md5 })));
      if (ids.length) have = new Set([...have, ...ids]);
    } catch {
      /* library unavailable: no badges */
    }
  }

  // Keep badges in sync with the queue.
  let unlisten: UnlistenFn | null = null;
  listen<Job[]>("queue-updated", (e) => {
    const mine = e.payload.filter((j) => j.provider === provider);
    queued = new Set(mine.filter((j) => j.state === "queued" || j.state === "downloading").map((j) => j.id));
    const done = mine.filter((j) => j.state === "done" || j.state === "duplicate").map((j) => j.id);
    if (done.some((id) => !have.has(id))) have = new Set([...have, ...done]);
  }).then((u) => (unlisten = u));
  onDestroy(() => unlisten?.());

  // From the detail panel: click = search that tag, shift+click = add it.
  function searchTag(tag: string, add: boolean) {
    const current = query.trim().split(/\s+/).filter(Boolean);
    query = add ? [...current.filter((t) => t !== tag), tag].join(" ") : tag;
    search();
  }

  function onTile(e: MouseEvent, p: BrowsePost, index: number) {
    const toggle = selectMode || e.ctrlKey || e.metaKey || e.shiftKey;
    if (!toggle) {
      selected = p;
      return;
    }
    if (!allowsBatch) {
      queueMsg = { kind: "err", text: `Multi-download isn't available for ${providerName || provider}.` };
      return;
    }
    const next = new Set(picked);
    if (e.shiftKey && lastPicked !== null) {
      const [a, b] = [Math.min(lastPicked, index), Math.max(lastPicked, index)];
      for (const q of posts.slice(a, b + 1)) if (!have.has(q.id)) next.add(q.id);
    } else if (next.has(p.id)) {
      next.delete(p.id);
    } else {
      next.add(p.id);
    }
    lastPicked = index;
    picked = next;
  }

  function selectAllLoaded() {
    picked = new Set(posts.filter((p) => !have.has(p.id) && !queued.has(p.id)).map((p) => p.id));
  }

  function clearPicked() {
    picked = new Set();
    lastPicked = null;
  }

  async function downloadPicked() {
    const items = posts.filter((p) => picked.has(p.id)).map((p) => ({ id: p.id, thumb_key: p.thumb_key }));
    if (!items.length) return;
    try {
      const n = await api.queueAdd(provider, items);
      queueMsg = {
        kind: "ok",
        text: n ? `Queued ${n} download${n === 1 ? "" : "s"}. See the Downloads tab.` : "Those are already queued or downloaded.",
      };
      clearPicked();
      selectMode = false;
    } catch (err) {
      queueMsg = { kind: "err", text: errorText(err) };
    }
  }

  // The image server sometimes says "slow down"; the backend already waits
  // and retries, and if a tile still fails we try it again a bit later.
  function thumbFailed(id: string) {
    const n = (thumbRetries[id] ?? 0) + 1;
    const gen = generation;
    if (n > MAX_THUMB_RETRIES) {
      thumbRetries = { ...thumbRetries, [id]: n };
      return;
    }
    setTimeout(() => {
      if (gen === generation) thumbRetries = { ...thumbRetries, [id]: n };
    }, 15000 * n);
  }

  const thumbSrc = (key: string, id: string) =>
    thumbRetries[id] ? `${thumbUrl(key)}?retry=${thumbRetries[id]}` : thumbUrl(key);

  const fmtRes = (p: BrowsePost) => (p.width && p.height ? `${p.width}×${p.height}` : "?");
</script>

<svelte:window onkeydown={onKey} />

{#if viewerIndex !== null && active}
  <Viewer
    count={posts.length}
    index={viewerIndex}
    resolve={resolveMedia}
    onindex={viewerMove}
    onclose={() => (viewerIndex = null)}
  />
{/if}

<div class="browse">
  <form class="searchbar" onsubmit={search}>
    <TagInput
      bind:this={searchBox}
      bind:value={query}
      placeholder="Search tags, e.g. long_hair blue_eyes (Danbooru: max 2 tags without Gold)"
      fetch={(w) => api.suggestTags(w)}
      onsubmit={() => search()}
    />
    <button type="submit" disabled={loading && posts.length === 0}>Search</button>
    <SavedSearches
      scope={provider}
      query={activeQuery ?? ""}
      onrun={(q) => {
        query = q;
        search();
      }}
    />
    <button
      type="button"
      class="ghost"
      class:on={selectMode}
      disabled={!allowsBatch}
      title={allowsBatch
        ? "Pick several posts to download (or Ctrl/Shift+click tiles)"
        : `Multi-download is disabled for ${providerName || provider}`}
      onclick={() => {
        selectMode = !selectMode;
        if (!selectMode) clearPicked();
      }}>{selectMode ? "Done selecting" : "Select"}</button
    >
  </form>

  <div class="status">
    {#if activeQuery === null}
      Loading the newest posts…
    {:else}
      {posts.length} loaded{total !== null ? ` of ${total.toLocaleString()} matches` : ""}
      {#if loading}· loading…{/if}
    {/if}
  </div>

  {#if picked.size || selectMode}
    <div class="selbar">
      <strong>{picked.size} selected</strong>
      <button disabled={!picked.size} onclick={downloadPicked}>Download {picked.size || ""} originals</button>
      <button class="ghost" onclick={selectAllLoaded} disabled={!posts.length}>Select all loaded (not yet downloaded)</button>
      <button class="ghost" onclick={clearPicked} disabled={!picked.size}>Clear</button>
      <span class="dim">Ctrl+click toggles · Shift+click selects a range</span>
    </div>
  {/if}
  {#if queueMsg}
    <p class={`qmsg ${queueMsg.kind}`}>
      {queueMsg.text}
      <button class="ghost tiny" onclick={() => (queueMsg = null)} aria-label="Dismiss">✕</button>
    </p>
  {/if}

  <div class="body">
    <VirtualGrid bind:this={grid} items={posts} key={(p) => p.id} onend={loadMore}>
      {#snippet tile(p, i)}
        <button
          class="tile"
          class:sel={selected?.id === p.id}
          class:picked={picked.has(p.id)}
          onclick={(e) => onTile(e, p, i)}
          ondblclick={() => openViewer(i)}
          title={`#${p.id} · ${fmtRes(p)}`}
        >
          {#if p.thumb_key && (thumbRetries[p.id] ?? 0) <= MAX_THUMB_RETRIES}
            <img src={thumbSrc(p.thumb_key, p.id)} alt={`Post ${p.id}`} decoding="async" onerror={() => thumbFailed(p.id)} />
          {:else}
            <span class="nothumb">no preview</span>
          {/if}
          {#if p.file_ext}<span class="badge">{p.file_ext}</span>{/if}
          {#if have.has(p.id)}
            <span class="mark have" title="In your library">✓</span>
          {:else if queued.has(p.id)}
            <span class="mark queued" title="Queued">⇣</span>
          {:else if picked.has(p.id)}
            <span class="mark pick">●</span>
          {/if}
        </button>
      {/snippet}
      {#snippet after()}
        {#if error}
          <p class="err">{error} <button class="ghost" onclick={() => { error = null; hasMore = true; loadMore(); }}>Retry</button></p>
        {:else if depthLimited}
          <p class="note">Reached this board's search depth limit. Add tags to narrow the search.</p>
        {:else if activeQuery !== null && !hasMore && !loading}
          <p class="note">{posts.length ? "End of results." : "No results."}</p>
        {/if}
      {/snippet}
    </VirtualGrid>

    {#if selected}
      <PostPanel
        post={selected}
        onclose={() => (selected = null)}
        ontag={searchTag}
        onview={() => openViewer(posts.findIndex((p) => p.id === selected?.id))}
      />
    {/if}
  </div>
</div>

<style>
  .browse { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .searchbar { display: flex; gap: 8px; padding: 14px 16px 6px; }
  .status { padding: 2px 18px 8px; color: #9a9ba3; font-size: 0.85rem; }
  .selbar {
    display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin: 0 16px 8px;
    padding: 8px 12px; background: #1d2030; border: 1px solid #343a63; border-radius: 8px; font-size: 0.88rem;
  }
  .qmsg { margin: 0 18px 8px; font-size: 0.85rem; }
  .qmsg.ok { color: #6fd39a; }
  .qmsg.err { color: #f07b7b; }
  .dim { color: #7d7e87; font-size: 0.8rem; }
  .body { flex: 1; display: flex; min-height: 0; }
  .tile {
    position: relative; width: 100%; height: 100%; padding: 0; border-radius: 6px; overflow: hidden;
    background: #1f2026; border: 2px solid transparent; cursor: pointer; color: inherit;
    display: flex; align-items: center; justify-content: center;
  }
  .tile:hover { border-color: #3a3b46; }
  .tile.sel { border-color: #4b5bd6; }
  .tile.picked { border-color: #8fa0ff; box-shadow: inset 0 0 0 2px #8fa0ff; }
  .tile.picked img { opacity: 0.75; }
  .tile img { width: 100%; height: 100%; object-fit: contain; }
  .nothumb { color: #6b6c75; font-size: 0.8rem; }
  .badge {
    position: absolute; right: 4px; bottom: 4px; padding: 1px 5px; border-radius: 4px;
    background: rgba(0, 0, 0, 0.65); color: #ddd; font-size: 0.7rem; text-transform: uppercase;
  }
  .mark {
    position: absolute; left: 4px; top: 4px; min-width: 20px; height: 20px; border-radius: 10px;
    display: flex; align-items: center; justify-content: center; font-size: 0.75rem; font-weight: 700;
  }
  .mark.have { background: #1d6b40; color: #fff; }
  .mark.queued { background: #6b5a1d; color: #fff; }
  .mark.pick { background: #4b5bd6; color: #fff; }
  .note, .err { text-align: center; color: #9a9ba3; margin: 18px 0; }
  .err { color: #f07b7b; }
  button {
    padding: 8px 14px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: white;
    font: inherit; cursor: pointer;
  }
  .selbar button { padding: 5px 10px; font-size: 0.85rem; }
  button.ghost { background: transparent; border-color: #4a4b55; color: #c9c9cf; }
  button.ghost.on { border-color: #8fa0ff; color: #fff; }
  button.tiny { padding: 0 6px; font-size: 0.75rem; }
  button:disabled { opacity: 0.45; cursor: default; }
</style>
