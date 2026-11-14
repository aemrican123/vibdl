<script lang="ts">
  import { onMount } from "svelte";
  import LocalPanel from "$lib/LocalPanel.svelte";
  import {
    api,
    errorText,
    libraryThumbUrl,
    type Facets,
    type LibraryItem,
    type LibrarySort,
  } from "$lib/api";

  let query = $state("");
  let activeQuery = $state("");
  let sort = $state<LibrarySort>("newest");
  let items = $state<LibraryItem[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let facets = $state<Facets | null>(null);
  let selected = $state<LibraryItem | null>(null);
  let sentinel = $state<HTMLDivElement | null>(null);
  let failed = $state<Set<string>>(new Set());
  let generation = 0;

  const hasMore = $derived(items.length < total);

  function search(e?: Event) {
    e?.preventDefault();
    generation++;
    activeQuery = query.trim();
    items = [];
    total = 0;
    error = null;
    selected = null;
    failed = new Set();
    loadMore(true);
    const gen = generation;
    api
      .libraryFacets(activeQuery)
      .then((f) => gen === generation && (facets = f))
      .catch(() => {});
  }

  async function loadMore(first = false) {
    if (loading || (!first && !hasMore)) return;
    const gen = generation;
    loading = true;
    try {
      const r = await api.librarySearch(activeQuery, sort, items.length);
      if (gen !== generation) return;
      items = [...items, ...r.items];
      total = r.total;
    } catch (err) {
      if (gen === generation) error = errorText(err);
    } finally {
      if (gen === generation) loading = false;
    }
  }

  $effect(() => {
    if (!sentinel) return;
    const io = new IntersectionObserver((es) => es.some((e) => e.isIntersecting) && loadMore(), {
      rootMargin: "800px",
    });
    io.observe(sentinel);
    return () => io.disconnect();
  });

  /** Add a filter term (or replace the search with it). */
  function filterBy(term: string, add = true) {
    const parts = query.trim().split(/\s+/).filter(Boolean);
    query = add ? [...parts.filter((p) => p !== term), term].join(" ") : term;
    search();
  }

  function removeTerm(term: string) {
    query = query.trim().split(/\s+/).filter((p) => p && p !== term).join(" ");
    search();
  }

  const terms = $derived(activeQuery.split(/\s+/).filter(Boolean));

  onMount(() => search());

  /** Re-run when the Library tab is shown again (new downloads). */
  export function refresh() {
    search();
  }

  const FACET_GROUPS: { key: keyof Facets; label: string; prefix: string }[] = [
    { key: "artist", label: "Artists", prefix: "artist:" },
    { key: "character", label: "Characters", prefix: "char:" },
    { key: "copyright", label: "Copyrights", prefix: "copy:" },
  ];
</script>

<div class="lib">
  <form class="bar" onsubmit={search}>
    <input
      bind:value={query}
      placeholder="Filter: long_hair -monochrome artist:name char:name copy:series rating:g type:png board:danbooru"
      spellcheck="false"
      autocomplete="off"
    />
    <select bind:value={sort} onchange={() => search()} aria-label="Sort">
      <option value="newest">Newest downloads</option>
      <option value="oldest">Oldest downloads</option>
      <option value="resolution">Highest resolution</option>
      <option value="file_size">Largest files</option>
    </select>
    <button type="submit">Filter</button>
  </form>

  <div class="status">
    {total.toLocaleString()} item{total === 1 ? "" : "s"}{activeQuery ? " match" : " in your library"}
    {#each terms as t}
      <button class="chip" onclick={() => removeTerm(t)} title="Remove filter">{t} ✕</button>
    {/each}
    {#if loading}<span class="dim">· loading…</span>{/if}
  </div>

  <div class="body">
    <aside class="facets">
      {#if facets}
        {#each FACET_GROUPS as g}
          {#if facets[g.key].length}
            <h4>{g.label}</h4>
            <ul>
              {#each facets[g.key] as f}
                <li>
                  <button class={`facet ${g.key}`} onclick={() => filterBy(g.prefix + f.name)}>
                    <span>{f.name.replaceAll("_", " ")}</span><span class="n">{f.count}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        {/each}
        {#if !facets.artist.length && !facets.character.length && !facets.copyright.length}
          <p class="dim small">No artist/character/copyright tags in these results.</p>
        {/if}
      {/if}
    </aside>

    <div class="grid-wrap">
      {#if !loading && total === 0 && !error}
        <p class="note">
          {activeQuery ? "Nothing matches these filters." : "Your library is empty. Download originals from the Browse tab."}
        </p>
      {/if}
      <div class="grid">
        {#each items as it (it.provider + it.remote_id)}
          <button
            class="tile"
            class:sel={selected?.provider === it.provider && selected?.remote_id === it.remote_id}
            onclick={() => (selected = it)}
            title={`${it.provider} #${it.remote_id} · ${it.width ?? "?"}×${it.height ?? "?"}`}
          >
            {#if !failed.has(it.md5)}
              <img
                src={libraryThumbUrl(it)}
                alt={`${it.provider} ${it.remote_id}`}
                loading="lazy"
                decoding="async"
                onerror={() => (failed = new Set(failed).add(it.md5))}
              />
            {:else}
              <span class="nothumb">{it.file_ext.toUpperCase()}</span>
            {/if}
            <span class="badge">{it.file_ext}</span>
          </button>
        {/each}
      </div>
      <div bind:this={sentinel} class="sentinel"></div>
      {#if error}<p class="err">{error}</p>{/if}
    </div>

    {#if selected}
      <LocalPanel item={selected} onclose={() => (selected = null)} onfilter={filterBy} />
    {/if}
  </div>
</div>

<style>
  .lib { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .bar { display: flex; gap: 8px; padding: 14px 16px 6px; }
  .bar input, .bar select {
    padding: 9px 12px; background: #15161a; color: inherit; border: 1px solid #353640;
    border-radius: 6px; font: inherit;
  }
  .bar input { flex: 1; }
  .status { padding: 2px 18px 8px; color: #9a9ba3; font-size: 0.85rem; display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
  .chip {
    background: #26272e; border: 1px solid #353640; color: #d6d7dc; border-radius: 12px;
    padding: 1px 8px; font: inherit; font-size: 0.78rem; cursor: pointer;
  }
  .chip:hover { border-color: #f07b7b; }
  .body { flex: 1; display: flex; min-height: 0; }
  .facets { width: 210px; flex-shrink: 0; overflow-y: auto; padding: 0 8px 24px 16px; }
  h4 { margin: 12px 0 6px; font-size: 0.78rem; text-transform: uppercase; letter-spacing: 0.05em; color: #b5b6bf; }
  ul { list-style: none; margin: 0; padding: 0; }
  .facet {
    width: 100%; display: flex; justify-content: space-between; gap: 6px; text-align: left;
    background: none; border: none; color: #d6d7dc; padding: 3px 6px; border-radius: 4px;
    font: inherit; font-size: 0.82rem; cursor: pointer;
  }
  .facet:hover { background: #26272e; }
  .facet span:first-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .facet.artist { color: #ff8a8a; }
  .facet.character { color: #7fdc8f; }
  .facet.copyright { color: #d19bff; }
  .n { color: #7d7e87; flex-shrink: 0; }
  .grid-wrap { flex: 1; overflow-y: auto; padding: 0 16px 24px 8px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
  .tile {
    position: relative; aspect-ratio: 1; padding: 0; border-radius: 6px; overflow: hidden;
    background: #1f2026; border: 2px solid transparent; cursor: pointer; color: inherit;
    display: flex; align-items: center; justify-content: center;
    content-visibility: auto; contain-intrinsic-size: 150px 150px;
  }
  .tile:hover { border-color: #3a3b46; }
  .tile.sel { border-color: #4b5bd6; }
  .tile img { width: 100%; height: 100%; object-fit: contain; }
  .nothumb { color: #6b6c75; font-size: 0.85rem; }
  .badge {
    position: absolute; right: 4px; bottom: 4px; padding: 1px 5px; border-radius: 4px;
    background: rgba(0, 0, 0, 0.65); color: #ddd; font-size: 0.7rem; text-transform: uppercase;
  }
  .sentinel { height: 1px; }
  .note { color: #9a9ba3; text-align: center; margin: 40px 0; }
  .err { color: #f07b7b; text-align: center; }
  .dim { color: #7d7e87; }
  .small { font-size: 0.8rem; }
  button[type="submit"] {
    padding: 8px 14px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: white;
    font: inherit; cursor: pointer;
  }
</style>
