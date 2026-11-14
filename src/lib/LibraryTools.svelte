<script lang="ts">
  import {
    api,
    errorText,
    fmtSize,
    libraryThumbUrl,
    type CheckReport,
    type ExportReport,
    type LibraryItem,
    type SimilarReport,
  } from "$lib/api";

  let {
    query,
    total,
    onopen,
    onchanged,
  }: {
    /** Current Library filter; export uses it. */
    query: string;
    total: number;
    onopen: (item: LibraryItem) => void;
    /** Something was deleted/removed: reload the gallery. */
    onchanged: () => void;
  } = $props();

  // Export
  let dest = $state("");
  let pattern = $state("{artist} - {copyright} - {board} {id}");
  let exporting = $state(false);
  let exportReport = $state<ExportReport | null>(null);
  let exportError = $state<string | null>(null);

  async function runExport() {
    exporting = true;
    exportError = null;
    exportReport = null;
    try {
      exportReport = await api.libraryExport(query, dest, pattern);
    } catch (e) {
      exportError = errorText(e);
    } finally {
      exporting = false;
    }
  }

  // Similar images
  let scanning = $state(false);
  let similar = $state<SimilarReport | null>(null);
  let similarError = $state<string | null>(null);
  let confirmKey = $state<string | null>(null);

  async function runSimilar() {
    scanning = true;
    similarError = null;
    try {
      similar = await api.librarySimilar();
    } catch (e) {
      similarError = errorText(e);
    } finally {
      scanning = false;
    }
  }

  async function deleteItem(it: LibraryItem) {
    try {
      await api.libraryDelete(it.provider, it.remote_id);
      confirmKey = null;
      if (similar) {
        similar = {
          ...similar,
          groups: similar.groups
            .map((g) => g.filter((x) => !(x.provider === it.provider && x.remote_id === it.remote_id)))
            .filter((g) => g.length > 1),
        };
      }
      onchanged();
    } catch (e) {
      similarError = errorText(e);
    }
  }

  // Check
  let checking = $state(false);
  let check = $state<CheckReport | null>(null);
  let checkMsg = $state<string | null>(null);

  async function runCheck() {
    checking = true;
    checkMsg = null;
    try {
      check = await api.libraryCheck();
    } catch (e) {
      checkMsg = errorText(e);
    } finally {
      checking = false;
    }
  }

  async function removeMissing() {
    checking = true;
    try {
      const n = await api.libraryRemoveMissing();
      checkMsg = `Removed ${n} entr${n === 1 ? "y" : "ies"} from the index.`;
      check = await api.libraryCheck();
      onchanged();
    } catch (e) {
      checkMsg = errorText(e);
    } finally {
      checking = false;
    }
  }

  const key = (it: LibraryItem) => `${it.provider}:${it.remote_id}`;
</script>

<div class="tools">
  <section class="card">
    <h2>Export with readable names</h2>
    <p class="hint">
      Copies the <strong>{total.toLocaleString()}</strong> item{total === 1 ? "" : "s"} matching your current filter
      {query ? `(“${query}”)` : "(whole library)"} to a folder. Originals are copied unchanged; nothing is overwritten.
    </p>
    <div class="pickrow">
      <label>Destination folder <input bind:value={dest} placeholder="e.g. D:\Exports\VIBDL" spellcheck="false" /></label>
      <button
        class="ghost"
        onclick={async () => {
          const p = await api.pickFolder("Export to folder", dest);
          if (p) dest = p;
        }}>Browse…</button
      >
    </div>
    <label>
      Name pattern <input bind:value={pattern} spellcheck="false" />
      <span class="hint small">Tokens: {"{artist} {character} {copyright} {board} {id} {md5} {rating}"}</span>
    </label>
    <button disabled={exporting || !dest.trim() || total === 0} onclick={runExport}>
      {exporting ? "Exporting…" : `Export ${total.toLocaleString()} item${total === 1 ? "" : "s"}`}
    </button>
    {#if exportError}<p class="err">{exportError}</p>{/if}
    {#if exportReport}
      <p class="ok">
        Copied {exportReport.copied}{exportReport.skipped_existing ? `, skipped ${exportReport.skipped_existing} already there` : ""} → {exportReport.dest}
      </p>
      {#each exportReport.errors as e}<p class="err small">{e}</p>{/each}
    {/if}
  </section>

  <section class="card">
    <h2>Similar images</h2>
    <p class="hint">
      Exact duplicates are already blocked when downloading. This finds near-duplicates (resized or re-saved versions)
      by comparing thumbnails. The first scan takes a while; later scans only check new items.
    </p>
    <button disabled={scanning} onclick={runSimilar}>{scanning ? "Scanning…" : "Find similar images"}</button>
    {#if similarError}<p class="err">{similarError}</p>{/if}
    {#if similar}
      <p class="hint">
        Scanned {similar.scanned}{similar.skipped ? `, skipped ${similar.skipped} without a thumbnail` : ""} ·
        {similar.groups.length} group{similar.groups.length === 1 ? "" : "s"} found
      </p>
      {#each similar.groups as g, gi}
        <div class="group">
          <div class="gtitle">Group {gi + 1}</div>
          <div class="members">
            {#each g as it, i (key(it))}
              <div class="member">
                <button class="thumb" onclick={() => onopen(it)} title="Open in Library">
                  <img src={libraryThumbUrl(it)} alt="" loading="lazy" />
                </button>
                <div class="meta">
                  <div>{it.provider} #{it.remote_id}{i === 0 ? " · largest" : ""}</div>
                  <div class="dim">{it.width ?? "?"}×{it.height ?? "?"} · {it.file_ext.toUpperCase()} · {fmtSize(it.file_size)}</div>
                </div>
                {#if confirmKey === key(it)}
                  <button class="danger-solid small" onclick={() => deleteItem(it)}>Recycle</button>
                  <button class="ghost small" onclick={() => (confirmKey = null)}>Keep</button>
                {:else}
                  <button class="ghost small danger" onclick={() => (confirmKey = key(it))}>Delete…</button>
                {/if}
              </div>
            {/each}
          </div>
        </div>
      {/each}
    {/if}
  </section>

  <section class="card">
    <h2>Check library</h2>
    <p class="hint">Finds index entries whose image file was moved or deleted outside VIBDL.</p>
    <button disabled={checking} onclick={runCheck}>{checking ? "Checking…" : "Check now"}</button>
    {#if check}
      {#if check.missing.length === 0}
        <p class="ok">All {check.total} files are present.</p>
      {:else}
        <p class="err">{check.missing.length} of {check.total} files are missing:</p>
        <ul class="missing">
          {#each check.missing.slice(0, 50) as m}<li>{m.provider} #{m.remote_id} · {m.local_path}</li>{/each}
        </ul>
        <button class="ghost" disabled={checking} onclick={removeMissing}>Remove missing from index</button>
        <span class="hint small">Their leftover metadata files go to the Recycle Bin.</span>
      {/if}
    {/if}
    {#if checkMsg}<p class="hint">{checkMsg}</p>{/if}
  </section>
</div>

<style>
  .tools { max-width: 820px; margin: 0 auto; padding: 8px 20px 40px; }
  .card { background: #1f2026; border: 1px solid #2c2d35; border-radius: 10px; padding: 16px 18px; margin-bottom: 14px; }
  h2 { font-size: 1rem; margin: 0 0 8px; }
  .hint { color: #9a9ba3; font-size: 0.86rem; }
  .small { font-size: 0.78rem; }
  .dim { color: #7d7e87; font-size: 0.8rem; }
  label { display: block; margin: 10px 0; font-size: 0.88rem; }
  label input {
    display: block; width: 100%; box-sizing: border-box; margin: 4px 0 2px; padding: 8px 10px;
    background: #15161a; color: inherit; border: 1px solid #353640; border-radius: 6px; font: inherit;
  }
  .ok { color: #6fd39a; font-size: 0.88rem; }
  .pickrow { display: flex; gap: 8px; align-items: flex-end; }
  .pickrow label { flex: 1; }
  .pickrow button { margin-bottom: 12px; }
  .err { color: #f07b7b; font-size: 0.88rem; }
  .group { border-top: 1px solid #2c2d35; padding: 10px 0; }
  .gtitle { font-size: 0.8rem; color: #b5b6bf; margin-bottom: 6px; }
  .members { display: flex; flex-direction: column; gap: 6px; }
  .member { display: grid; grid-template-columns: 64px 1fr auto auto; gap: 10px; align-items: center; }
  .thumb { width: 64px; height: 64px; padding: 0; border: none; border-radius: 4px; overflow: hidden; background: #15161a; cursor: pointer; }
  .thumb img { width: 100%; height: 100%; object-fit: cover; }
  .meta { font-size: 0.85rem; text-transform: capitalize; }
  .missing { font-size: 0.8rem; color: #c9c9cf; max-height: 200px; overflow-y: auto; }
  button {
    padding: 7px 12px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: white;
    font: inherit; font-size: 0.86rem; cursor: pointer;
  }
  button.ghost { background: transparent; border-color: #4a4b55; color: #c9c9cf; }
  button.small { padding: 3px 9px; font-size: 0.8rem; }
  button.danger { color: #f07b7b; border-color: #5a2a2a; }
  button.danger-solid { background: #b8433f; border-color: #b8433f; }
  button:disabled { opacity: 0.45; cursor: default; }
</style>
