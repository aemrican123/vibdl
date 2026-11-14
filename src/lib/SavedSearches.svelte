<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, type SavedSearch, type Settings } from "$lib/api";

  /** Saved searches for one scope (a board id, or "library"). */
  let {
    scope,
    query,
    onrun,
  }: { scope: string; query: string; onrun: (query: string) => void } = $props();

  let settings = $state<Settings | null>(null);
  let open = $state(false);
  let naming = $state(false);
  let name = $state("");
  let error = $state<string | null>(null);

  const mine = $derived((settings?.saved_searches ?? []).filter((s) => s.scope === scope));
  const isSaved = $derived(mine.some((s) => s.query === query.trim().split(/\s+/).filter(Boolean).join(" ")));

  onMount(async () => {
    try {
      settings = await api.getSettings();
    } catch (e) {
      error = errorText(e);
    }
  });

  async function store(list: SavedSearch[]) {
    try {
      const fresh = await api.getSettings(); // don't overwrite changes made elsewhere
      settings = await api.saveSettings({ ...fresh, saved_searches: list });
      error = null;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function save() {
    const q = query.trim();
    const others = (settings?.saved_searches ?? []).filter((s) => !(s.scope === scope && s.query === q));
    await store([...others, { name: name.trim() || q || "Newest", scope, query: q }]);
    naming = false;
    name = "";
  }

  async function remove(s: SavedSearch) {
    await store((settings?.saved_searches ?? []).filter((x) => !(x.scope === s.scope && x.query === s.query)));
  }

  function run(s: SavedSearch) {
    open = false;
    onrun(s.query);
  }
</script>

<div class="saved">
  {#if naming}
    <form
      class="naming"
      onsubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={name} placeholder={query.trim() || "Name"} autofocus />
      <button type="submit">Save</button>
      <button type="button" class="ghost" onclick={() => (naming = false)}>✕</button>
    </form>
  {:else}
    <button
      type="button"
      class="ghost star"
      class:on={isSaved}
      title={isSaved ? "This search is saved" : "Save this search"}
      disabled={isSaved}
      onclick={() => (naming = true)}>{isSaved ? "★" : "☆"}</button
    >
  {/if}
  <button type="button" class="ghost" onclick={() => (open = !open)} title="Saved searches">
    Saved{mine.length ? ` (${mine.length})` : ""} ▾
  </button>
  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="menu" onmouseleave={() => (open = false)}>
      {#each mine as s (s.query)}
        <div class="row">
          <button type="button" class="item" onclick={() => run(s)} title={s.query || "(newest)"}>
            <span class="n">{s.name}</span>
            {#if s.name !== s.query}<span class="q">{s.query || "(newest)"}</span>{/if}
          </button>
          <button type="button" class="x" onclick={() => remove(s)} aria-label={`Delete ${s.name}`}>✕</button>
        </div>
      {:else}
        <p class="empty">No saved searches yet. Use ☆ to save the current one.</p>
      {/each}
      {#if error}<p class="err">{error}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .saved { position: relative; display: flex; gap: 6px; align-items: center; }
  .naming { display: flex; gap: 4px; }
  .naming input {
    width: 160px; padding: 7px 9px; background: #15161a; color: inherit; border: 1px solid #353640;
    border-radius: 6px; font: inherit; font-size: 0.86rem;
  }
  button {
    padding: 8px 12px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: white;
    font: inherit; font-size: 0.86rem; cursor: pointer; white-space: nowrap;
  }
  button.ghost { background: transparent; border-color: #4a4b55; color: #c9c9cf; }
  .star { font-size: 1rem; padding: 6px 10px; }
  .star.on { color: #ffc46b; border-color: #6b5a1d; }
  button:disabled { cursor: default; }
  .menu {
    position: absolute; right: 0; top: calc(100% + 4px); z-index: 30; width: 300px; max-height: 360px; overflow-y: auto;
    background: #1f2026; border: 1px solid #353640; border-radius: 8px; padding: 4px; box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
  }
  .row { display: flex; align-items: center; }
  .item {
    flex: 1; display: flex; flex-direction: column; align-items: flex-start; gap: 1px; background: none; border: none;
    color: #d6d7dc; padding: 6px 8px; border-radius: 5px; text-align: left; min-width: 0;
  }
  .item:hover { background: #2d2f3a; }
  .n { max-width: 100%; overflow: hidden; text-overflow: ellipsis; }
  .q { color: #7d7e87; font-size: 0.76rem; max-width: 100%; overflow: hidden; text-overflow: ellipsis; }
  .x { background: none; border: none; color: #7d7e87; padding: 4px 8px; }
  .x:hover { color: #f07b7b; }
  .empty { color: #8b8c95; font-size: 0.82rem; margin: 8px; }
  .err { color: #f07b7b; font-size: 0.8rem; margin: 6px 8px; }
</style>
