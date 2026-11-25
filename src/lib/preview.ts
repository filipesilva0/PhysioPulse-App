// Browser preview only: an in-memory stand-in for the phase 2 server (patients, schedule, alerts).
// It follows the same rules as the real server where they matter for the screens: reception gets the
// reduced patient file, signed notes are locked, overlaps answer 409 "overlap", series repeat weekly.
// Nothing here runs in the desktop app; nothing is saved (reloading the page starts again).
import { FORMS, scoreForm, type Answers } from "./forms";
import type {
  Anamnesis, Appointment, BookingRequest, ConsentKind, ConsentText, Note, PatientRec, Prescription, TestResult, Waiting,
} from "./data";
import { previewAgenda, reopenRequest } from "./previewAgenda";
import { add as addDoc, consentPdf, docs as pdocs, docsFor, dropDocs, miniPdf, notePdf, previewDocs, seedDocs } from "./previewDocs";
import { addWorkingDays, workingDaysBetween } from "./workdays";
import { previewDeviceAlert } from "./previewDevices";
import { nifOk, patientNifOk } from "./validate";

/** A valid NIF for sample patient i (2xxxxxxx + check digit). */
const sampleNif = (i: number) => {
  for (let c = 0; c < 10; c++) { const n = `2${String(31415 + i * 977).padStart(7, "0")}${c}`; if (nifOk(n)) return n; }
  return null;
};
const tidyNif = (raw: string) => { const n = raw.replace(/[\s.-]/g, "").toUpperCase(); return n.startsWith("PT") && n.length === 11 ? n.slice(2) : n; };
import { moneyAlerts, moneyFacts, moneyOnAttended, moneyOnStatus, moneySeed, paidFor, paymentsOf, preview3, type PreviewState } from "./preview3";

type Role = "owner" | "therapist" | "reception";
type M = (pt: string, en: string) => string;
type Body = Record<string, unknown>;

let role: Role = "owner";
const ME: Record<Role, { id: string; name: string }> = { owner: { id: "fs", name: "Filipe Silva" }, therapist: { id: "ar", name: "Ana Rocha" }, reception: { id: "mc", name: "Marta Costa" } };
export const setPreviewRole = (r: Role) => { role = r; };
export const getPreviewRole = () => role;
/** Phase 4b stand-ins (previewWebsite.ts › previewPortal) need a few facts about a patient. */
export function previewPatientLite(id: string) {
  const p = patients.find((x) => x.id === id && !x.deleted);
  if (!p) return null;
  const age = p.birth_date ? Math.floor((Date.now() - new Date(p.birth_date).getTime()) / (365.25 * 864e5)) : null;
  return { name: p.name, phone: p.phone, email: p.email, lang: p.lang, therapist_id: p.therapist_id, minor: age != null && age < 16 };
}
const clin = () => role !== "reception";

const fail = (status: number, message: string, code?: string, details?: unknown): never => { throw { status, message, code, details }; };
const uid = () => (crypto.randomUUID ? crypto.randomUUID() : Math.random().toString(36).slice(2));
const nowIso = () => new Date().toISOString();
const pad = (n: number) => String(n).padStart(2, "0");
const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const at = (day: Date, h: number, m = 0) => { const d = new Date(day); d.setHours(h, m, 0, 0); return d; };
const addDays = (d: Date, n: number) => { const x = new Date(d); x.setDate(x.getDate() + n); return x; };
const addMin = (d: Date, n: number) => new Date(d.getTime() + n * 60000);

// ───────── seed (relative to today, so the preview always looks current) ─────────

const TH = [{ id: "fs", name: "Filipe Silva", role: "owner" as const }, { id: "ar", name: "Ana Rocha", role: "therapist" as const }];
const thName = (id: string | null) => TH.find((t) => t.id === id)?.name ?? null;

interface PRow extends Omit<PatientRec, "therapist_name"> { anamnesis: Anamnesis | null; deleted?: boolean }
const patients: PRow[] = [];
const notes: (Note & { patient_id: string })[] = [];
const tests: (TestResult & { patient_id: string; created_at: string })[] = [];
const rxs: (Omit<Prescription, "used" | "booked" | "remaining">)[] = [];
const texts: ConsentText[] = [];
const remindersSent: { appointment_id: string; patient_id: string; channel: string; sent_at: string; by: string }[] = [];
const reminderTpl = {
  pt: "Olá {nome}! Lembramos a sua sessão na {clinica} {dia} às {hora}. Se não puder vir, avise-nos, por favor. Obrigado!",
  en: "Hi {name}! A reminder of your session at {clinic} {day} at {time}. If you can’t come, please let us know. Thank you!",
};
const consents: { id: string; patient_id: string; text_id: string; kind: ConsentKind; granted: boolean; signed_by_name: string; signature_png: string; signed_at: string; expires_on: string | null; revoked_at: string | null }[] = [];
const appts: (Omit<Appointment, "therapist_name" | "patient_name" | "patient_number" | "attendees"> & { attendees: { patient_id: string; attended: boolean | null }[] })[] = [];
const waitlist: (Omit<Waiting, "therapist_name">)[] = [];
const requests: (BookingRequest & { appointment_id: string | null })[] = (() => {
  // Two requests from the public website (phase 4a), so Schedule › Booking Requests shows how they look.
  const day = (n: number, h: number) => { const d = new Date(); d.setDate(d.getDate() + n); d.setHours(h, 0, 0, 0); return d.toISOString(); };
  return [
    { id: "req-web1", name: "Joana Ferreira", phone: "912 345 678", email: "", patient_id: null, service: "assessment", preferred_at: day(2, 9), message: "Dor no joelho depois de correr.",
      status: "pending", created_at: day(0, 8), appointment_id: null, source: "website", day_part: "morning", address: "", lang: "pt" },
    { id: "req-web2", name: "Peter Walsh", phone: "", email: "peter.walsh@example.com", patient_id: null, service: "home", preferred_at: day(4, 14), message: "",
      status: "pending", created_at: day(0, 7), appointment_id: null, source: "website", day_part: "afternoon", address: "Rua das Gaivotas 12, Cascais", lang: "en" },
    // Phase 4b: from the patient area — a cancellation already made, and a request for another time.
    { id: "req-por1", name: "Rui Fernandes", phone: "936 112 450", email: "", patient_id: "p2", service: "session", preferred_at: day(3, 8),
      message: "", status: "pending", created_at: day(0, 9), appointment_id: null, source: "portal", day_part: "", address: "", lang: "pt", kind: "cancelled" },
    { id: "req-por2", name: "Joana Martins", phone: "912 345 678", email: "", patient_id: "p1", service: "session", preferred_at: day(5, 14),
      message: "Às terças à tarde dá-me mais jeito.", status: "pending", created_at: day(0, 10), appointment_id: null, source: "portal", day_part: "afternoon", address: "", lang: "pt",
      kind: "move", about_starts_at: day(1, 10) },
  ];
})();
const log: { id: number; at: string; staff_name: string; action: string; subject_type: string | null; subject_id: string | null; device: string; detail: string }[] = [];
let logN = 100;
/** Older sample entries (sign-ins, accounts) under the ones this preview session writes. */
const LOG_SEED: typeof log = [
  { id: 6, at: "2026-11-04T10:02:11Z", staff_name: "Filipe Silva", action: "login.ok", subject_type: null, subject_id: null, device: "MacBook-Filipe", detail: "" },
  { id: 5, at: "2026-11-04T09:58:40Z", staff_name: "Filipe Silva", action: "login.failed", subject_type: null, subject_id: null, device: "MacBook-Filipe", detail: "" },
  { id: 4, at: "2026-11-03T18:30:02Z", staff_name: "Ana Rocha", action: "logout", subject_type: null, subject_id: null, device: "PC-Sala-2", detail: "" },
  { id: 3, at: "2026-11-02T09:12:45Z", staff_name: "Marta Costa", action: "staff.joined", subject_type: "staff", subject_id: "mc", device: "PC-Rececao", detail: "reception" },
  { id: 2, at: "2026-11-02T09:01:10Z", staff_name: "Filipe Silva", action: "invite.created", subject_type: null, subject_id: null, device: "MacBook-Filipe", detail: "reception" },
  { id: 1, at: "2026-10-01T09:00:00Z", staff_name: "Filipe Silva", action: "setup.owner_created", subject_type: null, subject_id: null, device: "MacBook-Filipe", detail: "" },
];
const audit = (action: string, pid: string | null, detail = "") => {
  log.unshift({ id: ++logN, at: nowIso(), staff_name: ME[role].name, action, subject_type: pid ? "patient" : null, subject_id: pid, device: role === "reception" ? "PC-Rececao" : "MacBook-Filipe", detail });
};

const SIG = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

(function seed() {
  const people: [string, string, string, string | null, string][] = [
    ["Joana Martins", "1992-03-14", "912 345 678", "fs", "f"], ["Rui Fernandes", "1974-07-02", "936 112 450", "fs", "m"],
    ["Inês Carvalho", "1999-01-21", "965 870 221", "ar", "f"], ["Manuel Sousa", "1958-10-09", "917 604 338", "fs", "m"],
    ["Beatriz Lopes", "1985-05-30", "924 555 019", "ar", "f"], ["Tiago Pereira", "1996-12-11", "968 300 742", "fs", "m"],
    ["Carla Nunes", "1967-08-25", "912 908 116", "ar", "f"], ["André Gomes", "1981-02-17", "933 271 845", "fs", "m"],
  ];
  const created = addDays(new Date(), -40).toISOString();
  people.forEach(([name, birth, phone, th, sex], i) => patients.push({
    id: "p" + (i + 1), number: i + 1, name, birth_date: birth, sex: sex as "f" | "m", nif: i % 2 ? null : sampleNif(i), sns_number: null,
    phone, email: name.split(" ")[0].normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase() + "@exemplo.pt", address: i === 3 ? "Rua das Flores 12, 2.º Esq., Setúbal" : null,
    guardian_name: null, guardian_phone: null, lang: i === 5 ? "en" : "pt", reminders: "sms", therapist_id: th, admin_note: "", active: true,
    anonymised_at: null, created_at: created, updated_at: created, anamnesis: null,
  }));
  const p1 = patients[0];
  p1.anamnesis = {
    main_complaint: "Dor cervical à direita há 3 meses, irradia para a omoplata.", history: "Início gradual, sem trauma. Episódios semelhantes em 2023.",
    medication: "Ibuprofeno 400 mg em SOS.", medical_history: "Sem antecedentes relevantes.", goals: "Trabalhar 8 horas sem dor e voltar ao ginásio.",
    regions: [{ region: "cervical", symptoms: ["pain", "stiffness"], notes: "Pior ao fim do dia ao computador." }, { region: "shoulder_r", symptoms: ["pain"], notes: "" }],
    red_flags: [], updated_at: created, updated_by_name: "Filipe Silva",
  };
  const t0 = addDays(new Date(), -15);
  [[-15, 6, "Avaliação inicial. NDI 24/50."], [-8, 5, "Mobilização C5–C6 e estabilização profunda."], [-4, 4, "Rotação melhorou 15°. Plano de exercícios atualizado."]].forEach(([d, pain, text], i) => {
    const day = addDays(new Date(), d as number).toISOString();
    notes.push({ id: "n" + i, patient_id: "p1", appointment_id: null, template: "neck",
      blocks: [{ type: "slider", key: "pain", label: "Dor (0–10)", value: pain as number, min: 0, max: 10 }, { type: "slider", key: "rotation", label: "Rotação cervical", value: 50 + i * 10, min: 0, max: 90, step: 5, unit: "°" }],
      text: text as string, created_at: day, updated_at: day, created_by: "fs", created_by_name: "Filipe Silva", signed_at: day, signed_by_name: "Filipe Silva" });
  });
  ([["p1", "ndi", [24, 21, 18, 14], 50, true], ["p1", "nprs", [7, 6, 6, 4], 10, true], ["p1", "psfs", [3, 4, 4, 5], 10, false], ["p5", "spadi", [48, 41, 46], 100, true]] as const)
    .forEach(([pid, code, pts, max, hiw]) => pts.forEach((score, i) => tests.push({
      id: uid(), patient_id: pid, test_code: code, answers: {}, score, max_score: max, higher_is_worse: hiw,
      taken_on: ymd(addDays(t0, i * 4 + (pid === "p5" ? 3 : 0))), recorded_by_name: "Filipe Silva", created_at: nowIso(),
    })));
  rxs.push({ id: "rx1", patient_id: "p1", doctor: "Dr. Paulo Matos", specialty: "Medicina geral e familiar", issued_on: ymd(addDays(new Date(), -20)), sessions_total: 10, valid_until: ymd(addDays(new Date(), 40)), payer: "ADSE", document_id: null });
  rxs.push({ id: "rx2", patient_id: "p3", doctor: "Dra. Sofia Lima", specialty: "Ortopedia", issued_on: ymd(addDays(new Date(), -50)), sessions_total: 20, valid_until: ymd(addDays(new Date(), 10)), payer: "Médis", document_id: null });

  const T: [ConsentKind, string, string, string, string, number | null][] = [
    ["treatment", "Consentimento informado para tratamento", "Informed consent for treatment", "Fui informado(a) sobre a avaliação e o tratamento de fisioterapia propostos, os seus benefícios e riscos, e posso retirar este consentimento a qualquer momento.", "I have been told about the proposed physiotherapy assessment and treatment, its benefits and risks, and I can withdraw this consent at any time.", null],
    ["data", "Tratamento de dados pessoais (RGPD)", "Personal data processing (GDPR)", "Autorizo a clínica a tratar os meus dados pessoais e de saúde para a prestação de cuidados, nos termos do RGPD. Os dados ficam guardados no servidor da clínica.", "I allow the clinic to process my personal and health data to provide care, under the GDPR. The data is kept on the clinic's own server.", null],
    ["media", "Fotografias e vídeos", "Photos and videos", "Autorizo fotografias e vídeos para acompanhar a evolução clínica. Não são partilhados fora da clínica.", "I allow photos and videos to follow my progress. They are not shared outside the clinic.", 12],
    ["marketing", "Mensagens de marketing", "Marketing messages", "Aceito receber novidades e campanhas da clínica por SMS ou email.", "I agree to receive news and offers from the clinic by SMS or email.", null],
    ["guardian", "Encarregado de educação (menores)", "Guardian consent (minors)", "Como encarregado(a) de educação, autorizo a avaliação e o tratamento do(a) menor.", "As the guardian, I allow the assessment and treatment of the minor.", null],
  ];
  T.forEach(([kind, tpt, ten, bpt, ben, vm]) => {
    texts.push({ id: uid(), kind, version: 1, lang: "pt", title: tpt, body: bpt, valid_months: vm, created_at: created, current: true, draft: false, approved_at: created });
    texts.push({ id: uid(), kind, version: 1, lang: "en", title: ten, body: ben, valid_months: vm, created_at: created, current: true, draft: false, approved_at: created });
  });
  // Like a fresh server: the invasive-techniques text arrives as a draft for the owner to review.
  texts.push({ id: uid(), kind: "invasive", version: 1, lang: "pt", title: "Consentimento informado para técnicas invasivas (punção seca)", valid_months: 12, created_at: created, current: false, draft: true, approved_at: null,
    body: "O(a) fisioterapeuta propôs-me técnicas invasivas, em particular punção seca: a introdução de uma agulha fina e estéril, sem medicamento, em pontos do músculo, para reduzir a dor e melhorar a função.\n\nFui informado(a), de forma clara e com tempo para fazer perguntas, sobre:\n\n• o objetivo e como a técnica é feita, com agulhas de uso único;\n• os benefícios esperados;\n• os efeitos frequentes: dor durante a punção, dor muscular nas 24–48 horas seguintes, pequenas nódoas negras;\n• os riscos raros mas graves: desmaio, infeção, lesão de um nervo ou vaso e, junto ao tórax, pneumotórax;\n• as alternativas e o que pode acontecer se não fizer esta técnica.\n\nSei que posso recusar ou pedir para parar a qualquer momento. Aceito a técnica proposta." });
  texts.push({ id: uid(), kind: "invasive", version: 1, lang: "en", title: "Informed consent for invasive techniques (dry needling)", valid_months: 12, created_at: created, current: false, draft: true, approved_at: null,
    body: "The physiotherapist has proposed invasive techniques, in particular dry needling: inserting a thin sterile needle, with no medicine, into points in a muscle to reduce pain and improve function.\n\nIt was explained to me clearly:\n\n• the purpose and how it's done, with single-use needles;\n• the expected benefits;\n• common effects: pain during needling, soreness for 24–48 hours, small bruises;\n• rare but serious risks: fainting, infection, injury to a nerve or vessel and, near the chest, pneumothorax;\n• the alternatives and what may happen without it.\n\nI can refuse or ask to stop at any time. I accept the proposed technique." });
  const tx = (k: ConsentKind) => texts.find((t) => t.kind === k && t.lang === "pt")!;
  const signed = addDays(new Date(), -15);
  patients.forEach((p, i) => {
    if (i === 0) {
      consents.push({ id: uid(), patient_id: p.id, text_id: tx("treatment").id, kind: "treatment", granted: true, signed_by_name: p.name, signature_png: SIG, signed_at: signed.toISOString(), expires_on: null, revoked_at: null });
      consents.push({ id: uid(), patient_id: p.id, text_id: tx("media").id, kind: "media", granted: true, signed_by_name: p.name, signature_png: SIG, signed_at: signed.toISOString(), expires_on: ymd(addDays(new Date(), 20)), revoked_at: null });
    } else if (i !== 3) {
      for (const k of ["treatment", "data"] as ConsentKind[]) consents.push({ id: uid(), patient_id: p.id, text_id: tx(k).id, kind: k, granted: true, signed_by_name: p.name, signature_png: SIG, signed_at: signed.toISOString(), expires_on: null, revoked_at: null });
    }
  });

  // This week and next: a weekly pattern, some done in the past.
  const mon = (() => { const d = new Date(); d.setHours(0, 0, 0, 0); d.setDate(d.getDate() - ((d.getDay() + 6) % 7)); return d; })();
  const W: [number, number, number, number, string, string | null, string?][] = [
    [0, 8, 30, 45, "session", "p2", "fs"], [0, 10, 0, 45, "session", "p1", "fs"], [0, 11, 0, 60, "session", "p3", "ar"], [0, 14, 30, 45, "session", "p7", "ar"],
    [1, 8, 0, 45, "session", "p2", "fs"], [1, 9, 0, 45, "session", "p5", "ar"], [1, 10, 0, 45, "session", "p1", "fs"], [1, 11, 0, 60, "session", "p3", "ar"], [1, 14, 0, 60, "home", "p4", "fs"],
    [1, 16, 30, 45, "session", "p6", "fs"], [1, 18, 0, 60, "class", null, "fs"],
    [2, 9, 0, 60, "session", "p3", "ar"], [2, 10, 30, 45, "assessment", "p8", "fs"], [2, 15, 0, 45, "session", "p5", "ar"], [2, 17, 30, 45, "session", "p2", "fs"],
    [3, 8, 30, 45, "session", "p2", "fs"], [3, 10, 0, 45, "session", "p1", "fs"], [3, 11, 0, 60, "session", "p3", "ar"], [3, 14, 30, 45, "session", "p7", "ar"], [3, 18, 0, 60, "class", null, "fs"],
    [4, 9, 0, 45, "session", "p5", "ar"], [4, 10, 0, 45, "session", "p4", "fs"], [4, 12, 0, 45, "session", "p8", "fs"],
    [5, 9, 30, 60, "class", null, "ar"], [5, 10, 30, 45, "session", "p5", "ar"],
  ];
  const now = new Date();
  for (const wk of [0, 1]) for (const [d, h, m, dur, kind, pid, th] of W) {
    const s = at(addDays(mon, d + wk * 7), h, m);
    const isClass = kind === "class";
    appts.push({
      id: uid(), kind: kind as Appointment["kind"], starts_at: s.toISOString(), ends_at: addMin(s, dur).toISOString(),
      status: addMin(s, dur) < now ? "done" : "booked", therapist_id: th ?? null, patient_id: pid, series_id: null,
      prescription_id: pid === "p1" ? "rx1" : pid === "p3" ? "rx2" : null, title: isClass ? "Pilates clínico" : "", color: "",
      address: kind === "home" ? "Rua das Flores 12, Setúbal" : "", travel_minutes: kind === "home" ? 20 : 0, capacity: isClass ? 6 : null, admin_note: "",
      attendees: isClass ? [{ patient_id: "p7", attended: null }, { patient_id: "p8", attended: null }, { patient_id: "p6", attended: null }, { patient_id: "p2", attended: null }] : [],
    });
  }
  for (const [d, h, dur, th, title] of [[2, 12, 120, "fs", "training"], [4, 16, 180, "ar", "holiday"]] as const) {
    const s = at(addDays(mon, d), h);
    appts.push({ id: uid(), kind: "block", starts_at: s.toISOString(), ends_at: addMin(s, dur).toISOString(), status: "booked", therapist_id: th, patient_id: null, series_id: null, prescription_id: null, title, color: "", address: "", travel_minutes: 0, capacity: null, admin_note: "", attendees: [] });
  }
  // A sample patient's prescription has a few sessions done already.
  waitlist.push({ id: uid(), patient_id: "p2", name: "", phone: "", email: "", therapist_id: null, weekdays: [1, 2, 3], from_time: "08:00:00", to_time: "12:00:00", note: "", status: "waiting", created_at: addDays(new Date(), -3).toISOString() });
  waitlist.push({ id: uid(), patient_id: null, name: "Helena Duarte", phone: "961 222 333", email: "", therapist_id: "ar", weekdays: [], from_time: null, to_time: null, note: "Ombro", status: "waiting", created_at: addDays(new Date(), -1).toISOString() });
  requests.push({ id: uid(), name: "Sofia Ramos", phone: "913 456 789", email: "", patient_id: null, service: "assessment", preferred_at: at(addDays(new Date(), 2), 17).toISOString(), message: "Dor lombar há 2 semanas.", status: "pending", created_at: nowIso(), appointment_id: null });
  requests.push({ id: uid(), name: "Pedro Alves", phone: "", email: "pedro@exemplo.pt", patient_id: null, service: "session", preferred_at: at(addDays(new Date(), 3), 9).toISOString(), message: "Joelho.", status: "pending", created_at: nowIso(), appointment_id: null });
  log.push(
    { id: 6, at: addDays(new Date(), -1).toISOString(), staff_name: "Filipe Silva", action: "patient.viewed", subject_type: "patient", subject_id: "p1", device: "MacBook-Filipe", detail: "full" },
    { id: 5, at: addDays(new Date(), -4).toISOString(), staff_name: "Filipe Silva", action: "note.signed", subject_type: "patient", subject_id: "p1", device: "MacBook-Filipe", detail: "" },
    { id: 4, at: addDays(new Date(), -15).toISOString(), staff_name: "Marta Costa", action: "consent.signed", subject_type: "patient", subject_id: "p1", device: "PC-Rececao", detail: "treatment granted" },
    { id: 3, at: created, staff_name: "Marta Costa", action: "patient.created", subject_type: "patient", subject_id: "p1", device: "PC-Rececao", detail: "" },
  );
  const scan = seedDocs((n) => addDays(new Date(), n).toISOString());
  // Consents signed before: their PDFs are in the archive too.
  for (const c of consents) consentPdf(c, texts.find((t) => t.id === c.text_id)!, patients.find((p) => p.id === c.patient_id)!, { id: "mc", name: "Marta Costa" }, "signed");
  rxs[0].document_id = scan;
  for (const n of notes) if (n.signed_at) notePdf(n, patients.find((x) => x.id === n.patient_id)!, { id: "fs", name: "Filipe Silva" });
})();

// Phase 3 (money) stand-in shares this state.
const S3: PreviewState = { role: () => role, me: () => ME[role], patients, appts, audit };
moneySeed(S3);

// ───────── helpers ─────────

const P = (id: string) => patients.find((p) => p.id === id && !p.deleted) ?? fail(404, "That item doesn’t exist or was removed.");
const pubPatient = (p: PRow): PatientRec => { const { anamnesis: _a, deleted: _d, ...rest } = p; return { ...rest, therapist_name: thName(p.therapist_id) }; };
const rxUse = (r: (typeof rxs)[number]): Prescription => {
  const used = appts.filter((a) => a.prescription_id === r.id && a.status === "done").length;
  const booked = appts.filter((a) => a.prescription_id === r.id && a.status === "booked").length;
  return { ...r, used, booked, remaining: r.sessions_total - used };
};
const docsAll = (pid: string) => pdocs.filter((d) => d.patient_id === pid);
const consentState = (c: (typeof consents)[number]) =>
  c.revoked_at ? "revoked" : c.expires_on && c.expires_on < ymd(new Date()) ? "expired" : !c.granted ? "refused" : "valid";
const apptOut = (a: (typeof appts)[number]): Appointment => {
  const p = a.patient_id ? patients.find((x) => x.id === a.patient_id) : undefined;
  return { ...a, therapist_name: thName(a.therapist_id), patient_name: p?.name ?? null, patient_number: p?.number ?? null, paid_cents: paidFor(a.id, a.kind === "class" ? null : a.patient_id),
    attendees: a.kind === "class" ? a.attendees.map((x) => ({ ...x, name: P(x.patient_id).name })).sort((x, y) => x.name.localeCompare(y.name)) : null };
};
const overlapsOf = (ids: string[]) => {
  const out: unknown[] = [];
  for (const id of ids) {
    const a = appts.find((x) => x.id === id)!;
    if (a.kind === "block" || a.status !== "booked") continue;
    const s = addMin(new Date(a.starts_at), -a.travel_minutes), e = addMin(new Date(a.ends_at), a.travel_minutes);
    for (const b of appts) {
      if (b.id === a.id || ids.includes(b.id) || b.therapist_id !== a.therapist_id || b.status !== "booked") continue;
      const bs = addMin(new Date(b.starts_at), -b.travel_minutes), be = addMin(new Date(b.ends_at), b.travel_minutes);
      if (bs < e && s < be) out.push({ id: a.id, clashes_with: b.id, starts_at: b.starts_at, ends_at: b.ends_at, name: b.kind === "block" ? b.title : P(b.patient_id ?? "p1").name, therapist_name: thName(b.therapist_id) });
    }
  }
  return out;
};
const q = (path: string) => new URLSearchParams(path.split("?")[1] ?? "");
const s = (v: unknown) => (typeof v === "string" ? v.trim() : v == null ? "" : String(v));

function createAppt(b: Body, m: M) {
  const kind = s(b.kind) as Appointment["kind"];
  if (!["session", "assessment", "class", "home", "block"].includes(kind)) fail(400, m("Escolha o tipo de marcação.", "Choose the type of appointment."));
  if (["session", "assessment", "home"].includes(kind) && !b.patient_id) fail(400, m("Escolha o utente.", "Choose the patient."));
  if (kind !== "block" && !b.therapist_id) fail(400, m("Escolha o fisioterapeuta.", "Choose the therapist."));
  if (kind === "class" && !(Number(b.capacity) > 0)) fail(400, m("Indique quantas pessoas cabem na aula.", "Enter how many people fit in the class."));
  const start = new Date(s(b.starts_at)), end = new Date(s(b.ends_at));
  if (!(end > start) || end.getTime() - start.getTime() > 864e5) fail(400, m("O fim tem de ser depois do início, no mesmo dia.", "The end must be after the start, on the same day."));
  const rep = b.repeat as { weekdays: number[]; until?: string; count?: number } | undefined;
  let times: [Date, Date][] = [[start, end]];
  let series: string | null = null;
  if (rep) {
    if (!rep.weekdays?.length) fail(400, m("Escolha os dias da semana em que se repete.", "Choose the days of the week to repeat on."));
    if (!rep.until && !rep.count) fail(400, m("Escolha uma data final ou um número de sessões.", "Choose an end date or a number of sessions."));
    times = []; series = uid();
    const lim = Math.min(rep.count ?? 200, 200), until = rep.until ? new Date(rep.until + "T23:59:59") : addDays(start, 366);
    for (let d = new Date(start); d <= until && times.length < lim; d = addDays(d, 1)) {
      const iso = ((d.getDay() + 6) % 7) + 1;
      if (rep.weekdays.includes(iso) || ymd(d) === ymd(start)) times.push([new Date(d), addMin(d, (end.getTime() - start.getTime()) / 60000)]);
    }
  }
  const ids = times.map(([st, en]) => {
    const id = uid();
    appts.push({ id, kind, starts_at: st.toISOString(), ends_at: en.toISOString(), status: "booked", therapist_id: (b.therapist_id as string) ?? null,
      patient_id: (b.patient_id as string) || null, series_id: series, prescription_id: (b.prescription_id as string) || null, title: s(b.title), color: s(b.color),
      address: s(b.address), travel_minutes: Number(b.travel_minutes) || 0, capacity: kind === "class" ? Number(b.capacity) : null, admin_note: s(b.admin_note), attendees: [] });
    return id;
  });
  const o = overlapsOf(ids);
  if (o.length && !b.allow_overlap && kind !== "block") {
    for (const id of ids) appts.splice(appts.findIndex((x) => x.id === id), 1);
    fail(409, m("Sobrepõe-se a outra marcação do mesmo fisioterapeuta.", "This overlaps another appointment for the same therapist."), "overlap", o);
  }
  audit("appointment.created", (b.patient_id as string) || null, `${kind} x${ids.length}`);
  return { ids, series_id: series, overlaps: o };
}

const waitMatches = (a: (typeof appts)[number]) => {
  if (!["session", "assessment"].includes(a.kind) || new Date(a.starts_at) < new Date()) return [];
  const st = new Date(a.starts_at), en = new Date(a.ends_at), iso = ((st.getDay() + 6) % 7) + 1;
  const t = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}:00`;
  return waitlist.filter((w) => w.status === "waiting" && (!w.therapist_id || w.therapist_id === a.therapist_id) && (!w.weekdays.length || w.weekdays.includes(iso))
    && (!w.from_time || t(st) >= w.from_time) && (!w.to_time || t(en) <= w.to_time) && w.patient_id !== a.patient_id)
    .map((w) => ({ id: w.id, patient_id: w.patient_id, name: w.patient_id ? P(w.patient_id).name : w.name, phone: w.patient_id ? P(w.patient_id).phone : w.phone }));
};

// ───────── legal tools (same rules as server/src/routes/legal.rs) ─────────

const sameName = (a: string, b: string) => { const n = (x: string) => x.trim().split(/\s+/).join(" ").toLowerCase(); return a.trim() !== "" && n(a) === n(b); };

/** Removes what identifies the person; clinical content stays. */
function scrub(r: PRow, name: string) {
  Object.assign(r, { name, nif: null, sns_number: null, phone: null, email: null, address: null, guardian_name: null, guardian_phone: null, admin_note: "",
    reminders: "none", active: false, birth_date: r.birth_date ? `${r.birth_date.slice(0, 4)}-01-01` : null, anonymised_at: r.anonymised_at ?? nowIso(), updated_at: nowIso() });
  for (const c of consents) if (c.patient_id === r.id) { c.signed_by_name = ""; c.signature_png = ""; }
  dropDocs(r.id);
  const now = nowIso();
  for (const a of appts) {
    if (a.patient_id === r.id && a.status === "booked" && a.starts_at > now) a.status = "cancelled";
    if (a.status === "booked" && a.starts_at > now) a.attendees = a.attendees.filter((x) => x.patient_id !== r.id);
  }
  for (let i = waitlist.length - 1; i >= 0; i--) if (waitlist[i].patient_id === r.id) waitlist.splice(i, 1);
  for (let i = requests.length - 1; i >= 0; i--) if (requests[i].patient_id === r.id) requests.splice(i, 1);
}
const dropWhere = <T,>(arr: T[], f: (x: T) => boolean) => { for (let i = arr.length - 1; i >= 0; i--) if (f(arr[i])) arr.splice(i, 1); };

// ───────── complaints log and delete a day (same rules as server/src/routes/complaints.rs and days.rs) ─────────

type ComplaintRow = { id: string; received_on: string; source: "paper" | "electronic" | "other"; book_ref: string; patient_id: string | null; complainant: string;
  summary: string; reply: string; replied_on: string | null; sent_to_ers_on: string | null; due_on: string; created_at: string; updated_at: string };
const complaints: ComplaintRow[] = [];
{
  const t = new Date(), d = (n: number) => ymd(addDays(t, n));
  const mk = (rec: string, source: ComplaintRow["source"], who: string, summary: string, extra: Partial<ComplaintRow> = {}) =>
    complaints.push({ id: uid(), received_on: rec, source, book_ref: source === "paper" ? `Folha ${complaints.length + 12}` : "", patient_id: null, complainant: who, summary,
      reply: "", replied_on: null, sent_to_ers_on: null, due_on: addWorkingDays(rec, 10), created_at: nowIso(), updated_at: nowIso(), ...extra });
  mk(d(-11), "paper", "Rui Almeida", "Esperou 40 minutos para ser atendido sem aviso.");
  mk(d(-3), "electronic", "Carla Nunes", "Fatura enviada com o NIF errado.");
  mk(d(-160), "paper", "Joana Faria", "Marcação desmarcada no próprio dia.", { reply: "Pedimos desculpa; a terapeuta adoeceu. Oferecemos a sessão seguinte.", replied_on: d(-155), sent_to_ers_on: d(-152) });
}

function previewClinic(method: string, seg: string[], b: Body, m: M): unknown {
  if (seg[0] !== "complaints" && seg[0] !== "days") return undefined;
  if (role !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
  const logIt = (action: string, sid: string | null, detail = "") =>
    log.unshift({ id: ++logN, at: nowIso(), staff_name: ME[role].name, action, subject_type: seg[0] === "days" ? "day" : "complaint", subject_id: sid, device: "MacBook-Filipe", detail });
  if (seg[0] === "complaints") {
    const pick = (x: ComplaintRow) => ({ ...x, patient_name: x.patient_id ? patients.find((p) => p.id === x.patient_id && !p.deleted)?.name ?? null : null,
      patient_number: x.patient_id ? patients.find((p) => p.id === x.patient_id)?.number ?? null : null, created_by_name: "Filipe Silva" });
    const fields = (x: ComplaintRow) => {
      for (const k of ["source", "book_ref", "patient_id", "complainant", "summary", "reply", "replied_on", "sent_to_ers_on"] as const) if (k in b) (x as Record<string, unknown>)[k] = b[k] === "" ? null : b[k];
      if (typeof b.received_on === "string") { x.received_on = b.received_on; x.due_on = addWorkingDays(b.received_on, 10); }
      x.updated_at = nowIso();
    };
    if (method === "GET" && seg.length === 1) return complaints.map(pick).sort((a, c) => c.received_on.localeCompare(a.received_on));
    if (method === "POST" && seg.length === 1) {
      if (typeof b.received_on !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(b.received_on)) fail(400, m("Indique a data em que a reclamação foi recebida.", "Enter the date the complaint was received."));
      const x: ComplaintRow = { id: uid(), received_on: "", source: "other", book_ref: "", patient_id: null, complainant: "", summary: "", reply: "", replied_on: null, sent_to_ers_on: null, due_on: "", created_at: nowIso(), updated_at: nowIso() };
      fields(x); complaints.push(x); logIt("complaint.created", x.id);
      return { id: x.id, due_on: x.due_on };
    }
    const x = complaints.find((c) => c.id === seg[1]) ?? fail(404, "That item doesn’t exist or was removed.");
    if (method === "PATCH") { fields(x); logIt("complaint.updated", x.id); return { ok: true }; }
    if (method === "DELETE") { complaints.splice(complaints.indexOf(x), 1); logIt("complaint.deleted", x.id); return { ok: true }; }
    return undefined;
  }
  const day = seg[1] ?? "";
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) fail(400, m("Escolha um dia.", "Choose a day."));
  const onDay = (iso: string) => ymd(new Date(iso)) === day;
  const A = appts.filter((a) => onDay(a.starts_at)), N = notes.filter((n) => onDay(n.created_at)), T = tests.filter((t) => t.taken_on === day);
  if (method === "GET" && seg.length === 2) {
    const pats = new Set([...A.flatMap((a) => [a.patient_id, ...a.attendees.map((x) => x.patient_id)]), ...N.map((n) => n.patient_id), ...T.map((t) => t.patient_id)].filter(Boolean));
    return { day, appointments: A.filter((a) => a.kind !== "block").length, blocks: A.filter((a) => a.kind === "block").length, attendees: A.reduce((k, a) => k + a.attendees.length, 0),
      notes_signed: N.filter((n) => n.signed_at).length, notes_draft: N.filter((n) => !n.signed_at).length, tests: T.length, documents: 0, payments_kept: 0, patients: pats.size };
  }
  if (method === "POST" && seg[2] === "delete") {
    if (b.confirm !== day) fail(400, m("O dia não corresponde. Escolha o dia outra vez.", "The day doesn’t match. Choose the day again."));
    dropWhere(appts, (a) => onDay(a.starts_at)); dropWhere(notes, (n) => onDay(n.created_at)); dropWhere(tests, (t) => t.taken_on === day);
    logIt("day.deleted", day, `${A.length} appointments, ${N.length} notes, ${T.length} tests, 0 documents`);
    return { ok: true, day, appointments: A.length, notes: N.length, tests: T.length, documents: 0, files_not_removed: 0 };
  }
  return undefined;
}

function previewLegal(method: string, seg: string[], b: Body, m: M): unknown {
  if (seg[0] !== "patients" || !seg[1]) return undefined;
  const id = seg[1];
  const owner = () => { if (role !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this.")); };
  const wrongName = () => fail(400, m("O nome não corresponde. Escreva o nome do utente tal como aparece na ficha.", "The name doesn’t match. Type the patient’s name exactly as it appears in the file."));
  if (method === "GET" && seg[2] === "export" && seg.length === 4) {
    if (!clin()) fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
    if (!["full", "summary"].includes(seg[3])) fail(400, "Choose a full or summary export.");
    P(id); audit("patient.exported", id, seg[3]);
    return { ok: true };
  }
  if (method === "GET" && seg[2] === "legal" && seg.length === 3) {
    owner(); const r = P(id); const now = nowIso();
    return { number: r.number, name: r.name, anonymised: !!r.anonymised_at,
      notes_signed: notes.filter((n) => n.patient_id === id && n.signed_at).length, notes_draft: notes.filter((n) => n.patient_id === id && !n.signed_at).length,
      documents: docsFor(id, true).length, tests: tests.filter((t) => t.patient_id === id).length, consents: consents.filter((c) => c.patient_id === id).length,
      upcoming: appts.filter((a) => a.patient_id === id && a.status === "booked" && a.starts_at > now).length,
      visits: appts.filter((a) => a.patient_id === id && a.status === "done").length, plans_active: 0, ...moneyFacts(S3, id),
      last_contact: [...appts.filter((a) => a.patient_id === id && (a.status === "done" || a.status === "no_show")).map((a) => ymd(new Date(a.starts_at))),
        ...notes.filter((n) => n.patient_id === id).map((n) => ymd(new Date(n.created_at))), ...tests.filter((t) => t.patient_id === id).map((t) => t.taken_on)].sort().pop() ?? null };
  }
  if (method === "POST" && seg[2] === "anonymise" && seg.length === 3) {
    owner(); const r = P(id);
    if (r.anonymised_at) fail(409, m("Este utente já foi anonimizado.", "This patient is already anonymised."));
    if (!sameName(s(b.confirm_name), r.name)) wrongName();
    scrub(r, "Utente anonimizado"); audit("patient.anonymised", id);
    return { ok: true, number: r.number, folder_removed: true };
  }
  if (method === "POST" && seg[2] === "delete" && seg.length === 3) {
    owner(); const r = P(id);
    if (!sameName(s(b.confirm_name), r.name)) wrongName();
    const kept = moneyFacts(S3, id).payments;
    scrub(r, "Utente apagado");
    dropWhere(notes, (x) => x.patient_id === id); dropWhere(tests, (x) => x.patient_id === id); dropWhere(consents, (x) => x.patient_id === id);
    dropWhere(rxs, (x) => x.patient_id === id); dropWhere(appts, (x) => x.patient_id === id);
    for (const a of appts) a.attendees = a.attendees.filter((x) => x.patient_id !== id);
    r.anamnesis = null; r.deleted = true; r.birth_date = null; r.sex = null; r.therapist_id = null;
    if (!kept) patients.splice(patients.indexOf(r), 1);
    audit("patient.deleted", id, kept ? `${kept} payments kept without a name` : "removed");
    return { ok: true, number: r.number, payments_kept: kept, folder_removed: true };
  }
  return undefined;
}


// ───────── reimbursement pack and discharge letters (same rules as server/src/routes/letters.rs) ─────────

type LetterRow = { id: string; patient_id: string; lang: "pt" | "en"; addressee: string; body: string; ai_drafted: boolean; created_by: string; created_at: string; updated_at: string; approved_by: string | null; approved_at: string | null; document_id: string | null };
const aiConf: { url: string | null; model: string | null; speech?: string | null; assistant?: string | null } = { url: null, model: null, speech: null, assistant: null };
const letters: LetterRow[] = [];
const proIds: Record<string, string> = { fs: "", ar: "" };
const dmy = (iso: string) => { const d = iso.length <= 10 ? new Date(iso + "T12:00:00") : new Date(iso); return `${pad(d.getDate())}/${pad(d.getMonth() + 1)}/${d.getFullYear()}`; };
const REG_PT: Record<string, string> = { cervical: "coluna cervical", thoracic: "coluna dorsal", lumbar: "coluna lombar", shoulder_r: "ombro direito", shoulder_l: "ombro esquerdo", knee_r: "joelho direito", knee_l: "joelho esquerdo", hip: "anca", ankle_foot: "tornozelo e pé", wrist_hand: "punho e mão" };
const SYM_PT: Record<string, string> = { pain: "dor", stiffness: "rigidez", weakness: "fraqueza", tingling: "formigueiro", numbness: "dormência", swelling: "edema", instability: "instabilidade", cramps: "cãibras" };
const andList = (a: string[], en: boolean) => a.length <= 1 ? (a[0] ?? "") : `${a.slice(0, -1).join(", ")} ${en ? "and" : "e"} ${a[a.length - 1]}`;

function facts(pid: string, from?: string, to?: string) {
  const r = P(pid);
  if (r.anonymised_at) fail(404, "Not found.");
  const visits = appts.filter((a) => a.kind !== "block" && ((a.patient_id === pid && a.status === "done") || a.attendees.some((x) => x.patient_id === pid && x.attended)))
    .filter((a) => (!from || a.starts_at.slice(0, 10) >= from) && (!to || a.starts_at.slice(0, 10) <= to)).sort((a, b) => a.starts_at.localeCompare(b.starts_at));
  const techs: Record<string, number> = {};
  for (const n of notes.filter((x) => x.patient_id === pid && x.signed_at)) for (const b of n.blocks as { type: string; value: unknown }[])
    if (b.type === "checklist" && Array.isArray(b.value)) for (const t of b.value as string[]) techs[t.toLowerCase()] = (techs[t.toLowerCase()] ?? 0) + 1;
  const codes = [...new Set(tests.filter((t) => t.patient_id === pid).map((t) => t.test_code))];
  const changes = codes.map((c) => {
    const rs = tests.filter((t) => t.patient_id === pid && t.test_code === c).sort((a, b) => a.taken_on.localeCompare(b.taken_on));
    const sc = (t: (typeof rs)[number]) => `${t.score}${t.max_score ? `/${t.max_score}` : ""}`;
    const [f, l] = [rs[0], rs[rs.length - 1]];
    return rs.length > 1 ? `${c.toUpperCase()}: de ${sc(f)} para ${sc(l)} (${dmy(f.taken_on)} – ${dmy(l.taken_on)})` : `${c.toUpperCase()}: ${sc(f)} (${dmy(f.taken_on)})`;
  });
  const complaints = (r.anamnesis?.regions ?? []).map((x) => `${andList(x.symptoms.map((y) => SYM_PT[y] ?? y), false)} — ${REG_PT[x.region] ?? x.region}`).join("; ");
  return { r, visits, techs: Object.entries(techs).sort((a, b) => b[1] - a[1]).slice(0, 8).map((x) => x[0]), changes, complaints, rx: rxs.filter((x) => x.patient_id === pid).sort((a, b) => b.issued_on.localeCompare(a.issued_on)) };
}

function letterTemplate(pid: string, en: boolean) {
  const f = facts(pid);
  const todo = en ? "[to complete]" : "[a completar]";
  const age = f.r.birth_date ? Math.floor((Date.now() - new Date(f.r.birth_date).getTime()) / 31557600000) : null;
  const first = f.visits[0], last = f.visits[f.visits.length - 1];
  const n = f.visits.length;
  let body = en
    ? `I am writing to let you know that ${f.r.name}${age ? `, aged ${age},` : ""} attended physiotherapy at this clinic${n ? ` from ${dmy(first.starts_at)} to ${dmy(last.starts_at)}, ${n} session${n === 1 ? "" : "s"} in total` : ""}.`
    : `Venho por este meio informar que ${f.r.name}${age ? `, de ${age} anos,` : ""} foi ${f.r.sex === "f" ? "acompanhada" : f.r.sex === "m" ? "acompanhado" : "acompanhado(a)"} em fisioterapia nesta clínica${n ? ` entre ${dmy(first.starts_at)} e ${dmy(last.starts_at)}, num total de ${n} ${n === 1 ? "sessão" : "sessões"}` : ""}.`;
  const reason = f.r.anamnesis?.main_complaint?.trim().replace(/\.+$/, "");
  body += `\n\n${en ? "Reason for referral" : "Motivo da referenciação"}: ${reason || todo}.`;
  if (f.complaints && !en) body += ` Na avaliação inicial: ${f.complaints}.`;
  body += `\n\n${en ? "Treatment included" : "O tratamento incluiu"} ${f.techs.length ? andList(f.techs, en) : todo}.`;
  if (f.changes.length) body += `\n\n${en ? "Outcome measures" : "Evolução nos testes"}:\n• ${f.changes.join("\n• ")}`;
  body += en ? `\n\nAt discharge: ${todo}.\n\nRecommendations: ${todo}.` : `\n\nÀ data da alta: ${todo}.\n\nRecomendações: ${todo}.`;
  const doc = f.rx[0]?.doctor ?? "";
  const titled = /^(Dr|Prof)/.test(doc);
  const addressee = doc ? (en ? `To ${titled ? "" : "Dr "}${doc}` : `Ao(À) Exmo.(a) Sr.(a) ${titled ? "" : "Dr.(a) "}${doc}`) : en ? "To the attending physician" : "Ao(À) médico(a) assistente";
  return { lang: en ? "en" : "pt", addressee, body };
}

function previewLetters(method: string, seg: string[], path: string, b: Body, m: M): unknown {
  const forbid = () => fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
  const needClin = () => { if (!clin()) forbid(); };
  const owner = () => { if (role !== "owner") forbid(); };
  const qs = q(path);
  // ── pack ──
  if (seg[0] === "patients" && seg[2] === "reimbursement" && seg.length === 3) {
    needClin();
    const pid = seg[1];
    const f0 = facts(pid);
    const rxId = method === "GET" ? qs.get("prescription_id") : (b.prescription_id as string | null);
    const rx = rxId ? f0.rx.find((x) => x.id === rxId) ?? fail(404, "Not found.") : undefined;
    const from = (method === "GET" ? qs.get("from") : s(b.from)) || rx?.issued_on || ymd(addDays(new Date(), -91));
    const to = (method === "GET" ? qs.get("to") : s(b.to)) || ymd(new Date());
    const f = facts(pid, from, to);
    const pays = paymentsOf(pid).filter((p) => p.received_on >= from && p.received_on <= to);
    if (method === "GET") {
      audit("patient.viewed", pid, "reimbursement");
      const all = f0.visits;
      let report = `${f.r.name} realizou ${all.length} ${all.length === 1 ? "sessão" : "sessões"} de fisioterapia nesta clínica${all.length ? ` entre ${dmy(all[0].starts_at)} e ${dmy(all[all.length - 1].starts_at)}` : ""}${rx ? `, por prescrição de ${[rx.doctor, rx.specialty].filter(Boolean).join(", ")} de ${dmy(rx.issued_on)}` : ""}.`;
      if (f.r.anamnesis?.main_complaint) report += `\n\nMotivo: ${f.r.anamnesis.main_complaint.trim().replace(/\.+$/, "")}.`;
      if (f.complaints) report += ` Na avaliação inicial: ${f.complaints}.`;
      if (f.techs.length) report += `\n\nTratamento: ${andList(f.techs, false)}.`;
      if (f.changes.length) report += `\n\nEvolução:\n• ${f.changes.join("\n• ")}`;
      return {
        from, to, prescription_id: rx?.id ?? null, payer: rx?.payer ?? "", signer: ME[role].name, professional_id: proIds[ME[role].id] ?? "",
        prescriptions: f0.rx.map((x) => ({ id: x.id, doctor: x.doctor, specialty: x.specialty, payer: x.payer, issued: dmy(x.issued_on), issued_iso: x.issued_on, total: x.sessions_total, used: rxUse(x).used, document_id: x.document_id })),
        sessions: f.visits.map((a) => ({ id: a.id, on: dmy(a.starts_at), time: (() => { const d = new Date(a.starts_at); return `${pad(d.getHours())}:${pad(d.getMinutes())}`; })(), kind: a.kind, title: a.title, therapist: thName(a.therapist_id), minutes: Math.round((+new Date(a.ends_at) - +new Date(a.starts_at)) / 60000) })),
        payments: pays.map((p) => ({ id: p.id, number: p.number, received_on: p.received_on, amount_cents: p.amount_cents, description: p.description, invoice_number: p.invoice_number, has_file: !!p.invoice_number, file_mime: p.invoice_number ? "application/pdf" : null })),
        report,
      };
    }
    if (method === "POST") {
      if (!s(b.report)) fail(400, m("Escreva o relatório clínico.", "Write the short clinical report."));
      proIds[ME[role].id] = s(b.professional_id);
      const ids = (b.payment_ids as string[]) ?? [];
      const chosen = paymentsOf(pid).filter((p) => ids.includes(p.id));
      if (chosen.length !== ids.length) fail(400, m("Alguns pagamentos não são deste utente.", "Some payments aren’t this patient’s or were voided. Refresh and try again."));
      const payer = s(b.payer);
      const lines = [{ t: "Pedido de reembolso — fisioterapia", size: 16, bold: true }, { t: `Utente: ${f.r.name} (n.º ${f.r.number})`, gap: 8 },
        ...(payer ? [{ t: `Entidade: ${payer}` }] : []), ...(rx ? [{ t: `Prescrição: ${rx.doctor}, ${dmy(rx.issued_on)} (${rx.sessions_total} sessões)` }] : []),
        { t: `Sessões realizadas de ${dmy(from)} a ${dmy(to)}: ${f.visits.length}`, bold: true, gap: 8 },
        { t: `Faturas: ${chosen.length} · ${(chosen.reduce((t, p) => t + p.amount_cents, 0) / 100).toFixed(2).replace(".", ",")} €`, bold: true, gap: 4 },
        ...chosen.map((p) => ({ t: `${dmy(p.received_on)} · ${p.invoice_number || `Recibo interno n.º ${p.number}`} · ${(p.amount_cents / 100).toFixed(2).replace(".", ",")} €` })),
        { t: "Relatório clínico", bold: true, gap: 8 }, ...s(b.report).split("\n").filter(Boolean).flatMap((x) => (x.match(/.{1,95}(\s|$)/g) ?? [x]).map((t) => ({ t: t.trim() }))),
        { t: `${ME[role].name}, fisioterapeuta${proIds[ME[role].id] ? ` — cédula profissional n.º ${proIds[ME[role].id]}` : ""}`, gap: 12 },
        { t: "(Pré-visualização: no servidor, a prescrição e cada fatura vêm nas páginas seguintes.)", gap: 8 }];
      const filename = `${ymd(new Date())} Pacote de reembolso${payer ? ` ${payer}` : ""}.pdf`;
      const d = addDoc({ patient_id: pid, kind: "report", filename, mime: "application/pdf", uploaded_at: nowIso(), uploaded_by: ME[role].id, uploaded_by_name: ME[role].name, data: miniPdf(lines) });
      audit("patient.reimbursement_pack", pid, `${chosen.length} invoices`);
      const skipped = chosen.filter((p) => !p.invoice_number).map((p) => ({ what: `Recibo interno n.º ${p.number}`, reason: "no_file" }));
      return { document_id: d.id, filename, pages: 1 + (rx?.document_id ? 1 : 0) + chosen.length - skipped.length, attached: [], skipped };
    }
  }
  // ── letters ──
  // Local AI: address and model set in Settings › Local AI (server: ai.rs, letters.rs ai_settings/ai_test).
  if (seg[0] === "ai") {
    // Status for anyone with AI on (the owner and therapists switched on); changing the settings: the owner.
    // The assistant itself is for anyone whose AI switch is on (previewAgenda checks it below).
    if (!(method === "GET" && seg[1] === "status" && role === "therapist") && !(seg[1] === "assistant" && role === "therapist")) owner();
    const norm = (u: string): string | null => {
      const t = u.trim().replace(/\/+$/, "").replace(/^http:\/\//, "");
      if (!t) return null;
      const hp = /:\d+$/.test(t) ? t : `${t}:11434`;
      const host = hp.replace(/:\d+$/, "");
      const o = host.split(".").map(Number);
      const local = host === "ollama" || host === "localhost" || (o.length === 4 && o.every((n) => n >= 0 && n < 256) && (o[0] === 10 || o[0] === 127 || (o[0] === 192 && o[1] === 168) || (o[0] === 172 && o[1] >= 16 && o[1] < 32) || (o[0] === 100 && o[1] >= 64 && o[1] < 128)));
      return local && !t.includes("/") ? `http://${hp}` : null;
    };
    const fakeCheck = (url: string, model: string) => url.includes("100.86.97.21")
      ? { available: ["gemma4:12b", "qwen3:8b", "qwen3:14b"].includes(model), reason: ["gemma4:12b", "qwen3:8b", "qwen3:14b"].includes(model) ? undefined : "no_model", url, model, models: ["gemma4:12b", "qwen3:8b", "qwen3:14b"] }
      : { available: false, reason: "offline", url, model, models: [] };
    const input = () => {
      const u = s(b.url).trim(), mdl = s(b.model).trim();
      const url = u ? norm(u) : null;
      if (u && !url) fail(400, m("Use o endereço de um computador da clínica, como 100.86.97.21 ou 192.168.1.20:11434. Endereços da internet não são permitidos.", "Use the address of a computer in the clinic, like 100.86.97.21 or 192.168.1.20:11434. Internet addresses aren’t allowed."));
      if (mdl && !/^[A-Za-z0-9._\-:/]{1,100}$/.test(mdl)) fail(400, m("Esse nome de modelo não é válido. Use um nome como gemma3:12b.", "That model name isn’t valid. Use a name like gemma3:12b."));
      return { url, model: mdl || null };
    };
    const SERVER = { url: "http://100.86.97.21:11434", model: "gemma4:12b" };
    const cur = () => ({ url: aiConf.url ?? SERVER.url, model: aiConf.model ?? SERVER.model });
    const speech = () => ({ available: false, url: aiConf.speech ?? "http://192.168.1.66:8178", reason: "offline" });
    const asst = () => { const c = cur(); const mdl = aiConf.assistant ?? c.model; const k = fakeCheck(c.url, mdl); return { model: mdl, available: k.available, reason: k.reason, own: aiConf.assistant != null }; };
    if (method === "GET" && seg[1] === "status") { const c = cur(); return { ...fakeCheck(c.url, c.model), speech: speech(), assistant: asst() }; }
    if (method === "PUT" && seg[1] === "settings" && "assistant_model" in b && !("url" in b)) { const mdl = s(b.assistant_model).trim();
      if (mdl && !/^[A-Za-z0-9._\-:/]{1,100}$/.test(mdl)) fail(400, m("Esse nome de modelo não é válido. Use um nome como gemma3:12b.", "That model name isn’t valid. Use a name like gemma3:12b."));
      aiConf.assistant = mdl || null; audit("ai.settings_changed", null, `assistant ${mdl || "main"}`); const c = cur(); return { ...fakeCheck(c.url, c.model), speech: speech(), assistant: asst() }; }
    if (method === "POST" && seg[1] === "test" && s(b.assistant_model).trim()) { const c = cur(); return new Promise((res) => setTimeout(() => res(fakeCheck(c.url, s(b.assistant_model).trim())), 600)); }
    if (method === "GET" && seg[1] === "settings") { const c = cur(); return { ...c, url_in_app: aiConf.url !== null, model_in_app: aiConf.model !== null, server_url: SERVER.url, server_model: SERVER.model,
      speech_url: aiConf.speech ?? "http://192.168.1.66:8178", speech_url_in_app: aiConf.speech != null, server_speech_url: "http://192.168.1.66:8178", assistant_model: aiConf.assistant ?? null }; }
    if (method === "POST" && seg[1] === "speech") { const u = s(b.url); const url = u ? norm(u.includes(":") ? u : `${u}:8178`) : "http://192.168.1.66:8178";
      if (u && !url) fail(400, m("Use o endereço de um computador da clínica, como 192.168.1.66 ou 100.86.97.21:8178. Endereços da internet não são permitidos.", "Use the address of a computer in the clinic, like 192.168.1.66 or 100.86.97.21:8178. Internet addresses aren’t allowed."));
      return new Promise((res) => setTimeout(() => res({ available: false, url, reason: "offline" }), 600)); }
    if (method === "PUT" && seg[1] === "settings" && "speech_url" in b && !("url" in b)) { const u = s(b.speech_url); const url = u ? norm(u.includes(":") ? u : `${u}:8178`) : null;
      if (u && !url) fail(400, m("Use o endereço de um computador da clínica, como 192.168.1.66 ou 100.86.97.21:8178. Endereços da internet não são permitidos.", "Use the address of a computer in the clinic, like 192.168.1.66 or 100.86.97.21:8178. Internet addresses aren’t allowed."));
      aiConf.speech = url; audit("ai.settings_changed", null, `speech ${url ?? "server"}`); const c = cur(); return { ...fakeCheck(c.url, c.model), speech: speech() }; }
    if (method === "POST" && seg[1] === "test") { const i = input(); return new Promise((res) => setTimeout(() => res(fakeCheck(i.url ?? SERVER.url, i.model ?? SERVER.model)), 600)); }
    if (method === "PUT" && seg[1] === "settings") { const i = input(); aiConf.url = i.url; aiConf.model = i.model; audit("ai.settings_changed", null, `${cur().url} · ${cur().model}`); const c = cur(); return fakeCheck(c.url, c.model); }
  }
  if (seg[0] === "patients" && seg[2] === "letters") {
    needClin();
    const pid = seg[1];
    if (method === "GET" && seg.length === 3) {
      P(pid);
      return { professional_id: proIds[ME[role].id] ?? "", letters: letters.filter((l) => l.patient_id === pid).sort((a, c) => c.created_at.localeCompare(a.created_at))
        .map((l) => ({ ...l, created_by_name: thName(l.created_by), approved_by_name: thName(l.approved_by) })) };
    }
    if (method === "GET" && seg[3] === "template") { audit("patient.viewed", pid, "letter"); return letterTemplate(pid, qs.get("lang") === "en"); }
    if (method === "POST" && seg[3] === "ai-draft") {
      owner();
      const f = facts(pid); const en = b.lang === "en";
      audit("ai.letter_drafted", pid, "ok");
      const t = en
        ? `${f.r.name} attended ${f.visits.length} physiotherapy sessions at this clinic for ${f.r.anamnesis?.main_complaint?.replace(/\.+$/, "").toLowerCase() || "[to complete]"}.\n\nTreatment combined ${andList(f.techs, true) || "[to complete]"}, with a gradual return to daily activities.\n\n${f.changes.length ? `Outcome measures improved:\n• ${f.changes.join("\n• ")}` : ""}\n\nAt discharge: [to complete].\n\nWe recommend continuing the home exercise programme.`
        : `${f.r.name} realizou ${f.visits.length} sessões de fisioterapia nesta clínica por ${f.r.anamnesis?.main_complaint?.replace(/\.+$/, "").toLowerCase() || "[a completar]"}.\n\nO tratamento combinou ${andList(f.techs, false) || "[a completar]"}, com retoma progressiva das atividades do dia a dia.\n\n${f.changes.length ? `Os testes mostraram melhoria:\n• ${f.changes.join("\n• ")}` : ""}\n\nÀ data da alta: [a completar].\n\nRecomenda-se manter o programa de exercícios em casa.`;
      return new Promise((res) => setTimeout(() => res({ text: t.replace(/\n\n\n\n/g, "\n\n"), model: "gemma3:12b" }), 2200));
    }
    if (method === "POST" && seg.length === 3) {
      P(pid);
      const l: LetterRow = { id: uid(), patient_id: pid, lang: b.lang === "en" ? "en" : "pt", addressee: s(b.addressee), body: s(b.body), ai_drafted: !!b.ai_drafted, created_by: ME[role].id, created_at: nowIso(), updated_at: nowIso(), approved_by: null, approved_at: null, document_id: null };
      letters.push(l); audit("letter.created", pid); return { id: l.id };
    }
  }
  if (seg[0] === "letters") {
    needClin();
    const l = letters.find((x) => x.id === seg[1]) ?? fail(404, "Not found.");
    const mineOrOwner = () => { if (role !== "owner" && l.created_by !== ME[role].id) fail(409, m("Só o proprietário ou quem começou o rascunho o pode alterar.", "Only the owner or whoever started this draft can change it.")); };
    if (method === "PATCH" && seg.length === 2) {
      if (l.approved_at) fail(409, m("Esta carta está aprovada e não pode ser alterada.", "This letter is approved and can’t be changed. Write a new letter instead."));
      mineOrOwner();
      if (b.lang) l.lang = b.lang === "en" ? "en" : "pt"; if ("addressee" in b) l.addressee = s(b.addressee); if ("body" in b) l.body = s(b.body); if (b.ai_drafted) l.ai_drafted = true;
      l.updated_at = nowIso(); audit("letter.updated", l.patient_id); return { ok: true };
    }
    if (method === "DELETE" && seg.length === 2) {
      if (l.approved_at) fail(409, m("As cartas aprovadas são mantidas.", "Approved letters are kept."));
      mineOrOwner(); letters.splice(letters.indexOf(l), 1); audit("letter.draft_deleted", l.patient_id); return { ok: true };
    }
    if (method === "POST" && seg[2] === "approve") {
      owner();
      if (l.approved_at) fail(409, m("Esta carta já está aprovada.", "This letter is already approved."));
      if (/\[(a completar|to complete)\]/.test(l.body)) fail(400, m("Complete as partes marcadas [a completar] antes de aprovar.", "Fill in the parts marked [a completar] before approving."));
      if (typeof b.professional_id === "string") proIds[ME[role].id] = s(b.professional_id);
      l.approved_at = nowIso(); l.approved_by = ME[role].id;
      const r = P(l.patient_id);
      if (b.discharge) r.active = false;
      const en = l.lang === "en"; const pro = proIds[ME[role].id];
      const lines = [{ t: en ? "Physiotherapy discharge letter" : "Carta de alta de fisioterapia", size: 16, bold: true }, { t: dmy(l.approved_at), gap: 6 }, { t: l.addressee, gap: 8 },
        { t: `${en ? "Re: discharge from physiotherapy" : "Assunto: alta de fisioterapia"} — ${r.name} (n.º ${r.number})`, bold: true, gap: 8 },
        ...l.body.split("\n").filter(Boolean).flatMap((x) => (x.match(/.{1,95}(\s|$)/g) ?? [x]).map((t) => ({ t: t.trim() }))),
        { t: en ? "Yours sincerely," : "Com os melhores cumprimentos,", gap: 10 }, { t: ME[role].name, gap: 16 },
        { t: `${en ? "Physiotherapist" : "Fisioterapeuta"}${pro ? ` — ${en ? "professional licence no." : "cédula profissional n.º"} ${pro}` : ""}` }];
      const t = new Date(l.approved_at);
      const d = addDoc({ patient_id: l.patient_id, kind: "letter", filename: `${ymd(t)} Carta de alta ${pad(t.getHours())}h${pad(t.getMinutes())}.pdf`, mime: "application/pdf", uploaded_at: l.approved_at, uploaded_by: ME[role].id, uploaded_by_name: ME[role].name, letter_id: l.id, data: miniPdf(lines) });
      l.document_id = d.id;
      audit("letter.approved", l.patient_id, b.discharge ? "discharged" : "");
      return { document_id: d.id, discharged: !!b.discharge };
    }
  }
  return undefined;
}

// ───────── the routes ─────────

/** Answers a phase 2 request, or returns undefined if the path isn't one of these. */
export function preview2(method: string, path: string, body: unknown, m: M): unknown {
  const b = (body ?? {}) as Body;
  const p = path.split("?")[0];
  const seg = p.split("/").filter(Boolean);
  const key = `${method} /${seg.map((x, i) => (i % 2 === 1 && !["sign", "pdf", "revoke", "accept", "decline", "reopen", "attendees", "anamnesis", "notes", "tests", "prescriptions", "consents", "sessions", "sent", "template", "duplicates"].includes(x) ? ":id" : x)).join("/")}`;
  const id = seg[1];
  const needClin = () => { if (!clin()) fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this.")); };
  /** Same rules as the server (routes/patients.rs check_nif): a valid NIF, and never one another patient has. */
  const checkNif = (body: Body, me: string | null) => {
    if (typeof body.nif !== "string") return;
    if (!patientNifOk(body.nif)) fail(400, m("Esse NIF não é válido. Confirme os 9 dígitos.", "That NIF isn’t valid. Check the 9 digits."));
    body.nif = tidyNif(body.nif);
    const o = body.nif ? patients.find((x) => x.nif === body.nif && x.id !== me && !x.deleted && !x.anonymised_at) : undefined;
    if (o) fail(409, m("Já existe um utente com este NIF. Abra essa ficha em vez de criar outra.", "There’s already a patient with this NIF. Open that file instead."), "duplicate_nif", { id: o.id, number: o.number, name: o.name });
  };

  // The assistant's schedule jobs and one-press booking of requests (11 Nov 2026).
  const ag = previewAgenda(method, path, body, m, {
    appts, patients, requests, therapists: TH, role: () => role, me: () => ({ id: ME[role].id, ai_enabled: role === "owner" || role === "therapist" }), audit, createAppt,
    newPatient: (name, phone, email) => {
      const n = Math.max(0, ...patients.map((x) => x.number)) + 1;
      const rec: PRow = { id: uid(), number: n, name, birth_date: null, sex: null, nif: null, sns_number: null, phone: phone || null, email: email || null, address: null, guardian_name: null, guardian_phone: null,
        lang: "pt", reminders: "sms", therapist_id: null, admin_note: "", active: true, anonymised_at: null, created_at: nowIso(), updated_at: nowIso(), anamnesis: null };
      patients.push(rec); audit("patient.created", rec.id); return rec.id;
    },
    dropPatient: (id) => { if (appts.some((a) => a.patient_id === id)) return false; const i = patients.findIndex((x) => x.id === id); if (i >= 0) patients.splice(i, 1); return i >= 0; },
  });
  if (ag !== undefined) return ag;

  const lr = previewLegal(method, seg, b, m);
  if (lr !== undefined) return lr;

  const cl = previewClinic(method, seg, b, m);
  if (cl !== undefined) return cl;

  const lt = previewLetters(method, seg, path, b, m);
  if (lt !== undefined) return lt;

  const dr = previewDocs(method, seg, b, m, { clinical: clin(), role, me: ME[role], audit, patientExists: (x) => patients.some((r) => r.id === x) });
  if (dr !== undefined) return dr;

  switch (key) {
    case "GET /therapists": return TH;
    // reminders sent by hand (server: routes/reminders.rs)
    case "GET /reminders": {
      const d = q(path).get("day") || ymd(addDays(new Date(), 1));
      const items = appts.filter((a) => a.status === "booked" && ymd(new Date(a.starts_at)) === d && a.kind !== "block")
        .flatMap((a) => (a.kind === "class" ? a.attendees.map((x) => x.patient_id) : a.patient_id ? [a.patient_id] : []).map((pid) => ({ a, pid })))
        .filter(({ pid }) => patients.some((x) => x.id === pid && !x.deleted && !x.anonymised_at))
        .map(({ a, pid }) => {
          const r = P(pid); const sent = remindersSent.find((x) => x.appointment_id === a.id && x.patient_id === pid);
          return { appointment_id: a.id, patient_id: pid, name: r.name, number: r.number, phone: r.phone || r.guardian_phone || null, email: r.email || null, lang: r.lang, prefers: r.reminders,
            kind: a.kind, title: a.title, starts_at: a.starts_at, therapist_name: thName(a.therapist_id), home: a.kind === "home",
            sent_at: sent?.sent_at ?? null, sent_channel: sent?.channel ?? null, sent_by_name: sent?.by ?? null };
        }).sort((x, y) => x.starts_at.localeCompare(y.starts_at) || x.name.localeCompare(y.name));
      audit("reminders.listed", null, d);
      return { day: d, clinic: "Clínica de Fisioterapia", template_pt: reminderTpl.pt, template_en: reminderTpl.en, items };
    }
    case "POST /reminders/sent": {
      const a = appts.find((x) => x.id === b.appointment_id && (x.patient_id === b.patient_id || x.attendees.some((y) => y.patient_id === b.patient_id))) ?? fail(404, "Not found");
      const i = remindersSent.findIndex((x) => x.appointment_id === a.id && x.patient_id === b.patient_id);
      if (i >= 0) remindersSent.splice(i, 1);
      if (b.sent) { remindersSent.push({ appointment_id: a.id, patient_id: s(b.patient_id), channel: s(b.channel) || "other", sent_at: nowIso(), by: ME[role].name }); audit("appointment.reminded", s(b.patient_id), s(b.channel)); }
      else audit("appointment.reminder_undone", s(b.patient_id));
      return { ok: true };
    }
    case "PUT /reminders/template": {
      if (role !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
      if (!s(b.pt) || !s(b.en)) fail(400, m("Escreva a mensagem nas duas línguas (até 600 caracteres).", "Write the message in both languages (up to 600 characters)."));
      reminderTpl.pt = s(b.pt); reminderTpl.en = s(b.en); audit("reminder_text.updated", null); return { ok: true };
    }
    case "GET /access-log": case "GET /access-log/:id": { // :id = "export" (the preview key turns the 2nd segment into :id)
      // Same filters as the server (audit phase C): from/to days, staff_id, actions (exact or "prefix."), q words, before (id).
      if (role !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
      const qs = q(path), sid = qs.get("subject_id"), from = qs.get("from"), to = qs.get("to"), who = qs.get("staff_id");
      const acts = (qs.get("actions") ?? "").split(",").map((x) => x.trim()).filter(Boolean);
      const words = (qs.get("q") ?? "").trim().toLowerCase(), before = Number(qs.get("before") ?? 0);
      if (from && to && from > to) fail(400, m("O primeiro dia tem de ser antes do último.", "The first day must be before the last day."));
      const lim = Math.min(1000, Math.max(1, Number(qs.get("limit") ?? 200)));
      const day = (iso: string) => { const d = new Date(iso); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
      const staffName = who ? ({ fs: ME.owner.name, ar: ME.therapist.name, mc: ME.reception.name } as Record<string, string>)[who] : null;
      const rows = [...log, ...LOG_SEED].map((r) => {
        const p = r.subject_type === "patient" ? patients.find((x) => x.id === r.subject_id && !x.deleted) : undefined;
        return { ...r, subject_label: p ? `${String(p.number).padStart(4, "0")} ${p.name}` : null };
      }).filter((r) => (!sid || r.subject_id === sid) && (!from || day(r.at) >= from) && (!to || day(r.at) <= to)
        && (!who || r.staff_name === staffName)
        && (!acts.length || acts.some((a) => a.endsWith(".") ? r.action.startsWith(a) : r.action === a))
        && (!words || words.split(/\s+/).every((w) => [r.staff_name, r.subject_label, r.device, r.action, r.detail].join(" ").toLowerCase().includes(w)))
        && (!before || r.id < before)).sort((a, c) => c.id - a.id);
      // Preview ids aren't in time order (sample history is written at start-up), so show newest first by time;
      // the server orders by id, which is time order there.
      rows.sort((a, c) => c.at.localeCompare(a.at) || c.id - a.id);
      if (path.startsWith("/access-log/export")) { audit("access_log.exported", null, `${rows.length} rows`); return { ok: true }; }
      return rows.slice(0, lim);
    }

    // patients
    case "GET /patients": {
      const t = (q(path).get("q") ?? "").trim().toLowerCase(), one = q(path).get("id");
      const lim = Math.min(2000, Math.max(1, Number(q(path).get("limit") ?? 500))), off = Math.max(0, Number(q(path).get("offset") ?? 0));
      const list = patients.filter((x) => !x.deleted && (one ? x.id === one : (x.active || q(path).get("include_inactive") === "true") && (!t || x.name.toLowerCase().includes(t) || String(x.number) === t
        || (x.phone ?? "").replace(/ /g, "").includes(t.replace(/ /g, "")) || (x.email ?? "").includes(t) || x.nif === t.replace(/ /g, ""))))
        .sort((a, c) => a.name.localeCompare(c.name, "pt") || a.number - c.number).slice(off, off + lim);
      audit("patients.listed", null, `${list.length} ${t ? "search" : "all"}`);
      const now = new Date().toISOString();
      return list.map((x) => ({ id: x.id, number: x.number, name: x.name, birth_date: x.birth_date, phone: x.phone, email: x.email, active: x.active,
        therapist_id: x.therapist_id, therapist_name: thName(x.therapist_id),
        next_at: appts.filter((a) => a.patient_id === x.id && a.status === "booked" && a.starts_at > now).map((a) => a.starts_at).sort()[0] ?? null,
        last_at: appts.filter((a) => a.patient_id === x.id && a.status === "done").map((a) => a.starts_at).sort().reverse()[0] ?? null }));
    }
    case "GET /patients/duplicates": {
      const nif = tidyNif(q(path).get("nif") ?? ""), ph = (q(path).get("phone") ?? "").replace(/\D/g, "").slice(-9), nm = (q(path).get("name") ?? "").trim().toLowerCase(), bd = q(path).get("birth_date") ?? "", ex = q(path).get("exclude");
      return patients.filter((x) => !x.deleted && !x.anonymised_at && x.id !== ex).map((x) => ({ x,
        reason: nif && x.nif === nif ? "nif" : ph.length === 9 && (x.phone ?? "").replace(/\D/g, "").slice(-9) === ph ? "phone" : nm && bd && x.name.trim().toLowerCase() === nm && x.birth_date === bd ? "name_birth" : "" }))
        .filter((r) => r.reason).slice(0, 5).map(({ x, reason }) => ({ id: x.id, number: x.number, name: x.name, birth_date: x.birth_date, phone: x.phone, reason }));
    }
    case "POST /patients": {
      if (!s(b.name)) fail(400, m("Escreva o nome do utente.", "Enter the patient’s name."));
      checkNif(b, null);
      const n = Math.max(0, ...patients.map((x) => x.number)) + 1;
      const rec: PRow = { id: uid(), number: n, name: s(b.name), birth_date: s(b.birth_date) || null, sex: (s(b.sex) || null) as PRow["sex"], nif: s(b.nif) || null, sns_number: s(b.sns_number) || null,
        phone: s(b.phone) || null, email: s(b.email) || null, address: s(b.address) || null, guardian_name: s(b.guardian_name) || null, guardian_phone: s(b.guardian_phone) || null,
        lang: (s(b.lang) || "pt") as "pt", reminders: (s(b.reminders) || "sms") as "sms", therapist_id: s(b.therapist_id) || null, admin_note: s(b.admin_note), active: true,
        anonymised_at: null, created_at: nowIso(), updated_at: nowIso(), anamnesis: null };
      patients.push(rec); audit("patient.created", rec.id);
      return { id: rec.id, number: n };
    }
    case "PATCH /patients/:id": {
      const r = P(id);
      if ("name" in b && !s(b.name)) fail(400, m("Escreva o nome do utente.", "Enter the patient’s name."));
      checkNif(b, id);
      for (const [k, v] of Object.entries(b)) if (k in r && k !== "id" && k !== "number") (r as unknown as Body)[k] = typeof v === "string" ? (v.trim() || (["admin_note", "name"].includes(k) ? v.trim() : null)) : v;
      r.updated_at = nowIso(); audit("patient.updated", id, Object.keys(b).join(","));
      return { ok: true };
    }
    case "GET /patients/:id": {
      const r = P(id);
      const base = {
        patient: pubPatient(r),
        consents: consents.filter((c) => c.patient_id === id).sort((a, c) => c.signed_at.localeCompare(a.signed_at)).map((c) => {
          const t = texts.find((x) => x.id === c.text_id)!;
          return { id: c.id, kind: c.kind, granted: c.granted, signed_by_name: c.signed_by_name, signed_at: c.signed_at, expires_on: c.expires_on, revoked_at: c.revoked_at, version: t.version, title: t.title, state: consentState(c),
            pdf_id: docsAll(c.patient_id).find((d) => d.consent_id === c.id && d.consent_event === "signed")?.id ?? null,
            revoked_pdf_id: docsAll(c.patient_id).find((d) => d.consent_id === c.id && d.consent_event === "revoked")?.id ?? null };
        }),
        appointments: appts.filter((a) => a.patient_id === id || a.attendees.some((x) => x.patient_id === id)).sort((a, c) => c.starts_at.localeCompare(a.starts_at))
          .map((a) => ({ id: a.id, kind: a.kind, starts_at: a.starts_at, ends_at: a.ends_at, status: a.status, therapist_id: a.therapist_id, therapist_name: thName(a.therapist_id), series_id: a.series_id })),
        prescriptions: rxs.filter((x) => x.patient_id === id).map(rxUse).sort((a, c) => c.issued_on.localeCompare(a.issued_on)),
      };
      audit("patient.viewed", id, clin() ? "full" : "reduced");
      if (!clin()) return { ...base, view: "reduced", documents: docsFor(id, false) };
      return { ...base, view: "full", anamnesis: r.anamnesis,
        notes: notes.filter((n) => n.patient_id === id).sort((a, c) => c.created_at.localeCompare(a.created_at)),
        tests: tests.filter((t) => t.patient_id === id).sort((a, c) => a.test_code.localeCompare(c.test_code) || a.taken_on.localeCompare(c.taken_on)),
        documents: docsFor(id, true) };
    }
    case "PUT /patients/:id/anamnesis": {
      needClin(); const r = P(id);
      const cur = r.anamnesis ?? { main_complaint: "", history: "", medication: "", medical_history: "", goals: "", regions: [], red_flags: [], updated_at: "", updated_by_name: null };
      r.anamnesis = { ...cur, ...(b as Partial<Anamnesis>), updated_at: nowIso(), updated_by_name: ME[role].name };
      audit("anamnesis.updated", id); return { ok: true };
    }
    case "POST /patients/:id/notes": {
      needClin(); P(id);
      const n: (typeof notes)[number] = { id: uid(), patient_id: id, appointment_id: (b.appointment_id as string) || null, template: s(b.template), blocks: (b.blocks as Note["blocks"]) ?? [], text: s(b.text),
        created_at: nowIso(), updated_at: nowIso(), created_by: ME[role].id, created_by_name: ME[role].name, signed_at: null, signed_by_name: null };
      notes.push(n); audit("note.created", id); return { id: n.id };
    }
    case "PATCH /notes/:id": {
      needClin(); const n = notes.find((x) => x.id === id) ?? fail(404, m("Esse item não existe ou foi removido.", "That item doesn’t exist or was removed."));
      if (n.signed_at || (role !== "owner" && n.created_by !== ME[role].id)) fail(409, m("Esta nota está assinada ou é de outra pessoa. Acrescente uma nota nova.", "This note is signed or belongs to someone else, so it can’t be changed. Add a new note instead."));
      Object.assign(n, { ...("blocks" in b ? { blocks: b.blocks } : {}), ...("text" in b ? { text: s(b.text) } : {}), ...("template" in b ? { template: s(b.template) } : {}), updated_at: nowIso() });
      audit("note.updated", n.patient_id); return { ok: true };
    }
    case "DELETE /notes/:id": {
      needClin(); const i = notes.findIndex((x) => x.id === id);
      if (i < 0 || notes[i].signed_at) fail(409, m("As notas assinadas são mantidas. Acrescente uma nota nova para corrigir.", "Signed notes are kept. Add a new note to correct it."));
      audit("note.draft_deleted", notes[i].patient_id); notes.splice(i, 1); return { ok: true };
    }
    case "POST /notes/:id/sign": {
      needClin(); const n = notes.find((x) => x.id === id);
      if (!n || n.signed_at || (role !== "owner" && n.created_by !== ME[role].id)) fail(409, m("Esta nota já está assinada ou é de outra pessoa.", "This note is already signed or belongs to someone else."));
      n!.signed_at = nowIso(); n!.signed_by_name = ME[role].name; audit("note.signed", n!.patient_id);
      // Scales nobody set aren't part of the signed record (server: patients.rs sign_note).
      n!.blocks = n!.blocks.filter((x) => !(x.type === "slider" && x.value == null));
      return { signed_at: n!.signed_at, document_id: notePdf(n!, P(n!.patient_id), ME[role]).id };
    }
    case "POST /notes/:id/pdf": {
      needClin(); const n = notes.find((x) => x.id === id && x.signed_at) ?? fail(404, "Not found");
      return { document_id: notePdf(n, P(n.patient_id), ME[role]).id };
    }
    case "POST /patients/:id/tests": {
      needClin(); P(id);
      const code = s(b.test_code).toLowerCase();
      // Forms answered question by question are scored here, like the server (forms.rs).
      const ans = b.answers as Answers | undefined;
      const form = ans?.items && FORMS[code] ? scoreForm(code, ans) : null;
      if (form && form.score === null) fail(400, m("Há demasiadas perguntas em branco para calcular a pontuação.", "Too many questions left blank to work out a score."));
      const score = form ? form.score! : Number(b.score);
      if (!code || !Number.isFinite(score)) fail(400, m("Escolha um teste e escreva a pontuação.", "Choose a test and enter its score."));
      const taken = s(b.taken_on) || ymd(new Date());
      const hiw = form ? FORMS[code].higherIsWorse : b.higher_is_worse !== false;
      const prev = tests.filter((t) => t.patient_id === id && t.test_code === code && t.taken_on <= taken).sort((a, c) => c.taken_on.localeCompare(a.taken_on) || c.created_at.localeCompare(a.created_at))[0];
      const answers = form ? { form: 1, items: ans!.items, ...(Object.keys(form.sub).length ? { sub: form.sub } : {}), ...(code === "psfs" ? { activities: ans!.activities } : {}) } : {};
      const t = { id: uid(), patient_id: id, test_code: code, answers, score, max_score: form ? FORMS[code].max : b.max_score == null ? null : Number(b.max_score), higher_is_worse: hiw, taken_on: taken, recorded_by_name: ME[role].name, created_at: nowIso() };
      tests.push(t); audit("test.recorded", id, code);
      return { id: t.id, score, previous: prev?.score ?? null, worse: prev ? (hiw ? score > prev.score : score < prev.score) : false };
    }
    case "DELETE /tests/:id": { needClin(); const i = tests.findIndex((x) => x.id === id); if (i >= 0) { audit("test.deleted", tests[i].patient_id); tests.splice(i, 1); } return { ok: true }; }
    case "POST /patients/:id/prescriptions": {
      needClin(); P(id);
      if (!s(b.issued_on) || !(Number(b.sessions_total) > 0)) fail(400, m("Indique a data da prescrição e o número de sessões (pelo menos 1).", "Enter the prescription date and the number of sessions (at least 1)."));
      const r = { id: uid(), patient_id: id, doctor: s(b.doctor), specialty: s(b.specialty), issued_on: s(b.issued_on), sessions_total: Number(b.sessions_total), valid_until: s(b.valid_until) || null, payer: s(b.payer), document_id: null };
      rxs.push(r); audit("prescription.created", id); return { id: r.id };
    }
    case "PATCH /prescriptions/:id": { needClin(); const r = rxs.find((x) => x.id === id) ?? fail(404, "Not found"); Object.assign(r, b); audit("prescription.updated", r.patient_id); return { ok: true }; }

    // consents
    case "GET /consent-texts": {
      return texts.map((t) => ({ ...t, draft: !t.approved_at, current: !!t.approved_at && t.version === Math.max(...texts.filter((x) => x.kind === t.kind && x.lang === t.lang && x.approved_at).map((x) => x.version)) }))
        .sort((a, c) => a.kind.localeCompare(c.kind) || a.lang.localeCompare(c.lang) || c.version - a.version);
    }
    case "POST /consent-texts": {
      if (role !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
      if (!s(b.title) || !s(b.body)) fail(400, m("Escreva o título e o texto.", "Enter a title and the text."));
      const v = Math.max(0, ...texts.filter((t) => t.kind === b.kind && t.lang === b.lang).map((t) => t.version)) + 1;
      texts.push({ id: uid(), kind: b.kind as ConsentKind, version: v, lang: b.lang as "pt", title: s(b.title), body: s(b.body), valid_months: b.valid_months ? Number(b.valid_months) : null, created_at: nowIso(), current: true, draft: false, approved_at: nowIso() });
      audit("consent_text.created", null, `${b.kind} v${v} ${b.lang}`); return { id: texts[texts.length - 1].id, version: v };
    }
    case "POST /consent-texts/:id/approve": {
      if (role !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
      const t = texts.find((x) => x.id === id) ?? fail(404, "Not found");
      if (t.approved_at) fail(409, m("Este texto já está aprovado.", "This text is already approved."));
      t.approved_at = nowIso(); audit("consent_text.approved", null, `${t.kind} v${t.version} ${t.lang}`); return { ok: true };
    }
    case "POST /patients/:id/consents": {
      P(id);
      if (!s(b.signed_by_name)) fail(400, m("Escreva o nome de quem assina.", "Enter the name of the person signing."));
      if (!s(b.signature_png).startsWith("data:image/png;base64,")) fail(400, m("Falta a assinatura. Peça ao utente para assinar de novo.", "The signature is missing. Ask the patient to sign again."));
      const t = texts.find((x) => x.id === b.text_id && x.approved_at) ?? fail(400, m("Este texto ainda não foi aprovado. O proprietário aprova-o em Definições › Consentimentos.", "This consent text isn’t approved yet. The owner approves it in Settings › Consents."));
      const exp = t.valid_months ? ymd((() => { const d = new Date(); d.setMonth(d.getMonth() + t.valid_months!); return d; })()) : null;
      const c = { id: uid(), patient_id: id, text_id: t.id, kind: t.kind, granted: b.granted !== false, signed_by_name: s(b.signed_by_name), signature_png: s(b.signature_png), signed_at: nowIso(), expires_on: exp, revoked_at: null };
      consents.push(c); audit("consent.signed", id, `${t.kind} ${c.granted ? "granted" : "refused"}`);
      return { id: c.id, expires_on: exp, document_id: consentPdf(c, t, P(id), ME[role], "signed").id };
    }
    case "GET /consents/:id": {
      const c = consents.find((x) => x.id === id) ?? fail(404, "Not found");
      const t = texts.find((x) => x.id === c.text_id)!;
      audit("patient.viewed", c.patient_id, "consent");
      return { ...c, title: t.title, body: t.body, version: t.version, lang: t.lang, state: consentState(c) };
    }
    case "POST /consents/:id/revoke": {
      const c = consents.find((x) => x.id === id) ?? fail(404, "Not found");
      if (c.revoked_at) fail(409, m("Este consentimento já foi retirado.", "This consent was already withdrawn."));
      c.revoked_at = nowIso(); audit("consent.revoked", c.patient_id);
      return { ok: true, document_id: consentPdf(c, texts.find((x) => x.id === c.text_id)!, P(c.patient_id), ME[role], "revoked").id };
    }
    case "POST /consents/:id/pdf": {
      const c = consents.find((x) => x.id === id) ?? fail(404, "Not found"); const t = texts.find((x) => x.id === c.text_id)!;
      const d = consentPdf(c, t, P(c.patient_id), ME[role], "signed");
      const r = c.revoked_at ? consentPdf(c, t, P(c.patient_id), ME[role], "revoked") : null;
      audit("consent.pdf_saved", c.patient_id); return { document_id: d.id, revoked_document_id: r?.id ?? null };
    }

    // schedule
    case "GET /appointments": {
      const from = q(path).get("from") ?? "", to = q(path).get("to") ?? "", th = q(path).get("therapist_id"), inc = q(path).get("include_cancelled") === "true";
      if (!(new Date(to) > new Date(from)) || new Date(to).getTime() - new Date(from).getTime() > 62 * 864e5) fail(400, m("Escolha um período até dois meses.", "Choose a period of up to two months."));
      return appts.filter((a) => a.starts_at < to && a.ends_at > from && (!th || a.therapist_id === th) && (inc || a.status !== "cancelled"))
        .sort((a, c) => a.starts_at.localeCompare(c.starts_at)).map(apptOut);
    }
    case "POST /appointments": return createAppt(b, m);
    case "PATCH /appointments/:id": {
      const a = appts.find((x) => x.id === id) ?? fail(404, m("Esse item não existe ou foi removido.", "That item doesn’t exist or was removed."));
      const scope = s(b.scope) || "one";
      const status = b.status as Appointment["status"] | undefined;
      if (status && status !== "cancelled" && scope !== "one") fail(400, m("Só é possível marcar como realizada ou falta uma marcação de cada vez.", "Mark sessions as done or no-show one at a time."));
      const group = scope === "one" || !a.series_id ? [a] : appts.filter((x) => x.series_id === a.series_id && (scope === "all" || x.starts_at >= a.starts_at) && x.status === "booked");
      const before = group.map((x) => ({ ...x }));
      if (b.starts_at && b.ends_at) {
        const ns = new Date(s(b.starts_at)), ne = new Date(s(b.ends_at)), os = new Date(a.starts_at);
        const dd = Math.round((new Date(ns.getFullYear(), ns.getMonth(), ns.getDate()).getTime() - new Date(os.getFullYear(), os.getMonth(), os.getDate()).getTime()) / 864e5);
        const dur = (ne.getTime() - ns.getTime()) / 60000;
        for (const x of group) { const d = addDays(new Date(x.starts_at), dd); d.setHours(ns.getHours(), ns.getMinutes(), 0, 0); x.starts_at = d.toISOString(); x.ends_at = addMin(d, dur).toISOString(); }
      }
      if ("charge_waived" in b && role !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
      for (const k of ["therapist_id", "prescription_id", "title", "color", "address", "travel_minutes", "capacity", "admin_note", "plan_id", "charge_waived"] as const) if (k in b) for (const x of group) (x as unknown as Body)[k] = b[k];
      if (status) { for (const x of group) x.status = status; moneyOnStatus(S3, group, status); }
      else if ("plan_id" in b) moneyOnStatus(S3, group, "done", false);
      const o = overlapsOf(group.map((x) => x.id));
      if (o.length && !b.allow_overlap && a.kind !== "block" && (b.starts_at || b.therapist_id)) {
        group.forEach((x, i) => Object.assign(x, before[i]));
        fail(409, m("Sobrepõe-se a outra marcação do mesmo fisioterapeuta.", "This overlaps another appointment for the same therapist."), "overlap", o);
      }
      audit("appointment.updated", a.patient_id, `${scope} x${group.length}`);
      return { ids: group.map((x) => x.id), overlaps: o, waitlist_matches: status === "cancelled" && scope === "one" ? waitMatches(a) : [] };
    }
    case "POST /appointments/:id/attendees": {
      const a = appts.find((x) => x.id === id) ?? fail(404, "Not found");
      if (a.kind !== "class") fail(400, m("Só as aulas têm inscritos.", "Only classes have attendees."));
      if (a.attendees.length >= (a.capacity ?? 0)) fail(409, m("A aula está cheia.", "This class is full."));
      if (!a.attendees.some((x) => x.patient_id === b.patient_id)) a.attendees.push({ patient_id: s(b.patient_id), attended: null });
      audit("class.attendee_added", s(b.patient_id)); return { ok: true };
    }
    case "PATCH /appointments/:id/attendees/:id": { const a = appts.find((x) => x.id === id)!; const at = a.attendees.find((x) => x.patient_id === seg[3]); if (at) { at.attended = !!b.attended; moneyOnAttended(S3, a, seg[3], !!b.attended); } return { ok: true }; }
    case "DELETE /appointments/:id/attendees/:id": { const a = appts.find((x) => x.id === id)!; a.attendees = a.attendees.filter((x) => x.patient_id !== seg[3]); return { ok: true }; }

    case "GET /waitlist": return waitlist.filter((w) => w.status === "waiting" || w.status === "offered").map((w) => ({ ...w, name: w.patient_id ? P(w.patient_id).name : w.name, phone: w.patient_id ? P(w.patient_id).phone ?? "" : w.phone, therapist_name: thName(w.therapist_id) }));
    case "POST /waitlist": {
      if (!b.patient_id && !s(b.name)) fail(400, m("Escolha um utente ou escreva o nome e o telefone.", "Choose a patient or enter a name and phone."));
      waitlist.push({ id: uid(), patient_id: (b.patient_id as string) || null, name: s(b.name), phone: s(b.phone), email: s(b.email), therapist_id: (b.therapist_id as string) || null,
        weekdays: (b.weekdays as number[]) ?? [], from_time: s(b.from_time) || null, to_time: s(b.to_time) || null, note: s(b.note), status: "waiting", created_at: nowIso() });
      audit("waitlist.added", (b.patient_id as string) || null); return { id: waitlist[waitlist.length - 1].id };
    }
    case "PATCH /waitlist/:id": { const w = waitlist.find((x) => x.id === id) ?? fail(404, "Not found"); Object.assign(w, b); return { ok: true }; }

    case "GET /booking-requests": return requests.filter((r) => r.status === (q(path).get("status") ?? "pending")).sort((a, c) => a.preferred_at.localeCompare(c.preferred_at));
    case "POST /booking-requests": {
      if (!s(b.name) || (!s(b.phone) && !s(b.email))) fail(400, m("Escreva o nome e um telefone ou email.", "Enter a name and a phone or email."));
      requests.push({ id: uid(), name: s(b.name), phone: s(b.phone), email: s(b.email), patient_id: (b.patient_id as string) || null, service: s(b.service) || "session",
        preferred_at: s(b.preferred_at), message: s(b.message), status: "pending", created_at: nowIso(), appointment_id: null, source: "staff" });
      return { id: requests[requests.length - 1].id };
    }
    case "POST /booking-requests/:id/accept": {
      const r = requests.find((x) => x.id === id) ?? fail(404, "Not found");
      if (r.status !== "pending") fail(409, m("Este pedido já foi respondido.", "This request was already answered."));
      const res = createAppt({ ...b, repeat: undefined }, m);
      Object.assign(r, { status: "accepted", appointment_id: res.ids[0], patient_id: r.patient_id ?? b.patient_id ?? null });
      return { appointment_id: res.ids[0], overlaps: res.overlaps };
    }
    case "POST /booking-requests/:id/decline": {
      const r = requests.find((x) => x.id === id) ?? fail(404, "Not found");
      if (r.status !== "pending") fail(409, m("Este pedido já foi respondido.", "This request was already answered."));
      r.status = "declined"; return { ok: true };
    }
    case "POST /booking-requests/:id/reopen": {
      const r = requests.find((x) => x.id === id) ?? fail(404, "Not found");
      if (r.appointment_id && !appts.some((x) => x.id === r.appointment_id && x.status === "booked")) fail(409, m("A marcação deste pedido já foi usada, por isso não dá para desfazer.", "The appointment from this request has already been used, so it can’t be undone."));
      return { ok: true, patient_removed: reopenRequest({ appts, patients, requests, therapists: TH, role: () => role, me: () => ME[role], audit, createAppt, newPatient: () => "", dropPatient: (pid) => { if (appts.some((a) => a.patient_id === pid)) return false; const i = patients.findIndex((x) => x.id === pid); if (i >= 0) patients.splice(i, 1); return i >= 0; } }, r) };
    }

    case "GET /alerts": {
      const today = ymd(new Date()), in7 = addDays(new Date(), 7).toISOString(), in30 = ymd(addDays(new Date(), 30)), in14 = ymd(addDays(new Date(), 14)), now = nowIso();
      const validOf = (pid: string, k: ConsentKind) => consents.some((c) => c.patient_id === pid && c.kind === k && !c.revoked_at && (k !== "treatment" || c.granted) && (!c.expires_on || c.expires_on >= today));
      const missing = patients.filter((x) => x.active && appts.some((a) => a.patient_id === x.id && a.status === "booked" && a.starts_at >= now && a.starts_at <= in7))
        .map((x) => ({ patient_id: x.id, name: x.name, number: x.number, missing: (["treatment", "data"] as ConsentKind[]).filter((k) => !validOf(x.id, k)) })).filter((x) => x.missing.length);
      const expiring = consents.filter((c) => !c.revoked_at && c.expires_on && c.expires_on >= today && c.expires_on <= in30
        && !consents.some((c2) => c2.patient_id === c.patient_id && c2.kind === c.kind && c2.signed_at > c.signed_at)).map((c) => ({ patient_id: c.patient_id, name: P(c.patient_id).name, kind: c.kind, expires_on: c.expires_on }));
      const rx = rxs.map(rxUse).filter((r) => r.remaining > 0 && (r.remaining <= 2 || (r.valid_until && r.valid_until >= today && r.valid_until <= in14)))
        .map((r) => ({ patient_id: r.patient_id, name: P(r.patient_id).name, prescription_id: r.id, remaining: r.remaining, valid_until: r.valid_until }));
      const worse = clin() ? Object.values(tests.reduce<Record<string, typeof tests>>((acc, t) => { (acc[t.patient_id + t.test_code] ??= []).push(t); return acc; }, {}))
        .map((g) => g.sort((a, c) => c.taken_on.localeCompare(a.taken_on) || c.created_at.localeCompare(a.created_at)))
        .filter((g) => g.length > 1 && g[0].taken_on >= ymd(addDays(new Date(), -30)) && (g[0].higher_is_worse ? g[0].score > g[1].score : g[0].score < g[1].score))
        .map((g) => ({ patient_id: g[0].patient_id, name: P(g[0].patient_id).name, test_code: g[0].test_code, score: g[0].score, previous: g[1].score, taken_on: g[0].taken_on })) : null;
      const cdue = role !== "owner" ? null : complaints.filter((c) => !(c.source === "paper" ? c.sent_to_ers_on : c.replied_on))
        .map((c) => ({ id: c.id, received_on: c.received_on, due_on: c.due_on, complainant: c.complainant, working_days_left: workingDaysBetween(today, c.due_on) }))
        .filter((c) => c.working_days_left <= 3).sort((a, c) => a.due_on.localeCompare(c.due_on));
      return { booking_requests: requests.filter((r) => r.status === "pending").length, consents_missing: missing, consents_expiring: expiring, prescriptions: rx, worse_scores: worse, ...moneyAlerts(S3), complaints: cdue, devices: role === "owner" ? previewDeviceAlert() : null };
    }
  }
  return preview3(method, path, body, m, S3);
}
