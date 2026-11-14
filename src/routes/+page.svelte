<script lang="ts">
  import { onMount } from "svelte";
  import Browse from "$lib/Browse.svelte";
  import Settings from "$lib/Settings.svelte";
  import { api } from "$lib/api";

  type Tab = "browse" | "settings";
  let tab = $state<Tab>("settings");
  let ready = $state(false); // age confirmed + credentials saved

  async function refreshReady() {
    try {
      const [s, c] = await Promise.all([api.getSettings(), api.credentialStatus()]);
      ready = s.age_confirmed && c.has_user_id && c.has_api_key;
    } catch {
      ready = false;
    }
  }

  onMount(async () => {
    await refreshReady();
    if (ready) tab = "browse";
  });
</script>

<div class="shell">
  <header>
    <span class="logo">VIBDL</span>
    <nav>
      <button class:active={tab === "browse"} disabled={!ready} onclick={() => (tab = "browse")}>
        Browse
      </button>
      <button disabled title="Milestone 5">Library</button>
      <button class:active={tab === "settings"} onclick={() => (tab = "settings")}>Settings</button>
    </nav>
  </header>

  <main>
    <!-- Keep Browse mounted while switching tabs so results and scroll survive. -->
    <div class="pane" class:hidden={tab !== "browse"}>
      {#if ready}<Browse />{/if}
    </div>
    {#if tab === "settings"}
      <div class="pane scroll"><Settings onchange={refreshReady} /></div>
    {/if}
  </main>
</div>

<style>
  :global(html, body) { height: 100%; }
  :global(body) {
    margin: 0;
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    background: #15161a;
    color: #e6e6e9;
  }
  .shell { display: flex; flex-direction: column; height: 100vh; }
  header {
    display: flex; align-items: center; gap: 24px; padding: 0 16px; height: 46px; flex-shrink: 0;
    border-bottom: 1px solid #2c2d35; background: #1a1b20;
  }
  .logo { font-weight: 700; letter-spacing: 0.08em; }
  nav { display: flex; gap: 4px; }
  nav button {
    background: transparent; border: none; color: #9a9ba3; padding: 6px 12px; border-radius: 6px;
    font: inherit; cursor: pointer;
  }
  nav button:hover:not(:disabled) { color: #e6e6e9; background: #26272e; }
  nav button.active { color: #fff; background: #2d2f3a; }
  nav button:disabled { opacity: 0.4; cursor: default; }
  main { flex: 1; min-height: 0; position: relative; }
  .pane { position: absolute; inset: 0; }
  .pane.scroll { overflow-y: auto; }
  .hidden { display: none; }
</style>
