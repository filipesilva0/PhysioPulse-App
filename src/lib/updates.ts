// In-app updates (desktop app): new versions come from GitHub Releases and must carry the owner's signature,
// or the Rust side refuses them (src-tauri/src/updates.rs, claude/RELEASING.md). One shared state for the
// whole app, so the Settings row, the menu command and the check at launch agree.
import { isTauri } from "./platform";

export interface UpdateInfo { version: string; current: string; notes: string | null; date: string | null }
export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "latest"; at: number }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "installing"; info: UpdateInfo; pct: number | null }
  | { kind: "error"; message: string };

let state: UpdateState = { kind: "idle" };
const subs = new Set<() => void>();
const set = (s: UpdateState) => { state = s; subs.forEach((f) => f()); };
export const updateState = () => state;
export const onUpdateState = (f: () => void) => { subs.add(f); return () => { subs.delete(f); }; };

async function core() { return import("@tauri-apps/api/core"); }

// Browser preview: a stand-in so the row can be clicked through. Nothing is downloaded.
const PREVIEW: UpdateInfo = { version: "0.15.0", current: "0.14.0", notes: "(Pré-visualização) As novidades da nova versão aparecem aqui.", date: null };

/** Asks GitHub for a newer version. Never throws: the result is in the shared state. */
export async function checkForUpdate(): Promise<UpdateState> {
  if (state.kind === "checking" || state.kind === "installing") return state;
  set({ kind: "checking" });
  try {
    const info = isTauri ? await (await core()).invoke<UpdateInfo | null>("update_check")
      : await new Promise<UpdateInfo>((r) => setTimeout(() => r(PREVIEW), 700));
    set(info ? { kind: "available", info } : { kind: "latest", at: Date.now() });
  } catch (e) {
    set({ kind: "error", message: String(e) });
  }
  return state;
}

/** Downloads, checks the signature, installs and restarts. Only returns if something went wrong. */
export async function installUpdate(): Promise<void> {
  if (state.kind !== "available") return;
  const info = state.info;
  set({ kind: "installing", info, pct: null });
  if (!isTauri) {
    for (let p = 0; p <= 100; p += 10) { await new Promise((r) => setTimeout(r, 150)); set({ kind: "installing", info, pct: p }); }
    set({ kind: "available", info });
    return;
  }
  const { listen } = await import("@tauri-apps/api/event");
  const off = await listen<{ downloaded: number; total: number | null }>("update:progress", (e) => {
    const { downloaded, total } = e.payload;
    set({ kind: "installing", info, pct: total ? Math.min(100, Math.round((downloaded / total) * 100)) : null });
  });
  try {
    await (await core()).invoke("update_install");
  } catch (e) {
    set(String(e) === "no_update" ? { kind: "latest", at: Date.now() } : { kind: "error", message: String(e) });
  } finally {
    off();
  }
}

/** Once at launch (after sign-in) and then twice a day, quietly. Only a new version is announced. */
export function startUpdateChecks(onNew: (info: UpdateInfo) => void): () => void {
  if (!isTauri) return () => {};
  let last = "";
  const run = () => checkForUpdate().then((s) => { if (s.kind === "available" && s.info.version !== last) { last = s.info.version; onNew(s.info); } });
  const first = setTimeout(run, 8_000);
  const again = setInterval(run, 12 * 3600_000);
  return () => { clearTimeout(first); clearInterval(again); };
}
