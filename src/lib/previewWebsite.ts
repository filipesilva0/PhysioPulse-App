// Browser preview only: stand-ins for Settings › Website (PhysioPulse plan, phase 4a; server: routes/website.rs).
// Same rules as the server where the screens depend on them: owner only, plain text with length limits,
// publishing needs the clinic's name, NIF and a contact; reception never appears on the team page.
import type { WebsiteInfo, WebsitePerson, WebsiteSite } from "./data";
import { products } from "./preview3";

type Role = "owner" | "therapist" | "reception";
type M = (pt: string, en: string) => string;
const fail = (status: number, message: string): never => { throw { status, message }; };

const site: WebsiteSite = {
  published: false, requests_open: true, home_visits: true, opening_soon: true, show_prices: true, accounts_open: false,
  tagline_pt: "Fisioterapia com tempo para si", tagline_en: "Physiotherapy with time for you",
  intro_pt: "Avaliamos com calma, explicamos o que se passa e combinamos consigo um plano de tratamento. Cada sessão é individual e acompanhada pelo mesmo fisioterapeuta.",
  intro_en: "We take the time to assess you, explain what is going on and agree a treatment plan with you. Every session is one-to-one, with the same physiotherapist.",
  home_text_pt: "Se não puder vir à clínica, vamos a sua casa, com o mesmo cuidado e o material necessário.",
  home_text_en: "If you can't come to the clinic, we come to your home, with the same care and the equipment we need.",
  updated_at: new Date().toISOString(),
};
const team: WebsitePerson[] = [
  { id: "fs", name: "Filipe Silva", role: "owner", web_visible: true, web_title_pt: "Fisioterapeuta e diretor clínico", web_title_en: "Physiotherapist and clinical director",
    web_bio_pt: "Fisioterapeuta há dez anos, com especial interesse no ombro e na coluna.", web_bio_en: "A physiotherapist for ten years, with a special interest in shoulders and the spine." },
  { id: "ar", name: "Ana Rocha", role: "therapist", web_visible: false, web_title_pt: "", web_title_en: "", web_bio_pt: "", web_bio_en: "" },
];
const hidden = new Set<string>();
const LIMIT: Record<string, number> = { tagline_pt: 90, tagline_en: 90, web_title_pt: 60, web_title_en: 60, web_bio_pt: 500, web_bio_en: 500 };

function checkTexts(b: Record<string, unknown>, m: M) {
  for (const [k, v] of Object.entries(b)) {
    if (typeof v !== "string") continue;
    if (v.trim().length > (LIMIT[k] ?? 700)) fail(400, m("Alguns textos são demasiado longos. Encurte-os e tente de novo.", "Some texts are too long. Shorten them and try again."));
    if (/[<>]/.test(v)) fail(400, m("Escreva só texto, sem < nem >.", "Write plain text, without < or >."));
  }
}

function info(clinic: () => WebsiteInfo["clinic"]): WebsiteInfo {
  return {
    site: { ...site }, team: team.map((t) => ({ ...t })),
    products: products.filter((p) => p.active).sort((a, b) => a.sort - b.sort).map((p) => ({ id: p.id, name_pt: p.name_pt, name_en: p.name_en, kind: p.kind, price_cents: p.price_cents, web_visible: !hidden.has(p.id) })),
    clinic: clinic(), requests_30d: 2, site_url: "https://physiopulse.duckdns.org",
  };
}

export function previewWebsite(method: string, path: string, body: unknown, m: M, role: Role, clinic: () => WebsiteInfo["clinic"]): unknown {
  const seg = path.split("?")[0].split("/").filter(Boolean);
  if (seg[0] !== "website") return undefined;
  if (role !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role can’t do this."));
  const b = (body ?? {}) as Record<string, unknown>;
  if (method === "GET" && seg.length === 1) return info(clinic);
  if (method === "PUT" && seg.length === 1) {
    checkTexts(b, m);
    if (b.published === true) {
      const c = clinic();
      if (!c.name.trim() || !c.nif.trim() || (!c.phone.trim() && !c.email.trim()))
        fail(400, m("Antes de publicar, preencha o nome da clínica, o NIF e um telefone ou email em Definições › Clínica.", "Before publishing, fill in the clinic’s name, NIF and a phone or email in Settings › Clinic."));
    }
    for (const k of Object.keys(site) as (keyof WebsiteSite)[]) if (k in b && k !== "updated_at") (site as unknown as Record<string, unknown>)[k] = typeof b[k] === "string" ? (b[k] as string).trim() : b[k];
    site.updated_at = new Date().toISOString();
    return info(clinic);
  }
  if (method === "PUT" && seg[1] === "team") {
    const t = team.find((x) => x.id === seg[2]) ?? fail(404, m("Não encontrado.", "Not found."));
    checkTexts(b, m);
    Object.assign(t, b);
    return { ok: true };
  }
  if (method === "PUT" && seg[1] === "products") {
    if (!products.some((p) => p.id === seg[2])) fail(404, m("Não encontrado.", "Not found."));
    if (b.web_visible === false) hidden.add(seg[2]); else hidden.delete(seg[2]);
    return { ok: true };
  }
  return undefined;
}

// ───────── Phase 4b: the patient area, from the patient's file (server: routes/portal.rs) ─────────
type Acc = { id: string; email: string; relation: "self" | "guardian"; two_step: boolean; last_login_at: string | null; created_at: string };
type Inv = { id: string; relation: "self" | "guardian"; created_at: string; expires_at: string; created_by_name: string | null };
const portalAcc: Record<string, Acc[]> = {};
const portalInv: Record<string, Inv[]> = {};
const CODE_ABC = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const newCode = () => { let s = ""; for (let i = 0; i < 12; i++) s += CODE_ABC[Math.floor(Math.random() * CODE_ABC.length)]; return `${s.slice(0, 4)}-${s.slice(4, 8)}-${s.slice(8)}`; };

export function previewPortal(method: string, path: string, body: unknown, m: M, role: Role, me: { id: string; name: string },
  patient: (id: string) => { name: string; phone: string | null; email: string | null; lang: "pt" | "en"; therapist_id: string | null; minor: boolean } | null): unknown {
  const seg = path.split("?")[0].split("/").filter(Boolean);
  if (seg[0] !== "patients" || seg[2] !== "portal") return undefined;
  const pid = seg[1];
  const p = patient(pid);
  if (!p) fail(404, m("Não encontrado.", "Not found."));
  const canManage = role === "owner" || role === "reception" || (role === "therapist" && p!.therapist_id === me.id);
  const b = (body ?? {}) as Record<string, string>;
  portalAcc[pid] ??= pid === "p1" ? [{ id: "acc1", email: "joana.martins@example.com", relation: "self", two_step: false, last_login_at: new Date(Date.now() - 2 * 864e5).toISOString(), created_at: "2026-11-20T10:00:00Z" }] : [];
  portalInv[pid] ??= [];
  const info = () => ({ accounts: portalAcc[pid], invites: portalInv[pid], open: site.published && !!site.accounts_open, minor: p!.minor, can_manage: canManage,
    site_url: "https://physiopulse.duckdns.org", clinic: "PhysioPulse", patient: { name: p!.name, phone: p!.phone, email: p!.email, lang: p!.lang } });
  if (method === "GET" && seg.length === 3) return info();
  if (method === "POST" && seg[3] === "invite") {
    const relation = b.relation === "guardian" ? "guardian" : "self";
    if (relation === "self" && p!.minor) fail(400, m("Os menores de 16 anos veem as sessões pela conta de um pai, mãe ou encarregado de educação. Crie um convite para o encarregado.", "Under 16s see their sessions through a parent’s or guardian’s account. Make a guardian invite."));
    const expires = new Date(Date.now() + 14 * 864e5).toISOString();
    const id = `inv${Date.now()}`;
    portalInv[pid].unshift({ id, relation, created_at: new Date().toISOString(), expires_at: expires, created_by_name: me.name });
    return { id, code: newCode(), relation, expires_at: expires };
  }
  if (method === "DELETE" && seg[3] === "invites") { portalInv[pid] = portalInv[pid].filter((x) => x.id !== seg[4]); return { ok: true }; }
  if (method === "POST" && seg[3] === "reset") {
    if (!canManage) fail(400, m("Só a receção, o fisioterapeuta do utente ou o proprietário podem fazer isto.", "Only reception, the patient’s therapist or the owner can do this."));
    return { code: newCode(), expires_at: new Date(Date.now() + 864e5).toISOString() };
  }
  if (method === "DELETE" && seg[3] === "accounts") {
    if (!canManage) fail(400, m("Só a receção, o fisioterapeuta do utente ou o proprietário podem fazer isto.", "Only reception, the patient’s therapist or the owner can do this."));
    portalAcc[pid] = portalAcc[pid].filter((x) => x.id !== seg[4]);
    return { ok: true };
  }
  return undefined;
}
