// Browser preview only: stand-ins for device approval (PhysioPulse plan, phase 3; server: routes/devices.rs).
// Same rules as the server where the screens depend on them: owner-only list, can't remove this device,
// a waiting sign-in is approved after a few checks (as if the owner approved it on another computer).
import type { DeviceRow } from "./api";

type Role = "owner" | "therapist" | "reception";
type M = (pt: string, en: string) => string;
const fail = (status: number, message: string, code?: string): never => { throw { status, message, code }; };
const iso = (hoursAgo: number) => new Date(Date.now() - hoursAgo * 3600_000).toISOString().replace(/\.\d+Z$/, "Z");

const devices: DeviceRow[] = [
  { id: "d-phone", kind: "phone", name: "iPhone de Ana", code: "R3V-8PX", status: "pending", how: "", created_at: iso(0.05), approved_at: null, revoked_at: null, last_seen: null, last_ip: "85.240.12.31",
    requested_by_name: "Ana Reis", requested_by_role: "therapist", approved_by_name: null, this_device: false, sessions: 0, signed_in: [] },
  { id: "d-new", name: "MacBook-Rui", code: "K7Q-M2X", status: "pending", how: "", created_at: iso(0.2), approved_at: null, revoked_at: null, last_seen: null, last_ip: "85.240.12.31",
    requested_by_name: "Rui Lopes", requested_by_role: "therapist", approved_by_name: null, this_device: false, sessions: 0, signed_in: [] },
  { id: "d-me", name: "MacBook-Filipe", code: "P4D-9HT", status: "approved", how: "carried_over", created_at: iso(24 * 9), approved_at: iso(24 * 9), revoked_at: null, last_seen: iso(0), last_ip: "192.168.1.20",
    requested_by_name: "Filipe Silva", requested_by_role: "owner", approved_by_name: "Filipe Silva", this_device: true, sessions: 1, signed_in: ["Filipe Silva"] },
  { id: "d-rec", name: "Receção", code: "W2N-6LB", status: "approved", how: "carried_over", created_at: iso(24 * 9), approved_at: iso(24 * 9), revoked_at: null, last_seen: iso(0.4), last_ip: "192.168.1.31",
    requested_by_name: "Marta Costa", requested_by_role: "reception", approved_by_name: "Marta Costa", this_device: false, sessions: 1, signed_in: ["Marta Costa"] },
  { id: "d-ana", name: "Portátil da Ana", code: "H8R-3QZ", status: "approved", how: "owner", created_at: iso(24 * 2), approved_at: iso(24 * 2 - 0.1), revoked_at: null, last_seen: iso(26), last_ip: "94.63.140.7",
    requested_by_name: "Ana Reis", requested_by_role: "therapist", approved_by_name: "Filipe Silva", this_device: false, sessions: 0, signed_in: [] },
  { id: "d-old", name: "PC-antigo", code: "Z5C-7YF", status: "revoked", how: "carried_over", created_at: iso(24 * 9), approved_at: iso(24 * 9), revoked_at: iso(24 * 3), last_seen: iso(24 * 5), last_ip: "192.168.1.44",
    requested_by_name: "Filipe Silva", requested_by_role: "owner", approved_by_name: "Filipe Silva", this_device: false, sessions: 0, signed_in: [] },
];

/** Signed-in iPhones per person (phase 5). */
const day = (n: number) => new Date(Date.now() + n * 86400_000).toISOString().replace(/\.\d+Z$/, "Z");
const phones: Record<string, { id: string; device_id: string; name: string; code: string; signed_in: string; last_seen: string; expires_at: string; this_device: boolean }[]> = {
  fs: [{ id: "ps-fs", device_id: "d-ip-fs", name: "iPhone de Filipe", code: "M4K-2WQ", signed_in: day(-3), last_seen: iso(0.1), expires_at: day(27), this_device: true }],
  ar: [{ id: "ps-ar", device_id: "d-ip-ar", name: "iPhone de Ana", code: "T9C-5HD", signed_in: day(-12), last_seen: iso(5), expires_at: day(18), this_device: true }],
  mc: [],
};
const meId = (role: Role) => (role === "owner" ? "fs" : role === "therapist" ? "ar" : "mc");
const phoneLayout = () => typeof location !== "undefined" && new URLSearchParams(location.search).has("phone");

/** The preview's waiting sign-in: approved on the 4th check (the owner "approves it" meanwhile). */
let checks = 0;
export function previewPendingStart(): { next: "pending"; device: { code: string; name: string }; owner: boolean } {
  checks = 0;
  return { next: "pending", device: { code: "B6T-4KM", name: "Este computador" }, owner: false };
}

export const previewDeviceAlert = () => devices.filter((d) => d.status === "pending").map((d) => ({ id: d.id, name: d.name, code: d.code, kind: d.kind ?? "computer", requested_by_name: d.requested_by_name }));

export function previewDevices(method: string, path: string, body: unknown, m: M, role: Role, session: () => unknown): unknown {
  const p = path.split("?")[0];
  const seg = p.split("/").filter(Boolean);
  const owner = () => { if (role !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role can’t do this.")); };
  if (method === "POST" && p === "/auth/pending") {
    checks += 1;
    if (checks >= 4) return session();
    return { next: "pending", device: { code: "B6T-4KM", name: "Este computador" }, owner: false };
  }
  if (method === "POST" && p === "/auth/pending/recover") {
    const c = String((body as { recovery_code?: string })?.recovery_code ?? "").trim();
    if (!/^[A-Z0-9]{4}-?[A-Z0-9]{4}-?[A-Z0-9]{4}$/i.test(c) || /^AAAA/i.test(c)) fail(401, m("Esse código de recuperação não está certo, ou já foi usado.", "That recovery code isn’t right, or it was already used."));
    return { ...(session() as object), codes_left: 9 };
  }
  // Team: a person's signed-in iPhones (phase 5). The owner sees anyone's; everyone sees their own.
  if (seg[0] === "staff" && seg[2] === "phones") {
    const list = phones[seg[1]] ?? (phones[seg[1]] = []);
    if (method === "GET" && seg.length === 3) {
      if (role !== "owner" && seg[1] !== meId(role)) fail(403, m("A sua função não permite fazer isto.", "Your role can’t do this."));
      return list.map((x) => ({ ...x, this_device: x.this_device && seg[1] === meId(role) && phoneLayout() }));
    }
    if (method === "DELETE" && seg.length === 4) {
      owner();
      const i = list.findIndex((x) => x.id === seg[3]);
      if (i < 0) fail(404, m("Não encontrado.", "Not found."));
      list.splice(i, 1);
      return { ok: true };
    }
  }
  if (seg[0] !== "devices") return undefined;
  owner();
  if (method === "GET" && seg.length === 1) {
    const rank = (d: DeviceRow) => (d.status === "pending" ? 0 : d.status === "approved" ? 1 : 2);
    return [...devices].sort((a, b) => rank(a) - rank(b));
  }
  const d = devices.find((x) => x.id === seg[1]);
  if (!d) return fail(404, m("Não encontrado.", "Not found."));
  if (method === "POST" && seg[2] === "approve") {
    Object.assign(d, { status: "approved", how: "owner", approved_at: iso(0), revoked_at: null, approved_by_name: "Filipe Silva" });
    return { ok: true, status: "approved" };
  }
  if (method === "POST" && seg[2] === "revoke") {
    if (d.this_device) fail(400, m("Está a usar este dispositivo. Remova-o a partir de outro dispositivo aprovado.", "You’re using this device. Remove it from another approved device."));
    const ended = d.sessions;
    Object.assign(d, { status: "revoked", revoked_at: iso(0), sessions: 0, signed_in: [] });
    return { ok: true, status: "revoked", sessions_ended: ended };
  }
  if (method === "PATCH" && seg.length === 2) {
    const name = String((body as { name?: string })?.name ?? "").trim().replace(/\s+/g, " ");
    if (!name || name.length > 60) fail(400, m("Dê um nome ao dispositivo (até 60 caracteres).", "Give the device a name (up to 60 characters)."));
    d.name = name;
    return { ok: true, name };
  }
  return undefined;
}
