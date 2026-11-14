<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, type Settings, type CredentialStatus, type ConnectionTest } from "$lib/api";

  let { onchange }: { onchange?: () => void } = $props();

  const RATINGS = ["general", "sensitive", "questionable", "explicit"];

  let settings = $state<Settings | null>(null);
  let creds = $state<CredentialStatus>({ has_user_id: false, has_api_key: false });
  let userId = $state("");
  let apiKey = $state("");
  let busy = $state(false);
  let message = $state<{ kind: "ok" | "err"; text: string } | null>(null);
  let test = $state<ConnectionTest | null>(null);
  let adultConfirm = $state(false);

  onMount(async () => {
    try {
      settings = await api.getSettings();
      creds = await api.credentialStatus();
      userId = settings.gelbooru_user_id ?? "";
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

  const saveCreds = () =>
    run(async () => {
      creds = await api.setCredentials(userId, apiKey);
      apiKey = ""; // never keep the key in UI state longer than needed
      settings = await api.getSettings();
      message = { kind: "ok", text: "Saved. Your API key is stored in Windows Credential Manager." };
    });

  const clearCreds = () =>
    run(async () => {
      await api.clearCredentials();
      creds = await api.credentialStatus();
      userId = "";
      test = null;
      message = { kind: "ok", text: "Credentials removed." };
    });

  const testConn = () =>
    run(async () => {
      test = await api.testConnection();
      message = { kind: "ok", text: "Connected to Gelbooru." };
    });

  function toggleRating(r: string) {
    if (!settings) return;
    if (r !== "general" && !settings.allowed_ratings.includes(r) && !adultConfirm) return;
    const next = settings.allowed_ratings.includes(r)
      ? settings.allowed_ratings.filter((x) => x !== r)
      : [...settings.allowed_ratings, r];
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
      <h2>Gelbooru account</h2>
      <p class="hint">
        Find your user ID and API key on Gelbooru under <em>My Account → Options</em>. The key is stored in the
        OS credential store, never in files or logs.
      </p>
      <p>
        Status:
        <strong class:ok={creds.has_user_id && creds.has_api_key}>
          {creds.has_user_id && creds.has_api_key ? "credentials saved" : "not set up"}
        </strong>
      </p>
      <label>User ID <input bind:value={userId} autocomplete="off" /></label>
      <label>
        API key
        <input
          type="password"
          bind:value={apiKey}
          autocomplete="off"
          placeholder={creds.has_api_key ? "•••••••• (saved, enter to replace)" : ""}
        />
      </label>
      <div class="row">
        <button disabled={busy || !userId.trim() || !apiKey.trim()} onclick={saveCreds}>Save</button>
        <button disabled={busy || !(creds.has_user_id && creds.has_api_key)} onclick={testConn}>Test connection</button>
        <button class="ghost" disabled={busy} onclick={clearCreds}>Remove</button>
      </div>

      {#if test}
        <div class="result">
          <div>General-rated posts reported: <strong>{test.total_general_posts ?? "n/a"}</strong></div>
          <div>Sample post ID: <strong>{test.sample_post_id ?? "none"}</strong></div>
          <details>
            <summary>Fields in the response ({test.sample_fields.length})</summary>
            <code>{test.sample_fields.join(", ")}</code>
          </details>
        </div>
      {/if}
    </section>

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
  .settings { max-width: 640px; margin: 0 auto; padding: 24px 20px; }
  .card { background: #1f2026; border: 1px solid #2c2d35; border-radius: 10px; padding: 18px 20px; margin-bottom: 16px; }
  h2 { font-size: 1.05rem; margin: 0 0 10px; }
  .hint { color: #9a9ba3; font-size: 0.9rem; }
  label { display: block; margin: 10px 0; font-size: 0.9rem; }
  label input:not([type="checkbox"]) {
    display: block; width: 100%; box-sizing: border-box; margin-top: 4px; padding: 8px 10px;
    background: #15161a; color: inherit; border: 1px solid #353640; border-radius: 6px; font: inherit;
  }
  .check { display: inline-flex; gap: 6px; align-items: center; margin-right: 14px; }
  .row { display: flex; gap: 10px; flex-wrap: wrap; align-items: center; margin-top: 12px; }
  button {
    padding: 8px 14px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: white;
    font: inherit; cursor: pointer;
  }
  button.ghost { background: transparent; border-color: #4a4b55; color: #c9c9cf; }
  button:disabled { opacity: 0.45; cursor: default; }
  .result { margin-top: 14px; padding: 10px 12px; background: #15161a; border-radius: 6px; font-size: 0.9rem; }
  code { font-size: 0.8rem; word-break: break-word; color: #b8bdf5; }
  strong.ok, p.ok { color: #6fd39a; }
  p.err { color: #f07b7b; }
</style>
