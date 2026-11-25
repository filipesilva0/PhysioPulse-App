// Detects whether the UI runs inside the Tauri desktop shell or in a browser preview.
export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** Shortcut label: ⌘ on Mac, Ctrl on Windows. */
export const ks = (mac: string, win: string) => (isMac ? mac : win);

/** Native menu bar (Tauri) → same command ids as the in-window menus. */
export async function onNativeMenu(cb: (id: string) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>("menu", (e) => cb(e.payload));
}

/** Server calls go through the Rust core in the desktop app. In the preview they are not connected. */
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!isTauri) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}
