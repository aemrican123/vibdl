<script lang="ts" generics="T">
  import type { Snippet } from "svelte";

  /**
   * Square-tile grid that only renders the rows near the viewport, so
   * thousands of items cost about as much as a screenful.
   */
  let {
    items,
    key,
    minSize = 150,
    gap = 8,
    overscan = 3,
    onend,
    tile,
    before,
    after,
  }: {
    items: T[];
    key: (item: T) => string;
    minSize?: number;
    gap?: number;
    /** Extra rows rendered above/below the viewport. */
    overscan?: number;
    /** Called when the user scrolls near the end. */
    onend?: () => void;
    tile: Snippet<[T, number]>;
    before?: Snippet;
    after?: Snippet;
  } = $props();

  let scroller = $state<HTMLDivElement | null>(null);
  let width = $state(0);
  let height = $state(0);
  let scrollTop = $state(0);
  let headH = $state(0);

  const cols = $derived(Math.max(1, Math.floor((width + gap) / (minSize + gap))));
  const size = $derived(width > 0 ? (width - gap * (cols - 1)) / cols : minSize);
  const rowH = $derived(size + gap);
  const rows = $derived(Math.ceil(items.length / cols));
  const gridTop = $derived(headH);
  const first = $derived(Math.max(0, Math.floor((scrollTop - gridTop) / rowH) - overscan));
  const last = $derived(Math.min(rows - 1, Math.ceil((scrollTop - gridTop + height) / rowH) + overscan));
  const visible = $derived(
    rows === 0 ? [] : items.slice(first * cols, (last + 1) * cols).map((item, i) => ({ item, index: first * cols + i })),
  );

  $effect(() => {
    if (!scroller) return;
    const ro = new ResizeObserver(() => {
      if (!scroller) return;
      width = scroller.clientWidth - 32; // matches horizontal padding below
      height = scroller.clientHeight;
    });
    ro.observe(scroller);
    return () => ro.disconnect();
  });

  // Ask for more when the last rendered row is close to the end.
  $effect(() => {
    if (onend && rows > 0 && last >= rows - 1 - overscan) onend();
  });
  // Also when the content doesn't fill the viewport yet.
  $effect(() => {
    if (onend && height > 0 && rows * rowH + gridTop < height) onend();
  });

  /** Scroll so item `index` is visible. */
  export function reveal(index: number) {
    if (!scroller) return;
    const row = Math.floor(index / cols);
    const top = gridTop + row * rowH;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (top + rowH > scroller.scrollTop + height) scroller.scrollTop = top + rowH - height;
  }

  /** Reset to the top (new search). */
  export function toTop() {
    if (scroller) scroller.scrollTop = 0;
  }
</script>

<div class="scroller" bind:this={scroller} onscroll={() => (scrollTop = scroller?.scrollTop ?? 0)}>
  <div bind:clientHeight={headH}>{@render before?.()}</div>
  <div class="space" style:height={`${Math.max(0, rows * rowH - gap)}px`}>
    {#each visible as v (key(v.item))}
      <div
        class="cell"
        style:width={`${size}px`}
        style:height={`${size}px`}
        style:transform={`translate(${(v.index % cols) * (size + gap)}px, ${Math.floor(v.index / cols) * rowH}px)`}
      >
        {@render tile(v.item, v.index)}
      </div>
    {/each}
  </div>
  {@render after?.()}
</div>

<style>
  .scroller { flex: 1; overflow-y: auto; padding: 0 16px 24px; position: relative; min-height: 0; }
  .space { position: relative; }
  .cell { position: absolute; top: 0; left: 0; }
</style>
