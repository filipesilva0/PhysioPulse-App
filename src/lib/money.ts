// Phase 3 money: the shapes the clinic server returns, labels, and small helpers.
// Amounts travel as whole cents; eur() takes euros, so divide by 100 (see `c2e`).
import type { Lang } from "./ctx";

export type Method = "cash" | "mbway" | "transfer" | "card" | "stripe";
export type ExpenseMethod = "cash" | "mbway" | "transfer" | "card" | "debit";

export interface Product {
  id: string; code: string; kind: "visit" | "pack" | "monthly"; visit_kind: "session" | "assessment" | "home" | "class" | null;
  applies_to: "session" | "class"; name_pt: string; name_en: string; price_cents: number; sessions: number | null; valid_months: number | null;
  shareable: boolean; instalments: number; includes_assessment: boolean; active: boolean; sort: number;
  /** Exempt from VAT (art. 9.º, n.º 1 CIVA): therapeutic physiotherapy. Fitness-style classes aren't (23 %). */
  vat_exempt: boolean;
}

export interface Plan {
  id: string; patient_id: string; patient_name: string; product_id: string | null; kind: "pack" | "monthly"; applies_to: "session" | "class";
  name_pt: string; name_en: string; price_cents: number; sessions: number | null; instalments: number; shareable: boolean;
  shared_with: string | null; shared_with_name: string | null; starts_on: string; expires_on: string | null;
  status: "active" | "paused" | "ended" | "cancelled"; ended_on: string | null; note: string;
  used: number; used_month: number; remaining: number | null; paid_cents: number;
}

export interface Payment {
  id: string; number: number; received_on: string; patient_id: string | null; patient_name: string | null; patient_number: number | null;
  amount_cents: number; method: Method; description: string; appointment_id: string | null; plan_id: string | null; covers_month: string | null;
  invoice_number: string; invoice_file_id: string | null; invoiced_at: string | null; note: string;
  created_by: string | null; created_by_name: string | null; created_at: string; voided_at: string | null; void_reason: string;
}

export interface Owed {
  type: "appointment" | "plan" | "month"; patient_id: string; patient_name: string; patient_number: number;
  appointment_id: string | null; plan_id: string | null; month: string | null; day: string; what: string;
  name_pt: string | null; name_en: string | null; price_cents: number; due_cents: number;
}

export interface PatientMoney { plans: Plan[]; payments: Payment[]; owed: Owed[]; due_cents: number }

export interface Totals {
  revenue_cents: number; expenses_cents: number; salaries_cents: number; result_cents: number; payments: number;
  /** Employer Social Security on the month's salaries (part of the cost; audit phase C). */
  employer_ss_cents?: number;
  by_method: Partial<Record<Method, number>>; expenses_by_category: Record<string, number>; uninvoiced: number; uninvoiced_cents: number;
}
export interface Overview extends Totals {
  month: string; previous: Totals; series: { month: string; revenue_cents: number; out_cents: number }[];
  owed_cents: number; owed_patients: number; closed: { closed_at: string; closed_by_name: string | null } | null;
}

export interface Expense {
  id: string; spent_on: string; supplier: string; supplier_nif: string; category: string; description: string;
  amount_cents: number; vat_cents: number | null; method: ExpenseMethod; document_ref: string; file_id: string | null; filename: string | null; closed: boolean; ai_read?: boolean;
}

export interface CashDay {
  day: string; expected: Partial<Record<Method, number>>; payments: number;
  close: { counted: Partial<Record<Method, number>>; expected: Partial<Record<Method, number>>; difference_cents: number; note: string; closed_at: string; closed_by_name: string | null } | null;
  recent: { day: string; difference_cents: number }[];
}

export interface SalaryRow {
  staff_id: string; name: string; role: "owner" | "therapist" | "reception"; active: boolean; model: "fixed" | "commission" | "rental"; fixed_cents: number;
  /** Their own employer Social Security rate (basis points), or null for the clinic's. */
  employer_ss_bp: number | null; effective_ss_bp: number;
  paid: { id: string; gross_cents: number; employer_ss_cents: number; paid_on: string; method: "cash" | "transfer"; note: string }[];
}
/** `employer_ss_bp`: the clinic's employer Social Security rate in basis points (2375 = 23,75 %). */
export interface Salaries { month: string; closed: boolean; employer_ss_bp: number; staff: SalaryRow[] }
/** 2375 → "23,75 %" */
export const pctBp = (bp: number, lang: Lang) => `${(bp / 100).toLocaleString(lang === "pt" ? "pt-PT" : "en-GB", { maximumFractionDigits: 2 })} %`;
/** The employer's contribution on a gross amount, rounded half up to the cent (same as the server). */
export const employerSs = (grossCents: number, bp: number) => Math.floor((grossCents * bp + 5000) / 10000);
export interface MonthClose { month: string; totals: Totals; closed_at: string; closed_by_name: string | null }

export interface MoneyAlerts {
  plans_low: { patient_id: string; name: string; plan_id: string; name_pt: string; name_en: string; remaining: number; expires_on: string | null }[];
  money: { owed_cents: number; owed_patients: number; today_cents: number; owed: { patient_id: string; name: string; due_cents: number }[] } | null;
}

// ───────── helpers ─────────

/** Cents → euros for eur(). */
export const c2e = (c: number) => c / 100;
/** "12,50" or "12.50" or "12" → 1250 cents; null if it isn't a number. */
export const parseEuros = (s: string): number | null => {
  const t = s.replace(/[€\s]/g, "").replace(/\.(?=\d{3}(\D|$))/g, "").replace(",", ".");
  if (!t || !/^\d+(\.\d{0,2})?$/.test(t)) return null;
  return Math.round(parseFloat(t) * 100);
};
/** 1250 → "12,50" (pt) / "12.50" (en), for editable fields. */
export const centsField = (c: number, lang: Lang) => (c / 100).toFixed(2).replace(".", lang === "pt" ? "," : ".");
export const thisMonth = () => { const d = new Date(); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`; };
export const shiftMonth = (m: string, n: number) => { const [y, mo] = m.split("-").map(Number); const d = new Date(y, mo - 1 + n, 1); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`; };
export const monthLabel = (m: string, lang: Lang, short = false) => {
  const [y, mo] = m.split("-").map(Number);
  return new Intl.DateTimeFormat(lang === "pt" ? "pt-PT" : "en-GB", short ? { month: "short" } : { month: "long", year: "numeric" }).format(new Date(y, mo - 1, 1));
};

type T2 = [string, string];
const pick = (t: T2, lang: Lang) => (lang === "pt" ? t[0] : t[1]);

export const METHODS: [Method, T2][] = [["cash", ["Numerário", "Cash"]], ["mbway", ["MB WAY", "MB WAY"]], ["transfer", ["Transferência", "Transfer"]], ["card", ["Cartão (TPA)", "Card (terminal)"]], ["stripe", ["Stripe", "Stripe"]]];
export const methodLabel = (m: string, lang: Lang) => { const x = METHODS.find((v) => v[0] === m) ?? EXPENSE_METHODS.find((v) => v[0] === m); return x ? pick(x[1], lang) : m; };
export const EXPENSE_METHODS: [ExpenseMethod, T2][] = [["transfer", ["Transferência", "Transfer"]], ["debit", ["Débito direto", "Direct debit"]], ["card", ["Cartão", "Card"]], ["mbway", ["MB WAY", "MB WAY"]], ["cash", ["Numerário", "Cash"]]];

/** Same codes as the server's CATEGORIES (server/src/routes/money.rs). */
export const CATEGORIES: [string, T2][] = [
  ["rent", ["Renda", "Rent"]], ["utilities", ["Eletricidade, água e gás", "Electricity, water and gas"]], ["telecom", ["Telecomunicações", "Phone and internet"]],
  ["supplies", ["Consumíveis clínicos", "Clinical supplies"]], ["equipment", ["Equipamento", "Equipment"]], ["software", ["Software e subscrições", "Software and subscriptions"]],
  ["insurance", ["Seguros", "Insurance"]], ["training", ["Formação", "Training"]], ["marketing", ["Marketing", "Marketing"]],
  ["accounting", ["Contabilidade e serviços", "Accounting and services"]], ["office", ["Material de escritório", "Office supplies"]], ["bank", ["Comissões bancárias", "Bank fees"]],
  ["taxes", ["Impostos e taxas", "Taxes and fees"]], ["other", ["Outras", "Other"]],
];
export const categoryLabel = (c: string, lang: Lang) => { const x = CATEGORIES.find((v) => v[0] === c); return x ? pick(x[1], lang) : c; };

export const planName = (p: { name_pt: string | null; name_en: string | null }, lang: Lang) => (lang === "pt" ? p.name_pt : p.name_en) ?? "";
export const productName = (p: Product, lang: Lang) => (lang === "pt" ? p.name_pt : p.name_en);

const VISIT: Record<string, T2> = { session: ["Sessão", "Session"], assessment: ["Avaliação inicial", "First assessment"], home: ["Domicílio", "Home visit"], class: ["Aula de grupo", "Group class"] };
/** What an owed item is, in words: "Sessão, 3 nov." / "Pacote de 5" / "Acompanhamento, 4 por mês — novembro de 2026". */
export const owedLabel = (o: Owed, lang: Lang) => {
  const day = new Intl.DateTimeFormat(lang === "pt" ? "pt-PT" : "en-GB", { day: "numeric", month: "short" }).format(new Date(o.day + "T12:00:00"));
  if (o.type === "appointment") return `${o.what === "class" && o.name_pt ? o.name_pt : pick(VISIT[o.what] ?? ["Marcação", "Appointment"], lang)}, ${day}`;
  if (o.type === "month" && o.month) return `${planName(o, lang)} — ${monthLabel(o.month.slice(0, 7), lang)}`;
  return planName(o, lang);
};
/** A stable key for an owed item (to pick it in a list). */
export const owedKey = (o: Owed) => `${o.type}:${o.appointment_id ?? ""}:${o.plan_id ?? ""}:${o.month ?? ""}`;

export const planStatusLabel = (s: Plan["status"], lang: Lang) =>
  pick(({ active: ["Ativo", "Active"], paused: ["Em pausa", "Paused"], ended: ["Terminado", "Ended"], cancelled: ["Anulado", "Cancelled"] } as Record<string, T2>)[s], lang);

/** Read a picked file as base64 (no data: prefix) for /finance/files. */
export const fileToBase64 = (f: File) => new Promise<string>((res, rej) => {
  const r = new FileReader();
  r.onload = () => res(String(r.result).split("base64,")[1] ?? "");
  r.onerror = () => rej(r.error);
  r.readAsDataURL(f);
});
