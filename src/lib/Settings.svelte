<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    errorText,
    type ConnectionTest,
    type CredentialStatus,
    type ProviderInfo,
    type Settings,
  } from "$lib/api";

  let { onchange }: { onchange?: () => void } = $props();

  const RATINGS = ["general", "sensitive", "questionable", "explicit"];

  let settings = $state<Settings | null>(null);
  let providers = $state<ProviderInfo[]>([]);
  let statuses = $state<Record<string, CredentialStatus>>({});
  /** Board whose account section is open (defaults to the active one). */
  let editing = $state("");
  let account = $state("");
  let apiKey = $state("");
  let busy = $state(false);
  let message = $state<{ kind: "ok" | "err"; text: string } | null>(null);
  let test = $state<ConnectionTest | null>(null);
  let adultConfirm = $state(false);

  const current = $derived(providers.find((p) => p.id === editing));
  const currentStatus = $derived(statuses[editing]);

  async function refreshStatuses() {
    const entries = await Promise.all(providers.map(async (p) => [p.id, await api.credentialStatus(p.id)] as const));
    statuses = Object.fromEntries(entries);
  }

  function edit(id: string) {
    editing = id;
    account = settings?.accounts[id] ?? "";
    apiKey = "";
    test = null;
  }

  onMount(async () => {
    try {
      settings = await api.getSettings();
      providers = await api.listProviders();
      await refreshStatuses();
      edit(settings.active_provider);
    } catch (e) {
      message = { kind: "err", text: errorText(e) };
    }
  });

  async function run(fn: () => Promise<void>) {
    busy = true;
    message = null;
    try {
      await fn();
      onchange?.();
    } catch (e) {
      message = { kind: "err", text: errorText(e) };
    } finally {
      busy = false;
    }
  }

  const confirmAge = () =>
    run(async () => {
      settings = await api.saveSettings({ ...settings!, age_confirmed: true });
    });

  const choose = (id: string) =>
    run(async () => {
      settings = await api.saveSettings({ ...settings!, active_provider: id });
      edit(id);
    });

  const saveCreds = () =>
    run(async () => {
      const id = editing;
      statuses = { ...statuses, [id]: await api.setCredentials(id, account, apiKey) };
      apiKey = ""; // never keep the key in UI state longer than needed
      settings = await api.getSettings();
      message = { kind: "ok", text: "Saved. The API key is stored in Windows Credential Manager." };
    });

  const clearCreds = () =>
    run(async () => {
      const id = editing;
      statuses = { ...statuses, [id]: await api.clearCredentials(id) };
      settings = await api.getSettings();
      account = "";
      test = null;
      message = { kind: "ok", text: "Credentials removed." };
    });

  const testConn = () =>
    run(async () => {
      test = await api.testConnection(editing);
      message = { kind: "ok", text: `Connected to ${current?.name}.` };
    });

  function toggleRating(r: string) {
    if (!settings) return;
    if (r !== "general" && !settings.allowed_ratings.includes(r) && !adultConfirm) return;
    const next = settings.allowed_ratings.includes(r)
      ? settings.allowed_ratings.filter((x) => x !== r)
      : [...settings.allowed_ratings, r];
    if (next.length === 0) return; // at least one rating must stay on
    run(async () => {
      settings = await api.saveSettings({ ...settings!, allowed_ratings: next });
    });
  }
</script>

<div class="settings">
  {#if !settings}
    <p>Loading…</p>
  {:else if !settings.age_confirmed}
    <section class="card">
      <h2>Before you start</h2>
      <p>
        VIBDL connects to imageboards that host adult content. You must be 18 or older to use it.
        Only general-rated content is shown unless you change it in Settings.
      </p>
      <button disabled={busy} onclick={confirmAge}>I am 18 or older</button>
    </section>
  {:else}
    <section class="card">
      <h2>Imageboard</h2>
      <p class="hint">The board used in Browse. Each board keeps its own account.</p>
      <div class="boards">
        {#each providers as p (p.id)}
          {@const st = statuses[p.id]}
          <div class="board" class:active={settings.active_provider === p.id}>
            <label class="pick">
              <input
                type="radio"
                name="board"
                checked={settings.active_provider === p.id}
                disabled={busy}
                onchange={() => choose(p.id)}
              />
              <span class="name">{p.name}</span>
              {#if st?.ready}
                <span class="pill ok">{st.has_api_key ? "account saved" : "ready"}</span>
              {:else}
                <span class="pill">needs account</span>
              {/if}
            </label>
            {#if p.warning}<p class="warn">⚠ {p.warning}</p>{/if}
            <button class="ghost small" onclick={() => edit(p.id)} disabled={editing === p.id}>Account…</button>
          </div>
        {/each}
      </div>
    </section>

    {#if current}
      <section class="card">
        <h2>{current.name} account {current.requires_account ? "" : "(optional)"}</h2>
        <p class="hint">{current.key_help}</p>
        <p>
          Status:
          <strong class:ok={currentStatus?.has_api_key}>
            {currentStatus?.has_api_key
              ? `saved for ${currentStatus.account}`
              : current.requires_account
                ? "not set up"
                : "using anonymous access"}
          </strong>
        </p>
        <label>{current.account_label} <input bind:value={account} autocomplete="off" spellcheck="false" /></label>
        <label>
          API key
          <input
            type="password"
            bind:value={apiKey}
            autocomplete="off"
            placeholder={currentStatus?.has_api_key ? "•••••••• (saved, enter to replace)" : ""}
          />
        </label>
        <div class="row">
          <button disabled={busy || !account.trim() || !apiKey.trim()} onclick={saveCreds}>Save</button>
          <button disabled={busy || !currentStatus?.ready} onclick={testConn}>Test connection</button>
          <button class="ghost" disabled={busy || !currentStatus?.has_api_key} onclick={clearCreds}>Remove</button>
        </div>

        {#if test}
          <div class="result">
            <div>Signed in: <strong>{test.authenticated ? "yes" : "no (anonymous)"}</strong></div>
            {#if test.total_general_posts !== null}
              <div>General-rated posts reported: <strong>{test.total_general_posts.toLocaleString()}</strong></div>
            {/if}
            <div>Sample post ID: <strong>{test.sample_post_id ?? "none"}</strong></div>
            <details>
              <summary>Fields in the response ({test.sample_fields.length})</summary>
              <code>{test.sample_fields.join(", ")}</code>
            </details>
          </div>
        {/if}
      </section>
    {/if}

    <section class="card">
      <h2>Content ratings</h2>
      <label class="check">
        <input type="checkbox" bind:checked={adultConfirm} />
        Unlock non-general ratings
      </label>
      <div class="row">
        {#each RATINGS as r}
          <label class="check">
            <input
              type="checkbox"
              checked={settings.allowed_ratings.includes(r)}
              disabled={busy || (r !== "general" && !adultConfirm && !settings.allowed_ratings.includes(r))}
              onchange={() => toggleRating(r)}
            />
            {r}
          </label>
        {/each}
      </div>
    </section>
  {/if}

  {#if message}
    <p class={message.kind}>{message.text}</p>
  {/if}
</div>

<style>
  .settings { max-width: 680px; margin: 0 auto; padding: 24px 20px; }
  .card { background: #1f2026; border: 1px solid #2c2d35; border-radius: 10px; padding: 18px 20px; margin-bottom: 16px; }
  h2 { font-size: 1.05rem; margin: 0 0 10px; }
  .hint { color: #9a9ba3; font-size: 0.9rem; }
  label { display: block; margin: 10px 0; font-size: 0.9rem; }
  label input:not([type="checkbox"]):not([type="radio"]) {
    display: block; width: 100%; box-sizing: border-box; margin-top: 4px; padding: 8px 10px;
    background: #15161a; color: inherit; border: 1px solid #353640; border-radius: 6px; font: inherit;
  }
  .check { display: inline-flex; gap: 6px; align-items: center; margin-right: 14px; }
  .row { display: flex; gap: 10px; flex-wrap: wrap; align-items: center; margin-top: 12px; }
  .boards { display: flex; flex-direction: column; gap: 8px; }
  .board {
    display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 2px 12px;
    padding: 10px 12px; border: 1px solid #2c2d35; border-radius: 8px; background: #1a1b20;
  }
  .board.active { border-color: #4b5bd6; }
  .pick { display: flex; align-items: center; gap: 8px; margin: 0; cursor: pointer; }
  .name { font-weight: 600; }
  .warn { grid-column: 1 / -1; grid-row: 2; margin: 2px 0 0 24px; color: #e0b465; font-size: 0.82rem; }
  .pill { font-size: 0.72rem; padding: 1px 7px; border-radius: 10px; background: #2a2b33; color: #9a9ba3; }
  .pill.ok { background: #1d3527; color: #6fd39a; }
  button {
    padding: 8px 14px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: white;
    font: inherit; cursor: pointer;
  }
  button.ghost { background: transparent; border-color: #4a4b55; color: #c9c9cf; }
  button.small { padding: 3px 10px; font-size: 0.82rem; }
  button:disabled { opacity: 0.45; cursor: default; }
  .result { margin-top: 14px; padding: 10px 12px; background: #15161a; border-radius: 6px; font-size: 0.9rem; }
  code { font-size: 0.8rem; word-break: break-word; color: #b8bdf5; }
  strong.ok, p.ok { color: #6fd39a; }
  p.err { color: #f07b7b; }
</style>
