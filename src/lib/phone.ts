// The staff iPhone app (PhysioPulse plan, phase 5): the bridge to the Rust core's Face ID lock.
// The session token never reaches this code: it lives in the iPhone's keychain behind Face ID and, once unlocked,
// in Rust memory (src-tauri/src/phone.rs). Here we only ask Rust to unlock, lock and sign out.
// In the browser preview the same calls are stand-ins, so the screens can be tried.
import { isTauri } from "./platform";
import { localise, type ApiErr } from "./api";

/** Show the phone layout: inside the iPhone app, or the browser preview opened with `?phone`. */
export const isPhone = typeof window !== "undefined" && (
  (isTauri && /iPhone|iPad|iPod/.test(navigator.userAgent)) || new URLSearchParams(location.search).has("phone"));

/** Who is signed in on this phone (no token). */
export interface PhoneUser { name: string; email: string; server: string }

const KEY = "physiopulse-preview-phone";
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  try { return await invoke<T>(cmd, args); } catch (e) { throw localise(e); }
}

/** The person signed in on this phone, or null (then the sign-in screen shows). */
export async function phoneUser(): Promise<PhoneUser | null> {
  if (isTauri) return invoke<PhoneUser | null>("phone_user").catch(() => null);
  try { const s = localStorage.getItem(KEY); return s ? JSON.parse(s) as PhoneUser : null; } catch { return null; }
}

/** Preview only: remember the person after a sign-in (the real app does it in Rust, with the token). */
export function previewKeep(u: PhoneUser) {
  if (isTauri) return;
  try { localStorage.setItem(KEY, JSON.stringify(u)); } catch { /* storage unavailable */ }
}

/** Face ID (or the passcode). Throws `unlock_cancelled` (stay on the lock screen) or `signed_out` (sign in). */
export async function unlock(prompt: string): Promise<void> {
  if (isTauri) return invoke<void>("phone_unlock", { prompt });
  await wait(700);
}

/** The app left the screen: forget the token in memory until Face ID again. */
export async function lock(): Promise<void> {
  if (isTauri) await invoke<void>("phone_lock").catch(() => undefined);
}

/** Sign out on this phone: the session ends on the server and the keychain item is deleted. */
export async function signOutPhone(): Promise<void> {
  if (isTauri) { await invoke<void>("phone_sign_out").catch(() => undefined); return; }
  try { localStorage.removeItem(KEY); } catch { /* ignore */ }
}

export const isCancel = (e: unknown) => (e as ApiErr)?.code === "unlock_cancelled";
