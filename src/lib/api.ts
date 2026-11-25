// One way to talk to the clinic server.
// Desktop app: goes through the Rust core (which holds the session token).
// Browser preview: a small stand-in server so the same screens can be tried.
import { isTauri } from "./platform";
import { preview2, setPreviewRole, getPreviewRole, previewPatientLite } from "./preview";
import { previewAccount, previewClinic, previewUseRecoveryCode } from "./previewAccount";
import { previewPortal, previewWebsite } from "./previewWebsite";
import { previewRec } from "./previewRec";
import { previewDevices, previewPendingStart } from "./previewDevices";
export { setPreviewRole };

export type Role = "owner" | "therapist" | "reception";
export interface Me { id: string; email: string; name: string; role: Role;
  /** The person's AI switch (Team): always on for the owner, off for reception (PhysioPulse plan, phase 2). */
  ai_enabled?: boolean }
export interface ApiErr { status: number; message: string; code?: string; details?: unknown }
export interface Enrol { challenge: string; otpauth_uri: string; secret: string; qr_svg: string }
export interface StaffRow { id: string; email: string; name: string; role: Role; active: boolean; totp_enabled: boolean; ai_enabled?: boolean; created_at: string; last_seen: string | null }
/** A device (one copy of the app on one computer or phone) in Settings › Devices (phase 3). */
export interface DeviceRow {
  id: string; name: string; code: string; status: "pending" | "approved" | "revoked"; how: string;
  /** "phone" for the staff iPhone app (phase 5): its sessions last 30 days behind Face ID. */
  kind?: "computer" | "phone";
  created_at: string; approved_at: string | null; revoked_at: string | null; last_seen: string | null; last_ip: string;
  requested_by_name: string | null; requested_by_role: Role | null; approved_by_name: string | null;
  this_device: boolean; sessions: number; signed_in: string[];
}
/** The answer while a new device waits for the owner (the ticket itself stays in the Rust core). */
export interface Pending { next: "pending"; device: { code: string; name: string }; owner: boolean }
export interface LogRow { id: number; at: string; staff_name: string | null; action: string; subject_type: string | null; subject_id: string | null; device: string; detail: string; subject_label?: string | null }

export const isErr = (e: unknown): e is ApiErr => typeof e === "object" && e !== null && "message" in e;

const pt = () => document.documentElement.lang.startsWith("pt");

/** Errors made by the desktop app itself (no answer, timeout, bad address…) and the server's generic ones,
 *  in the app's language. Rust sends a `code` and an English fallback (`src-tauri/src/server.rs`). */
const LOCAL: Record<string, [string, string]> = {
  offline: ["Não foi possível contactar o servidor. Verifique a ligação à internet.", "Can’t reach the server. Check the internet connection."],
  offline_address: ["Não foi possível contactar o servidor. Verifique a ligação à internet e se o endereço está certo.", "Can’t reach the server. Check the internet connection and that the address is right."],
  https_needed: ["Os endereços na internet têm de começar por https://.", "Addresses on the internet must start with https://."],
  // Device approval (PhysioPulse plan, phase 3; server/src/routes/devices.rs)
  app_update: ["Atualize a aplicação PhysioPulse neste computador para entrar.", "Update the PhysioPulse app on this computer to sign in."],
  device_revoked: ["O responsável da clínica removeu este dispositivo. Peça-lhe que o aprove de novo em Definições › Dispositivos.", "The clinic owner removed this device. Ask them to approve it again in Settings › Devices."],
  no_pending: ["Este passo expirou. Entre de novo.", "This step expired. Sign in again."],
  // The staff iPhone app (phase 5; src-tauri/src/phone.rs)
  unlock_cancelled: ["Desbloqueie o PhysioPulse com o Face ID para continuar.", "Unlock PhysioPulse with Face ID to continue."],
  signed_out: ["Entre de novo.", "Sign in again."],
  no_phone: ["Isto só funciona no iPhone.", "This only works on the iPhone."],
  timeout: ["O servidor demorou demasiado a responder. Tente de novo.", "The server took too long to answer. Try again."],
  ai_timeout: ["A IA local demorou demasiado a responder. Verifique se o computador da IA está ligado e tente de novo.", "The local AI took too long to answer. Check the AI computer is on, then try again."],
  bad_address: ["Use um endereço como clinica.duckdns.org ou 192.168.1.64:8095.", "Use an address like clinica.duckdns.org or 192.168.1.64:8095."],
  not_physiopulse: ["Esse endereço não é um servidor PhysioPulse.", "That address isn’t a PhysioPulse server."],
  no_server: ["Ainda não há servidor configurado.", "No server set up yet."],
  invalid: ["Pedido inválido.", "Invalid request."],
  interrupted: ["A transferência foi interrompida. Tente de novo.", "The download was interrupted. Try again."],
  no_downloads: ["Não foi possível encontrar a pasta Transferências.", "Can’t find the Downloads folder."],
  save_failed: ["Não foi possível guardar o ficheiro em Transferências.", "Couldn’t save the file in Downloads."],
  // Local AI (server/src/routes/letters.rs ai_draft)
  ai_offline: ["Sem resposta da IA local. Verifique se o computador da IA está ligado e o Ollama a correr, e o endereço em Definições › IA local.", "Can’t reach the local AI. Check the AI computer is on and Ollama is running, and its address in Settings › Local AI."],
  ai_no_model: ["O modelo escolhido não está instalado no computador da IA. Escolha outro em Definições › IA local.", "The chosen model isn’t installed on the AI computer. Choose another in Settings › Local AI."],
  ai_failed: ["A IA não conseguiu acabar. Tente de novo, ou faça-o à mão.", "The AI couldn’t finish. Try again, or do it by hand."],
  ai_no_vision: ["O modelo de IA escolhido não lê imagens. Escolha um que leia (por exemplo gemma3:12b ou qwen2.5vl:7b) em Definições › IA local, ou escreva os dados.", "The chosen AI model can’t read pictures. Choose one that can (for example gemma3:12b or qwen2.5vl:7b) in Settings › Local AI, or type the details."],
  ai_scan_pdf: ["Este PDF é uma digitalização, por isso a IA não o consegue ler. Anexe uma foto do talão, ou escreva os dados.", "This PDF is a scanned picture, so the AI can’t read it. Add a photo of the receipt instead, or type the details."],
  ai_heic: ["A IA não lê fotos HEIC. Use JPEG ou PDF (no iPhone: Definições › Câmara › Formatos › Mais compatível), ou escreva os dados.", "The AI can’t read HEIC photos. Use a JPEG or PDF (on an iPhone: Settings › Camera › Formats › Most Compatible), or type the details."],
  // Generic server answers (server/src/error.rs)
  forbidden: ["A sua função não permite fazer isto.", "Your role can’t do this."],
  not_found: ["Não encontrado.", "Not found."],
  too_many: ["Demasiadas tentativas. Tente de novo daqui a 15 minutos.", "Too many attempts. Try again in 15 minutes."],
  internal: ["Algo correu mal no servidor.", "Something went wrong on the server."],
};
/** Plain-text errors from the reminder opener (`src-tauri/src/messages.rs`), and server messages for a server older
 *  than 0.13.0 (from 0.13.0 the server answers in the app's language itself: `server/src/i18n.rs`, audit M13). */
const TEXTS: Record<string, string> = {
  "This link can’t be opened.": "Não é possível abrir esta ligação.",
  "No app on this computer opens this kind of message.": "Nenhuma aplicação deste computador abre este tipo de mensagem.",
  "Something went wrong.": "Algo correu mal.",
  // Server answers for Settings › Local AI (server/src/routes/letters.rs)
  "Use the address of a computer in the clinic, like 100.86.97.21 or 192.168.1.20:11434. Internet addresses aren’t allowed.": "Use o endereço de um computador da clínica, como 100.86.97.21 ou 192.168.1.20:11434. Endereços da internet não são permitidos.",
  "That model name isn’t valid. Use a name like gemma3:12b.": "Esse nome de modelo não é válido. Use um nome como gemma3:12b.",
  // Phase 4 (server/src/routes/assist.rs, assistant.rs)
  "Fill in the note first: the AI only rewrites what’s there.": "Preencha a nota primeiro: a IA só reescreve o que lá está.",
  "The report is empty.": "O relatório está vazio.",
  "Each day needs an opening time before its closing time, like 08:00–20:00, or Closed.": "Cada dia precisa de uma hora de abertura antes da de fecho, como 08:00–20:00, ou Fechado.",
  "Choose today or a later day.": "Escolha hoje ou um dia a seguir.",
  "The last day must be on or after the first day.": "O último dia tem de ser igual ou depois do primeiro.",
  "A closure can be at most three months. Add another one for longer.": "Um encerramento pode ter no máximo três meses. Para mais tempo, acrescente outro.",
  "Choose the first day.": "Escolha o primeiro dia.", "Choose the last day.": "Escolha o último dia.",
  "Write what you’d like the assistant to do.": "Escreva o que quer que o assistente faça.",
  "This change was already applied or cancelled.": "Esta alteração já foi aplicada ou cancelada.",
  "This proposal is more than a day old. Ask the assistant again.": "Esta proposta tem mais de um dia. Peça de novo ao assistente.",
  "Those days have passed. Ask the assistant again.": "Esses dias já passaram. Peça de novo ao assistente.",
  "This was changed since the assistant suggested it. Ask the assistant again.": "Isto foi alterado depois da proposta do assistente. Peça de novo.",
  "Only an applied change can be undone.": "Só se pode anular uma alteração aplicada.",
  // Accounts, clinic details, server and backups (server/src/routes/account.rs, clinic.rs, system.rs — audit phase A)
  "Choose a password with at least 12 characters.": "Escolha uma palavra-passe com pelo menos 12 caracteres.",
  "That password is too long.": "Essa palavra-passe é demasiado longa.",
  "Write your name.": "Escreva o seu nome.",
  "Your current password isn’t right.": "A palavra-passe atual não está certa.",
  "Choose a password that’s different from the current one.": "Escolha uma palavra-passe diferente da atual.",
  "Your password isn’t right.": "A palavra-passe não está certa.",
  "This step expired. Sign in again.": "Este passo expirou. Entre de novo.",
  "That recovery code isn’t right, or it was already used.": "Esse código de recuperação não está certo, ou já foi usado.",
  "This account is no longer active.": "Esta conta já não está ativa.",
  "That code isn’t right for this email, or it has expired. Ask the clinic owner for a new one.": "Esse código não serve para este email, ou expirou. Peça um novo ao proprietário.",
  "Use Settings › Account for your own account.": "Para a sua própria conta, use Definições › Conta.",
  "This invite was already used or withdrawn.": "Este convite já foi usado ou retirado.",
  "Write the clinic’s name.": "Escreva o nome da clínica.",
  "That NIF isn’t valid. Check the 9 digits.": "Esse NIF não é válido. Confirme os 9 dígitos.",
  "Check the email address.": "Confirme o endereço de email.",
  "Check the phone number.": "Confirme o número de telefone.",
  "Some details are too long.": "Alguns dados são demasiado longos.",
  "The logo must be a JPEG or PNG picture.": "O logótipo tem de ser uma imagem JPEG ou PNG.",
  "The logo is too big. Use a picture under 512 KB.": "O logótipo é demasiado grande. Use uma imagem com menos de 512 KB.",
  "The backup service isn’t answering. Update the server, then try again.": "O serviço de cópias de segurança não está a responder. Atualize o servidor e tente de novo.",
  "The email or password is wrong.": "O email ou a palavra-passe estão errados.",
  "That code isn’t right. Check the time on your phone and try the newest code.": "Esse código não está certo. Confirme a hora do telemóvel e use o código mais recente.",
  "You can’t change your own role or deactivate yourself.": "Não pode mudar a sua própria função nem desativar-se.",
  "Choose a role.": "Escolha uma função.",
  "This was changed again since, so it can’t be undone here. Change it by hand.": "Isto já foi alterado outra vez, por isso não pode ser anulado aqui. Altere-o à mão.",
};

/** Any error from Rust or the server → an ApiErr whose message is in the app's language when we know it. */
export function localise(e: unknown): ApiErr {
  const err: ApiErr = isErr(e) ? { ...e } : { status: 0, message: String(e) };
  const t = err.code ? LOCAL[err.code] : undefined;
  if (t) err.message = pt() ? t[0] : t[1];
  else if (pt() && TEXTS[err.message]) err.message = TEXTS[err.message];
  return err;
}

/** Fired when the server says the session is over; the app locks. */
export const SIGNED_OUT = "physiopulse:signed-out";
/** Fired after any successful change (POST/PATCH/PUT/DELETE), so open screens reload what they show. */
export const CHANGED = "physiopulse:changed";

/** `quiet`: don't tell open screens to reload (autosave: a reload would re-open the patient file and log it again). */
export async function api<T>(method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE", path: string, body?: unknown, opts?: { quiet?: boolean }): Promise<T> {
  try {
    let r: T;
    if (isTauri) {
      const { invoke } = await import("@tauri-apps/api/core");
      r = await invoke<T>("api", { method, path, body: body ?? null, lang: pt() ? "pt" : "en" });
    } else {
      r = (await mock(method, path, body)) as T;
    }
    if (method !== "GET" && !opts?.quiet && !path.startsWith("/auth/") && path !== "/setup") setTimeout(() => window.dispatchEvent(new Event(CHANGED)), 0);
    return r;
  } catch (e) {
    const err = localise(e);
    if (err.status === 401 && !path.startsWith("/auth/") && path !== "/setup") window.dispatchEvent(new Event(SIGNED_OUT));
    throw err;
  }
}

/** Save a server file (invoice PDF, receipt, accountant export, patient document) to the computer's Downloads folder.
 *  The desktop app downloads it in Rust (the token never reaches the UI). Returns where it was saved. */
export async function download(fileId: string, filename: string, from: "finance" | "document" = "finance"): Promise<string> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    const path = from === "document" ? `/documents/${fileId}/raw` : `/finance/files/${fileId}/raw`;
    try { return await invoke<string>("download", { path, filename }); }
    catch (e) { const err = localise(e); if (err.status === 401) window.dispatchEvent(new Event(SIGNED_OUT)); throw err; }
  }
  await wait(400);
  return `Downloads/${filename}`;
}

/** An invoice PDF or receipt from the server, to show inside the app (nothing is saved on the computer). */
export async function viewFile(fileId: string): Promise<Blob> {
  if (!isTauri) { await wait(300); throw { status: 0, message: pt() ? "A pré-visualização no navegador não tem ficheiros." : "The browser preview has no files." } as ApiErr; }
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    const buf = new Uint8Array(await invoke<ArrayBuffer>("view_file", { path: `/finance/files/${fileId}/raw` }));
    const head = String.fromCharCode(...buf.slice(0, 12));
    const mime = head.startsWith("%PDF") ? "application/pdf" : buf[0] === 0xff && buf[1] === 0xd8 ? "image/jpeg"
      : head.startsWith("\x89PNG") ? "image/png" : head.startsWith("RIFF") && head.slice(8) === "WEBP" ? "image/webp" : "application/octet-stream";
    return new Blob([buf], { type: mime });
  } catch (e) { const err = localise(e); if (err.status === 401) window.dispatchEvent(new Event(SIGNED_OUT)); throw err; }
}

/** A session recording (WAV) to play inside the app. Fetched by the Rust core; nothing is saved on the computer.
 *  The server writes every play to the access log. */
export async function recordingAudio(id: string): Promise<Blob> {
  if (!isTauri) { await wait(300); throw { status: 0, message: pt() ? "A pré-visualização no navegador não tem áudio." : "The browser preview has no audio." } as ApiErr; }
  const { invoke } = await import("@tauri-apps/api/core");
  try { return new Blob([new Uint8Array(await invoke<ArrayBuffer>("view_file", { path: `/recordings/${id}/audio` }))], { type: "audio/wav" }); }
  catch (e) { const err = localise(e); if (err.status === 401) window.dispatchEvent(new Event(SIGNED_OUT)); throw err; }
}

/** Export a patient's clinical record (a zip) straight to Downloads. `kind`: "full" or "summary". */
export async function downloadRecord(patientId: string, kind: "full" | "summary", filename: string): Promise<string> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    try { return await invoke<string>("download", { path: `/patients/${patientId}/export/${kind}`, filename }); }
    catch (e) { const err = localise(e); if (err.status === 401) window.dispatchEvent(new Event(SIGNED_OUT)); throw err; }
  }
  await mock("GET", `/patients/${patientId}/export/${kind}`, undefined);
  await wait(600);
  return `Downloads/${filename}`;
}

/** The access log as a CSV (owner), with the same filters as the list, straight to Downloads (audit phase C). */
export async function downloadAccessLog(query: string, filename: string): Promise<string> {
  const path = `/access-log/export${query ? `?${query}` : ""}`;
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    try { return await invoke<string>("download", { path, filename }); }
    catch (e) { const err = localise(e); if (err.status === 401) window.dispatchEvent(new Event(SIGNED_OUT)); throw err; }
  }
  await mock("GET", path, undefined);
  await wait(400);
  return `Downloads/${filename}`;
}

/** Opens a prepared reminder (sms:, https://wa.me/ or mailto: link) in Messages, WhatsApp or email.
 *  In the desktop app Rust checks the link first; in the browser preview it opens a new tab. */
export async function openMessage(url: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    try { await invoke("open_message", { url }); } catch (e) { throw localise(e); }
    return;
  }
  window.open(url, "_blank", "noopener");
}

export async function getServer(): Promise<string | null> {
  if (!isTauri) return "100.68.180.126:8095";
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string | null>("get_server");
}

export async function setServer(url: string): Promise<{ setup_needed: boolean; clinic: string | null }> {
  if (!isTauri) { await wait(500); return { setup_needed: firstRun(), clinic: "Clínica de Fisioterapia" }; }
  const { invoke } = await import("@tauri-apps/api/core");
  try { return await invoke("set_server", { url }); } catch (e) { throw localise(e); }
}

/** The last person who signed in on this computer (name + email only; the trust token stays in Rust). */
export interface Remembered { email: string; name: string }
export async function getRemembered(): Promise<Remembered | null> {
  if (!isTauri) { try { const r = localStorage.getItem("physiopulse-preview-account"); return r ? JSON.parse(r) as Remembered : null; } catch { return null; } }
  const { invoke } = await import("@tauri-apps/api/core");
  try { return await invoke<Remembered | null>("remembered_account"); } catch { return null; }
}
/** Removes this person from this computer: next time they need their email, password and the 6-digit code. */
export async function forgetRemembered(email: string): Promise<void> {
  if (!isTauri) { try { localStorage.removeItem("physiopulse-preview-account"); } catch { /* ignore */ } return; }
  const { invoke } = await import("@tauri-apps/api/core");
  try { await invoke("forget_account", { email }); } catch { /* best effort */ }
}

// ───────────── Browser preview stand-in ─────────────
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
export const firstRun = () => { try { return localStorage.getItem("physiopulse-preview-firstrun") === "1"; } catch { return false; } };
export const setFirstRun = (on: boolean) => { try { on ? localStorage.setItem("physiopulse-preview-firstrun", "1") : localStorage.removeItem("physiopulse-preview-firstrun"); } catch { /* ignore */ } };

let previewMe: Me = { id: "fs", email: "filipe@clinica.pt", name: "Filipe Silva", role: "owner", ai_enabled: true };
const previewStaff: StaffRow[] = [
  { id: "fs", email: "filipe@clinica.pt", name: "Filipe Silva", role: "owner", active: true, totp_enabled: true, ai_enabled: true, created_at: "2026-10-01T09:00:00Z", last_seen: "2026-11-04T10:02:00Z" },
  { id: "ar", email: "ana@clinica.pt", name: "Ana Rocha", role: "therapist", active: true, totp_enabled: true, ai_enabled: true, created_at: "2026-10-02T09:00:00Z", last_seen: "2026-11-04T09:12:00Z" },
  { id: "mc", email: "marta@clinica.pt", name: "Marta Costa", role: "reception", active: true, totp_enabled: true, ai_enabled: false, created_at: "2026-10-02T10:00:00Z", last_seen: "2026-11-03T16:40:00Z" },
];

function qrSvg(seed: string) {
  const n = 25; let s = [...seed].reduce((a, c) => a + c.charCodeAt(0), 7); let rects = "";
  for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) {
    s = (s * 1103515245 + 12345) & 0x7fffffff;
    const f = (x < 7 && y < 7) || (x > n - 8 && y < 7) || (x < 7 && y > n - 8);
    if (!f && s % 3 === 0) rects += `<rect x="${x}" y="${y}" width="1" height="1"/>`;
  }
  const finder = (fx: number, fy: number) => `<rect x="${fx + .5}" y="${fy + .5}" width="6" height="6" fill="none" stroke="#000"/><rect x="${fx + 2}" y="${fy + 2}" width="3" height="3"/>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="-2 -2 ${n + 4} ${n + 4}" width="200" height="200"><rect x="-2" y="-2" width="${n + 4}" height="${n + 4}" fill="#fff"/>${rects}${finder(0, 0)}${finder(n - 7, 0)}${finder(0, n - 7)}</svg>`;
}
const enrol = (email: string): Enrol => ({
  challenge: "preview-enrol", secret: "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP", qr_svg: qrSvg(email),
  otpauth_uri: `otpauth://totp/Cl%C3%ADnica:${encodeURIComponent(email)}?secret=JBSWY3DPEHPK3PXP&issuer=Cl%C3%ADnica`,
});
export const fail = (status: number, message: string, code?: string, details?: unknown): never => { throw { status, message, code, details }; };

async function mock(method: string, path: string, body: unknown): Promise<unknown> {
  await wait(350);
  const b = (body ?? {}) as Record<string, string>;
  const pt = document.documentElement.lang.startsWith("pt");
  const M = (p: string, e: string) => (pt ? p : e);
  const rd = previewDevices(method, path, body, M, getPreviewRole(), () => ({ next: "session", staff: previewMe, idle_minutes: 15 }));
  if (rd !== undefined) return rd;
  const ra = previewAccount(method, path, body, M, getPreviewRole(), previewMe);
  if (ra !== undefined) return ra;
  const rp = previewPortal(method, path, body, M, getPreviewRole(), previewMe, previewPatientLite);
  if (rp !== undefined) return rp;
  const rw = previewWebsite(method, path, body, M, getPreviewRole(), previewClinic);
  if (rw !== undefined) return rw;
  const rr = previewRec(method, path, body, M, getPreviewRole(), () => undefined);
  if (rr !== undefined) return rr;
  const r = preview2(method, path, body, M);
  if (r !== undefined) return r;
  switch (`${method} ${path.split("?")[0]}`) {
    case "GET /status": return { setup_needed: firstRun(), clinic: "Clínica de Fisioterapia" };
    case "POST /setup":
      if ((b.password ?? "").length < 12) fail(400, M("Escolha uma palavra-passe com pelo menos 12 caracteres.", "Choose a password with at least 12 characters."));
      previewMe = { id: "fs", email: b.email, name: b.name, role: "owner" };
      return enrol(b.email);
    case "POST /auth/signup":
      if (!/^[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4}$/i.test((b.invite ?? "").trim())) fail(400, M("Esse código de convite não é válido ou expirou. Peça um novo ao proprietário.", "That invite code isn’t valid or has expired. Ask the clinic owner for a new one."));
      if ((b.password ?? "").length < 12) fail(400, M("Escolha uma palavra-passe com pelo menos 12 caracteres.", "Choose a password with at least 12 characters."));
      previewMe = { id: "new", email: b.email, name: b.name, role: "therapist" };
      return enrol(b.email);
    case "POST /auth/login":
      if (!b.email || !b.password || b.password === "wrong") fail(401, M("O email ou a palavra-passe estão errados.", "The email or password is wrong."));
      previewMe = { ...previewMe, email: b.email.trim().toLowerCase() };
      // Preview of a trusted computer: the remembered person only needs the password.
      { const r = await getRemembered(); if (r && r.email === previewMe.email) return { next: "session", staff: { ...previewMe, name: r.name }, idle_minutes: 15 }; }
      return { next: "code", challenge: "preview-login" };
    case "POST /auth/verify":
      if (!/^\d{6}$/.test(b.code ?? "")) fail(401, M("Esse código não está certo. Use o código mais recente.", "That code isn’t right. Try the newest code."));
      try { localStorage.setItem("physiopulse-preview-account", JSON.stringify({ email: previewMe.email, name: previewMe.name })); } catch { /* ignore */ }
      // The owner's first two-step setup comes with ten recovery codes (server: routes/auth.rs verify).
      if (b.challenge === "preview-enrol" && previewMe.role === "owner") {
        const A = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        const c = () => { let x = ""; for (let i = 0; i < 12; i++) x += A[Math.floor(Math.random() * A.length)]; return `${x.slice(0, 4)}-${x.slice(4, 8)}-${x.slice(8)}`; };
        return { staff: previewMe, idle_minutes: 15, recovery_codes: Array.from({ length: 10 }, c) };
      }
      // Phase 3: someone who just joined signs in on a device the owner hasn't approved yet.
      if (b.challenge === "preview-enrol") return previewPendingStart();
      return { staff: previewMe, idle_minutes: 15 };
    case "POST /auth/recover":
      if (!/^[A-Z0-9]{4}-?[A-Z0-9]{4}-?[A-Z0-9]{4}$/i.test((b.recovery_code ?? "").trim()) || /^AAAA/i.test(b.recovery_code ?? "")) fail(401, M("Esse código de recuperação não está certo, ou já foi usado.", "That recovery code isn’t right, or it was already used."));
      return { next: "enrol", enrol: { ...enrol(previewMe.email), challenge: "preview-reenrol" }, codes_left: previewUseRecoveryCode() };
    case "PATCH /me": {
      const name = String(b.name ?? "").trim().replace(/\s+/g, " ");
      if (!name) fail(400, M("Escreva o seu nome.", "Write your name."));
      previewMe = { ...previewMe, name };
      const st = previewStaff.find((x) => x.id === previewMe.id); if (st) st.name = name;
      return previewMe;
    }
    case "POST /auth/logout": return { ok: true };
    case "GET /me": return previewMe;
    case "GET /staff": return previewStaff;
    case "PATCH /staff/ar": case "PATCH /staff/mc": {
      const st = previewStaff.find((x) => path.endsWith("/" + x.id));
      if (st && b.role) st.role = b.role as Role;
      if (st && "active" in (body as object)) st.active = !!(body as { active: boolean }).active;
      if (st && "ai_enabled" in (body as object)) {
        if (st.role !== "therapist") fail(400, M("O interruptor da IA é para fisioterapeutas. O proprietário tem-no sempre; a receção nunca.", "The AI switch is for therapists. The owner always has it; reception never does."));
        st.ai_enabled = !!(body as { ai_enabled: boolean }).ai_enabled;
      }
      return { ok: true };
    }
    case "DELETE /staff/ar/sessions": case "DELETE /staff/mc/sessions": return { ended: 1 };
  }
  return fail(501, M("Ainda não construído.", "Not built yet."));
}
