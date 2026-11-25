// Phase 2 data: the shapes the clinic server returns, a small loading hook, and shared labels.
// Everything here comes from the server through api(); nothing is stored on the computer.
import { useCallback, useEffect, useRef, useState } from "react";
import { api, CHANGED, isErr } from "./api";
import type { Lang } from "./ctx";
import type { MoneyAlerts } from "./money";

// ───────── server shapes ─────────

export interface Therapist { id: string; name: string; role: "owner" | "therapist" }

export interface PatientRow {
  id: string; number: number; name: string; birth_date: string | null; phone: string | null; email: string | null;
  active: boolean; therapist_id: string | null; therapist_name: string | null; next_at: string | null; last_at: string | null;
}

export interface PatientRec {
  id: string; number: number; name: string; birth_date: string | null; sex: "f" | "m" | "x" | null; nif: string | null; sns_number: string | null;
  phone: string | null; email: string | null; address: string | null; guardian_name: string | null; guardian_phone: string | null;
  lang: Lang; reminders: "sms" | "email" | "none"; therapist_id: string | null; therapist_name: string | null; admin_note: string;
  active: boolean; anonymised_at: string | null; created_at: string; updated_at: string;
}

export interface Region { region: string; symptoms: string[]; notes: string }
export interface Anamnesis {
  main_complaint: string; history: string; medication: string; medical_history: string; goals: string;
  regions: Region[]; red_flags: string[]; updated_at: string; updated_by_name: string | null;
}

export type Block =
  /** `value` null = not measured this session (shown as "—", left out of the signed note; audit C3). */
  | { type: "slider"; key: string; label: string; value: number | null; min: number; max: number; step?: number; unit?: string }
  | { type: "choice"; key: string; label: string; value: string; options: string[] }
  | { type: "checklist"; key: string; label: string; value: string[]; options: string[] }
  | { type: "text"; key: string; label: string; value: string };

export interface Note {
  id: string; appointment_id: string | null; template: string; blocks: Block[]; text: string;
  created_at: string; updated_at: string; created_by: string | null; created_by_name: string | null;
  signed_at: string | null; signed_by_name: string | null; ai_assisted?: boolean;
}

export interface TestResult {
  id: string; test_code: string; answers: unknown; score: number; max_score: number | null;
  higher_is_worse: boolean; taken_on: string; recorded_by_name: string | null;
}

export interface Prescription {
  id: string; patient_id: string; doctor: string; specialty: string; issued_on: string; sessions_total: number;
  valid_until: string | null; payer: string; document_id: string | null; used: number; booked: number; remaining: number;
}

export type ConsentKind = "treatment" | "data" | "media" | "marketing" | "guardian" | "invasive";
export interface ConsentRow {
  id: string; kind: ConsentKind; granted: boolean; signed_by_name: string; signed_at: string; expires_on: string | null;
  revoked_at: string | null; version: number; title: string; state: "valid" | "expired" | "revoked" | "refused";
  /** The PDF saved in the patient's Consentimentos folder when signed, and when withdrawn. */
  pdf_id: string | null; revoked_pdf_id: string | null;
}
export interface ConsentText { id: string; kind: ConsentKind; version: number; lang: Lang; title: string; body: string; valid_months: number | null; created_at: string;
  /** current = the newest approved version (the one people sign); draft = waiting for the owner's approval. */
  current: boolean; draft: boolean; approved_at: string | null }
export interface ConsentFull extends ConsentRow { signature_png: string; body: string; lang: Lang }

export interface FileAppt { id: string; kind: ApptKind; starts_at: string; ends_at: string; status: ApptStatus; therapist_id: string | null; therapist_name: string | null; series_id: string | null }
export type DocKind = "photo" | "report" | "imaging" | "prescription" | "letter" | "other" | "note" | "consent";
export interface DocumentRow {
  id: string; kind: DocKind; filename: string; mime: string; bytes: number; uploaded_at: string;
  uploaded_by: string | null; uploaded_by_name: string | null; note_id: string | null; consent_id?: string | null; letter_id?: string | null;
}
/** GET /documents/:id — the file itself as base64, to show inside the app. */
export interface DocumentFull { id: string; kind: DocKind; filename: string; mime: string; bytes: number; data: string }

export interface PatientFile {
  view: "full" | "reduced";
  patient: PatientRec; consents: ConsentRow[]; appointments: FileAppt[]; prescriptions: Prescription[];
  anamnesis?: Anamnesis | null; notes?: Note[]; tests?: TestResult[]; documents?: DocumentRow[];
}

export type ApptKind = "session" | "assessment" | "class" | "home" | "block";
export type ApptStatus = "booked" | "done" | "cancelled" | "no_show";
export interface Attendee { patient_id: string; name: string; attended: boolean | null }
export interface Appointment {
  id: string; kind: ApptKind; starts_at: string; ends_at: string; status: ApptStatus;
  therapist_id: string | null; therapist_name: string | null; patient_id: string | null; patient_name: string | null; patient_number: number | null;
  series_id: string | null; prescription_id: string | null; title: string; color: string; address: string; travel_minutes: number;
  capacity: number | null; admin_note: string; attendees: Attendee[] | null;
  /** Phase 3: the package a done appointment used, what it costs, and what was paid for it. */
  plan_id?: string | null; charge_cents?: number | null; charge_waived?: boolean; paid_cents?: number;
}

export interface Waiting {
  id: string; patient_id: string | null; name: string; phone: string; email: string; therapist_id: string | null; therapist_name: string | null;
  weekdays: number[]; from_time: string | null; to_time: string | null; note: string; status: "waiting" | "offered"; created_at: string;
}
export interface BookingRequest {
  id: string; name: string; phone: string; email: string; patient_id: string | null; service: string; preferred_at: string;
  message: string; status: "pending" | "accepted" | "declined"; created_at: string;
  /** Phase 4a: requests from the public website (time of day instead of an hour; address for home visits). */
  source?: "staff" | "website" | "portal"; day_part?: "" | "morning" | "afternoon" | "any"; address?: string; lang?: "pt" | "en";
  /** Phase 4b: from a signed-in patient — a new appointment, another time for `about_appointment_id`, or a cancellation they made. */
  kind?: "book" | "move" | "cancelled"; about_appointment_id?: string | null; about_starts_at?: string | null;
  /** The one-press booking made this patient's file (Undo removes it again if nothing else uses it). */
  created_patient?: boolean;
}
/** What one press would book for a request (GET /booking-requests/:id/suggest, 11 Nov 2026). */
export interface RequestSlot { starts_at: string; ends_at: string; therapist_id: string; therapist_name: string }
export interface RequestSuggestion {
  kind: string; minutes: number; slot: RequestSlot | null; alternatives: RequestSlot[];
  /** The request is already linked to this patient. */
  patient: { id: string; name: string; number: number } | null;
  /** Patients that look like the person who asked (same phone, e-mail or name). */
  lookalikes: { id: string; number: number; name: string; reason: "phone" | "email" | "name" }[];
}
/** Settings › Website (phase 4a, owner). */
export interface WebsiteSite {
  published: boolean; requests_open: boolean; home_visits: boolean; opening_soon: boolean; show_prices: boolean; accounts_open?: boolean;
  tagline_pt: string; tagline_en: string; intro_pt: string; intro_en: string; home_text_pt: string; home_text_en: string; updated_at: string;
}
export interface WebsitePerson { id: string; name: string; role: "owner" | "therapist"; web_visible: boolean; web_title_pt: string; web_title_en: string; web_bio_pt: string; web_bio_en: string }
export interface WebsiteInfo {
  site: WebsiteSite; team: WebsitePerson[];
  products: { id: string; name_pt: string; name_en: string; kind: "visit" | "pack" | "monthly"; price_cents: number; web_visible: boolean }[];
  clinic: { name: string; address: string; phone: string; email: string; nif: string; ers_number: string };
  requests_30d: number; site_url: string;
}
export interface Alerts {
  booking_requests: number;
  consents_missing: { patient_id: string; name: string; number: number; missing: ConsentKind[] }[];
  consents_expiring: { patient_id: string; name: string; kind: ConsentKind; expires_on: string }[];
  prescriptions: { patient_id: string; name: string; prescription_id: string; remaining: number; valid_until: string | null }[] | null;
  worse_scores: { patient_id: string; name: string; test_code: string; score: number; previous: number; taken_on: string }[] | null;
  /** Phase 3 (see lib/money.ts): packages running out (everyone), money (owner only). */
  plans_low?: MoneyAlerts["plans_low"]; money?: MoneyAlerts["money"];
  /** Owner only: open complaints due within 3 working days, or late (null for everyone else). */
  complaints?: { id: string; received_on: string; due_on: string; complainant: string; working_days_left: number }[] | null;
  /** Owner only: the backup is late, failed or can't be seen (null when it's fine, and for everyone else). */
  backup?: { state: "old" | "failed" | "unknown"; last_ok: string | null } | null;
  /** Owner only: devices waiting for approval (PhysioPulse plan, phase 3; null for everyone else). */
  devices?: { id: string; name: string; code: string; kind?: "computer" | "phone"; requested_by_name: string | null }[] | null;
}
export interface Overlap { id: string; clashes_with: string; starts_at: string; ends_at: string; therapist_name?: string; name?: string }

// ───────── loading ─────────

/** GET a path and keep it fresh: reloads after any change made through api() (see CHANGED). Pass null to skip.
 *  `keep`: when the path changes, keep showing the old answer until the new one arrives (paged lists). */
export function useApi<T>(path: string | null, opts?: { keep?: boolean }) {
  const [data, setData] = useState<T | undefined>(undefined);
  const [error, setError] = useState<string | undefined>();
  const [loading, setLoading] = useState(!!path);
  const seq = useRef(0);
  const load = useCallback(() => {
    if (!path) { setData(undefined); setLoading(false); return; }
    const n = ++seq.current;
    setLoading(true);
    api<T>("GET", path)
      .then((d) => { if (n === seq.current) { setData(d); setError(undefined); } })
      .catch((e) => { if (n === seq.current) setError(isErr(e) ? e.message : String(e)); })
      .finally(() => { if (n === seq.current) setLoading(false); });
  }, [path]);
  const keep = !!opts?.keep;
  useEffect(() => { if (!keep || !path) setData(undefined); load(); }, [load]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    window.addEventListener(CHANGED, load);
    return () => window.removeEventListener(CHANGED, load);
  }, [load]);
  return { data, error, loading, reload: load };
}

// ───────── dates (shown in the computer's time zone, which is the clinic's) ─────────

const loc = (lang: Lang) => (lang === "pt" ? "pt-PT" : "en-GB");
export const pad = (n: number) => String(n).padStart(2, "0");
/** "2026-11-04" for a Date, in local time. */
export const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
/** A "YYYY-MM-DD" date as a local Date at midnight. */
export const fromYmd = (s: string) => { const [y, m, d] = s.slice(0, 10).split("-").map(Number); return new Date(y, m - 1, d); };
export const hm = (iso: string) => { const d = new Date(iso); return `${pad(d.getHours())}:${pad(d.getMinutes())}`; };
export const fmtDate = (s: string | Date, lang: Lang, o: Intl.DateTimeFormatOptions = { day: "numeric", month: "short", year: "numeric" }) =>
  new Intl.DateTimeFormat(loc(lang), o).format(typeof s === "string" ? (s.length <= 10 ? fromYmd(s) : new Date(s)) : s);
export const fmtDay = (d: Date, lang: Lang) => new Intl.DateTimeFormat(loc(lang), { weekday: "long", day: "numeric", month: "long" }).format(d);
export const weekdayShort = (lang: Lang) => (lang === "pt" ? ["Seg", "Ter", "Qua", "Qui", "Sex", "Sáb", "Dom"] : ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
/** Monday of the week that holds d. */
export const monday = (d: Date) => { const x = new Date(d.getFullYear(), d.getMonth(), d.getDate()); x.setDate(x.getDate() - ((x.getDay() + 6) % 7)); return x; };
export const addDays = (d: Date, n: number) => { const x = new Date(d); x.setDate(x.getDate() + n); return x; };
export const sameDay = (a: Date, b: Date) => ymd(a) === ymd(b);
/** Hours since midnight, e.g. 9.5 for 09:30 (local). */
export const hourOf = (iso: string) => { const d = new Date(iso); return d.getHours() + d.getMinutes() / 60; };
export const minutesBetween = (a: string, b: string) => Math.round((new Date(b).getTime() - new Date(a).getTime()) / 60000);
export const age = (birth: string | null) => {
  if (!birth) return null;
  const b = fromYmd(birth), n = new Date();
  return n.getFullYear() - b.getFullYear() - (n.getMonth() < b.getMonth() || (n.getMonth() === b.getMonth() && n.getDate() < b.getDate()) ? 1 : 0);
};
/** Query string range for /appointments: [from, to) as RFC 3339. */
export const range = (from: Date, to: Date) => `from=${encodeURIComponent(from.toISOString())}&to=${encodeURIComponent(to.toISOString())}`;

// ───────── colours: one per therapist, stable across the app ─────────

const TONES = ["var(--t1)", "var(--t2)", "var(--t3)", "var(--t4)", "var(--t5)"];
/** Therapist colour from their position in the list (owner first). */
export const toneFor = (list: { id: string }[] | undefined, id: string | null | undefined) => {
  const i = list?.findIndex((t) => t.id === id) ?? -1;
  return i >= 0 ? TONES[i % TONES.length] : "var(--text3)";
};

// ───────── labels ─────────

type T2 = [string, string];
const pick = (t: T2, lang: Lang) => (lang === "pt" ? t[0] : t[1]);

export const KIND: Record<ApptKind, T2> = {
  session: ["Sessão", "Session"], assessment: ["Avaliação inicial", "First assessment"], class: ["Aula de grupo", "Group class"],
  home: ["Domicílio", "Home visit"], block: ["Tempo bloqueado", "Blocked time"],
};
export const kindLabel = (k: ApptKind, lang: Lang) => pick(KIND[k], lang);
export const STATUS: Record<ApptStatus, T2> = { booked: ["Marcada", "Booked"], done: ["Realizada", "Done"], cancelled: ["Cancelada", "Cancelled"], no_show: ["Faltou", "No-show"] };
export const statusLabel = (s: ApptStatus, lang: Lang) => pick(STATUS[s], lang);

export const BLOCK_LABELS: [string, T2, string][] = [
  ["holiday", ["Férias", "Holiday"], "#2F8F5B"], ["training", ["Formação", "Training"], "#4653C2"],
  ["illness", ["Doença", "Illness"], "#B4442E"], ["admin", ["Administração", "Admin time"], "#7A746C"],
];
export const blockLabel = (title: string, lang: Lang) => { const b = BLOCK_LABELS.find((x) => x[0] === title); return b ? pick(b[1], lang) : title || pick(KIND.block, lang); };

export const REGIONS: [string, T2][] = [
  ["cervical", ["Coluna cervical", "Cervical spine"]], ["thoracic", ["Coluna dorsal", "Thoracic spine"]], ["lumbar", ["Coluna lombar", "Lumbar spine"]],
  ["shoulder_r", ["Ombro direito", "Right shoulder"]], ["shoulder_l", ["Ombro esquerdo", "Left shoulder"]], ["elbow_r", ["Cotovelo direito", "Right elbow"]],
  ["elbow_l", ["Cotovelo esquerdo", "Left elbow"]], ["wrist_hand", ["Punho e mão", "Wrist and hand"]], ["hip", ["Anca", "Hip"]],
  ["knee_r", ["Joelho direito", "Right knee"]], ["knee_l", ["Joelho esquerdo", "Left knee"]], ["ankle_foot", ["Tornozelo e pé", "Ankle and foot"]],
];
export const regionLabel = (k: string, lang: Lang) => { const r = REGIONS.find((x) => x[0] === k); return r ? pick(r[1], lang) : k; };
export const SYMPTOMS: [string, T2][] = [
  ["pain", ["Dor", "Pain"]], ["stiffness", ["Rigidez", "Stiffness"]], ["weakness", ["Fraqueza", "Weakness"]], ["tingling", ["Formigueiro", "Tingling"]],
  ["numbness", ["Dormência", "Numbness"]], ["swelling", ["Edema", "Swelling"]], ["instability", ["Instabilidade", "Instability"]], ["cramps", ["Cãibras", "Cramps"]],
];
export const symptomLabel = (k: string, lang: Lang) => { const r = SYMPTOMS.find((x) => x[0] === k); return r ? pick(r[1], lang) : k; };
export const RED_FLAGS: [string, T2][] = [
  ["weight_loss", ["Perda de peso", "Weight loss"]], ["fever", ["Febre", "Fever"]], ["night_pain", ["Dor noturna", "Night pain"]],
  ["neuro", ["Alterações neurológicas", "Neurological changes"]], ["trauma", ["Trauma recente", "Recent trauma"]],
];

export const CONSENT_KINDS: [ConsentKind, T2][] = [
  ["treatment", ["Consentimento informado para tratamento", "Informed consent for treatment"]],
  ["data", ["Tratamento de dados pessoais (RGPD)", "Personal data processing (GDPR)"]],
  ["media", ["Fotografias e vídeos", "Photos and videos"]],
  ["marketing", ["Comunicações da clínica", "News from the clinic"]],
  ["guardian", ["Representante legal (menores)", "Legal representative (minors)"]],
  ["invasive", ["Técnicas invasivas (punção seca)", "Invasive techniques (dry needling)"]],
];
export const consentLabel = (k: ConsentKind, lang: Lang) => pick(CONSENT_KINDS.find((x) => x[0] === k)![1], lang);
/** What a document is. "note" and "consent" are only made by the server (signed notes, signed consents). */
export const DOC_KINDS: [DocKind, T2][] = [
  ["report", ["Relatório", "Report"]], ["imaging", ["Exame", "Imaging"]], ["photo", ["Fotografia", "Photo"]],
  ["prescription", ["Prescrição", "Prescription"]], ["letter", ["Carta", "Letter"]], ["other", ["Outro", "Other"]],
  ["note", ["Nota assinada", "Signed note"]], ["consent", ["Consentimento", "Consent"]],
];
/** PDFs the server makes are part of the record: they can't be renamed, re-filed or removed. */
/** PDFs the server makes are part of the record: signed notes, consents and approved letters can't be renamed or removed. */
export const serverMade = (d: Pick<DocumentRow, "kind" | "letter_id">) => d.kind === "note" || d.kind === "consent" || !!d.letter_id;
export const docKindLabel = (k: DocKind, lang: Lang) => pick((DOC_KINDS.find((x) => x[0] === k) ?? DOC_KINDS[5])[1], lang);
/** File size the way Finder shows it: 840 KB, 2,4 MB. */
export const fileSize = (b: number, lang: Lang) => b < 1024 * 1024 ? `${Math.max(1, Math.round(b / 1024))} KB`
  : `${(b / 1024 / 1024).toLocaleString(lang === "pt" ? "pt-PT" : "en-GB", { maximumFractionDigits: 1 })} MB`;
/** Types the server accepts (checked again there by the file's first bytes). */
export const DOC_ACCEPT = "application/pdf,image/jpeg,image/png,image/heic,image/webp,.heic";

/** Kinds the patient may refuse (the record still keeps their answer). */
export const REFUSABLE: ConsentKind[] = ["data", "media", "marketing"];

/** Tests the app can record today: the total score is typed in. Question-by-question forms with
 *  automatic scoring arrive with the PhysioPulse library (see OPEN_QUESTIONS.md). */
export const TEST_LIBRARY: { code: string; name: T2; max: number; higherIsWorse: boolean }[] = [
  { code: "nprs", name: ["Escala numérica da dor (0–10)", "Numeric pain rating (0–10)"], max: 10, higherIsWorse: true },
  { code: "ndi", name: ["Neck Disability Index", "Neck Disability Index"], max: 50, higherIsWorse: true },
  { code: "odi", name: ["Oswestry Disability Index (%)", "Oswestry Disability Index (%)"], max: 100, higherIsWorse: true },
  { code: "quickdash", name: ["QuickDASH", "QuickDASH"], max: 100, higherIsWorse: true },
  { code: "spadi", name: ["SPADI (%)", "SPADI (%)"], max: 100, higherIsWorse: true },
  { code: "lefs", name: ["LEFS", "LEFS"], max: 80, higherIsWorse: false },
  { code: "koos", name: ["KOOS (0–100)", "KOOS (0–100)"], max: 100, higherIsWorse: false },
  { code: "psfs", name: ["Escala funcional específica (PSFS)", "Patient-Specific Functional Scale"], max: 10, higherIsWorse: false },
];
export const testName = (code: string, lang: Lang) => { const t = TEST_LIBRARY.find((x) => x.code === code); return t ? pick(t.name, lang) : code.toUpperCase(); };

/** Progress-note templates. Blocks are copied into the note, so editing a template never changes old notes. */
export const NOTE_TEMPLATES: { id: string; name: T2; blocks: (lang: Lang) => Block[] }[] = [
  { id: "neck", name: ["Cervicalgia", "Neck pain"], blocks: (l) => [
    { type: "slider", key: "pain", label: pick(["Dor (0–10)", "Pain (0–10)"], l), value: null, min: 0, max: 10 },
    { type: "slider", key: "rotation", label: pick(["Rotação cervical", "Neck rotation"], l), value: null, min: 0, max: 90, step: 5, unit: "°" },
    { type: "slider", key: "function", label: pick(["Função (0–10)", "Function (0–10)"], l), value: null, min: 0, max: 10 },
    techniques(l),
  ] },
  { id: "lowback", name: ["Lombalgia", "Low back pain"], blocks: (l) => [
    { type: "slider", key: "pain", label: pick(["Dor (0–10)", "Pain (0–10)"], l), value: null, min: 0, max: 10 },
    { type: "slider", key: "flexion", label: pick(["Flexão lombar (dedos-chão, cm)", "Lumbar flexion (fingers to floor, cm)"], l), value: null, min: 0, max: 60, unit: " cm" },
    { type: "choice", key: "slr", label: pick(["Lasègue", "Straight leg raise"], l), value: "", options: [pick(["Negativo", "Negative"], l), pick(["Positivo à direita", "Positive right"], l), pick(["Positivo à esquerda", "Positive left"], l)] },
    techniques(l),
  ] },
  { id: "acl", name: ["LCA pós-operatório", "ACL post-op"], blocks: (l) => [
    { type: "slider", key: "pain", label: pick(["Dor (0–10)", "Pain (0–10)"], l), value: null, min: 0, max: 10 },
    { type: "slider", key: "flexion", label: pick(["Flexão do joelho", "Knee flexion"], l), value: null, min: 0, max: 150, step: 5, unit: "°" },
    { type: "slider", key: "extension_lag", label: pick(["Défice de extensão", "Extension lag"], l), value: null, min: 0, max: 30, unit: "°" },
    { type: "choice", key: "swelling", label: pick(["Edema", "Swelling"], l), value: "", options: [pick(["Nenhum", "None"], l), pick(["Ligeiro", "Mild"], l), pick(["Moderado", "Moderate"], l)] },
    techniques(l),
  ] },
  { id: "blank", name: ["Livre", "Blank"], blocks: () => [] },
];
function techniques(l: Lang): Block {
  return { type: "checklist", key: "techniques", label: pick(["Técnicas usadas", "Techniques used"], l), value: [],
    options: [pick(["Mobilização articular", "Joint mobilisation"], l), pick(["Tecido mole", "Soft tissue"], l), pick(["Exercício terapêutico", "Therapeutic exercise"], l),
      pick(["Educação", "Education"], l), "Taping", pick(["Agulhamento seco", "Dry needling"], l)] };
}
/** One-line summary of a note's blocks, for the list of earlier notes. */
export const blockSummary = (blocks: Block[]) => blocks.map((b) =>
  b.type === "slider" ? (b.value == null ? "" : `${b.label.replace(/ \(.*\)$/, "")} ${b.value}${b.unit ?? ""}`)
    : b.type === "checklist" ? (b.value.length ? b.value.join(", ") : "")
      : b.value ? `${b.label}: ${b.value}` : "").filter(Boolean).join(" · ");
