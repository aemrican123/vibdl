<script lang="ts" module>
  export interface ViewerMedia {
    kind: "image" | "video";
    src: string;
    /** Shown instantly while `src` loads. */
    placeholder?: string | null;
    /** Higher-resolution source available on request (Browse originals). */
    originalSrc?: string | null;
    title: string;
    subtitle?: string;
  }
</script>

<script lang="ts">
  /**
   * Full-screen image/video viewer.
   * ←/→ navigate · Esc close · F fit · 1 actual size · +/− zoom ·
   * wheel zoom at cursor · drag to pan · double-click fit/100 %.
   */
  let {
    count,
    index,
    resolve,
    onindex,
    onclose,
  }: {
    count: number;
    index: number;
    /** Media for item i (cached here). */
    resolve: (i: number) => Promise<ViewerMedia | null>;
    onindex: (i: number) => void;
    onclose: () => void;
  } = $props();

  let stage = $state<HTMLDivElement | null>(null);
  let media = $state<ViewerMedia | null>(null);
  let error = $state<string | null>(null);
  let loaded = $state(false);
  let useOriginal = $state(false);
  let nat = $state({ w: 0, h: 0 });
  let s = $state(1);
  let tx = $state(0);
  let ty = $state(0);
  let mode = $state<"fit" | "free">("fit");
  let drag: { x: number; y: number; tx: number; ty: number } | null = null;

  const cache = new Map<number, Promise<ViewerMedia | null>>();
  const get = (i: number) => {
    if (!cache.has(i)) cache.set(i, resolve(i).catch(() => null));
    return cache.get(i)!;
  };

  let shown = -1;
  $effect(() => {
    const i = index;
    if (i === shown) return;
    shown = i;
    loaded = false;
    error = null;
    useOriginal = false;
    get(i).then((m) => {
      if (shown !== i) return;
      media = m;
      if (!m) error = "Couldn't load this item.";
      // Warm up the neighbours.
      for (const j of [i + 1, i - 1]) {
        if (j >= 0 && j < count)
          get(j).then((n) => {
            if (n?.kind === "image") new Image().src = n.src;
          });
      }
    });
  });

  const src = $derived(media ? (useOriginal && media.originalSrc ? media.originalSrc : media.src) : "");

  function fit() {
    if (!stage || !nat.w) return;
    const cw = stage.clientWidth;
    const ch = stage.clientHeight;
    s = Math.min(cw / nat.w, ch / nat.h, 1);
    tx = (cw - nat.w * s) / 2;
    ty = (ch - nat.h * s) / 2;
    mode = "fit";
  }

  function zoomAt(f: number, px: number, py: number) {
    const ns = Math.min(20, Math.max(0.05, s * f));
    const k = ns / s;
    tx = px - (px - tx) * k;
    ty = py - (py - ty) * k;
    s = ns;
    mode = "free";
  }

  function center() {
    return stage ? { x: stage.clientWidth / 2, y: stage.clientHeight / 2 } : { x: 0, y: 0 };
  }

  function actual(px?: number, py?: number) {
    const c = center();
    zoomAt(1 / s, px ?? c.x, py ?? c.y);
  }

  function onLoad(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    const prev = nat;
    nat = { w: img.naturalWidth, h: img.naturalHeight };
    // Switching to the original keeps the same on-screen size and position.
    if (loaded && mode === "free" && prev.w) {
      s = (s * prev.w) / nat.w;
    } else {
      fit();
    }
    loaded = true;
  }

  $effect(() => {
    if (!stage) return;
    const ro = new ResizeObserver(() => mode === "fit" && fit());
    ro.observe(stage);
    return () => ro.disconnect();
  });

  function go(d: number) {
    const j = index + d;
    if (j >= 0 && j < count) onindex(j);
  }

  function onKey(e: KeyboardEvent) {
    const k = e.key;
    if (k === "Escape") onclose();
    else if (k === "ArrowRight") go(1);
    else if (k === "ArrowLeft") go(-1);
    else if (media?.kind === "image" && (k === "f" || k === "F" || k === "0")) fit();
    else if (media?.kind === "image" && k === "1") actual();
    else if (media?.kind === "image" && (k === "+" || k === "=")) zoomAt(1.25, center().x, center().y);
    else if (media?.kind === "image" && (k === "-" || k === "_")) zoomAt(0.8, center().x, center().y);
    else return;
    e.preventDefault();
    e.stopImmediatePropagation();
  }

  function onWheel(e: WheelEvent) {
    if (media?.kind !== "image" || !stage) return;
    e.preventDefault();
    const r = stage.getBoundingClientRect();
    zoomAt(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX - r.left, e.clientY - r.top);
  }

  function onDown(e: PointerEvent) {
    if (media?.kind !== "image" || e.button !== 0) return;
    drag = { x: e.clientX, y: e.clientY, tx, ty };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onMove(e: PointerEvent) {
    if (!drag) return;
    tx = drag.tx + (e.clientX - drag.x);
    ty = drag.ty + (e.clientY - drag.y);
    mode = "free";
  }
  function onUp() {
    drag = null;
  }
  function onDbl(e: MouseEvent) {
    if (media?.kind !== "image" || !stage) return;
    const r = stage.getBoundingClientRect();
    if (mode === "fit") actual(e.clientX - r.left, e.clientY - r.top);
    else fit();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="viewer" role="dialog" aria-modal="true" aria-label="Viewer">
  <div class="top">
    <div class="title">
      <strong>{media?.title ?? ""}</strong>
      {#if media?.subtitle}<span class="dim">{media.subtitle}</span>{/if}
    </div>
    <div class="ctrls">
      <span class="dim">{index + 1} / {count}</span>
      {#if media?.kind === "image"}
        <span class="dim">{Math.round(s * 100)}%</span>
        <button onclick={fit} title="Fit (F)">Fit</button>
        <button onclick={() => actual()} title="Actual size (1)">100%</button>
        {#if media.originalSrc && !useOriginal}
          <button onclick={() => (useOriginal = true)} title="Fetch the full-resolution original for viewing (not saved)">
            Load original
          </button>
        {/if}
      {/if}
      <button onclick={onclose} title="Close (Esc)" aria-label="Close">✕</button>
    </div>
  </div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="stage"
    class:grab={media?.kind === "image"}
    bind:this={stage}
    onwheel={onWheel}
    onpointerdown={onDown}
    onpointermove={onMove}
    onpointerup={onUp}
    onpointercancel={onUp}
    ondblclick={onDbl}
  >
    {#if error}
      <p class="msg">{error}</p>
    {:else if media?.kind === "video"}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video class="video" controls autoplay loop src={media.src} poster={media.placeholder ?? undefined}></video>
    {:else if media}
      {#if !loaded && media.placeholder}
        <img class="ph" src={media.placeholder} alt="" />
      {/if}
      <img
        class="img"
        class:hidden={!loaded}
        {src}
        alt={media.title}
        draggable="false"
        onload={onLoad}
        onerror={() => {
          if (useOriginal) useOriginal = false;
          else error = "The image failed to load.";
        }}
        style:transform={`translate(${tx}px, ${ty}px) scale(${s})`}
      />
      {#if useOriginal && !loaded}<p class="msg small">Loading original…</p>{/if}
    {:else}
      <p class="msg">Loading…</p>
    {/if}

    {#if index > 0}
      <button class="nav left" onclick={() => go(-1)} aria-label="Previous">‹</button>
    {/if}
    {#if index < count - 1}
      <button class="nav right" onclick={() => go(1)} aria-label="Next">›</button>
    {/if}
  </div>
</div>

<style>
  .viewer {
    position: fixed; inset: 0; z-index: 100; background: rgba(8, 8, 10, 0.96);
    display: flex; flex-direction: column;
  }
  .top {
    display: flex; justify-content: space-between; align-items: center; gap: 12px;
    padding: 8px 14px; color: #ddd; font-size: 0.88rem; background: rgba(0, 0, 0, 0.4);
  }
  .title { display: flex; gap: 10px; align-items: baseline; min-width: 0; text-transform: capitalize; }
  .ctrls { display: flex; gap: 8px; align-items: center; flex-shrink: 0; }
  .dim { color: #8b8c95; }
  .ctrls button {
    background: #26272e; border: 1px solid #3a3b46; color: #ddd; border-radius: 5px;
    padding: 4px 10px; font: inherit; font-size: 0.82rem; cursor: pointer;
  }
  .ctrls button:hover { background: #33343d; }
  .stage { position: relative; flex: 1; overflow: hidden; touch-action: none; }
  .stage.grab { cursor: grab; }
  .stage.grab:active { cursor: grabbing; }
  .img { position: absolute; left: 0; top: 0; transform-origin: 0 0; user-select: none; max-width: none; }
  .img.hidden { visibility: hidden; }
  .ph {
    position: absolute; inset: 0; margin: auto; max-width: 100%; max-height: 100%;
    object-fit: contain; filter: blur(3px); opacity: 0.6; width: 100%; height: 100%;
  }
  .video { position: absolute; inset: 0; margin: auto; max-width: 100%; max-height: 100%; width: 100%; height: 100%; background: #000; }
  .msg { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; color: #aaa; margin: 0; pointer-events: none; }
  .msg.small { align-items: flex-end; padding-bottom: 20px; font-size: 0.85rem; }
  .nav {
    position: absolute; top: 50%; transform: translateY(-50%); width: 48px; height: 80px;
    background: rgba(0, 0, 0, 0.35); border: none; color: #eee; font-size: 2.2rem; cursor: pointer; border-radius: 6px;
  }
  .nav:hover { background: rgba(0, 0, 0, 0.6); }
  .nav.left { left: 10px; }
  .nav.right { right: 10px; }
</style>
