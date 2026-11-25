// Browser preview only: stand-ins for the schedule half of the assistant and one-press booking of requests
// (server/src/routes/agenda.rs, 11 Nov 2026). Same answers, shapes and rules as the server, much simpler matching:
// the real understanding is the local AI's, which the preview doesn't have.
type M = (pt: string, en: string) => string;

type Body = Record<string, unknown>;
type Row = [string, string, string];
const fail = (status: number, message: string): never => { throw { status, message }; };
const uid = () => (crypto.randomUUID ? crypto.randomUUID() : Math.random().toString(36).slice(2));
const pad = (n: number) => String(n).padStart(2, "0");
const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const str = (v: unknown) => (typeof v === "string" ? v : "");
const fold = (s: string) => s.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/[^a-z0-9]+/g, " ").trim();
const later = <T,>(v: T, ms = 1200) => new Promise<T>((res) => setTimeout(() => res(v), ms));
const hmOf = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
const WD = { pt: ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"], en: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] };
const isoDow = (d: Date) => ((d.getDay() + 6) % 7) + 1;
const HOURS: Record<number, [number, number] | null> = { 1: [8, 20], 2: [8, 20], 3: [8, 20], 4: [8, 20], 5: [8, 20], 6: [9, 13], 7: null };

interface Ap { id: string; kind: string; starts_at: string; ends_at: string; status: string; therapist_id: string | null; patient_id: string | null; series_id: string | null; title: string; travel_minutes: number; admin_note: string }
interface Pt { id: string; name: string; number: number; phone: string | null; email: string | null; therapist_id: string | null; deleted?: boolean; anonymised_at: string | null }
interface Rq { id: string; name: string; phone: string; email: string; patient_id: string | null; service: string; preferred_at: string; message: string; status: string;
  appointment_id: string | null; source?: string; day_part?: string; address?: string; created_patient?: boolean }
export interface AgendaHooks {
  appts: Ap[]; patients: Pt[]; requests: Rq[]; therapists: { id: string; name: string }[];
  role: () => string; me: () => { id: string; ai_enabled?: boolean };
  audit: (action: string, pid: string | null, detail?: string) => void;
  createAppt: (b: Body, m: M) => { ids: string[] };
  newPatient: (name: string, phone: string, email: string) => string;
  dropPatient: (id: string) => boolean;
}

interface Change { id: string; skill: string; rows: Row[]; warnings: string[]; status: string; by: string; apply: () => void; undo: () => void }
const changes: Change[] = [];

const kindName = (k: string, en: boolean) => ({ assessment: en ? "Assessment" : "Avaliação", home: en ? "Home visit" : "Domicílio", class: en ? "Class" : "Aula" } as Record<string, string>)[k] ?? (en ? "Session" : "Sessão");
const mins = (k: string) => (k === "assessment" || k === "home" ? 60 : 45);
const when = (d: Date, en: boolean, time = true) => `${WD[en ? "en" : "pt"][isoDow(d) - 1]} ${pad(d.getDate())}/${pad(d.getMonth() + 1)}${time ? `, ${hmOf(d)}` : ""}`;

/** The first free times from `from` (a day), one therapist per time, inside opening hours. */
function free(H: AgendaHooks, from: Date, minutes: number, opts: { part?: string; days?: number; max?: number; around?: string; first?: string | null } = {}) {
  const out: { start: Date; end: Date; th: { id: string; name: string } }[] = [];
  const ths = [...H.therapists].sort((a, b) => (a.id === opts.first ? -1 : b.id === opts.first ? 1 : 0));
  const soon = Date.now() + 30 * 60000;
  for (let i = 0; i < (opts.days ?? 14) && out.length < (opts.max ?? 4); i++) {
    const d = new Date(from); d.setDate(d.getDate() + i); d.setHours(0, 0, 0, 0);
    const h = HOURS[isoDow(d)];
    if (!h) continue;
    let times: number[] = [];
    for (let t = h[0] * 60; t + minutes <= h[1] * 60; t += 15) times.push(t);
    times = times.filter((t) => opts.part === "morning" ? t < 780 : opts.part === "afternoon" ? t >= 780 : true);
    if (opts.around && i === 0) { const a = +opts.around.slice(0, 2) * 60 + +opts.around.slice(3, 5); times.sort((x, y) => Math.abs(x - a) - Math.abs(y - a) || x - y); }
    for (const t of times) {
      const s = new Date(d); s.setMinutes(t); const e = new Date(+s + minutes * 60000);
      if (+s < soon) continue;
      const th = ths.find((x) => !H.appts.some((a) => (a.status === "booked" || a.status === "done") && (a.therapist_id === x.id || !a.therapist_id)
        && +new Date(a.starts_at) - a.travel_minutes * 60000 < +e && +new Date(a.ends_at) + a.travel_minutes * 60000 > +s));
      if (th) { out.push({ start: s, end: e, th }); if (out.length >= (opts.max ?? 4)) break; }
    }
  }
  return out;
}

const looks = (H: AgendaHooks, r: Rq) => {
  const ph = r.phone.replace(/\D/g, "").slice(-9);
  return H.patients.filter((p) => !p.deleted && !p.anonymised_at && ((ph.length === 9 && (p.phone ?? "").replace(/\D/g, "").slice(-9) === ph) || (r.email && p.email?.toLowerCase() === r.email.toLowerCase()) || fold(p.name) === fold(r.name)))
    .map((p) => ({ id: p.id, number: p.number, name: p.name, reason: ph.length === 9 && (p.phone ?? "").replace(/\D/g, "").slice(-9) === ph ? "phone" : r.email && p.email?.toLowerCase() === r.email.toLowerCase() ? "email" : "name" }));
};
const svc = (s: string) => (s === "assessment" || s === "home" ? s : "session");

function bookRequest(H: AgendaHooks, m: M, r: Rq, start: string, end: string | null, th: string, pid: string | null) {
  if (r.status !== "pending") fail(409, m("Este pedido já foi respondido.", "This request was already answered."));
  const kind = svc(r.service);
  const created = !(pid ?? r.patient_id);
  const patient = pid ?? r.patient_id ?? H.newPatient(r.name, r.phone, r.email);
  const res = H.createAppt({ kind, starts_at: start, ends_at: end ?? new Date(+new Date(start) + mins(kind) * 60000).toISOString(), therapist_id: th, patient_id: patient,
    address: kind === "home" ? r.address : "", travel_minutes: kind === "home" ? 15 : 0, admin_note: r.message, allow_overlap: true }, m);
  Object.assign(r, { status: "accepted", appointment_id: res.ids[0], patient_id: patient, created_patient: created });
  H.audit("booking_request.accepted", patient);
  return { appointment_id: res.ids[0], patient_id: patient, patient_created: created, overlaps: [] };
}
export function reopenRequest(H: AgendaHooks, r: Rq) {
  if (r.appointment_id) { const i = H.appts.findIndex((x) => x.id === r.appointment_id); if (i >= 0) H.appts.splice(i, 1); }
  let removed = false;
  if (r.created_patient && r.patient_id) { removed = H.dropPatient(r.patient_id); if (removed) r.patient_id = null; }
  Object.assign(r, { status: "pending", appointment_id: null, created_patient: false });
  return removed;
}

/** Schedule stand-ins, or undefined if the request isn't one of them. */
export function previewAgenda(method: string, path: string, body: unknown, m: M, H: AgendaHooks): unknown {
  const b = (body ?? {}) as Body;
  const seg = path.split("?")[0].split("/").filter(Boolean);
  const en = b.lang === "en";
  const L = (pt: string, e: string) => (en ? e : pt);

  if (seg[0] === "booking-requests" && seg[2] === "suggest" && method === "GET") {
    const r = H.requests.find((x) => x.id === seg[1]) ?? fail(404, "Not found");
    const kind = svc(r.service), pref = new Date(r.preferred_at);
    const linked = r.patient_id ? H.patients.find((p) => p.id === r.patient_id) : undefined;
    const today = new Date(); today.setHours(0, 0, 0, 0);
    let slots = free(H, pref < today ? today : pref, mins(kind), { part: r.source === "website" ? r.day_part : "", around: r.source === "website" ? undefined : hmOf(pref), days: 1, max: 4, first: linked?.therapist_id });
    if (slots.length < 4) { const n = new Date(pref); n.setDate(n.getDate() + 1); slots = [...slots, ...free(H, n, mins(kind), { part: r.source === "website" ? r.day_part : "", days: 14, max: 4 - slots.length })]; }
    const sl = (x: (typeof slots)[number]) => ({ starts_at: x.start.toISOString(), ends_at: x.end.toISOString(), therapist_id: x.th.id, therapist_name: x.th.name });
    return { kind, minutes: mins(kind), slot: slots[0] ? sl(slots[0]) : null, alternatives: slots.slice(1).map(sl),
      patient: linked ? { id: linked.id, name: linked.name, number: linked.number } : null, lookalikes: linked ? [] : looks(H, r) };
  }
  if (seg[0] === "booking-requests" && seg[2] === "book" && method === "POST") {
    const r = H.requests.find((x) => x.id === seg[1]) ?? fail(404, "Not found");
    return bookRequest(H, m, r, str(b.starts_at), str(b.ends_at) || null, str(b.therapist_id), (b.patient_id as string) || null);
  }
  if (seg[0] === "booking-requests" && seg[2] === "reopen" && method === "POST") {
    const r = H.requests.find((x) => x.id === seg[1]) ?? fail(404, "Not found");
    return { ok: true, patient_removed: reopenRequest(H, r) };
  }

  if (seg[0] === "assistant" && seg[1] === "changes") {
    const c = changes.find((x) => x.id === seg[2]);
    if (!c) return undefined; // one of the owner's changes (previewAi)
    if (H.role() !== "owner" && c.by !== H.me().id) fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
    if (seg[3] === "apply") { if (c.status !== "pending") fail(409, m("Esta alteração já foi aplicada ou cancelada.", "This change was already applied or cancelled.")); c.apply(); c.status = "applied"; H.audit("assistant.applied", null, c.skill); return { ok: true, status: "applied" }; }
    if (seg[3] === "cancel") { if (c.status !== "pending") fail(409, m("Esta alteração já foi aplicada ou cancelada.", "This change was already applied or cancelled.")); c.status = "cancelled"; return { ok: true, status: "cancelled" }; }
    if (seg[3] === "undo") { if (c.status !== "applied") fail(409, m("Só se pode anular uma alteração aplicada.", "Only an applied change can be undone.")); c.undo(); c.status = "undone"; H.audit("assistant.undone", null, c.skill); return { ok: true, status: "undone" }; }
  }

  if (!(seg[0] === "ai" && seg[1] === "assistant" && method === "POST")) return undefined;
  if (H.role() !== "owner" && !H.me().ai_enabled) fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this."));
  const msg = str(b.message), t = fold(msg);
  const reply = (text: string, extra: Body = {}) => later({ reply: text, change: null, rows: null, list: null, choices: null, model: "gemma4:12b", ...extra });
  const propose = (skill: string, rows: Row[], warnings: string[], apply: () => void, undo: () => void, text: string, extra: Body = {}, undoable = true) => {
    const c: Change = { id: uid(), skill, rows, warnings, status: "pending", by: H.me().id, apply, undo };
    changes.push(c);
    return later({ reply: text, change: { id: c.id, skill, rows, warnings, status: "pending", undoable }, rows: null, list: null, choices: null, model: "gemma4:12b", ...extra });
  };
  const today = new Date(); today.setHours(0, 0, 0, 0);
  const dayIn = (): Date | null => {
    if (/amanha|tomorrow/.test(t)) { const d = new Date(today); d.setDate(d.getDate() + 1); return d; }
    if (/\bhoje\b|today/.test(t)) return new Date(today);
    const names = [["segunda", "monday"], ["terca", "tuesday"], ["quarta", "wednesday"], ["quinta", "thursday"], ["sexta", "friday"], ["sabado", "saturday"], ["domingo", "sunday"]];
    const i = names.findIndex((n) => n.some((x) => t.includes(x)));
    if (i < 0) return null;
    const d = new Date(today); do d.setDate(d.getDate() + 1); while (isoDow(d) !== i + 1); return d;
  };
  const timeIn = () => { const x = msg.match(/(\d{1,2})\s*(?:h|:)\s*(\d{2})?/i); return x ? `${pad(+x[1])}:${x[2] ?? "00"}` : null; };
  const live = H.patients.filter((p) => !p.deleted && !p.anonymised_at);
  const named = live.filter((p) => { const n = fold(p.name).split(" "); const words = t.split(" "); return n.filter((w) => w.length > 2 && words.includes(w)).length >= Math.min(2, n.length) || (words.includes(n[0]) && n.length && live.filter((q) => fold(q.name).split(" ")[0] === n[0]).length > 1); });
  const nameOf = (p: Pt) => `${p.name} (nº ${p.number})`;
  const item = (a: Ap) => { const p = H.patients.find((x) => x.id === a.patient_id); const th = H.therapists.find((x) => x.id === a.therapist_id);
    return { id: a.id, when: `${when(new Date(a.starts_at), en)}–${hmOf(new Date(a.ends_at))}`, title: a.kind === "class" ? a.title || kindName("class", en) : p ? nameOf(p) : a.title, sub: [kindName(a.kind, en), th?.name].filter(Boolean).join(" · "), status: a.status, patient_id: a.patient_id }; };
  H.audit("assistant.asked", null);

  // free times
  if (/vaga|livre|free|disponiv/.test(t)) {
    const d = dayIn() ?? new Date(today);
    const part = /manha|morning/.test(t) ? "morning" : /tarde|afternoon/.test(t) ? "afternoon" : "";
    const sl = free(H, d, 45, { part, days: 7, max: 6 });
    return reply(L("Horas livres para sessão de 45 min:", "Free times for a 45-minute session:"), { list: sl.map((x) => ({ when: when(x.start, en), title: x.th.name, sub: "45 min" })),
      steps: [L(`Procurou horas livres a partir de ${when(d, false, false)}`, `Looked for free times from ${when(d, true, false)}`)] });
  }
  // a patient's file (the assistant with tools, 13 Nov 2026): what it looked up shows above the answer
  if (/ficha|como esta|como correram|telefone|telemovel|phone|notas|notes|how is|how did/.test(t) && named.length === 1) {
    const p = named[0];
    H.audit("patient.viewed", p.id, H.role() === "reception" ? "reduced" : "full");
    const step = L(`Abriu a ficha de ${nameOf(p)}`, `Opened ${nameOf(p)}’s file`);
    if (/telefone|telemovel|phone/.test(t) || H.role() === "reception") return reply(p.phone ? L(`O telemóvel de ${p.name} é ${p.phone}.`, `${p.name}’s phone is ${p.phone}.`) : L(`${p.name} não tem telemóvel na ficha.`, `${p.name} has no phone in the file.`), { steps: [step] });
    return reply(L(`Nas duas últimas sessões, ${p.name} referiu menos dor (de 6 para 4 em 10) e tolerou bem os exercícios. A próxima está na agenda.`, `In the last two sessions ${p.name} reported less pain (6 down to 4 out of 10) and tolerated the exercises well. The next one is in the schedule.`), { steps: [step] });
  }
  // accept the waiting requests
  if (/pedido|request/.test(t) && /aceit|accept|marca|book/.test(t)) {
    const r = H.requests.find((x) => x.status === "pending");
    if (!r) return reply(L("Não há pedidos à espera.", "There are no waiting requests."));
    const kind = svc(r.service), pref = new Date(r.preferred_at);
    const sl = free(H, pref, mins(kind), { part: r.source === "website" ? r.day_part : "", days: 14, max: 1 })[0];
    if (!sl) return reply(L("Não encontrei uma hora livre que sirva.", "I couldn’t find a free time that fits."));
    const lk = looks(H, r);
    const warnings = [...(r.patient_id ? [] : [L("Cria uma ficha nova com o nome e os contactos do pedido.", "Makes a new patient file from the request’s name and contacts.")]),
      ...(lk[0] ? [L(`Parece ser ${lk[0].name} (nº ${lk[0].number}) — se for a mesma pessoa, marque em Agenda › Pedidos de marcação.`, `Looks like ${lk[0].name} (no. ${lk[0].number}) — if it’s the same person, book from Schedule › Booking Requests.`)] : []),
      L("A pessoa não é avisada automaticamente: ligue ou escreva-lhe a confirmar.", "The person isn’t told automatically: call or write to confirm.")];
    const asked = r.source === "website" ? (r.day_part === "morning" ? L("de manhã", "morning") : r.day_part === "afternoon" ? L("à tarde", "afternoon") : L("a qualquer hora", "any time")) : hmOf(pref);
    return propose("accept_request", [[r.name, `${kindName(kind, en)} · ${when(pref, en, false)}, ${asked}`, `${when(sl.start, en)}–${hmOf(sl.end)} · ${sl.th.name}`]], warnings,
      () => { bookRequest(H, m, r, sl.start.toISOString(), sl.end.toISOString(), sl.th.id, null); }, () => { reopenRequest(H, r); },
      L(`Aqui está a marcação para o pedido de ${r.name}.`, `Here’s the booking for ${r.name}’s request.`));
  }
  // questions about the agenda
  if (/o que temos|que tenho|what s on|what do i have|agenda|proxima|next/.test(t) && !/marca|book|cancel|passa|move/.test(t)) {
    const p = named.length === 1 ? named[0] : null;
    if (p) {
      const nx = H.appts.filter((a) => a.patient_id === p.id && a.status === "booked" && +new Date(a.starts_at) > Date.now()).sort((a, c) => a.starts_at.localeCompare(c.starts_at));
      return reply(nx[0] ? L(`A próxima de ${p.name} é ${when(new Date(nx[0].starts_at), false)}.`, `${p.name}’s next one is ${when(new Date(nx[0].starts_at), true)}.`) : L(`${p.name} não tem nada marcado.`, `${p.name} has nothing booked.`), { list: nx.slice(0, 3).map(item) });
    }
    const d = dayIn() ?? new Date(today); const e = new Date(d); e.setDate(e.getDate() + 1);
    const list = H.appts.filter((a) => a.status !== "cancelled" && a.kind !== "block" && +new Date(a.starts_at) >= +d && +new Date(a.starts_at) < +e).sort((a, c) => a.starts_at.localeCompare(c.starts_at));
    const n = list.length;
    const seenStep = [L(`Viu a agenda: ${when(d, false, false)}`, `Looked at the schedule: ${when(d, true, false)}`)];
    return reply(n ? L(`${when(d, false, false)}: ${n} marcaç${n === 1 ? "ão" : "ões"}, a primeira às ${hmOf(new Date(list[0].starts_at))} e a última às ${hmOf(new Date(list[n - 1].starts_at))}.`, `${when(d, true, false)}: ${n} appointment${n === 1 ? "" : "s"}, the first at ${hmOf(new Date(list[0].starts_at))} and the last at ${hmOf(new Date(list[n - 1].starts_at))}.`)
      : L("Nada marcado nesse dia.", "Nothing booked that day."), { list: list.map(item), steps: seenStep });
  }
  // money (the assistant, stage 2, 13 Nov 2026): a stand-in for record_payment, owed_list and void_payment
  const eur = (c: number) => en ? `€${(c / 100).toFixed(2)}` : `${(c / 100).toFixed(2).replace(".", ",")} €`;
  if (/deve|owe/.test(t) && !named.length && H.role() === "owner") {
    const who = live.slice(0, 3);
    return reply(L(`${who.length} utentes devem ${eur(who.length * 2000 + 4000)}: ${who.map((p) => p.name).join(", ")}. O valor mais antigo é de há duas semanas.`,
      `${who.length} patients owe ${eur(who.length * 2000 + 4000)}: ${who.map((p) => p.name).join(", ")}. The oldest is from two weeks ago.`), { steps: [L("Viu quem deve dinheiro", "Looked at who owes money")] });
  }
  if (/pagou|paid|pagamento|payment/.test(t) && named.length === 1 && !/anula|void/.test(t)) {
    const p = named[0];
    const how = /mb ?way/.test(t) ? "MB WAY" : /transfer/.test(t) ? L("Transferência", "Transfer") : /cart|card|multibanco/.test(t) ? L("Cartão (TPA)", "Card") : /dinheiro|numerario|cash/.test(t) ? L("Numerário", "Cash") : null;
    if (!how) return reply(L(`Como pagou ${p.name}: numerário, MB WAY, transferência ou cartão?`, `How did ${p.name} pay: cash, MB WAY, transfer or card?`), { steps: [L(`Viu os pagamentos de ${nameOf(p)}`, `Looked at ${nameOf(p)}’s payments`)] });
    const d1 = new Date(today); d1.setDate(d1.getDate() - 2); const d2 = new Date(today); d2.setDate(d2.getDate() - 1);
    const dd = (d: Date) => d.toLocaleDateString("pt-PT", { day: "2-digit", month: "2-digit", year: "numeric" });
    return propose("payment", [[`${nameOf(p)} — ${kindName("session", en)} ${dd(d1)}`, "—", `${eur(2000)} · ${how}`], [`${nameOf(p)} — ${kindName("home", en)} ${dd(d2)}`, "—", `${eur(4000)} · ${how}`]],
      [L("Depois de emitir a fatura no programa certificado, junte o número ao pagamento.", "After issuing the invoice in the certified software, add its number to the payment."),
       L("É um registo fiscal: depois de aplicado, só se anula (fica no histórico).", "It’s a fiscal record: once applied it can only be voided (it stays in the records).")],
      () => { H.audit("payment.recorded", p.id, "x2"); }, () => { H.audit("payment.voided", p.id, "assistant undo"); },
      L(`${p.name} devia ${eur(6000)}. Preparei os dois pagamentos.`, `${p.name} owed ${eur(6000)}. I’ve prepared both payments.`),
      { steps: [L(`Viu os pagamentos de ${nameOf(p)}`, `Looked at ${nameOf(p)}’s payments`)] });
  }
  if (/anula|void/.test(t) && /pagamento|payment/.test(t)) {
    const n = msg.match(/\d+/)?.[0] ?? "12";
    return propose("payment_void", [[`${L("Pagamento nº", "Payment no.")} ${n} — ${live[0]?.name ?? ""} · ${kindName("session", en)}`, eur(2000), `${L("Anulado", "Voided")} — ${L("Valor errado", "Wrong amount")}`]],
      [L("Não dá para desfazer: o pagamento anulado fica no histórico. Só o proprietário ou quem o registou, no próprio dia, pode anular.", "This can’t be undone: the voided payment stays in the records. Only the owner, or whoever recorded it on the same day, can void it.")],
      () => { H.audit("payment.voided", null, n); }, () => {}, L("Anulo este pagamento.", "I’ll void this payment."), {}, false);
  }
  // documents, letters, legal tools, settings (the assistant, stage 3, 13 Nov 2026): stand-ins for draft_letter,
  // reminders, complaints and website
  if (/carta|letter/.test(t) && named.length === 1 && H.role() !== "reception") {
    const p = named[0];
    H.audit("patient.viewed", p.id, "letter");
    return propose("letter_draft", [[`${nameOf(p)} — ${L("Carta de alta (rascunho)", "Discharge Letter (Draft)")}`, "—",
      `${L("Modelo da ficha", "From the file’s template")}: ${L(`${p.name} realizou sessões de fisioterapia nesta clínica por [a completar]. Na avaliação inicial…`, `${p.name} attended physiotherapy sessions at this clinic for [to complete]. At the first assessment…`)}`]],
      [L("Fica como rascunho na ficha. Só o proprietário aprova cartas, depois de as ler.", "It’s saved as a draft in the file. Only the owner approves letters, after reading them."),
       L("Tem partes marcadas [a completar] para preencher antes de aprovar.", "It has parts marked [to complete] to fill in before approving.")],
      () => { H.audit("letter.created", p.id); }, () => { H.audit("letter.draft_deleted", p.id); },
      L(`Preparei o rascunho da carta de alta de ${p.name} a partir da ficha.`, `I’ve prepared a draft discharge letter for ${p.name} from the file.`),
      { steps: [L(`Abriu a ficha de ${nameOf(p)}`, `Opened ${nameOf(p)}’s file`)] });
  }
  if (/carta|letter/.test(t) && !named.length && H.role() !== "reception") return reply(L("De quem é a carta? Diga o nome do utente como está na ficha.", "Whose letter is it? Use the patient’s name as in their file."));
  if (/lembr|remind/.test(t)) {
    const d = new Date(today); d.setDate(d.getDate() + 1); const e2 = new Date(d); e2.setDate(e2.getDate() + 1);
    const list = H.appts.filter((a) => a.status === "booked" && a.patient_id && +new Date(a.starts_at) >= +d && +new Date(a.starts_at) < +e2).sort((a, c) => a.starts_at.localeCompare(c.starts_at));
    return reply(list.length ? L(`Amanhã há ${list.length} ${list.length === 1 ? "pessoa" : "pessoas"} por lembrar. Envie em Lembretes e diga-me quando estiver feito.`, `${list.length} ${list.length === 1 ? "person" : "people"} to remind tomorrow. Send them from Reminders and tell me when it’s done.`)
      : L("Amanhã não há ninguém para lembrar.", "Nobody to remind tomorrow."), { list: list.map(item), steps: [L(`Viu os lembretes de ${when(d, false, false)}`, `Looked at the reminders for ${when(d, true, false)}`)] });
  }
  if (/reclama|complaint/.test(t) && H.role() === "owner") {
    return reply(L("Não há reclamações por responder. A última foi respondida dentro do prazo.", "There are no complaints waiting for an answer. The last one was answered in time."), { steps: [L("Viu o livro de reclamações", "Looked at the complaints log")] });
  }
  if (/site|website/.test(t) && /preco|price/.test(t) && H.role() === "owner") {
    return propose("website", [[L("Site — Mostrar preços", "Website — Show Prices"), L("Sim", "Yes"), L("Não", "No")]], [L("O site público muda logo que aplicar.", "The public site changes as soon as you apply.")],
      () => { H.audit("website.changed", null, "show_prices"); }, () => { H.audit("website.changed", null, "show_prices"); },
      L("Escondo os preços no site.", "I’ll hide the prices on the website."), { steps: [L("Viu as definições da clínica", "Looked at the clinic’s settings")] });
  }
  const action = /cancel/.test(t) ? "cancel" : /passa|muda|move|resched/.test(t) ? "move" : /marca|book/.test(t) ? "book" : null;
  if (!action) return undefined; // the owner's jobs (previewAi)
  if (named.length > 1) return reply(L("Há mais do que um utente com esse nome. Qual?", "More than one patient has that name. Which one?"), { choices: named.slice(0, 6).map((p) => ({ label: `${nameOf(p)}${p.phone ? ` · …${p.phone.replace(/\D/g, "").slice(-3)}` : ""}`, send: `${msg.replace(/\b(a|o)\s+\w+$/i, "")} ${nameOf(p)}` })) });
  const p = named[0];
  if (!p) return reply(L("Para quem? Diga o nome do utente como está na ficha.", "Who for? Use the patient’s name as in their file."));
  if (action === "book") {
    const d = dayIn(), tm = timeIn();
    if (!d) return reply(L("Para que dia?", "For which day?"));
    if (!tm) {
      const sl = free(H, d, 45, { days: 1, max: 6, first: p.therapist_id });
      return reply(L("A que horas? Estas estão livres:", "What time? These are free:"), { choices: sl.map((x) => ({ label: `${when(x.start, en)} · ${x.th.name}`, send: `${msg} ${L("às", "at")} ${hmOf(x.start)}` })) });
    }
    const s = new Date(d); s.setHours(+tm.slice(0, 2), +tm.slice(3), 0, 0); const e = new Date(+s + 45 * 60000);
    const th = H.therapists.find((x) => x.id === p.therapist_id) ?? H.therapists.find((x) => x.id === H.me().id) ?? H.therapists[0];
    const clash = H.appts.filter((a) => a.status === "booked" && a.therapist_id === th.id && +new Date(a.starts_at) < +e && +new Date(a.ends_at) > +s);
    let ids: string[] = [];
    return propose("book", [[nameOf(p), "—", `${when(s, en)}–${hmOf(e)} · ${kindName("session", en)} · ${th.name}`]],
      clash.length ? [L(`Sobrepõe-se a ${clash.length} marcaç${clash.length === 1 ? "ão" : "ões"} do mesmo terapeuta. Aplicar marca na mesma.`, `Overlaps ${clash.length} other appointment${clash.length === 1 ? "" : "s"} of the same therapist. Apply books it anyway.`)] : [],
      () => { ids = H.createAppt({ kind: "session", starts_at: s.toISOString(), ends_at: e.toISOString(), therapist_id: th.id, patient_id: p.id, allow_overlap: true }, m).ids; },
      () => { for (const id of ids) { const i = H.appts.findIndex((x) => x.id === id); if (i >= 0) H.appts.splice(i, 1); } },
      L(`Aqui está a marcação para ${p.name}.`, `Here’s the appointment for ${p.name}.`));
  }
  const nx = H.appts.filter((a) => a.patient_id === p.id && a.status === "booked" && +new Date(a.starts_at) > Date.now()).sort((a, c) => a.starts_at.localeCompare(c.starts_at))[0];
  if (!nx) return reply(L(`${p.name} não tem marcações por realizar.`, `${p.name} has no booked appointments.`));
  const th = H.therapists.find((x) => x.id === nx.therapist_id)?.name ?? "";
  const was = `${when(new Date(nx.starts_at), en)} · ${th}`;
  if (action === "cancel") {
    return propose("cancel", [[`${nameOf(p)} · ${when(new Date(nx.starts_at), en)}–${hmOf(new Date(nx.ends_at))}`, L("Marcada", "Booked"), L("Cancelada", "Cancelled")]], [],
      () => { nx.status = "cancelled"; }, () => { nx.status = "booked"; }, L("Cancelo esta marcação.", "I’ll cancel this appointment."));
  }
  const d = dayIn() ?? new Date(nx.starts_at), tm = timeIn() ?? hmOf(new Date(nx.starts_at));
  const s = new Date(d); s.setHours(+tm.slice(0, 2), +tm.slice(3), 0, 0); const len = +new Date(nx.ends_at) - +new Date(nx.starts_at); const e = new Date(+s + len);
  const before = [nx.starts_at, nx.ends_at];
  return propose("move", [[nameOf(p), was, `${when(s, en)}–${hmOf(e)} · ${th}`]], nx.series_id ? [L("Muda só esta da série.", "Only this one of the series changes.")] : [],
    () => { nx.starts_at = s.toISOString(); nx.ends_at = e.toISOString(); }, () => { [nx.starts_at, nx.ends_at] = before; }, L("Passo a marcação para esta hora.", "I’ll move the appointment to this time."));
}
