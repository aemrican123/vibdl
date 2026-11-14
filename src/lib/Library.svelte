<script lang="ts">
  import { onMount } from "svelte";
  import LibraryTools from "$lib/LibraryTools.svelte";
  import LocalPanel from "$lib/LocalPanel.svelte";
  import TagInput from "$lib/TagInput.svelte";
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
  let view = $state<"gallery" | "tools">("gallery");

  function removed(it: LibraryItem) {
    items = items.filter((x) => !(x.provider === it.provider && x.remote_id === it.remote_id));
    total = Math.max(0, total - 1);
    if (selected?.provider === it.provider && selected?.remote_id === it.remote_id) selected = null;
  }

  function openFromTools(it: LibraryItem) {
    view = "gallery";
    selected = it;
  }

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

  // Keep "-" and a category prefix ("artist:", "char:", …) when completing.
  function splitLibrary(word: string): [string, string] {
    const m = word.match(/^(-?)((?:artist|character|char|copyright|copy|general|meta):)?/i);
    const lead = m ? m[0] : "";
    return [lead, word.slice(lead.length)];
  }

  const FACET_GROUPS: { key: keyof Facets; label: string; prefix: string }[] = [
    { key: "artist", label: "Artists", prefix: "artist:" },
    { key: "character", label: "Characters", prefix: "char:" },
    { key: "copyright", label: "Copyrights", prefix: "copy:" },
    { key: "general", label: "General tags", prefix: "" },
  ];
  const COLLAPSED = 10;
  let expanded = $state<Record<string, boolean>>({});
</script>

<div class="lib">
  <form class="bar" onsubmit={search}>
    <TagInput
      bind:value={query}
      placeholder="Filter: long_hair -monochrome artist:name char:name copy:series rating:g type:png board:danbooru"
      fetch={(w) => api.librarySuggest(w)}
      split={splitLibrary}
      onsubmit={() => search()}
    />
    <select bind:value={sort} onchange={() => search()} aria-label="Sort">
      <option value="newest">Newest downloads</option>
      <option value="oldest">Oldest downloads</option>
      <option value="resolution">Highest resolution</option>
      <option value="file_size">Largest files</option>
    </select>
    <button type="submit">Filter</button>
    <div class="seg" role="tablist">
      <button type="button" class:on={view === "gallery"} onclick={() => (view = "gallery")}>Gallery</button>
      <button type="button" class:on={view === "tools"} onclick={() => (view = "tools")}>Tools</button>
    </div>
  </form>

  <div class="status">
    {total.toLocaleString()} item{total === 1 ? "" : "s"}{activeQuery ? " match" : " in your library"}
    {#each terms as t}
      <button class="chip" onclick={() => removeTerm(t)} title="Remove filter">{t} ✕</button>
    {/each}
    {#if loading}<span class="dim">· loading…</span>{/if}
  </div>

  {#if view === "tools"}
    <div class="toolswrap">
      <LibraryTools query={activeQuery} {total} onopen={openFromTools} onchanged={() => search()} />
    </div>
  {/if}

  <div class="body" class:hidden={view !== "gallery"}>
    <aside class="facets">
      {#if facets}
        {#each FACET_GROUPS as g}
          {#if facets[g.key].length}
            <h4>{g.label}</h4>
            <ul>
              {#each expanded[g.key] ? facets[g.key] : facets[g.key].slice(0, COLLAPSED) as f}
                <li>
                  <button class={`facet ${g.key}`} onclick={() => filterBy(g.prefix + f.name)}>
                    <span>{f.name.replaceAll("_", " ")}</span><span class="n">{f.count}</span>
                  </button>
                </li>
              {/each}
            </ul>
            {#if facets[g.key].length > COLLAPSED}
              <button class="more" onclick={() => (expanded = { ...expanded, [g.key]: !expanded[g.key] })}>
                {expanded[g.key] ? "Show fewer" : `Show all ${facets[g.key].length}`}
              </button>
            {/if}
          {/if}
        {/each}
        {#if !facets.artist.length && !facets.character.length && !facets.copyright.length && !facets.general.length}
          <p class="dim small">No tags in these results.</p>
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
      <LocalPanel item={selected} onclose={() => (selected = null)} onfilter={filterBy} ondeleted={removed} />
    {/if}
  </div>
</div>

<style>
  .lib { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .bar { display: flex; gap: 8px; padding: 14px 16px 6px; }
  .bar select {
    padding: 9px 12px; background: #15161a; color: inherit; border: 1px solid #353640;
    border-radius: 6px; font: inherit;
  }
  .status { padding: 2px 18px 8px; color: #9a9ba3; font-size: 0.85rem; display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
  .chip {
    background: #26272e; border: 1px solid #353640; color: #d6d7dc; border-radius: 12px;
    padding: 1px 8px; font: inherit; font-size: 0.78rem; cursor: pointer;
  }
  .chip:hover { border-color: #f07b7b; }
  .body { flex: 1; display: flex; min-height: 0; }
  .body.hidden { display: none; }
  .toolswrap { flex: 1; overflow-y: auto; }
  .seg { display: flex; border: 1px solid #353640; border-radius: 6px; overflow: hidden; }
  .seg button {
    background: transparent; border: none; color: #9a9ba3; padding: 8px 12px; font: inherit; cursor: pointer;
  }
  .seg button.on { background: #2d2f3a; color: #fff; }
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
  .more {
    background: none; border: none; color: #8fa0ff; font: inherit; font-size: 0.78rem;
    padding: 2px 6px; cursor: pointer;
  }
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
