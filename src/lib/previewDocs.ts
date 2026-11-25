// Browser preview only: patient documents and signed-note PDFs, kept in memory.
// Follows the server's rules (src: server/src/routes/documents.rs): reception handles scanned
// prescriptions and consent PDFs only, signed-note and consent PDFs can't be removed, the uploader may remove
// their own file the same day.
import type { DocKind, DocumentRow } from "./data";

type M = (pt: string, en: string) => string;
export interface PDoc extends DocumentRow { patient_id: string; data: string; consent_event?: "signed" | "revoked" }
export const docs: PDoc[] = [];

const fail = (status: number, message: string): never => { throw { status, message }; };
const uid = () => (crypto.randomUUID ? crypto.randomUUID() : Math.random().toString(36).slice(2));
const KINDS: DocKind[] = ["photo", "report", "imaging", "prescription", "letter", "other"];

/** A small one-page A4 PDF (Helvetica, Latin-1) as base64. */
export function miniPdf(lines: { t: string; size?: number; bold?: boolean; gap?: number }[]): string {
  const esc = (s: string) => [...s].map((c) => {
    const n = c === "€" ? 0x80 : c === "’" ? 0x92 : c === "–" ? 0x96 : c.charCodeAt(0);
    if (n > 255) return "?";
    if (c === "(" || c === ")" || c === "\\") return "\\" + c;
    return n < 128 ? c : "\\" + n.toString(8).padStart(3, "0");
  }).join("");
  let y = 790, content = "";
  for (const l of lines) {
    const size = l.size ?? 10;
    y -= (l.gap ?? 0) + size * 1.5;
    content += `BT /${l.bold ? "F2" : "F1"} ${size} Tf 56 ${y} Td (${esc(l.t)}) Tj ET\n`;
  }
  const objs = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Count 1 /Kids [5 0 R] >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents 6 0 R >>",
    `<< /Length ${content.length} >>\nstream\n${content}\nendstream`,
  ];
  let out = "%PDF-1.4\n";
  const offs: number[] = [];
  objs.forEach((o, i) => { offs.push(out.length); out += `${i + 1} 0 obj\n${o}\nendobj\n`; });
  const x = out.length;
  out += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n${offs.map((o) => `${String(o).padStart(10, "0")} 00000 n \n`).join("")}`;
  out += `trailer\n<< /Size ${objs.length + 1} /Root 1 0 R >>\nstartxref\n${x}\n%%EOF\n`;
  return btoa(out);
}

/** A placeholder picture drawn on a canvas (preview only). */
function picture(kind: "xray" | "scan", w: number, h: number): string {
  if (typeof document === "undefined") return "";
  const c = document.createElement("canvas"); c.width = w; c.height = h;
  const g = c.getContext("2d"); if (!g) return "";
  if (kind === "xray") {
    const bg = g.createRadialGradient(w / 2, h / 2, 10, w / 2, h / 2, w * .7); bg.addColorStop(0, "#3a4048"); bg.addColorStop(1, "#0b0d10");
    g.fillStyle = bg; g.fillRect(0, 0, w, h);
    g.fillStyle = "rgba(235,240,245,.85)";
    g.beginPath(); g.ellipse(w * .44, h * .28, w * .09, h * .26, -.05, 0, Math.PI * 2); g.fill();
    g.beginPath(); g.ellipse(w * .47, h * .74, w * .085, h * .25, .04, 0, Math.PI * 2); g.fill();
    g.fillStyle = "rgba(200,208,215,.7)"; g.beginPath(); g.ellipse(w * .6, h * .52, w * .05, h * .06, 0, 0, Math.PI * 2); g.fill();
    g.fillStyle = "#fff"; g.font = "600 13px system-ui"; g.fillText("R", 14, 24);
  } else {
    g.fillStyle = "#fbfaf6"; g.fillRect(0, 0, w, h);
    g.fillStyle = "#1f3b70"; g.font = "700 20px system-ui"; g.fillText("ADSE", 28, 44);
    g.fillStyle = "#333"; g.font = "14px system-ui"; g.fillText("Credencial de MFR — 10 sessões", 28, 76);
    g.fillStyle = "#bbb"; for (let i = 0; i < 9; i++) g.fillRect(28, 104 + i * 22, w - 56 - (i % 3) * 60, 7);
    g.strokeStyle = "#1f3b70"; g.lineWidth = 2; g.beginPath(); g.moveTo(w - 190, h - 50); g.bezierCurveTo(w - 160, h - 90, w - 120, h - 20, w - 70, h - 60); g.stroke();
  }
  return c.toDataURL(kind === "scan" ? "image/jpeg" : "image/png", .85).split("base64,")[1] ?? "";
}

export const add = (d: Omit<PDoc, "id" | "bytes" | "note_id"> & { note_id?: string | null }) => {
  const row: PDoc = { id: uid(), note_id: null, consent_id: null, ...d, bytes: Math.round((d.data.length * 3) / 4) };
  docs.push(row); return row;
};

/** Seeds sample files; returns the id of the scanned prescription (linked to the first prescription). */
export function seedDocs(day: (n: number) => string): string {
  add({ patient_id: "p1", kind: "report", filename: "Relatório RM cervical.pdf", mime: "application/pdf", uploaded_at: day(-30), uploaded_by: "fs", uploaded_by_name: "Filipe Silva",
    data: miniPdf([{ t: "Relatório de ressonância magnética — coluna cervical", size: 14, bold: true }, { t: "Exemplo para a pré-visualização.", gap: 10 },
      { t: "Protrusão discal C5–C6 sem compromisso radicular.", gap: 6 }, { t: "Sem outras alterações relevantes." }]) });
  add({ patient_id: "p1", kind: "imaging", filename: "Raio-X joelho.png", mime: "image/png", uploaded_at: day(-12), uploaded_by: "ar", uploaded_by_name: "Ana Rocha", data: picture("xray", 520, 640) });
  return add({ patient_id: "p1", kind: "prescription", filename: "Credencial ADSE.jpg", mime: "image/jpeg", uploaded_at: day(-20), uploaded_by: "mc", uploaded_by_name: "Marta Costa", data: picture("scan", 620, 420) }).id;
}

/** The PDF of a signed note (same layout idea as the server's). */
export function notePdf(note: { id: string; patient_id: string; template: string; text: string; signed_at: string | null; signed_by_name: string | null; blocks: unknown[] },
  patient: { name: string; number: number }, by: { id: string; name: string }): PDoc {
  const have = docs.find((d) => d.note_id === note.id); if (have) return have;
  const n = { ...note, signed_at: note.signed_at ?? new Date().toISOString() };
  const day = n.signed_at.slice(0, 10);
  const blocks = (n.blocks as { label: string; value: unknown; unit?: string }[])
    .map((b) => [b.label, Array.isArray(b.value) ? b.value.join(", ") : `${b.value ?? ""}${b.unit ?? ""}`]).filter(([, v]) => v.trim());
  const lines = [{ t: "Nota de evolução", size: 18, bold: true }, { t: `Utente: ${patient.name} (n.º ${patient.number})`, gap: 8 },
    ...blocks.map(([l, v]) => ({ t: `${l}: ${v}` })), ...n.text.split("\n").filter(Boolean).map((t) => ({ t: t.slice(0, 95) })),
    { t: `Assinada por ${n.signed_by_name} a ${new Date(n.signed_at).toLocaleString("pt-PT")}`, bold: true, gap: 16 }];
  const t = new Date(n.signed_at);
  return add({ patient_id: n.patient_id, kind: "note", filename: `${day} Nota de evolução ${String(t.getHours()).padStart(2, "0")}h${String(t.getMinutes()).padStart(2, "0")}.pdf`, mime: "application/pdf", uploaded_at: n.signed_at,
    uploaded_by: by.id, uploaded_by_name: by.name, note_id: n.id, data: miniPdf(lines) });
}

/** The PDF of a signed consent or of its withdrawal (same idea as the server's, without the signature picture). */
export function consentPdf(c: { id: string; patient_id: string; granted: boolean; signed_by_name: string; signed_at: string; revoked_at: string | null; kind: string },
  t: { title: string; body: string; version: number }, patient: { name: string; number: number }, by: { id: string; name: string }, event: "signed" | "revoked"): PDoc {
  const have = docs.find((d) => d.consent_id === c.id && d.consent_event === event); if (have) return have;
  const at = new Date(event === "revoked" ? c.revoked_at ?? new Date().toISOString() : c.signed_at);
  const short: Record<string, string> = { treatment: "tratamento", data: "dados pessoais", media: "fotografias e vídeos", marketing: "comunicações", guardian: "representante legal", invasive: "técnicas invasivas" };
  const hhmm = `${String(at.getHours()).padStart(2, "0")}h${String(at.getMinutes()).padStart(2, "0")}`;
  const lines = event === "revoked"
    ? [{ t: "Retirada de consentimento", size: 18, bold: true }, { t: `Utente: ${patient.name} (n.º ${patient.number})`, gap: 8 }, { t: `Consentimento: ${t.title} (v${t.version})` },
      { t: `Retirado a ${at.toLocaleString("pt-PT")}`, gap: 6 }, { t: `Registado por ${by.name}`, gap: 6 }]
    : [{ t: t.title.slice(0, 60), size: 16, bold: true }, { t: `Utente: ${patient.name} (n.º ${patient.number})`, gap: 8 },
      ...t.body.split("\n").filter(Boolean).slice(0, 14).map((x) => ({ t: x.slice(0, 95) })),
      ...(["data", "media", "marketing"].includes(c.kind) ? [{ t: c.granted ? "Decisão: autorizo" : "Decisão: não autorizo", bold: true, gap: 8 }] : []),
      { t: `Assinado por ${c.signed_by_name} a ${at.toLocaleString("pt-PT")}`, bold: true, gap: 12 }];
  return add({ patient_id: c.patient_id, kind: "consent", filename: `${at.toISOString().slice(0, 10)} ${event === "revoked" ? "Retirada consentimento" : "Consentimento"} ${short[c.kind] ?? c.kind} ${hhmm}.pdf`,
    mime: "application/pdf", uploaded_at: at.toISOString(), uploaded_by: by.id, uploaded_by_name: by.name, consent_id: c.id, consent_event: event, data: miniPdf(lines) });
}

const mayHandle = (clinical: boolean, kind: string) => clinical || kind === "prescription" || kind === "consent";
const serverMade = (d: { kind: string; letter_id?: string | null }) => d.kind === "note" || d.kind === "consent" || !!d.letter_id;
const isToday = (iso: string) => new Date(iso).toDateString() === new Date().toDateString();
const row = ({ data: _d, patient_id: _p, consent_event: _e, ...r }: PDoc): DocumentRow => r;

/** Legal tools: remove every document of a patient (the folder goes with them). */
export function dropDocs(pid: string) { for (let i = docs.length - 1; i >= 0; i--) if (docs[i].patient_id === pid) docs.splice(i, 1); }

export function docsFor(pid: string, clinical: boolean): DocumentRow[] {
  return docs.filter((d) => d.patient_id === pid && mayHandle(clinical, d.kind)).sort((a, c) => c.uploaded_at.localeCompare(a.uploaded_at)).map(row);
}

/** Answers a documents request, or undefined. `audit` writes to the preview's access log. */
export function previewDocs(method: string, seg: string[], b: Record<string, unknown>, m: M, ctx: {
  clinical: boolean; role: string; me: { id: string; name: string }; audit: (a: string, pid: string, d?: string) => void; patientExists: (id: string) => boolean;
}): unknown {
  const { clinical, me, audit } = ctx;
  if (method === "POST" && seg[0] === "patients" && seg[2] === "documents") {
    const pid = seg[1];
    if (!ctx.patientExists(pid)) fail(404, m("Não encontrado.", "Not found."));
    const kind = String(b.kind ?? "");
    if (!KINDS.includes(kind as DocKind)) fail(400, m("Escolha o que é este ficheiro.", "Choose what this file is."));
    if (!mayHandle(clinical, kind)) fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
    const data = String(b.data ?? "").split("base64,").pop() ?? "";
    let head = "";
    try { head = atob(data.slice(0, 24)); } catch { fail(400, m("Não foi possível ler o ficheiro. Tente outra vez.", "That file couldn’t be read. Try again.")); }
    const bytes = Math.round((data.length * 3) / 4);
    if (!bytes || bytes > 10 * 1024 * 1024) fail(400, m("Os ficheiros têm de ter menos de 10 MB.", "Files must be under 10 MB."));
    const type: [string, string] | null = head.startsWith("%PDF") ? ["application/pdf", "pdf"] : head.startsWith("\xff\xd8\xff") ? ["image/jpeg", "jpg"]
      : head.startsWith("\x89PNG") ? ["image/png", "png"] : head.slice(4, 8) === "ftyp" ? ["image/heic", "heic"] : head.startsWith("RIFF") && head.slice(8, 12) === "WEBP" ? ["image/webp", "webp"] : null;
    if (!type) fail(400, m("Use um PDF ou uma fotografia (JPEG, PNG, HEIC).", "Use a PDF or a photo (JPEG, PNG, HEIC)."));
    const name = String(b.filename ?? "ficheiro").replace(/[\\/:*?"<>|]/g, "_");
    const stem = name.includes(".") ? name.slice(0, name.lastIndexOf(".")) : name;
    const d = add({ patient_id: pid, kind: kind as DocKind, filename: `${stem.slice(0, 70)}.${type![1]}`, mime: type![0], uploaded_at: new Date().toISOString(), uploaded_by: me.id, uploaded_by_name: me.name, data });
    audit("document.uploaded", pid, kind);
    return { id: d.id, kind, filename: d.filename, mime: d.mime, bytes: d.bytes };
  }
  if (seg[0] !== "documents") return undefined;
  const d = docs.find((x) => x.id === seg[1] && mayHandle(clinical, x.kind)) ?? fail(404, m("Não encontrado.", "Not found."));
  if (method === "GET" && seg.length === 2) { audit("document.opened", d.patient_id, d.kind); return { id: d.id, kind: d.kind, filename: d.filename, mime: d.mime, bytes: d.bytes, data: d.data }; }
  if (method === "PATCH") {
    if (serverMade(d)) fail(409, m("Este PDF faz parte do registo clínico e não pode ser alterado.", "This PDF is part of the clinical record and can’t be changed."));
    const kind = String(b.kind ?? d.kind);
    if (!KINDS.includes(kind as DocKind)) fail(400, m("Escolha o que é este ficheiro.", "Choose what this file is."));
    if (!mayHandle(clinical, kind)) fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
    d.kind = kind as DocKind; if (typeof b.filename === "string" && b.filename.trim()) d.filename = b.filename.trim().replace(/[\\/:*?"<>|]/g, "_");
    audit("document.updated", d.patient_id, kind); return { ok: true };
  }
  if (method === "DELETE") {
    if (serverMade(d)) fail(409, m("Este PDF faz parte do registo clínico e é mantido.", "This PDF is part of the clinical record and is kept."));
    if (ctx.role !== "owner" && !(d.uploaded_by === me.id && isToday(d.uploaded_at)))
      fail(409, m("Só o proprietário pode remover este documento. Pode remover documentos que juntou hoje.", "Only the owner can remove this document. You can remove documents you added today."));
    docs.splice(docs.indexOf(d), 1); audit("document.removed", d.patient_id, d.kind); return { ok: true };
  }
  return undefined;
}
