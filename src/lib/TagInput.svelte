<script lang="ts">
  import type { Suggestion } from "$lib/api";

  /**
   * Search box with tag suggestions for the word under the cursor.
   * ↑/↓ choose · Tab or Enter accept · Esc close · Enter with nothing
   * highlighted submits the search.
   */
  let {
    value = $bindable(""),
    placeholder = "",
    fetch,
    split = defaultSplit,
    onsubmit,
  }: {
    value?: string;
    placeholder?: string;
    /** Suggestions for the typed word (already debounced here). */
    fetch: (token: string) => Promise<Suggestion[]>;
    /** Split a word into [kept prefix, part to complete], e.g. "-artist:lo" → ["-artist:", "lo"]. */
    split?: (token: string) => [string, string];
    onsubmit: () => void;
  } = $props();

  /** Focus the box (keyboard shortcut "/"). */
  export function focus() {
    input?.focus();
    input?.select();
  }

  function defaultSplit(token: string): [string, string] {
    const m = token.match(/^[-~]*/);
    const lead = m ? m[0] : "";
    return [lead, token.slice(lead.length)];
  }

  let input: HTMLInputElement | null = null;
  const listId = `tag-suggest-${Math.random().toString(36).slice(2, 8)}`;
  let open = $state(false);
  let items = $state<Suggestion[]>([]);
  let active = $state(-1);
  let loading = $state(false);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let seq = 0;
  // Last result set, used to filter instantly while the next request runs.
  let last: { query: string; items: Suggestion[] } | null = null;

  /** [start, end) of the word around the cursor. */
  function wordAt(text: string, caret: number): [number, number] {
    let s = caret;
    while (s > 0 && !/\s/.test(text[s - 1])) s--;
    let e = caret;
    while (e < text.length && !/\s/.test(text[e])) e++;
    return [s, e];
  }

  function currentWord(): { start: number; end: number; word: string } {
    const caret = input?.selectionStart ?? value.length;
    const [start, end] = wordAt(value, caret);
    return { start, end, word: value.slice(start, end) };
  }

  function onInput() {
    const { word } = currentWord();
    const [, q] = split(word);
    if (q.length < 2) {
      close();
      return;
    }
    // Instant: narrow the previous results if this word extends them.
    if (last && q.startsWith(last.query)) {
      const quick = last.items.filter((s) => s.name.toLowerCase().startsWith(q.toLowerCase()));
      if (quick.length) {
        items = quick;
        open = true;
        active = -1;
      }
    }
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => load(word, q), 250);
  }

  async function load(word: string, q: string) {
    const id = ++seq;
    loading = true;
    try {
      const res = await fetch(word);
      if (id !== seq) return;
      last = { query: q, items: res };
      items = res;
      open = res.length > 0;
      active = -1;
    } catch {
      if (id === seq) close();
    } finally {
      if (id === seq) loading = false;
    }
  }

  function close() {
    open = false;
    items = [];
    active = -1;
  }

  function accept(s: Suggestion) {
    const { start, end, word } = currentWord();
    const [lead] = split(word);
    const insert = lead + s.name + " ";
    const after = value.slice(end).replace(/^\s+/, "");
    value = value.slice(0, start) + insert + after;
    close();
    const caret = start + insert.length;
    queueMicrotask(() => {
      input?.focus();
      input?.setSelectionRange(caret, caret);
    });
  }

  function onKey(e: KeyboardEvent) {
    if (open && items.length) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        active = (active + 1) % items.length;
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        active = active <= 0 ? items.length - 1 : active - 1;
        return;
      }
      if ((e.key === "Tab" || e.key === "Enter") && (active >= 0 || e.key === "Tab")) {
        e.preventDefault();
        accept(items[Math.max(active, 0)]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        close();
        return;
      }
    }
    if (e.key === "Enter") {
      e.preventDefault();
      close();
      onsubmit();
    }
  }

  const fmtCount = (n: number | null) =>
    n === null ? "" : n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 1000 ? `${(n / 1000).toFixed(1)}k` : `${n}`;
</script>

<div class="wrap">
  <input
    bind:this={input}
    bind:value
    {placeholder}
    spellcheck="false"
    autocomplete="off"
    oninput={onInput}
    onkeydown={onKey}
    onblur={() => setTimeout(close, 150)}
    role="combobox"
    aria-controls={listId}
    aria-expanded={open}
    aria-autocomplete="list"
  />
  {#if loading}<span class="spin" aria-hidden="true"></span>{/if}
  {#if open}
    <ul class="menu" role="listbox" id={listId}>
      {#each items as s, i (s.name + s.category)}
        <li role="option" aria-selected={i === active}>
          <button
            type="button"
            class:active={i === active}
            onmousedown={(e) => {
              e.preventDefault();
              accept(s);
            }}
          >
            <span class={`name ${s.category}`}>{s.name}</span>
            <span class="cat">{s.category}</span>
            <span class="count">{fmtCount(s.count)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .wrap { position: relative; flex: 1; display: flex; }
  input {
    flex: 1; padding: 9px 12px; background: #15161a; color: inherit;
    border: 1px solid #353640; border-radius: 6px; font: inherit; min-width: 0;
  }
  .spin {
    position: absolute; right: 10px; top: 50%; width: 10px; height: 10px; margin-top: -5px;
    border: 2px solid #4b5bd6; border-top-color: transparent; border-radius: 50%; animation: spin 0.7s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .menu {
    position: absolute; z-index: 20; top: calc(100% + 4px); left: 0; right: 0; max-height: 320px; overflow-y: auto;
    list-style: none; margin: 0; padding: 4px; background: #1f2026; border: 1px solid #353640; border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
  }
  .menu button {
    width: 100%; display: grid; grid-template-columns: 1fr auto auto; gap: 10px; align-items: center;
    background: none; border: none; color: #d6d7dc; padding: 5px 8px; border-radius: 5px; font: inherit;
    font-size: 0.88rem; text-align: left; cursor: pointer;
  }
  .menu button:hover, .menu button.active { background: #2d2f3a; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .name.artist { color: #ff8a8a; }
  .name.character { color: #7fdc8f; }
  .name.copyright { color: #d19bff; }
  .name.meta { color: #ffc46b; }
  .name.deprecated { color: #8d8d8d; text-decoration: line-through; }
  .cat { color: #6b6c75; font-size: 0.72rem; text-transform: uppercase; }
  .count { color: #8b8c95; font-size: 0.8rem; min-width: 3.5em; text-align: right; }
</style>
