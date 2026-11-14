// Small per-device UI preferences (last search, sort). Browser storage can be
// unavailable or cleared, so every access is guarded and optional.

const PREFIX = "vibdl.";

export function loadPref<T>(name: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(PREFIX + name);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

export function savePref(name: string, value: unknown): void {
  try {
    localStorage.setItem(PREFIX + name, JSON.stringify(value));
  } catch {
    /* storage unavailable: preferences just aren't remembered */
  }
}

/** True when a key event comes from a text field (don't hijack typing). */
export function isTyping(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
}
