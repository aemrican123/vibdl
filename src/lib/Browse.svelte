<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, errorText, thumbUrl, type BrowsePost } from "$lib/api";

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
  let failedThumbs = $state<Set<string>>(new Set());
  let sentinel = $state<HTMLDivElement | null>(null);

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
    failedThumbs = new Set();
    loadMore();
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
      posts = [...posts, ...page.posts.filter((p) => !seen.has(p.id))];
      total = page.total;
      hasMore = page.has_more;
      depthLimited = page.depth_limited;
      nextPage = page.page + 1;
    } catch (err) {
      if (gen !== generation) return;
      error = errorText(err);
      hasMore = false;
    } finally {
      if (gen === generation) loading = false;
    }
  }

  // Load the next page when the bottom sentinel scrolls near the viewport.
  $effect(() => {
    if (!sentinel) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) loadMore();
      },
      { rootMargin: "800px" },
    );
    io.observe(sentinel);
    return () => io.disconnect();
  });

  function markFailed(id: string) {
    failedThumbs = new Set(failedThumbs).add(id);
  }

  const fmtRes = (p: BrowsePost) => (p.width && p.height ? `${p.width}×${p.height}` : "?");
</script>

<div class="browse">
  <form class="searchbar" onsubmit={search}>
    <input
      bind:value={query}
      placeholder="Search Gelbooru tags, e.g. long_hair blue_eyes -monochrome"
      spellcheck="false"
      autocomplete="off"
    />
    <button type="submit" disabled={loading && posts.length === 0}>Search</button>
  </form>

  <div class="status">
    {#if activeQuery === null}
      Enter tags and press Search. Leave it empty to see the newest posts.
    {:else}
      {posts.length} loaded{total !== null ? ` of ${total.toLocaleString()} matches` : ""}
      {#if loading}· loading…{/if}
    {/if}
  </div>

  <div class="body">
    <div class="grid-wrap">
      <div class="grid">
        {#each posts as p (p.id)}
          <button
            class="tile"
            class:sel={selected?.id === p.id}
            onclick={() => (selected = p)}
            title={`#${p.id} · ${fmtRes(p)}`}
          >
            {#if p.thumb_key && !failedThumbs.has(p.id)}
              <img
                src={thumbUrl(p.thumb_key)}
                alt={`Post ${p.id}`}
                loading="lazy"
                decoding="async"
                onerror={() => markFailed(p.id)}
              />
            {:else}
              <span class="nothumb">no preview</span>
            {/if}
            {#if p.file_ext}<span class="badge">{p.file_ext}</span>{/if}
          </button>
        {/each}
      </div>

      <div bind:this={sentinel} class="sentinel"></div>

      {#if error}
        <p class="err">{error} <button class="ghost" onclick={() => { error = null; hasMore = true; loadMore(); }}>Retry</button></p>
      {:else if depthLimited}
        <p class="note">Reached Gelbooru's search depth limit (~20,000 results). Add tags to narrow the search.</p>
      {:else if activeQuery !== null && !hasMore && !loading}
        <p class="note">{posts.length ? "End of results." : "No results."}</p>
      {/if}
    </div>

    {#if selected}
      <aside class="info">
        <div class="row-between">
          <h3>Post #{selected.id}</h3>
          <button class="ghost small" onclick={() => (selected = null)} aria-label="Close">✕</button>
        </div>
        <dl>
          <dt>Resolution</dt><dd>{fmtRes(selected)}</dd>
          <dt>Format</dt><dd>{selected.file_ext ?? "?"}</dd>
          <dt>Rating</dt><dd>{selected.rating ?? "?"}</dd>
          <dt>Tags</dt><dd>{selected.tags.length}</dd>
        </dl>
        <button onclick={() => openUrl(selected!.post_url)}>Open on Gelbooru</button>
        <p class="hint">Tag categories, sample preview and downloading come in the next milestones.</p>
        <div class="tags">
          {#each selected.tags as t}<span>{t}</span>{/each}
        </div>
      </aside>
    {/if}
  </div>
</div>

<style>
  .browse { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .searchbar { display: flex; gap: 8px; padding: 14px 16px 6px; }
  .searchbar input {
    flex: 1; padding: 9px 12px; background: #15161a; color: inherit;
    border: 1px solid #353640; border-radius: 6px; font: inherit;
  }
  .status { padding: 2px 18px 8px; color: #9a9ba3; font-size: 0.85rem; }
  .body { flex: 1; display: flex; min-height: 0; }
  .grid-wrap { flex: 1; overflow-y: auto; padding: 0 16px 24px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
  .tile {
    position: relative; aspect-ratio: 1; padding: 0; border-radius: 6px; overflow: hidden;
    background: #1f2026; border: 2px solid transparent; cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    content-visibility: auto; contain-intrinsic-size: 150px 150px;
  }
  .tile:hover { border-color: #3a3b46; }
  .tile.sel { border-color: #4b5bd6; }
  .tile img { width: 100%; height: 100%; object-fit: contain; }
  .nothumb { color: #6b6c75; font-size: 0.8rem; }
  .badge {
    position: absolute; right: 4px; bottom: 4px; padding: 1px 5px; border-radius: 4px;
    background: rgba(0, 0, 0, 0.65); color: #ddd; font-size: 0.7rem; text-transform: uppercase;
  }
  .sentinel { height: 1px; }
  .note, .err { text-align: center; color: #9a9ba3; margin: 18px 0; }
  .err { color: #f07b7b; }
  .info {
    width: 300px; flex-shrink: 0; overflow-y: auto; padding: 14px 16px;
    border-left: 1px solid #2c2d35; background: #1a1b20;
  }
  .row-between { display: flex; justify-content: space-between; align-items: center; }
  h3 { margin: 0; font-size: 1rem; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; font-size: 0.88rem; margin: 12px 0 14px; }
  dt { color: #9a9ba3; }
  dd { margin: 0; }
  .hint { color: #7d7e87; font-size: 0.8rem; }
  .tags { display: flex; flex-wrap: wrap; gap: 4px; }
  .tags span { background: #26272e; border-radius: 4px; padding: 2px 6px; font-size: 0.78rem; }
  button {
    padding: 8px 14px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: white;
    font: inherit; cursor: pointer;
  }
  button.ghost { background: transparent; border-color: #4a4b55; color: #c9c9cf; }
  button.small { padding: 2px 8px; }
  button:disabled { opacity: 0.45; cursor: default; }
  .tile.sel, .tile { color: inherit; }
</style>
