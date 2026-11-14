<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { api, errorText, fmtSize, thumbUrl, type Job, type QueueProgress } from "$lib/api";

  let jobs = $state<Job[]>([]);
  let error = $state<string | null>(null);

  const counts = $derived({
    active: jobs.filter((j) => j.state === "queued" || j.state === "downloading").length,
    done: jobs.filter((j) => j.state === "done").length,
    duplicate: jobs.filter((j) => j.state === "duplicate").length,
    failed: jobs.filter((j) => j.state === "failed").length,
    cancelled: jobs.filter((j) => j.state === "cancelled").length,
  });

  const unlisteners: UnlistenFn[] = [];
  onMount(async () => {
    jobs = await api.queueList();
    unlisteners.push(await listen<Job[]>("queue-updated", (e) => (jobs = e.payload)));
    unlisteners.push(
      await listen<QueueProgress>("queue-progress", (e) => {
        const i = jobs.findIndex((j) => j.key === e.payload.key);
        if (i >= 0) jobs[i] = { ...jobs[i], bytes: e.payload.bytes, total: e.payload.total };
      }),
    );
  });
  onDestroy(() => unlisteners.forEach((u) => u()));

  const act = (p: Promise<unknown>) => p.catch((e) => (error = errorText(e)));

  const LABEL: Record<Job["state"], string> = {
    queued: "Queued",
    downloading: "Downloading",
    done: "Saved",
    duplicate: "Already in library",
    failed: "Failed",
    cancelled: "Cancelled",
  };

  const pct = (j: Job) => (j.total ? Math.min(100, (j.bytes / j.total) * 100) : 0);
</script>

<div class="dl">
  <div class="toolbar">
    <span>
      <strong>{counts.active}</strong> active · {counts.done} saved · {counts.duplicate} already had · {counts.failed} failed
      {#if counts.cancelled}· {counts.cancelled} cancelled{/if}
    </span>
    <span class="spacer"></span>
    <button class="ghost" disabled={!counts.active} onclick={() => act(api.queueCancel(null))}>Cancel all</button>
    <button class="ghost" disabled={!counts.failed && !counts.cancelled} onclick={() => act(api.queueRetryFailed())}>Retry failed</button>
    <button class="ghost" disabled={jobs.length === counts.active} onclick={() => act(api.queueClearFinished())}>Clear finished</button>
  </div>
  <p class="hint">
    Downloads run one at a time, at most one per second. Network errors are retried twice. The queue is cleared when you close VIBDL.
  </p>
  {#if error}<p class="err">{error}</p>{/if}

  {#if !jobs.length}
    <p class="empty">Nothing queued. In Browse, click <em>Select</em> (or Ctrl+click tiles), then <em>Download originals</em>.</p>
  {/if}

  <ul>
    {#each jobs as j (j.key)}
      <li class={j.state}>
        <div class="thumb">
          {#if j.thumb_key}<img src={thumbUrl(j.thumb_key)} alt="" loading="lazy" />{/if}
        </div>
        <div class="main">
          <div class="line">
            <span class="name">{j.provider} #{j.id}</span>
            <span class={`state ${j.state}`}>{LABEL[j.state]}</span>
          </div>
          {#if j.state === "downloading"}
            <div class="bar"><div style:width={j.total ? `${pct(j)}%` : "100%"} class:indeterminate={!j.total}></div></div>
            <div class="sub">{fmtSize(j.bytes)}{j.total ? ` of ${fmtSize(j.total)}` : ""}{j.attempts > 1 ? ` · attempt ${j.attempts}` : ""}</div>
          {:else if j.error}
            <div class="sub err">{j.error}</div>
          {:else if j.local_path}
            <div class="sub">{j.state === "done" ? fmtSize(j.bytes) + " · " : ""}{j.local_path}</div>
          {/if}
        </div>
        <div class="actions">
          {#if j.state === "queued" || j.state === "downloading"}
            <button class="ghost small" onclick={() => act(api.queueCancel(j.key))}>Cancel</button>
          {:else if j.local_path}
            <button class="ghost small" onclick={() => act(api.openLocal(j.local_path!, true))}>Show</button>
          {/if}
        </div>
      </li>
    {/each}
  </ul>
</div>

<style>
  .dl { max-width: 900px; margin: 0 auto; padding: 18px 20px 40px; }
  .toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; font-size: 0.9rem; }
  .spacer { flex: 1; }
  .hint { color: #7d7e87; font-size: 0.82rem; margin: 8px 0 14px; }
  .empty { color: #9a9ba3; text-align: center; margin-top: 50px; }
  ul { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 6px; }
  li {
    display: grid; grid-template-columns: 56px 1fr auto; gap: 12px; align-items: center;
    padding: 8px 10px; background: #1f2026; border: 1px solid #2c2d35; border-radius: 8px;
  }
  li.failed { border-color: #5a2a2a; }
  .thumb { width: 56px; height: 56px; border-radius: 4px; overflow: hidden; background: #15161a; }
  .thumb img { width: 100%; height: 100%; object-fit: cover; }
  .line { display: flex; justify-content: space-between; gap: 10px; }
  .name { font-weight: 600; text-transform: capitalize; }
  .state { font-size: 0.8rem; color: #9a9ba3; }
  .state.downloading { color: #8fa0ff; }
  .state.done { color: #6fd39a; }
  .state.duplicate { color: #9fc7b0; }
  .state.failed { color: #f07b7b; }
  .sub { font-size: 0.78rem; color: #8b8c95; margin-top: 3px; overflow-wrap: anywhere; }
  .sub.err, .err { color: #f07b7b; }
  .bar { height: 6px; background: #26272e; border-radius: 3px; overflow: hidden; margin-top: 6px; }
  .bar > div { height: 100%; background: #4b5bd6; transition: width 0.2s; }
  .bar > div.indeterminate { opacity: 0.5; }
  button {
    padding: 7px 12px; border-radius: 6px; border: 1px solid #4b5bd6; background: #4b5bd6; color: white;
    font: inherit; font-size: 0.86rem; cursor: pointer;
  }
  button.ghost { background: transparent; border-color: #4a4b55; color: #c9c9cf; }
  button.small { padding: 3px 10px; font-size: 0.8rem; }
  button:disabled { opacity: 0.45; cursor: default; }
</style>
