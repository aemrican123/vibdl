<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import Browse from "$lib/Browse.svelte";
  import Downloads from "$lib/Downloads.svelte";
  import Library from "$lib/Library.svelte";
  import Settings from "$lib/Settings.svelte";
  import { api, type Job } from "$lib/api";

  type Tab = "browse" | "library" | "downloads" | "settings";
  let tab = $state<Tab>("settings");
  let ready = $state(false); // age confirmed + active board usable
  let ageOk = $state(false); // Library only needs the age confirmation
  let libraryView = $state<{ refresh: () => void } | null>(null);
  let provider = $state(""); // active board id
  let providerName = $state("");
  let allowsBatch = $state(false);
  let activeJobs = $state(0);
  let unlisten: UnlistenFn | null = null;
  listen<Job[]>("queue-updated", (e) => {
    activeJobs = e.payload.filter((j) => j.state === "queued" || j.state === "downloading").length;
  }).then((u) => (unlisten = u));
  onDestroy(() => unlisten?.());

  async function refreshReady() {
    try {
      const s = await api.getSettings();
      const [c, list] = await Promise.all([api.credentialStatus(s.active_provider), api.listProviders()]);
      provider = s.active_provider;
      const meta = list.find((p) => p.id === provider);
      providerName = meta?.name ?? provider;
      allowsBatch = meta?.allows_batch ?? false;
      ageOk = s.age_confirmed;
      ready = s.age_confirmed && c.ready;
    } catch {
      ready = false;
    }
  }

  onMount(async () => {
    await refreshReady();
    if (ready) tab = "browse";
    else if (ageOk) tab = "library";
  });
</script>

<div class="shell">
  <header>
    <span class="logo">VIBDL</span>
    <nav>
      <button class:active={tab === "browse"} disabled={!ready} onclick={() => (tab = "browse")}>
        Browse{providerName ? ` · ${providerName}` : ""}
      </button>
      <button
        class:active={tab === "library"}
        disabled={!ageOk}
        onclick={() => {
          if (tab !== "library") libraryView?.refresh();
          tab = "library";
        }}>Library</button
      >
      <button class:active={tab === "downloads"} disabled={!ageOk} onclick={() => (tab = "downloads")}>
        Downloads{activeJobs ? ` (${activeJobs})` : ""}
      </button>
      <button class:active={tab === "settings"} onclick={() => (tab = "settings")}>Settings</button>
    </nav>
  </header>

  <main>
    <!-- Keep Browse mounted while switching tabs so results and scroll survive. -->
    <div class="pane" class:hidden={tab !== "browse"}>
      <!-- Switching boards starts a fresh browser. -->
      {#if ready}{#key provider}<Browse {provider} {providerName} {allowsBatch} active={tab === "browse"} />{/key}{/if}
    </div>
    <div class="pane" class:hidden={tab !== "library"}>
      {#if ageOk}<Library bind:this={libraryView} active={tab === "library"} />{/if}
    </div>
    <div class="pane scroll" class:hidden={tab !== "downloads"}>
      {#if ageOk}<Downloads />{/if}
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
