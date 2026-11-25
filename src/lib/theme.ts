// Light, dark or the system's appearance (owner, 9 Nov 2026: "I'd like to change between light and dark
// mode manually"). Kept on this computer only. CSS reads `data-theme` on <html> (src/styles/app.css); in the
// desktop app Rust also sets the window's theme so the title bar, Mica/vibrancy and native pop-ups match.
import { isTauri } from "./platform";

export type Theme = "system" | "light" | "dark";
const KEY = "physiopulse-theme";

export function getTheme(): Theme {
  try { const t = localStorage.getItem(KEY); return t === "light" || t === "dark" ? t : "system"; } catch { return "system"; }
}

export function applyTheme(t: Theme) {
  const r = document.documentElement;
  if (t === "system") delete r.dataset.theme; else r.dataset.theme = t;
  if (isTauri) import("@tauri-apps/api/core").then(({ invoke }) => invoke("set_theme", { theme: t })).catch(() => { /* window keeps the system look */ });
}

export function setTheme(t: Theme) {
  try { if (t === "system") localStorage.removeItem(KEY); else localStorage.setItem(KEY, t); } catch { /* still applies for this session */ }
  applyTheme(t);
}
