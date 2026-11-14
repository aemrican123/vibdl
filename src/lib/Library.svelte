<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import LibraryTools from "$lib/LibraryTools.svelte";
  import LocalPanel from "$lib/LocalPanel.svelte";
  import TagInput from "$lib/TagInput.svelte";
  import VirtualGrid from "$lib/VirtualGrid.svelte";
  import SavedSearches from "$lib/SavedSearches.svelte";
  import Viewer, { type ViewerMedia } from "$lib/Viewer.svelte";
  import { isTyping, loadPref, savePref } from "$lib/prefs";
  import {
    api,
    errorText,
    isVideoExt,
    libraryFileUrl,
    libraryThumbUrl,
    libraryVideoUrl,
    type Facets,
    type LibraryItem,
    type LibrarySort,
  } from "$lib/api";

  let { active = true }: { active?: boolean } = $props();

  let query = $state(loadPref("library.query", ""));
  let activeQuery = $state("");
  let sort = $state<LibrarySort>(loadPref<LibrarySort>("library.sort", "newest"));
  let grid = $state<{ reveal: (i: number) => void; toTop: () => void } | null>(null);
  let searchBox = $state<{ focus: () => void } | null>(null);
  let items = $state<LibraryItem[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let facets = $state<Facets | null>(null);
  let selected = $state<LibraryItem | null>(null);
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
    picked = new Set();
    anchor = null;
    bulk = "none";
    savePref("library.query", activeQuery);
    savePref("library.sort", sort);
    grid?.toTop();
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
      if (r.total !== null) total = r.total;
    } catch (err) {
      if (gen === generation) error = errorText(err);
    } finally {
      if (gen === generation) loading = false;
    }
  }


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

  // ---- Multi-select & bulk actions
  const keyOf = (it: { provider: string; remote_id: string }) => `${it.provider}:${it.remote_id}`;
  let picked = $state<Set<string>>(new Set());
  let anchor: number | null = null;
  let selectMode = $state(false);
  let bulk = $state<"none" | "delete" | "export">("none");
  let busy = $state(false);
  let bulkMsg = $state<{ kind: "ok" | "err"; text: string } | null>(null);
  let progress = $state<{ done: number; total: number } | null>(null);
  let exportDest = $state(loadPref("library.exportDest", ""));
  let exportPattern = $state(loadPref("library.exportPattern", "{artist} - {copyright} - {board} {id}"));

  const pickedList = () =>
    [...picked].map((k) => {
      const i = k.indexOf(":");
      return { provider: k.slice(0, i), remote_id: k.slice(i + 1) };
    });

  function onTile(e: MouseEvent, it: LibraryItem, i: number) {
    if (e.shiftKey) {
      const si = indexOf(selected);
      const from = anchor ?? (si >= 0 ? si : i);
      const [a, b] = [Math.min(from, i), Math.max(from, i)];
      const next = new Set(picked);
      for (const x of items.slice(a, b + 1)) next.add(keyOf(x));
      picked = next;
      return;
    }
    anchor = i;
    if (e.ctrlKey || e.metaKey || selectMode) {
      const next = new Set(picked);
      const k = keyOf(it);
      if (next.has(k)) next.delete(k);
      else next.add(k);
      picked = next;
      return;
    }
    selected = it;
  }

  function clearPicked() {
    picked = new Set();
    bulk = "none";
  }

  async function selectAllMatching() {
    try {
      const keys = await api.libraryKeys(activeQuery);
      picked = new Set(keys.map(([p, r]) => `${p}:${r}`));
    } catch (e) {
      bulkMsg = { kind: "err", text: errorText(e) };
    }
  }

  const plural = (n: number, w: string) => `${n.toLocaleString()} ${w}${n === 1 ? "" : "s"}`;

  async function bulkDelete() {
    busy = true;
    bulkMsg = null;
    try {
      const r = await api.libraryDeleteMany(pickedList());
      bulkMsg = {
        kind: r.failed ? "err" : "ok",
        text: `Moved ${plural(r.done, "item")} to the Recycle Bin${r.failed ? `; ${r.failed} failed: ${r.errors[0] ?? ""}` : "."}`,
      };
      clearPicked();
      search();
    } catch (e) {
      bulkMsg = { kind: "err", text: errorText(e) };
    } finally {
      busy = false;
    }
  }

  async function bulkExport() {
    busy = true;
    bulkMsg = null;
    savePref("library.exportDest", exportDest);
    savePref("library.exportPattern", exportPattern);
    try {
      const r = await api.libraryExportSelected(pickedList(), exportDest, exportPattern);
      bulkMsg = {
        kind: r.errors.length ? "err" : "ok",
        text: `Copied ${plural(r.copied, "file")}${r.skipped_existing ? `, ${r.skipped_existing} already there` : ""} → ${r.dest}${r.errors.length ? ` · ${r.errors.length} error(s): ${r.errors[0]}` : ""}`,
      };
      bulk = "none";
    } catch (e) {
      bulkMsg = { kind: "err", text: errorText(e) };
    } finally {
      busy = false;
    }
  }

  async function bulkRefresh() {
    busy = true;
    bulkMsg = null;
    progress = { done: 0, total: picked.size };
    try {
      const r = await api.libraryRefreshMany(pickedList());
      bulkMsg = {
        kind: r.failed ? "err" : "ok",
        text: `Refreshed ${plural(r.done, "item")}${r.failed ? `; ${r.failed} failed: ${r.errors[0] ?? ""}` : "."}`,
      };
      search();
    } catch (e) {
      bulkMsg = { kind: "err", text: errorText(e) };
    } finally {
      busy = false;
      progress = null;
    }
  }

  function bulkReveal() {
    const first = items.find((x) => picked.has(keyOf(x)));
    if (first) api.openLocal(first.local_path, true).catch((e) => (bulkMsg = { kind: "err", text: errorText(e) }));
  }

  let unlisten: UnlistenFn | null = null;
  listen<{ done: number; total: number }>("bulk-progress", (e) => {
    if (progress) progress = e.payload;
  }).then((u) => (unlisten = u));
  onDestroy(() => unlisten?.());

  // Full-screen viewer.
  let viewerIndex = $state<number | null>(null);
  const indexOf = (it: LibraryItem | null) =>
    it ? items.findIndex((x) => x.provider === it.provider && x.remote_id === it.remote_id) : -1;

  function openViewer(i: number) {
    if (i < 0 || i >= items.length) return;
    selected = items[i];
    viewerIndex = i;
  }

  async function resolveMedia(i: number): Promise<ViewerMedia | null> {
    const it = items[i];
    if (!it) return null;
    const title = `${it.provider} #${it.remote_id}`;
    const subtitle = `${it.width ?? "?"}×${it.height ?? "?"} · ${it.file_ext.toUpperCase()}`;
    const placeholder = libraryThumbUrl(it);
    return isVideoExt(it.file_ext)
      ? { kind: "video", src: libraryVideoUrl(it), placeholder, title, subtitle }
      : { kind: "image", src: libraryFileUrl(it), placeholder, title, subtitle };
  }

  function viewerMove(i: number) {
    viewerIndex = i;
    selected = items[i];
    grid?.reveal(i);
    if (i >= items.length - 5) loadMore();
  }

  // Keyboard: "/" filter · Esc close · ←/→ previous/next item.
  function onKey(e: KeyboardEvent) {
    if (!active || view !== "gallery" || viewerIndex !== null || isTyping(e)) return;
    if (e.key === "Delete" && picked.size && !busy) {
      e.preventDefault();
      bulk = "delete";
      return;
    }
    if ((e.ctrlKey || e.metaKey) && (e.key === "a" || e.key === "A")) {
      e.preventDefault();
      selectAllMatching();
      return;
    }
    if (e.key === "Escape" && (bulk !== "none" || picked.size)) {
      if (bulk !== "none") bulk = "none";
      else clearPicked();
      return;
    }
    if (e.key === "Enter" && selected) {
      e.preventDefault();
      openViewer(indexOf(selected));
      return;
    }
    if (e.key === "/") {
      e.preventDefault();
      searchBox?.focus();
    } else if (e.key === "Escape" && selected) {
      selected = null;
    } else if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && selected) {
      e.preventDefault();
      const i = items.findIndex((x) => x.provider === selected!.provider && x.remote_id === selected!.remote_id);
      const j = e.key === "ArrowRight" ? i + 1 : i - 1;
      if (j >= 0 && j < items.length) {
        selected = items[j];
        grid?.reveal(j);
      }
    }
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

<svelte:window onkeydown={onKey} />

{#if viewerIndex !== null && active}
  <Viewer
    count={items.length}
    index={viewerIndex}
    resolve={resolveMedia}
    onindex={viewerMove}
    onclose={() => (viewerIndex = null)}
  />
{/if}

<div class="lib">
  <form class="bar" onsubmit={search}>
    <TagInput
      bind:this={searchBox}
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
    <SavedSearches
      scope="library"
      query={activeQuery}
      onrun={(q) => {
        query = q;
        search();
      }}
    />
    <button
      type="button"
      class="ghost"
      class:on={selectMode}
      title="Pick several items (or Ctrl+click / Shift+click tiles)"
      onclick={() => {
        selectMode = !selectMode;
        if (!selectMode) clearPicked();
      }}>{selectMode ? "Done" : "Select"}</button
    >
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

  {#if view === "gallery" && (picked.size || selectMode)}
    <div class="selbar">
      <strong>{picked.size.toLocaleString()} selected</strong>
      <button class="ghost" disabled={busy} onclick={selectAllMatching}>Select all {total.toLocaleString()}</button>
      <button class="ghost" disabled={busy || !picked.size} onclick={clearPicked}>Clear</button>
      <span class="sep"></span>
      <button class="ghost" disabled={busy || !picked.size} onclick={() => (bulk = bulk === "export" ? "none" : "export")}>Export / copy…</button>
      <button class="ghost" disabled={busy || !picked.size} onclick={bulkRefresh} title="Re-download tags and info from each board">Refresh metadata</button>
      <button class="ghost" disabled={busy || !picked.size} onclick={bulkReveal}>Show in folder</button>
      <button class="danger" disabled={busy || !picked.size} onclick={() => (bulk = "delete")}>Delete…</button>
      <span class="dim">Ctrl+click toggle · Shift+click range · Ctrl+A all · Del delete</span>
    </div>
    {#if bulk === "delete"}
      <div class="confirm">
        Move <strong>{plural(picked.size, "item")}</strong> (image, metadata file and thumbnail) to the
        <strong>Recycle Bin</strong> and remove them from the library?
        <button class="danger-solid" disabled={busy} onclick={bulkDelete}>{busy ? "Deleting…" : "Move to Recycle Bin"}</button>
        <button class="ghost" disabled={busy} onclick={() => (bulk = "none")}>Cancel</button>
      </div>
    {:else if bulk === "export"}
      <form
        class="confirm export"
        onsubmit={(e) => {
          e.preventDefault();
          bulkExport();
        }}
      >
        <label>Copy {plural(picked.size, "item")} to <input bind:value={exportDest} placeholder="D:\Exports\VIBDL" spellcheck="false" /></label>
        <button
          type="button"
          class="ghost"
          onclick={async () => {
            const p = await api.pickFolder("Copy selected items to…", exportDest);
            if (p) exportDest = p;
          }}>Browse…</button
        >
        <label>named <input bind:value={exportPattern} spellcheck="false" title="Tokens: {'{artist} {character} {copyright} {board} {id} {md5} {rating}'}" /></label>
        <button type="submit" disabled={busy || !exportDest.trim()}>{busy ? "Copying…" : "Copy"}</button>
        <button type="button" class="ghost" onclick={() => (bulk = "none")}>Cancel</button>
      </form>
    {/if}
  {/if}
  {#if progress}<p class="bulkmsg">Refreshing {progress.done} / {progress.total}…</p>{/if}
  {#if bulkMsg}
    <p class={`bulkmsg ${bulkMsg.kind}`}>{bulkMsg.text} <button class="x" onclick={() => (bulkMsg = null)} aria-label="Dismiss">✕</button></p>
  {/if}

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

    <VirtualGrid bind:this={grid} items={items} key={(it) => it.provider + it.remote_id} onend={() => loadMore()}>
      {#snippet before()}
        {#if !loading && total === 0 && !error}
          <p class="note">
            {activeQuery ? "Nothing matches these filters." : "Your library is empty. Download originals from the Browse tab."}
          </p>
        {/if}
      {/snippet}
      {#snippet tile(it, i)}
        <button
          class="tile"
          class:sel={selected?.provider === it.provider && selected?.remote_id === it.remote_id}
          class:picked={picked.has(keyOf(it))}
          onmousedown={(e) => e.shiftKey && e.preventDefault()}
          onclick={(e) => onTile(e, it, i)}
          ondblclick={() => openViewer(i)}
          title={`${it.provider} #${it.remote_id} · ${it.width ?? "?"}×${it.height ?? "?"}`}
        >
          {#if !failed.has(it.md5)}
            <img
              src={libraryThumbUrl(it)}
              alt={`${it.provider} ${it.remote_id}`}
              decoding="async"
              onerror={() => (failed = new Set(failed).add(it.md5))}
            />
          {:else}
            <span class="nothumb">{it.file_ext.toUpperCase()}</span>
          {/if}
          <span class="badge">{it.file_ext}</span>
          <span class="board">{it.provider}</span>
          {#if picked.has(keyOf(it))}<span class="mark">✓</span>{/if}
        </button>
      {/snippet}
      {#snippet after()}
        {#if error}<p class="err">{error}</p>{/if}
      {/snippet}
    </VirtualGrid>

    {#if selected}
      <LocalPanel
        item={selected}
        onclose={() => (selected = null)}
        onfilter={filterBy}
        ondeleted={removed}
        onview={() => openViewer(indexOf(selected))}
      />
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
  .tile {
    position: relative; width: 100%; height: 100%; padding: 0; border-radius: 6px; overflow: hidden;
    background: #1f2026; border: 2px solid transparent; cursor: pointer; color: inherit;
    display: flex; align-items: center; justify-content: center;
  }
  .tile:hover { border-color: #3a3b46; }
  .tile.sel { border-color: #4b5bd6; }
  .tile img { width: 100%; height: 100%; object-fit: contain; }
  .nothumb { color: #6b6c75; font-size: 0.85rem; }
  .badge {
    position: absolute; right: 4px; bottom: 4px; padding: 1px 5px; border-radius: 4px;
    background: rgba(0, 0, 0, 0.65); color: #ddd; font-size: 0.7rem; text-transform: uppercase;
  }
  .tile.picked { border-color: #8fa0ff; box-shadow: inset 0 0 0 2px #8fa0ff; }
  .tile.picked img { opacity: 0.75; }
  .mark {
    position: absolute; left: 4px; top: 4px; width: 20px; height: 20px; border-radius: 10px;
    background: #4b5bd6; color: #fff; font-size: 0.75rem; font-weight: 700;
    display: flex; align-items: center; justify-content: center;
  }
  .selbar {
    display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin: 0 16px 8px;
    padding: 8px 12px; background: #1d2030; border: 1px solid #343a63; border-radius: 8px; font-size: 0.86rem;
  }
  .selbar button, .confirm button {
    padding: 5px 10px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: #fff;
    font: inherit; font-size: 0.84rem; cursor: pointer;
  }
  .selbar button.ghost, .confirm button.ghost, .bar button.ghost {
    background: transparent; border-color: #4a4b55; color: #c9c9cf;
  }
  .bar button.ghost { padding: 8px 12px; border-radius: 6px; font: inherit; cursor: pointer; }
  .bar button.ghost.on { border-color: #8fa0ff; color: #fff; }
  .selbar button.danger { background: transparent; border-color: #5a2a2a; color: #f07b7b; }
  .confirm button.danger-solid { background: #b8433f; border-color: #b8433f; }
  .selbar button:disabled, .confirm button:disabled { opacity: 0.45; cursor: default; }
  .sep { width: 1px; height: 20px; background: #343a63; }
  .confirm {
    display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin: 0 16px 8px; padding: 8px 12px;
    background: #2a1d1f; border: 1px solid #5a2a2a; border-radius: 8px; font-size: 0.86rem;
  }
  .confirm.export { background: #1d2030; border-color: #343a63; }
  .confirm label { display: flex; gap: 6px; align-items: center; }
  .confirm input {
    padding: 5px 8px; background: #15161a; color: inherit; border: 1px solid #353640; border-radius: 5px;
    font: inherit; font-size: 0.84rem; width: 220px;
  }
  .bulkmsg { margin: 0 18px 8px; font-size: 0.85rem; color: #9a9ba3; }
  .bulkmsg.ok { color: #6fd39a; }
  .bulkmsg.err { color: #f07b7b; }
  .bulkmsg .x { background: none; border: none; color: inherit; cursor: pointer; }
  .tile { user-select: none; }
  .board {
    position: absolute; left: 4px; bottom: 4px; padding: 1px 5px; border-radius: 4px;
    background: rgba(0, 0, 0, 0.55); color: #bbb; font-size: 0.65rem; text-transform: capitalize;
  }
  .note { color: #9a9ba3; text-align: center; margin: 40px 0; }
  .err { color: #f07b7b; text-align: center; }
  .dim { color: #7d7e87; }
  .small { font-size: 0.8rem; }
  button[type="submit"] {
    padding: 8px 14px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: white;
    font: inherit; cursor: pointer;
  }
</style>
