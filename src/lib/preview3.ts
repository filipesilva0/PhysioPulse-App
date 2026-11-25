// Browser preview only: an in-memory stand-in for the phase 3 (money) API, following the same rules as
// server/src/routes/money.rs where the screens depend on them: owner-only Finance, done appointments use a
// package credit or cost the visit price, payments are voided (never deleted), closed months are locked.
// Nothing here runs in the desktop app; nothing is saved.
import type { Appointment } from "./data";
import { previewAi } from "./previewAi";
import type { CashDay, Expense, Method, MonthClose, Owed, Payment, Plan, Product, Totals } from "./money";

type Role = "owner" | "therapist" | "reception";
type M = (pt: string, en: string) => string;
type Body = Record<string, unknown>;
type Appt = Omit<Appointment, "therapist_name" | "patient_name" | "patient_number" | "attendees"> & { attendees: { patient_id: string; attended: boolean | null; plan_id?: string | null; charge_cents?: number | null }[] };
export interface PreviewState {
  role: () => Role;
  me: () => { id: string; name: string };
  patients: { id: string; name: string; number: number; nif: string | null; active: boolean }[];
  appts: Appt[];
  audit: (action: string, pid: string | null, detail?: string) => void;
}

const fail = (status: number, message: string): never => { throw { status, message }; };
const uid = () => (crypto.randomUUID ? crypto.randomUUID() : Math.random().toString(36).slice(2));
const pad = (n: number) => String(n).padStart(2, "0");
const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const ym = (d: Date | string) => (typeof d === "string" ? d.slice(0, 7) : ymd(d).slice(0, 7));
const addDays = (d: Date, n: number) => { const x = new Date(d); x.setDate(x.getDate() + n); return x; };
const addMonths = (s: string, n: number) => { const [y, m] = s.split("-").map(Number); const d = new Date(y, m - 1 + n, 1); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}`; };
const today = () => ymd(new Date());
const str = (v: unknown) => (typeof v === "string" ? v.trim() : v == null ? "" : String(v));
const MONTHS_PT = ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];

const P0: [string, Product["kind"], Product["visit_kind"], Product["applies_to"], string, string, number, number | null, number | null, boolean, number, boolean][] = [
  ["assessment", "visit", "assessment", "session", "Avaliação inicial (60 min)", "First assessment (60 min)", 3000, null, null, false, 1, false],
  ["single", "visit", "session", "session", "Sessão (45 min)", "Single session (45 min)", 2000, null, null, false, 1, false],
  ["home", "visit", "home", "session", "Domicílio (60 min)", "Home visit (60 min)", 4000, null, null, false, 1, false],
  ["class1", "visit", "class", "class", "Aula de grupo", "Group class, drop-in", 1000, null, null, false, 1, false],
  ["starter", "pack", null, "session", "Início: avaliação + 4 sessões", "Starter: assessment + 4 sessions", 10000, 5, 2, false, 1, true],
  ["pack5", "pack", null, "session", "Pacote de 5", "Pack of 5", 8000, 5, 3, false, 1, false],
  ["pack10", "pack", null, "session", "Pacote de 10", "Pack of 10", 17000, 10, 6, true, 1, false],
  ["rehab20", "pack", null, "session", "Reabilitação, 20 sessões", "Rehab pack of 20", 30000, 20, 9, true, 2, false],
  ["cc4", "monthly", null, "session", "Acompanhamento, 4 por mês", "Continuous care, 4 a month", 7200, 4, null, false, 1, false],
  ["cc8", "monthly", null, "session", "Acompanhamento, 8 por mês", "Continuous care, 8 a month", 13600, 8, null, false, 1, false],
  ["class10", "pack", null, "class", "Cartão de 10 aulas", "Class card, 10 classes", 8000, 10, 3, false, 1, false],
  ["class2w", "monthly", null, "class", "Aulas, 2 por semana", "Group classes, 2 a week", 7500, null, null, false, 1, false],
];
export const products: Product[] = P0.map(([code, kind, visit_kind, applies_to, name_pt, name_en, price_cents, sessions, valid_months, shareable, instalments, includes_assessment], i) =>
  ({ id: "prod-" + code, code, kind, visit_kind, applies_to, name_pt, name_en, price_cents, sessions, valid_months, shareable, instalments, includes_assessment, active: true, sort: i * 10,
     vat_exempt: applies_to !== "class" }));

type PlanRow = Omit<Plan, "patient_name" | "shared_with_name" | "used" | "used_month" | "remaining" | "paid_cents"> & { skipped: string[]; includes_assessment: boolean };
const plans: PlanRow[] = [];
const payments: Omit<Payment, "patient_name" | "patient_number" | "created_by_name">[] = [];
/** A patient's valid payments (reimbursement pack stand-in). */
export const paymentsOf = (pid: string) => payments.filter((p) => p.patient_id === pid && !p.voided_at);
const expenses: Omit<Expense, "filename" | "closed">[] = [];
const files: { id: string; filename: string }[] = [];
const cash: Record<string, NonNullable<CashDay["close"]>> = {};
const pay: Record<string, number> = { fs: 0, ar: 145000, mc: 105000 };
const salaries: { id: string; staff_id: string; month: string; gross_cents: number; employer_ss_cents: number; paid_on: string; method: "cash" | "transfer"; note: string }[] = [];
/** Employer Social Security: the clinic's rate and anyone's own rate, in basis points (audit phase C). */
let ssBp = 2375;
const ssOwn: Record<string, number | null> = {};
const ssFor = (staffId: string) => ssOwn[staffId] ?? ssBp;
const ssOf = (gross: number, bp: number) => Math.floor((gross * bp + 5000) / 10000);
const closes: Record<string, { at: string; by: string; totals: Totals }> = {};
const STAFF = [{ id: "fs", name: "Filipe Silva", role: "owner" as const }, { id: "ar", name: "Ana Rocha", role: "therapist" as const }, { id: "mc", name: "Marta Costa", role: "reception" as const }];
let payNo = 1;

const closed = (day: string) => !!closes[ym(day)];
const usedOf = (S: PreviewState, plan: string, month?: string, exclude?: [string, string | null]) =>
  S.appts.filter((a) => a.plan_id === plan && a.status === "done" && !(exclude && a.id === exclude[0]) && (!month || ym(new Date(a.starts_at)) === month)).length
  + S.appts.flatMap((a) => a.attendees.filter((x) => x.plan_id === plan && x.attended && !(exclude && a.id === exclude[0] && x.patient_id === exclude[1]) && (!month || ym(new Date(a.starts_at)) === month))).length;

function pickPlan(S: PreviewState, patient: string, applies: "session" | "class", day: string, kind: string, apptId: string, only?: string) {
  if (kind === "home") return null;
  return plans.filter((p) => (p.patient_id === patient || p.shared_with === patient) && p.applies_to === applies && p.status === "active" && p.starts_on <= day
    && (!p.expires_on || p.expires_on >= day) && (!only || p.id === only)
    && (kind !== "assessment" || (p.includes_assessment && !S.appts.some((x) => x.plan_id === p.id && x.kind === "assessment" && x.status === "done" && x.id !== apptId)))
    && (p.sessions == null || p.sessions > usedOf(S, p.id, p.kind === "monthly" ? ym(day) : undefined, [apptId, applies === "class" ? patient : null])))
    .sort((a, b) => (a.kind === "monthly" ? 0 : 1) - (b.kind === "monthly" ? 0 : 1) || (a.expires_on ?? "9999").localeCompare(b.expires_on ?? "9999"))[0]?.id ?? null;
}
const visitPrice = (kind: string) => products.find((p) => p.kind === "visit" && p.active && p.visit_kind === kind)?.price_cents ?? null;

/** Called by the schedule stand-in when appointments change status (same rules as the server). */
export function moneyOnStatus(S: PreviewState, group: Appt[], status: string, auto = true) {
  for (const a of group) {
    if (status !== "done") { a.charge_cents = null; continue; }
    if (!a.patient_id || !["session", "assessment", "home"].includes(a.kind)) continue;
    const day = ymd(new Date(a.starts_at));
    let plan = a.plan_id ? pickPlan(S, a.patient_id, "session", day, a.kind, a.id, a.plan_id) : null;
    if (!plan && !auto && a.plan_id) fail(400, "That package can’t be used for this appointment (used up, expired, or another kind).");
    if (!plan && auto) plan = pickPlan(S, a.patient_id, "session", day, a.kind, a.id);
    a.plan_id = plan; a.charge_cents = plan ? null : visitPrice(a.kind);
  }
}
export function moneyOnAttended(S: PreviewState, a: Appt, pid: string, attended: boolean) {
  const x = a.attendees.find((y) => y.patient_id === pid); if (!x) return;
  if (!attended) { x.charge_cents = null; return; }
  const plan = pickPlan(S, pid, "class", ymd(new Date(a.starts_at)), "class", a.id);
  x.plan_id = plan; x.charge_cents = plan ? null : visitPrice("class");
}
const live = (p: (typeof payments)[number]) => !p.voided_at;
export const paidFor = (appt: string, patient: string | null) => payments.filter((p) => live(p) && p.appointment_id === appt && (!patient || p.patient_id === patient)).reduce((t, p) => t + p.amount_cents, 0);

/** For the legal tools' confirmation: payments recorded for a patient and what they owe. */
export const moneyFacts = (S: PreviewState, pid: string) => ({ payments: payments.filter((p) => p.patient_id === pid).length, owed_cents: owed(S, pid).reduce((t, x) => t + x.due_cents, 0) });

function owed(S: PreviewState, only?: string): Owed[] {
  const out: Omit<Owed, "patient_name" | "patient_number">[] = [];
  for (const a of S.appts) {
    const day = ymd(new Date(a.starts_at));
    if (a.status === "done" && a.patient_id && (a.charge_cents ?? 0) > 0 && !a.charge_waived)
      out.push({ type: "appointment", patient_id: a.patient_id, appointment_id: a.id, plan_id: null, month: null, day, what: a.kind, name_pt: null, name_en: null, price_cents: a.charge_cents!, due_cents: a.charge_cents! - paidFor(a.id, a.patient_id) });
    if (a.kind === "class" && a.status !== "cancelled") for (const x of a.attendees) if (x.attended && (x.charge_cents ?? 0) > 0)
      out.push({ type: "appointment", patient_id: x.patient_id, appointment_id: a.id, plan_id: null, month: null, day, what: "class", name_pt: a.title, name_en: a.title, price_cents: x.charge_cents!, due_cents: x.charge_cents! - paidFor(a.id, x.patient_id) });
  }
  const cur = ym(new Date());
  for (const p of plans) {
    if (p.status === "cancelled") continue;
    if (p.kind === "pack") out.push({ type: "plan", patient_id: p.patient_id, appointment_id: null, plan_id: p.id, month: null, day: p.starts_on, what: "pack", name_pt: p.name_pt, name_en: p.name_en, price_cents: p.price_cents, due_cents: p.price_cents - payments.filter((x) => live(x) && x.plan_id === p.id).reduce((t, x) => t + x.amount_cents, 0) });
    else for (let m = ym(p.starts_on); m <= (p.ended_on ? ym(p.ended_on) : cur) && m <= cur; m = addMonths(m, 1)) {
      if (p.skipped.includes(m) || (p.status === "paused" && m === cur)) continue;
      out.push({ type: "month", patient_id: p.patient_id, appointment_id: null, plan_id: p.id, month: m + "-01", day: m + "-01", what: "monthly", name_pt: p.name_pt, name_en: p.name_en, price_cents: p.price_cents, due_cents: p.price_cents - payments.filter((x) => live(x) && x.plan_id === p.id && x.covers_month === m + "-01").reduce((t, x) => t + x.amount_cents, 0) });
    }
  }
  return out.filter((o) => o.due_cents > 0 && (!only || o.patient_id === only)).map((o) => { const pt = S.patients.find((x) => x.id === o.patient_id)!; return { ...o, patient_name: pt.name, patient_number: pt.number }; })
    .sort((a, b) => a.day.localeCompare(b.day) || a.patient_name.localeCompare(b.patient_name));
}

const planOut = (S: PreviewState, p: PlanRow): Plan => {
  const used = usedOf(S, p.id);
  return { ...p, patient_name: S.patients.find((x) => x.id === p.patient_id)?.name ?? "", shared_with_name: p.shared_with ? S.patients.find((x) => x.id === p.shared_with)?.name ?? null : null,
    used, used_month: usedOf(S, p.id, ym(new Date())), remaining: p.kind === "pack" ? (p.sessions ?? 0) - used : null,
    paid_cents: payments.filter((x) => live(x) && x.plan_id === p.id).reduce((t, x) => t + x.amount_cents, 0) };
};
const payOut = (S: PreviewState, p: (typeof payments)[number]): Payment => {
  const pt = p.patient_id ? S.patients.find((x) => x.id === p.patient_id) : undefined;
  return { ...p, patient_name: pt?.name ?? null, patient_number: pt?.number ?? null, created_by_name: STAFF.find((x) => x.id === p.created_by)?.name ?? null };
};
function totals(month: string): Totals {
  const ps = payments.filter((p) => live(p) && ym(p.received_on) === month);
  const es = expenses.filter((e) => ym(e.spent_on) === month);
  const by: Totals["by_method"] = {}; ps.forEach((p) => { by[p.method] = (by[p.method] ?? 0) + p.amount_cents; });
  const cats: Record<string, number> = {}; es.forEach((e) => { cats[e.category] = (cats[e.category] ?? 0) + e.amount_cents; });
  const revenue = ps.reduce((t, p) => t + p.amount_cents, 0), exp = es.reduce((t, e) => t + e.amount_cents, 0);
  const sal = salaries.filter((s) => s.month === month).reduce((t, s) => t + s.gross_cents, 0);
  const ss = salaries.filter((s) => s.month === month).reduce((t, s) => t + s.employer_ss_cents, 0);
  const un = ps.filter((p) => !p.invoice_number);
  return { revenue_cents: revenue, expenses_cents: exp, salaries_cents: sal, employer_ss_cents: ss, result_cents: revenue - exp - sal - ss, payments: ps.length, by_method: by, expenses_by_category: cats, uninvoiced: un.length, uninvoiced_cents: un.reduce((t, p) => t + p.amount_cents, 0) };
}

function addPayment(S: PreviewState, b: { patient_id: string | null; method: Method; received_on: string; amount_cents: number; description: string; appointment_id?: string | null; plan_id?: string | null; covers_month?: string | null; invoice_number?: string; by?: string }) {
  const p = { id: uid(), number: payNo++, received_on: b.received_on, patient_id: b.patient_id, amount_cents: b.amount_cents, method: b.method, description: b.description,
    appointment_id: b.appointment_id ?? null, plan_id: b.plan_id ?? null, covers_month: b.covers_month ?? null, invoice_number: b.invoice_number ?? "", invoice_file_id: null,
    invoiced_at: b.invoice_number ? new Date().toISOString() : null, note: "", created_by: b.by ?? S.me().id, created_at: new Date(b.received_on + "T12:00:00").toISOString(), voided_at: null, void_reason: "" };
  payments.push(p);
  return p;
}

/** Seed money data relative to today (after the schedule seed). */
export function moneySeed(S: PreviewState) {
  const t = new Date(), m0 = ym(t);
  const sell = (pid: string, code: string, starts: Date, share: string | null = null) => {
    const pr = products.find((x) => x.code === code)!;
    const p: PlanRow = { id: uid(), patient_id: pid, product_id: pr.id, kind: pr.kind as "pack", applies_to: pr.applies_to, name_pt: pr.name_pt, name_en: pr.name_en, price_cents: pr.price_cents,
      sessions: pr.sessions, instalments: pr.instalments, shareable: pr.shareable, shared_with: share, starts_on: ymd(starts),
      expires_on: pr.valid_months ? ymd(addDays(new Date(starts.getFullYear(), starts.getMonth() + pr.valid_months, starts.getDate()), -1)) : null,
      status: "active", ended_on: null, note: "", skipped: [], includes_assessment: pr.includes_assessment };
    plans.push(p); return p;
  };
  const p1 = sell("p1", "pack10", addDays(t, -21));
  const p2 = sell("p2", "cc4", addDays(t, -70));
  const p3 = sell("p3", "rehab20", addDays(t, -40), "p7");
  const p7 = sell("p7", "class10", addDays(t, -30));
  // Done appointments in the seed use credits or cost the visit price.
  S.appts.filter((a) => a.status === "done").sort((a, b) => a.starts_at.localeCompare(b.starts_at)).forEach((a) => moneyOnStatus(S, [a], "done"));
  S.appts.filter((a) => a.kind === "class" && a.status === "done").forEach((a) => a.attendees.forEach((x) => { x.attended = x.patient_id !== "p6"; moneyOnAttended(S, a, x.patient_id, !!x.attended); }));

  // Six months of history so the overview has something to show.
  const walkins = ["Sessão", "Avaliação inicial", "Aula de grupo", "Sessão", "Domicílio"];
  for (let k = 5; k >= 1; k--) {
    const m = addMonths(m0, -k), [y, mo] = m.split("-").map(Number), days = new Date(y, mo, 0).getDate();
    const n = 300 + ((k * 37) % 60) - (mo === 8 ? 90 : 0);
    for (let i = 0; i < n; i++) {
      const d = `${m}-${pad(1 + ((i * 7 + k) % days))}`;
      const w = walkins[i % walkins.length];
      const amount = w === "Avaliação inicial" ? 3000 : w === "Aula de grupo" ? 1000 : w === "Domicílio" ? 4000 : 2000;
      // Most history belongs to patients not in this small sample list (walk-ins in the preview).
      const pt = i % 25 === 0 ? S.patients[(i + k) % S.patients.length] : null;
      addPayment(S, { patient_id: pt?.id ?? null, method: (["mbway", "cash", "transfer", "mbway", "card"] as Method[])[i % 5], received_on: d, amount_cents: amount * (i % 9 === 0 ? 4 : 1), description: `${w} ${d.slice(8, 10)}/${d.slice(5, 7)}/${d.slice(0, 4)}`, invoice_number: `FR ${y}/${1000 + i + (5 - k) * 400}`, by: i % 2 ? "mc" : "fs" });
    }
    for (const [cat, sup, v, day] of [["rent", "Senhorio — Imóveis Sado", 85000, 1], ["utilities", "EDP Comercial", 6000 + k * 410, 8], ["telecom", "MEO", 3999, 12], ["supplies", "Fisiomed Lda.", 8000 + k * 900, 15], ["accounting", "Contas Certas, Lda.", 12300, 28]] as const)
      expenses.push({ id: uid(), spent_on: `${m}-${pad(day)}`, supplier: sup, supplier_nif: "", category: cat, description: "", amount_cents: v, vat_cents: null, method: cat === "rent" ? "transfer" : "debit", document_ref: "", file_id: null });
    for (const s of STAFF) if (pay[s.id]) salaries.push({ id: uid(), staff_id: s.id, month: m, gross_cents: pay[s.id], employer_ss_cents: ssOf(pay[s.id], ssBp), paid_on: `${m}-${pad(Math.min(28, days))}`, method: "transfer", note: "" });
    if (k >= 2) closes[m] = { at: new Date(y, mo, 3, 10).toISOString(), by: "Filipe Silva", totals: totals(m) };
  }
  // This month: package payments and a few sessions.
  addPayment(S, { patient_id: "p1", method: "mbway", received_on: p1.starts_on.slice(0, 7) === m0 ? p1.starts_on : `${m0}-01`, amount_cents: 17000, description: p1.name_pt, plan_id: p1.id, invoice_number: `FR ${t.getFullYear()}/412` });
  addPayment(S, { patient_id: "p2", method: "mbway", received_on: `${m0}-01`, amount_cents: 7200, description: `${p2.name_pt} — ${MONTHS_PT[t.getMonth()]} de ${t.getFullYear()}`, plan_id: p2.id, covers_month: `${m0}-01` });
  for (const m of [addMonths(m0, -2), addMonths(m0, -1)]) if (m >= ym(p2.starts_on)) addPayment(S, { patient_id: "p2", method: "mbway", received_on: `${m}-02`, amount_cents: 7200, description: `${p2.name_pt} — ${MONTHS_PT[+m.slice(5) - 1]}`, plan_id: p2.id, covers_month: `${m}-01`, invoice_number: `FR ${m.slice(0, 4)}/3${m.slice(5)}` });
  addPayment(S, { patient_id: "p3", method: "transfer", received_on: ym(p3.starts_on) === m0 ? p3.starts_on : `${m0}-01`, amount_cents: 15000, description: `${p3.name_pt} (1.ª prestação)`, plan_id: p3.id });
  addPayment(S, { patient_id: "p7", method: "cash", received_on: ym(p7.starts_on) === m0 ? p7.starts_on : `${m0}-01`, amount_cents: 8000, description: p7.name_pt, plan_id: p7.id, invoice_number: `FR ${t.getFullYear()}/415` });
  const someone = owed(S).find((o) => o.type === "appointment");
  if (someone) addPayment(S, { patient_id: someone.patient_id, method: "cash", received_on: someone.day, amount_cents: someone.due_cents, description: `Sessão ${someone.day.slice(8, 10)}/${someone.day.slice(5, 7)}`, appointment_id: someone.appointment_id });
  for (const [cat, sup, v, day] of [["rent", "Senhorio — Imóveis Sado", 85000, 1], ["supplies", "Fisiomed Lda.", 8640, 2]] as const)
    if (day <= t.getDate()) expenses.push({ id: uid(), spent_on: `${m0}-${pad(day)}`, supplier: sup, supplier_nif: "", category: cat, description: "", amount_cents: v, vat_cents: cat === "supplies" ? 1615 : null, method: "transfer", document_ref: "", file_id: null });
}

export function moneyAlerts(S: PreviewState) {
  const t = today(), in14 = ymd(addDays(new Date(), 14));
  const low = plans.filter((p) => p.kind === "pack" && p.status === "active").map((p) => planOut(S, p))
    .filter((p) => (p.remaining ?? 0) > 0 && ((p.remaining ?? 0) <= 1 || (p.expires_on ?? "9999") <= in14))
    .map((p) => ({ patient_id: p.patient_id, name: p.patient_name, plan_id: p.id, name_pt: p.name_pt, name_en: p.name_en, remaining: p.remaining!, expires_on: p.expires_on }));
  if (S.role() !== "owner") return { plans_low: low, money: null };
  const by = new Map<string, { patient_id: string; name: string; due_cents: number }>();
  owed(S).forEach((o) => { const x = by.get(o.patient_id) ?? { patient_id: o.patient_id, name: o.patient_name, due_cents: 0 }; x.due_cents += o.due_cents; by.set(o.patient_id, x); });
  const list = [...by.values()].sort((a, b) => b.due_cents - a.due_cents);
  return { plans_low: low, money: { owed_cents: list.reduce((t2, x) => t2 + x.due_cents, 0), owed_patients: list.length, owed: list.slice(0, 5), today_cents: payments.filter((p) => live(p) && p.received_on === t).reduce((t2, p) => t2 + p.amount_cents, 0) } };
}

/** Answers a phase 3 request, or undefined if it isn't one. */
export function preview3(method: string, path: string, body: unknown, m: M, S: PreviewState): unknown {
  const b = (body ?? {}) as Body;
  const [p, qs] = path.split("?");
  const q = new URLSearchParams(qs ?? "");
  const seg = p.split("/").filter(Boolean);
  const owner = () => { if (S.role() !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this.")); };
  const open = (day: string) => { if (closed(day)) fail(409, m("Esse mês está fechado. Reabra-o em Finanças › Fecho do mês para o alterar.", "That month is closed. Reopen it in Finance › Month Close to change it.")); };
  const key = `${method} /${seg.map((x, i) => (/^[0-9a-f-]{8,}$|^p\d+$|^prod-|^\d{4}-\d{2}$|^(fs|ar|mc)$/.test(x) && i > 0 ? ":id" : x)).join("/")}`;
  const id = seg[1];

  const ai = previewAi(method, path, body, m, S, { products, expenses, pay, staff: STAFF, totals });
  if (ai !== undefined) return ai;

  switch (key) {
    case "GET /products": return products.filter((x) => x.active || q.get("all") === "true").sort((a, c) => Number(c.active) - Number(a.active) || a.sort - c.sort);
    case "POST /products": {
      owner();
      const pr: Product = { id: uid(), code: "p-" + uid().slice(0, 8), kind: b.kind as Product["kind"], visit_kind: (b.visit_kind ?? null) as Product["visit_kind"], applies_to: (b.applies_to ?? "session") as Product["applies_to"],
        name_pt: str(b.name_pt), name_en: str(b.name_en), price_cents: Number(b.price_cents), sessions: (b.sessions as number) ?? null, valid_months: (b.valid_months as number) ?? null,
        shareable: !!b.shareable, instalments: Number(b.instalments ?? 1), includes_assessment: false, active: true, sort: 1000, vat_exempt: b.vat_exempt !== false };
      if (pr.kind === "visit" && products.some((x) => x.kind === "visit" && x.active && x.visit_kind === pr.visit_kind)) fail(409, m("Já existe um preço para este tipo de marcação. Altere esse.", "There’s already a price for this kind of appointment. Change that one instead."));
      products.push(pr); return { id: pr.id, code: pr.code };
    }
    case "PATCH /products/:id": { owner(); const pr = products.find((x) => x.id === id) ?? fail(404, "Not found"); Object.assign(pr, b); S.audit("product.updated", null); return { ok: true }; }

    case "GET /patients/:id/money": {
      const o = owed(S, id);
      S.audit("patient.viewed", id, "money");
      return { plans: plans.filter((x) => x.patient_id === id || x.shared_with === id).map((x) => planOut(S, x)).sort((a, c) => Number(["active", "paused"].includes(c.status)) - Number(["active", "paused"].includes(a.status)) || c.starts_on.localeCompare(a.starts_on)),
        payments: payments.filter((x) => x.patient_id === id).map((x) => payOut(S, x)).sort((a, c) => c.received_on.localeCompare(a.received_on) || c.number - a.number),
        owed: o, due_cents: o.reduce((t, x) => t + x.due_cents, 0) };
    }
    case "POST /patients/:id/plans": {
      const pr = products.find((x) => x.id === b.product_id && x.kind !== "visit" && x.active) ?? fail(400, m("Escolha um pacote ou plano mensal.", "Choose a package or monthly plan."));
      if (b.shared_with && !pr.shareable) fail(400, m("Este pacote não pode ser partilhado.", "This package can’t be shared."));
      const start = str(b.starts_on) || today(); const sd = new Date(start + "T12:00:00");
      const pl: PlanRow = { id: uid(), patient_id: id, product_id: pr.id, kind: pr.kind as "pack", applies_to: pr.applies_to, name_pt: pr.name_pt, name_en: pr.name_en, price_cents: pr.price_cents, sessions: pr.sessions,
        instalments: pr.instalments, shareable: pr.shareable, shared_with: (b.shared_with as string) || null, starts_on: start,
        expires_on: pr.kind === "pack" && pr.valid_months ? ymd(addDays(new Date(sd.getFullYear(), sd.getMonth() + pr.valid_months, sd.getDate()), -1)) : null,
        status: "active", ended_on: null, note: "", skipped: [], includes_assessment: pr.includes_assessment };
      plans.push(pl);
      let attached = 0;
      for (const a of S.appts.filter((x) => x.patient_id === id && x.status === "done" && !x.plan_id && (x.charge_cents ?? 0) > 0 && !x.charge_waived && ["session", "assessment"].includes(x.kind) && ymd(new Date(x.starts_at)) >= start && !paidFor(x.id, id)).sort((a2, c) => a2.starts_at.localeCompare(c.starts_at)))
        if (pickPlan(S, id, "session", ymd(new Date(a.starts_at)), a.kind, a.id, pl.id)) { a.plan_id = pl.id; a.charge_cents = null; attached++; }
      S.audit("plan.sold", id); return { id: pl.id, attached };
    }
    case "PATCH /plans/:id": {
      const pl = plans.find((x) => x.id === id) ?? fail(404, "Not found");
      const cur = ym(new Date());
      if (b.status === "paused") { if (pl.kind !== "monthly") fail(400, m("Só os planos mensais podem ficar em pausa. Termine o pacote.", "Only monthly plans can be paused. End a package instead.")); pl.status = "paused"; if (!payments.some((x) => live(x) && x.plan_id === pl.id && x.covers_month === cur + "-01")) pl.skipped.push(cur); }
      if (b.status === "active") { pl.status = "active"; pl.ended_on = null; pl.skipped = pl.skipped.filter((x) => x !== cur); }
      if (b.status === "ended") { pl.status = "ended"; pl.ended_on = today(); }
      if (b.status === "cancelled") { const o = planOut(S, pl); if (o.used || o.paid_cents) fail(409, m("Este pacote já foi usado ou pago, por isso não pode ser anulado. Termine-o.", "This package has been used or paid, so it can’t be cancelled. End it instead.")); pl.status = "cancelled"; }
      if ("shared_with" in b) pl.shared_with = (b.shared_with as string) || null;
      S.audit("plan.updated", pl.patient_id); return { ok: true };
    }

    case "GET /payments": {
      owner();
      const from = q.get("from") || `${ym(new Date())}-01`, to = q.get("to") || today();
      return payments.filter((x) => x.received_on >= from && x.received_on <= to && (q.get("uninvoiced") !== "true" || (!x.invoice_number && live(x))))
        .map((x) => payOut(S, x)).sort((a, c) => c.received_on.localeCompare(a.received_on) || c.number - a.number);
    }
    case "POST /payments": {
      const items = (b.items as Body[]) ?? [];
      if (!items.length) fail(400, m("Indique o que está a ser pago.", "Add what is being paid."));
      if (!["cash", "mbway", "transfer", "card"].includes(str(b.method))) fail(400, m("Escolha como foi pago.", "Choose how it was paid."));
      const day = str(b.received_on) || today();
      if (day > today()) fail(400, m("A data do pagamento não pode ser no futuro.", "The payment date can’t be in the future."));
      open(day);
      const ids: string[] = [], numbers: number[] = [];
      for (const it of items) {
        const amount = Number(it.amount_cents);
        if (!(amount > 0)) fail(400, m("Verifique o valor.", "Check the amount."));
        let desc = str(it.description), covers: string | null = null;
        if (it.appointment_id) { const a = S.appts.find((x) => x.id === it.appointment_id)!; const d = new Date(a.starts_at); desc ||= `${({ session: "Sessão", assessment: "Avaliação inicial", home: "Domicílio", class: "Aula de grupo" } as Record<string, string>)[a.kind] ?? "Marcação"} ${pad(d.getDate())}/${pad(d.getMonth() + 1)}/${d.getFullYear()}`; }
        if (it.plan_id) { const pl = plans.find((x) => x.id === it.plan_id)!; if (pl.kind === "monthly") { covers = str(it.covers_month).slice(0, 7) || ym(new Date()); desc ||= `${pl.name_pt} — ${MONTHS_PT[+covers.slice(5) - 1]} de ${covers.slice(0, 4)}`; covers += "-01"; } else desc ||= pl.name_pt; }
        if (!desc) fail(400, m("Descreva o que está a ser pago.", "Describe what is being paid."));
        const py = addPayment(S, { patient_id: (b.patient_id as string) || null, method: b.method as Method, received_on: day, amount_cents: amount, description: desc, appointment_id: (it.appointment_id as string) ?? null, plan_id: (it.plan_id as string) ?? null, covers_month: covers, invoice_number: str(b.invoice_number) });
        py.created_at = new Date().toISOString(); ids.push(py.id); numbers.push(py.number);
      }
      S.audit("payment.recorded", (b.patient_id as string) || null, `${b.method} x${ids.length}`);
      return { ids, numbers };
    }
    case "PATCH /payments/:id": {
      const py = payments.find((x) => x.id === id) ?? fail(404, "Not found");
      if (py.voided_at) fail(409, m("Este pagamento foi anulado.", "This payment was voided."));
      open(py.received_on);
      if (typeof b.invoice_number === "string") { py.invoice_number = b.invoice_number.trim(); py.invoiced_at = py.invoice_number ? py.invoiced_at ?? new Date().toISOString() : null; }
      if (b.invoice_file_id) py.invoice_file_id = b.invoice_file_id as string;
      return { ok: true };
    }
    case "POST /payments/:id/void": {
      const py = payments.find((x) => x.id === id) ?? fail(404, "Not found");
      if (!str(b.reason)) fail(400, m("Indique porque está a anular.", "Say why it’s being voided."));
      if (S.role() !== "owner" && !(py.created_by === S.me().id && py.created_at.slice(0, 10) === new Date().toISOString().slice(0, 10))) fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
      if (py.voided_at) fail(409, m("Este pagamento já foi anulado.", "This payment was already voided."));
      open(py.received_on);
      py.voided_at = new Date().toISOString(); py.void_reason = str(b.reason); S.audit("payment.voided", py.patient_id); return { ok: true };
    }

    case "GET /finance/owed": owner(); return owed(S);
    case "GET /finance/overview": {
      owner();
      const mo = q.get("month") || ym(new Date());
      const o = owed(S);
      return { ...totals(mo), month: mo, previous: totals(addMonths(mo, -1)),
        series: [5, 4, 3, 2, 1, 0].map((k) => { const x = addMonths(mo, -k), t = totals(x); return { month: x, revenue_cents: t.revenue_cents, out_cents: (t.employer_ss_cents ?? 0) + t.expenses_cents + t.salaries_cents }; }),
        owed_cents: o.reduce((t, x) => t + x.due_cents, 0), owed_patients: new Set(o.map((x) => x.patient_id)).size,
        closed: closes[mo] ? { closed_at: closes[mo].at, closed_by_name: closes[mo].by } : null };
    }

    case "GET /expenses": {
      owner();
      const from = q.get("from") || `${ym(new Date())}-01`, to = q.get("to") || today();
      return expenses.filter((e) => e.spent_on >= from && e.spent_on <= to).map((e) => ({ ...e, filename: files.find((f) => f.id === e.file_id)?.filename ?? null, closed: closed(e.spent_on) }))
        .sort((a, c) => c.spent_on.localeCompare(a.spent_on));
    }
    case "POST /expenses": {
      owner(); open(str(b.spent_on));
      if (!(Number(b.amount_cents) > 0)) fail(400, m("Verifique o valor.", "Check the amount."));
      expenses.push({ id: uid(), spent_on: str(b.spent_on), supplier: str(b.supplier), supplier_nif: str(b.supplier_nif), category: str(b.category) || "other", description: str(b.description),
        amount_cents: Number(b.amount_cents), vat_cents: (b.vat_cents as number) ?? null, method: (b.method ?? "transfer") as Expense["method"], document_ref: str(b.document_ref), file_id: (b.file_id as string) ?? null, ai_read: !!b.ai_read });
      S.audit("expense.created", null); return { ok: true };
    }
    case "PATCH /expenses/:id": { owner(); const e = expenses.find((x) => x.id === id) ?? fail(404, "Not found"); open(e.spent_on); Object.assign(e, b); return { ok: true }; }
    case "DELETE /expenses/:id": { owner(); const i = expenses.findIndex((x) => x.id === id); if (i >= 0) { open(expenses[i].spent_on); expenses.splice(i, 1); } return { ok: true }; }
    case "POST /finance/files": {
      if (b.kind === "expense") owner();
      const f = { id: uid(), filename: str(b.filename) || "ficheiro" }; files.push(f); return { ...f, bytes: Math.round(str(b.data).length * 0.75), mime: "application/pdf" };
    }

    case "GET /finance/cash": {
      owner();
      const day = q.get("day") || today();
      const exp: CashDay["expected"] = {}; payments.filter((x) => live(x) && x.received_on === day).forEach((x) => { exp[x.method] = (exp[x.method] ?? 0) + x.amount_cents; });
      return { day, expected: exp, payments: payments.filter((x) => live(x) && x.received_on === day).length, close: cash[day] ?? null,
        recent: Object.entries(cash).sort((a, c) => c[0].localeCompare(a[0])).slice(0, 10).map(([d, c]) => ({ day: d, difference_cents: c.difference_cents })) } satisfies CashDay;
    }
    case "POST /finance/cash": {
      owner(); const day = str(b.day); open(day);
      const exp: CashDay["expected"] = {}; payments.filter((x) => live(x) && x.received_on === day).forEach((x) => { exp[x.method] = (exp[x.method] ?? 0) + x.amount_cents; });
      const counted = (b.counted ?? {}) as Record<string, number>;
      const diff = Object.values(counted).reduce((t, v) => t + v, 0) - Object.values(exp).reduce((t, v) => t + (v ?? 0), 0);
      cash[day] = { counted, expected: exp, difference_cents: diff, note: str(b.note), closed_at: new Date().toISOString(), closed_by_name: S.me().name };
      return { ok: true, difference_cents: diff };
    }

    case "GET /finance/salaries": {
      owner(); const mo = q.get("month") || ym(new Date());
      return { month: mo, closed: !!closes[mo], employer_ss_bp: ssBp, staff: STAFF.map((s) => ({ staff_id: s.id, name: s.name, role: s.role, active: true, model: "fixed", fixed_cents: pay[s.id] ?? 0,
        employer_ss_bp: ssOwn[s.id] ?? null, effective_ss_bp: ssFor(s.id),
        paid: salaries.filter((x) => x.staff_id === s.id && x.month === mo).map(({ id: i, gross_cents, employer_ss_cents, paid_on, method: mt, note }) => ({ id: i, gross_cents, employer_ss_cents, paid_on, method: mt, note })) })) };
    }
    case "PUT /finance/salaries/rate": {
      owner(); const v = Number(b.employer_ss_bp);
      if (!Number.isInteger(v) || v < 0 || v > 10000) fail(400, m("Use uma taxa entre 0 e 100 %.", "Use a rate between 0 and 100 %."));
      ssBp = v; S.audit("salary.rate_changed", null, String(v)); return { ok: true, employer_ss_bp: v };
    }
    case "PUT /staff/:id/pay": { owner(); if (b.model !== "fixed") fail(400, m("Comissão e aluguer de sala ainda não estão disponíveis.", "Commission and room rental aren’t available yet.")); pay[id] = Number(b.fixed_cents); ssOwn[id] = b.employer_ss_bp == null ? null : Number(b.employer_ss_bp); return { ok: true }; }
    case "POST /finance/salaries": { owner(); const mo = str(b.month); open(mo + "-01"); salaries.push({ id: uid(), staff_id: str(b.staff_id), month: mo, gross_cents: Number(b.gross_cents), employer_ss_cents: b.employer_ss_cents == null ? ssOf(Number(b.gross_cents), ssFor(str(b.staff_id))) : Number(b.employer_ss_cents), paid_on: str(b.paid_on) || today(), method: (b.method ?? "transfer") as "transfer", note: "" }); return { ok: true }; }
    case "DELETE /finance/salaries/:id": { owner(); const i = salaries.findIndex((x) => x.id === id); if (i >= 0) { open(salaries[i].month + "-01"); salaries.splice(i, 1); } return { ok: true }; }

    case "GET /finance/months": owner(); return Object.entries(closes).sort((a, c) => c[0].localeCompare(a[0])).map(([mo, c]) => ({ month: mo, totals: c.totals, closed_at: c.at, closed_by_name: c.by })) satisfies MonthClose[];
    case "POST /finance/months/:id/close": {
      owner(); const mo = seg[2];
      if (mo >= ym(new Date())) fail(400, m("Um mês só pode ser fechado depois de terminar.", "A month can be closed once it has ended."));
      if (closes[mo]) fail(409, m("Esse mês já está fechado.", "That month is already closed."));
      closes[mo] = { at: new Date().toISOString(), by: S.me().name, totals: totals(mo) }; S.audit("month.closed", null, mo); return { ok: true, totals: closes[mo].totals };
    }
    case "POST /finance/months/:id/reopen": { owner(); delete closes[seg[2]]; S.audit("month.reopened", null, seg[2]); return { ok: true }; }
    case "POST /finance/export": {
      owner(); const from = str(b.from), to = str(b.to);
      const label = from === to ? from : `${from}_a_${to}`;
      let open2 = false; for (let x = from; x <= to; x = addMonths(x, 1)) if (!closes[x]) open2 = true;
      S.audit("finance.exported", null, label);
      return { id: uid(), filename: `Exportacao_${label}.zip`, bytes: 48213, path: `PhysioPulse/Financas/Exportacoes/Exportacao_${label}.zip`, provisional: open2 };
    }
  }
  return undefined;
}
