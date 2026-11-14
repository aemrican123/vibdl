<script lang="ts">
  import PostPanel from "$lib/PostPanel.svelte";
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
  // Per-post thumbnail retry count; > MAX_THUMB_RETRIES = give up.
  let thumbRetries = $state<Record<string, number>>({});
  const MAX_THUMB_RETRIES = 2;
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
    thumbRetries = {};
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

  // From the detail panel: click = search that tag, shift+click = add it.
  function searchTag(tag: string, add: boolean) {
    const current = query.trim().split(/\s+/).filter(Boolean);
    query = add ? [...current.filter((t) => t !== tag), tag].join(" ") : tag;
    search();
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

<div class="browse">
  <form class="searchbar" onsubmit={search}>
    <input
      bind:value={query}
      placeholder="Search tags, e.g. long_hair blue_eyes (Danbooru: max 2 tags without Gold)"
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
            {#if p.thumb_key && (thumbRetries[p.id] ?? 0) <= MAX_THUMB_RETRIES}
              <img
                src={thumbSrc(p.thumb_key, p.id)}
                alt={`Post ${p.id}`}
                loading="lazy"
                decoding="async"
                onerror={() => thumbFailed(p.id)}
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
      <PostPanel post={selected} onclose={() => (selected = null)} ontag={searchTag} />
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
  button {
    padding: 8px 14px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: white;
    font: inherit; cursor: pointer;
  }
  button.ghost { background: transparent; border-color: #4a4b55; color: #c9c9cf; }
  button:disabled { opacity: 0.45; cursor: default; }
  .tile.sel, .tile { color: inherit; }
</style>
