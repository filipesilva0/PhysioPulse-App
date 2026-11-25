// Browser preview only: stand-ins for the audit phase A routes (server: routes/account.rs, clinic.rs, system.rs).
import { nifOk } from "./validate";
// Same rules as the server where the screens depend on them: owner-only routes answer 403 for other roles,
// codes are shown once, a wrong current password is refused. Nothing is saved.
type Role = "owner" | "therapist" | "reception";
type M = (pt: string, en: string) => string;
type Body = Record<string, unknown>;

const fail = (status: number, message: string): never => { throw { status, message }; };
const pad = (n: number) => String(n).padStart(2, "0");
const code = () => { const A = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; let s = ""; for (let i = 0; i < 12; i++) s += A[Math.floor(Math.random() * A.length)]; return `${s.slice(0, 4)}-${s.slice(4, 8)}-${s.slice(8)}`; };
const iso = (hoursAgo: number) => new Date(Date.now() - hoursAgo * 3600_000).toISOString().replace(/\.\d+Z$/, "Z");
const stamp = (d: Date) => `${d.getUTCFullYear()}${pad(d.getUTCMonth() + 1)}${pad(d.getUTCDate())}-${pad(d.getUTCHours())}${pad(d.getUTCMinutes())}00`;

let clinic = { name: "Clínica de Fisioterapia", nif: "", ers_number: "", address: "", phone: "", email: "", logo: null as string | null, updated_at: null as string | null };
let codesLeft = 10;
let invites = [
  { id: "inv1", role: "therapist", email: "rui@clinica.pt", created_at: iso(30), expires_at: iso(-24 * 6), created_by_name: "Filipe Silva" },
];
const lastBackup = new Date(Date.now() - 6.5 * 3600_000);
let backup = {
  state: "ok", requested: false, last_ok: lastBackup.toISOString().replace(/\.\d+Z$/, "Z"), last_error: null as string | null,
  db_file: `db-${stamp(lastBackup)}.dump.age`, files_file: `files-${stamp(lastBackup)}.tar.age`, bytes: 48_213_504, kept_days: 30, offsite: "off",
  next_at: (() => { const d = new Date(); d.setUTCHours(3, 0, 0, 0); if (d.getTime() < Date.now()) d.setUTCDate(d.getUTCDate() + 1); return d.toISOString().replace(/\.\d+Z$/, "Z"); })(),
};

export function previewAccount(method: string, path: string, body: unknown, m: M, role: Role, me: { name: string; email: string }): unknown {
  const b = (body ?? {}) as Body;
  const p = path.split("?")[0];
  const owner = () => { if (role !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role can’t do this.")); };
  const seg = p.split("/").filter(Boolean);
  switch (`${method} ${p}`) {
    case "POST /me/password": {
      if (!b.current || b.current === "wrong") fail(400, m("A palavra-passe atual não está certa.", "Your current password isn’t right."));
      if (String(b.new ?? "").length < 12) fail(400, m("Escolha uma palavra-passe com pelo menos 12 caracteres.", "Choose a password with at least 12 characters."));
      if (b.new === b.current) fail(400, m("Escolha uma palavra-passe diferente da atual.", "Choose a password that’s different from the current one."));
      return { ok: true, other_sessions_ended: 1 };
    }
    case "GET /me/recovery-codes": owner(); return { left: codesLeft, total: 10, created_at: iso(24 * 40) };
    case "POST /me/recovery-codes":
      owner();
      if (!b.password || b.password === "wrong") fail(400, m("A palavra-passe não está certa.", "Your password isn’t right."));
      codesLeft = 10;
      return { codes: Array.from({ length: 10 }, code) };
    case "POST /auth/reset":
      if (String(b.password ?? "").length < 12) fail(400, m("Escolha uma palavra-passe com pelo menos 12 caracteres.", "Choose a password with at least 12 characters."));
      if (!/^[A-Z0-9]{4}-?[A-Z0-9]{4}-?[A-Z0-9]{4}$/i.test(String(b.code ?? "").trim()) || /^AAAA/i.test(String(b.code))) fail(400, m("Esse código não serve para este email, ou expirou. Peça um novo ao proprietário.", "That code isn’t right for this email, or it has expired. Ask the clinic owner for a new one."));
      return { ok: true };
    case "GET /invites": owner(); return invites;
    case "POST /invites": {
      owner();
      const c = "K7QM-2XRP-9TDA";
      invites = [{ id: `inv${Date.now()}`, role: String(b.role), email: (b.email as string) || null as unknown as string, created_at: iso(0), expires_at: iso(-24 * 7), created_by_name: me.name }, ...invites];
      return { code: c, role: b.role, expires_in_days: 7 };
    }
    case "GET /clinic": return clinic;
    case "PUT /clinic": {
      owner();
      const name = String(b.name ?? "").trim();
      if (!name) fail(400, m("Escreva o nome da clínica.", "Write the clinic’s name."));
      const nif = String(b.nif ?? "").replace(/\s/g, "");
      if (nif && !nifOk(nif)) fail(400, m("Esse NIF não é válido. Confirme os 9 dígitos.", "That NIF isn’t valid. Check the 9 digits."));
      const email = String(b.email ?? "").trim().toLowerCase();
      if (email && !/^[^\s@]+@[^\s@]+$/.test(email)) fail(400, m("Confirme o endereço de email.", "Check the email address."));
      clinic = { ...clinic, name, nif, ers_number: String(b.ers_number ?? "").trim(), address: String(b.address ?? "").trim(), phone: String(b.phone ?? "").trim(), email,
        logo: "logo" in b ? ((b.logo as string) || null) : clinic.logo, updated_at: new Date().toISOString() };
      return clinic;
    }
    case "GET /system/status":
      owner();
      return { version: "0.17.0", address: "https://physiopulse-clinica.duckdns.org", database: "PostgreSQL 16.4", archive: "/data/PhysioPulse", backup, stale_hours: 36, secrets_encrypted: true };
    case "POST /system/backup": {
      owner();
      backup = { ...backup, requested: true };
      // The backup service picks the request up within 30 s; the preview finishes it after a few seconds.
      setTimeout(() => { backup = { ...backup, state: "running", requested: false }; }, 2500);
      setTimeout(() => { const d = new Date(); backup = { ...backup, state: "ok", last_ok: d.toISOString().replace(/\.\d+Z$/, "Z"), db_file: `db-${stamp(d)}.dump.age`, files_file: `files-${stamp(d)}.tar.age` }; }, 7000);
      return { ok: true, backup };
    }
  }
  // /staff/:id/reset-code, /staff/:id/reset-two-step, /invites/:id
  if (method === "POST" && seg[0] === "staff" && seg[2] === "reset-code") { owner(); return { code: code(), email: "", expires_in_hours: 24 }; }
  if (method === "POST" && seg[0] === "staff" && seg[2] === "reset-two-step") { owner(); return { ok: true }; }
  if (method === "DELETE" && seg[0] === "invites" && seg[1]) {
    owner();
    if (!invites.some((x) => x.id === seg[1])) fail(409, m("Este convite já foi usado ou retirado.", "This invite was already used or withdrawn."));
    invites = invites.filter((x) => x.id !== seg[1]);
    return { ok: true };
  }
  return undefined;
}

/** Used by the preview's sign-in: one recovery code fewer. */
export const previewUseRecoveryCode = () => { codesLeft = Math.max(0, codesLeft - 1); return codesLeft; };
export const previewBackupState = () => backup.state;
/** The clinic details, for the website stand-in (phase 4a). */
export const previewClinic = () => ({ name: clinic.name, address: clinic.address, phone: clinic.phone, email: clinic.email, nif: clinic.nif, ers_number: clinic.ers_number });
