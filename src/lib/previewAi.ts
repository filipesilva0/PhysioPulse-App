// Browser preview only: stand-ins for the phase 4 API (server/src/routes/assist.rs and assistant.rs) — note rewrite,
// receipt reading, monthly reports, the admin assistant, opening hours and closures. Same roles and checks as the
// server (owner only for everything AI); the "AI" answers are canned, with a short delay. Nothing is saved.
import type { Expense, Product, Totals } from "./money";
import type { PreviewState } from "./preview3";

type M = (pt: string, en: string) => string;
type Body = Record<string, unknown>;
type Row = [string, string, string];
export interface AiHooks {
  products: Product[];
  expenses: Omit<Expense, "filename" | "closed">[];
  pay: Record<string, number>;
  staff: { id: string; name: string; role: string }[];
  totals: (month: string) => Totals;
}

const fail = (status: number, message: string, error?: string): never => { throw { status, message, code: error }; };
const uid = () => (crypto.randomUUID ? crypto.randomUUID() : Math.random().toString(36).slice(2));
const pad = (n: number) => String(n).padStart(2, "0");
const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const later = <T,>(v: T, ms = 1400) => new Promise<T>((res) => setTimeout(() => res(v), ms));
const str = (v: unknown) => (typeof v === "string" ? v : "");
const fold = (s: string) => s.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/[^a-z0-9]+/g, " ").trim();
const eur = (c: number, en: boolean) => en ? `€${(c / 100).toLocaleString("en-GB", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}` : `${(c / 100).toLocaleString("pt-PT", { minimumFractionDigits: 2, maximumFractionDigits: 2 })} €`;
const dmy = (iso: string) => `${iso.slice(8, 10)}/${iso.slice(5, 7)}/${iso.slice(0, 4)}`;
const MONTHS_PT = ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
const MONTHS_EN = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const monthName = (ym: string, en: boolean) => `${(en ? MONTHS_EN : MONTHS_PT)[Number(ym.slice(5, 7)) - 1]}${en ? " " : " de "}${ym.slice(0, 4)}`;
const prevMonth = (ym: string) => { const d = new Date(Number(ym.slice(0, 4)), Number(ym.slice(5, 7)) - 2, 1); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}`; };

type Day = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun";
const DAYS: Day[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const DAY_NAMES: Record<Day, [string, string]> = { mon: ["Segunda-feira", "Monday"], tue: ["Terça-feira", "Tuesday"], wed: ["Quarta-feira", "Wednesday"], thu: ["Quinta-feira", "Thursday"], fri: ["Sexta-feira", "Friday"], sat: ["Sábado", "Saturday"], sun: ["Domingo", "Sunday"] };
const hours: Record<Day, [string, string] | null> = { mon: ["08:00", "20:00"], tue: ["08:00", "20:00"], wed: ["08:00", "20:00"], thu: ["08:00", "20:00"], fri: ["08:00", "20:00"], sat: ["09:00", "13:00"], sun: null };
const closures: { id: string; starts_on: string; ends_on: string; label: string }[] = [];
const reports: { id: string; month: string; lang: "pt" | "en"; text: string; model: string; file_id: string; created_at: string; created_by_name: string }[] = [];
type Change = { id: string; skill: string; target: string | null; params: Body; before: unknown; after: unknown; rows: Row[]; warnings: string[]; status: "pending" | "applied" | "cancelled" | "undone" };
const changes: Change[] = [];
const MODEL = "gemma4:12b";

function facts(H: AiHooks, S: PreviewState, month: string, en: boolean): Row[] {
  const t = H.totals(month), p = H.totals(prevMonth(month));
  const act = (mo: string) => {
    const a = S.appts.filter((x) => x.starts_at.slice(0, 7) === mo);
    return { done: a.filter((x) => x.kind === "session" && x.status === "done").length, ass: a.filter((x) => x.kind === "assessment" && x.status === "done").length,
      home: a.filter((x) => x.kind === "home" && x.status === "done").length, classes: a.filter((x) => x.kind === "class" && x.status !== "cancelled").length,
      ns: a.filter((x) => x.kind !== "block" && x.status === "no_show").length, cx: a.filter((x) => x.kind !== "block" && x.status === "cancelled").length,
      seen: new Set(a.filter((x) => x.status === "done" && x.patient_id).map((x) => x.patient_id)).size };
  };
  const a = act(month), b = act(prevMonth(month));
  const L = (pt: string, e: string) => (en ? e : pt);
  return [
    [L("Receita", "Revenue"), eur(t.revenue_cents, en), eur(p.revenue_cents, en)], [L("Despesas", "Expenses"), eur(t.expenses_cents, en), eur(p.expenses_cents, en)],
    [L("Salários", "Salaries"), eur(t.salaries_cents, en), eur(p.salaries_cents, en)], [L("Resultado", "Result"), eur(t.result_cents, en), eur(p.result_cents, en)],
    [L("Pagamentos recebidos", "Payments received"), String(t.payments), String(p.payments)], [L("Sessões realizadas", "Sessions done"), String(a.done), String(b.done)],
    [L("Avaliações iniciais", "First assessments"), String(a.ass), String(b.ass)], [L("Domicílios", "Home visits"), String(a.home), String(b.home)],
    [L("Aulas de grupo", "Group classes"), String(a.classes), String(b.classes)], [L("Faltas", "No-shows"), String(a.ns), String(b.ns)],
    [L("Cancelamentos", "Cancellations"), String(a.cx), String(b.cx)], [L("Utentes atendidos", "Patients seen"), String(a.seen), String(b.seen)],
    ...(t.uninvoiced > 0 ? [[L("Pagamentos sem fatura registada", "Payments with no invoice recorded"), `${t.uninvoiced} (${eur(t.uninvoiced_cents, en)})`, "—"] as Row] : []),
  ];
}

/** A canned "AI" reading of what the owner typed (the real server asks the local model, then checks everything). */
function understand(msg: string, H: AiHooks): Body {
  const t = fold(msg);
  const euros = Number((msg.match(/(\d+(?:[.,]\d+)?)\s*(?:€|eur|euros?)?/i)?.[1] ?? "").replace(",", "."));
  if (/resum|summar|relatori|report/.test(t)) return { skill: "report", month: ymd(new Date()).slice(0, 7) };
  if (/fech|close|encerr/.test(t) && /(\d{1,2})/.test(t) && !/sabado|domingo|saturday|sunday|segunda|monday/.test(t)) {
    const y = new Date().getFullYear(), mo = /dez|dec/.test(t) ? 12 : new Date().getMonth() + 1;
    const days = [...t.matchAll(/\b(\d{1,2})\b/g)].map((x) => Number(x[1])).filter((n) => n >= 1 && n <= 31);
    const iso = (d: number) => `${y}-${pad(mo)}-${pad(d)}`;
    return { skill: "closure", from: iso(days[0]), to: iso(days[1] ?? days[0]), label: /natal|christmas/.test(t) ? "Natal" : "" };
  }
  const day = DAYS.find((d) => t.includes(fold(DAY_NAMES[d][0]).split(" ")[0]) || t.includes(DAY_NAMES[d][1].toLowerCase()));
  if (day) {
    const hs = [...msg.matchAll(/(\d{1,2})(?:[:h](\d{2}))?\s*h?/g)].map((x) => `${pad(Number(x[1]))}:${x[2] ?? "00"}`);
    return /fechad|closed/.test(t) ? { skill: "hours", day, closed: true } : { skill: "hours", day, open: hs[0], close: hs[1] };
  }
  const person = H.staff.find((s) => t.includes(fold(s.name).split(" ")[0]));
  if (person && /desativ|deactivat/.test(t)) return { skill: "staff_active", staff: person.name, active: false };
  if (person && /reativ|reactivat/.test(t)) return { skill: "staff_active", staff: person.name, active: true };
  if (person && euros) return { skill: "staff_pay", staff: person.name, amount_eur: euros };
  const prod = H.products.find((p) => t.includes(fold(p.name_pt).split(" ").slice(0, 2).join(" ")) || t.includes(fold(p.name_en).split(" ").slice(0, 2).join(" ")))
    ?? (/avalia|assessment/.test(t) ? H.products.find((p) => p.code === "assessment") : undefined);
  if (prod && /deixa de vender|stop selling|retir/.test(t)) return { skill: "product_active", product: prod.code, active: false };
  if (prod && euros) return { skill: "price", product: prod.code, price_eur: euros };
  return { skill: "answer" };
}

/** Phase 4 stand-ins, or undefined if the path isn't one of them. */
export function previewAi(method: string, path: string, body: unknown, m: M, S: PreviewState, H: AiHooks): unknown {
  const b = (body ?? {}) as Body;
  const [p, qs] = path.split("?");
  const q = new URLSearchParams(qs ?? "");
  const seg = p.split("/").filter(Boolean);
  const owner = () => { if (S.role() !== "owner") fail(403, m("A sua função não permite fazer isto.", "Your role doesn’t allow this.")); };
  const today = ymd(new Date());
  const en = b.lang === "en" || q.get("lang") === "en";
  const L = (pt: string, e: string) => (en ? e : pt);

  // ── opening hours and closures ──
  if (seg[0] === "clinic" && seg[1] === "hours") {
    if (method === "GET") return { hours, closures: closures.filter((c) => c.ends_on >= today).map((c) => ({ ...c, days: S.appts.filter((a) => a.admin_note === `closure:${c.id}`).length })).sort((a, c) => a.starts_on.localeCompare(c.starts_on)) };
    owner();
    const h = b.hours as Record<Day, [string, string] | null>;
    if (!h || DAYS.some((d) => h[d] !== null && !(Array.isArray(h[d]) && h[d]![0] < h[d]![1]))) fail(400, m("Cada dia precisa de uma hora de abertura antes da de fecho, como 08:00–20:00, ou Fechado.", "Each day needs an opening time before its closing time, like 08:00–20:00, or Closed."));
    Object.assign(hours, h); S.audit("clinic.hours_changed", null); return { ok: true };
  }
  const span = (from: string, to: string) => {
    if (!from || !to) fail(400, m("Escolha o primeiro dia.", "Choose the first day."));
    if (from < today) fail(400, m("Escolha hoje ou um dia a seguir.", "Choose today or a later day."));
    if (to < from) fail(400, m("O último dia tem de ser igual ou depois do primeiro.", "The last day must be on or after the first day."));
    const out: string[] = []; for (let d = new Date(from + "T12:00"); ymd(d) <= to; d.setDate(d.getDate() + 1)) out.push(ymd(d));
    if (out.length > 93) fail(400, m("Um encerramento pode ter no máximo três meses. Para mais tempo, acrescente outro.", "A closure can be at most three months. Add another one for longer."));
    const open = out.filter((d) => hours[DAYS[(new Date(d + "T12:00").getDay() + 6) % 7]]);
    const booked = S.appts.filter((a) => a.kind !== "block" && a.status === "booked" && a.starts_at.slice(0, 10) >= from && a.starts_at.slice(0, 10) <= to).length;
    return { open, booked };
  };
  const close = (from: string, to: string, label: string) => {
    const { open, booked } = span(from, to);
    const c = { id: uid(), starts_on: from, ends_on: to, label: label || "Clínica fechada" }; closures.push(c);
    for (const d of open) {
      const h = hours[DAYS[(new Date(d + "T12:00").getDay() + 6) % 7]]!;
      S.appts.push({ id: uid(), kind: "block", starts_at: new Date(`${d}T${h[0]}`).toISOString(), ends_at: new Date(`${d}T${h[1]}`).toISOString(), status: "booked", therapist_id: null, patient_id: null,
        series_id: null, prescription_id: null, title: c.label, color: "#56605D", address: "", travel_minutes: 0, capacity: null, admin_note: `closure:${c.id}`, attendees: [] } as unknown as PreviewState["appts"][number]);
    }
    return { id: c.id, days: open.length, booked };
  };
  const reopen = (id: string) => {
    const i = closures.findIndex((c) => c.id === id); if (i < 0) fail(404, "Not found");
    closures.splice(i, 1);
    for (let k = S.appts.length - 1; k >= 0; k--) if (S.appts[k].admin_note === `closure:${id}`) S.appts.splice(k, 1);
  };
  if (seg[0] === "closures") {
    owner();
    if (method === "GET" && seg[1] === "check") { const r = span(q.get("starts_on") ?? "", q.get("ends_on") ?? ""); return { open_days: r.open.length, booked: r.booked }; }
    if (method === "POST") { const r = close(str(b.starts_on), str(b.ends_on), str(b.label).trim()); S.audit("clinic.closure_added", null); return r; }
    if (method === "DELETE") { reopen(seg[1]); S.audit("clinic.closure_removed", null); return { ok: true }; }
  }

  // ── reports ──
  if (seg[0] === "finance" && seg[1] === "reports") {
    owner();
    if (method === "GET" && seg[2] === "figures") { const mo = q.get("month") ?? today.slice(0, 7); return { month: mo, rows: facts(H, S, mo, en) }; }
    if (method === "GET") return [...reports].sort((a, c) => c.month.localeCompare(a.month));
    if (method === "POST") {
      if (!str(b.text).trim()) fail(400, m("O relatório está vazio.", "The report is empty."));
      reports.push({ id: uid(), month: str(b.month), lang: en ? "en" : "pt", text: str(b.text).trim(), model: str(b.model), file_id: uid(), created_at: new Date().toISOString(), created_by_name: S.me().name });
      S.audit("ai.report_saved", null); return { ok: true };
    }
    if (method === "DELETE") { const i = reports.findIndex((r) => r.id === seg[2]); if (i >= 0) reports.splice(i, 1); S.audit("ai.report_deleted", null); return { ok: true }; }
  }

  if (seg[0] === "ai" && ["rewrite-note", "read-receipt", "report", "assistant"].includes(seg[1])) {
    // The note write-up is also for therapists with AI on (phase 2); the rest stays the owner's.
    if (!(seg[1] === "rewrite-note" && S.role() === "therapist") && !(seg[1] === "assistant" && S.role() === "therapist")) owner();
    if (seg[1] === "rewrite-note") {
      const blocks = (b.blocks as { type: string; label: string; value: unknown; max?: number }[]) ?? [];
      const lines = blocks.flatMap((x) => x.type === "slider" ? (x.value == null ? [] : [`${x.label}: ${x.value}/${x.max ?? 10}`]) : Array.isArray(x.value) ? (x.value.length ? [`${x.label}: ${x.value.join(", ")}`] : []) : x.value ? [`${x.label}: ${x.value}`] : []);
      if (!lines.length && !str(b.text).trim()) fail(400, m("Preencha a nota primeiro: a IA só reescreve o que lá está.", "Fill in the note first: the AI only rewrites what’s there."));
      S.audit("ai.note_rewritten", str(b.patient_id), "ok");
      const pain = lines.find((l) => /^(dor|pain)/i.test(l));
      const pv = pain ? pain.split(": ")[1] : "", rest = lines.filter((l) => l !== pain).map((l) => l.toLowerCase()).join(", ");
      return later({ model: MODEL, text: en
        ? `${pv ? `The patient reported pain at ${pv}.` : "[to complete]"} ${str(b.text).trim()}\n\n${rest ? `In this session: ${rest}.` : "[to complete]"} The current plan continues and is reassessed next session.`
        : `${pv ? `A utente referiu dor de ${pv}.` : "[a completar]"} ${str(b.text).trim()}\n\n${rest ? `Nesta sessão: ${rest}.` : "[a completar]"} Mantém-se o plano atual, a reavaliar na próxima sessão.` }, 2200);
    }
    if (seg[1] === "read-receipt") {
      S.audit("ai.receipt_read", null, "ok");
      return later({ model: MODEL, missing: [], fields: { supplier: "Fisiomed, Lda.", supplier_nif: "509876543", spent_on: today, amount_cents: 8640, vat_cents: 1615, category: "supplies",
        document_ref: "FT A/1234", description: en ? "Bandages and gel" : "Ligaduras e gel", method: "card" } }, 2000);
    }
    if (seg[1] === "report") {
      const mo = str(b.month), rows = facts(H, S, mo, en);
      S.audit("ai.report_drafted", null, mo);
      const [rev, exp, , res] = rows;
      return later({ month: mo, rows, model: MODEL, text: en
        ? `In ${monthName(mo, true)} revenue was ${rev[1]} (${rev[2]} the month before) and the result was ${res[1]} (${res[2]}).\n\nExpenses came to ${exp[1]}, against ${exp[2]} in the previous month.\n\nOn activity, ${rows[5][1]} sessions were done (${rows[5][2]}) with ${rows[9][1]} no-shows (${rows[9][2]}). Worth watching: payments still without an invoice number.`
        : `Em ${monthName(mo, false)} a receita foi de ${rev[1]} (${rev[2]} no mês anterior) e o resultado de ${res[1]} (${res[2]}).\n\nAs despesas somaram ${exp[1]}, contra ${exp[2]} no mês anterior.\n\nNa atividade, fizeram-se ${rows[5][1]} sessões (${rows[5][2]}) com ${rows[9][1]} faltas (${rows[9][2]}). A acompanhar: pagamentos ainda sem número de fatura.` }, 2600);
    }
    // the admin assistant
    const msg = str(b.message).trim();
    if (!msg) fail(400, m("Escreva o que quer que o assistente faça.", "Write what you’d like the assistant to do."));
    S.audit("assistant.asked", null);
    const it = understand(msg, H);
    const reply = (text: string, change: Change | null = null, rows: Row[] | null = null) => later({ reply: text, change: change && { id: change.id, skill: change.skill, rows: change.rows, warnings: change.warnings, status: change.status }, rows, model: MODEL });
    const propose = (c: Omit<Change, "id" | "status">, text: string) => { const x: Change = { ...c, id: uid(), status: "pending" }; changes.push(x); return reply(text, x); };
    switch (it.skill) {
      case "report": {
        const mo = str(it.month), rows = facts(H, S, mo, en);
        return reply(L(`Em ${monthName(mo, false)} a receita vai em ${rows[0][1]} e o resultado em ${rows[3][1]}. Fizeram-se ${rows[5][1]} sessões.`, `In ${monthName(mo, true)} revenue so far is ${rows[0][1]} and the result ${rows[3][1]}. ${rows[5][1]} sessions were done.`), null, rows.slice(0, 6));
      }
      case "price": case "product_active": {
        const pr = H.products.find((x) => x.code === it.product)!;
        const name = en ? pr.name_en : pr.name_pt;
        if (it.skill === "price") {
          const nc = Math.round(Number(it.price_eur) * 100);
          if (nc === pr.price_cents) return reply(L(`${name} já custa ${eur(nc, false)}.`, `${name} already costs ${eur(nc, true)}.`));
          return propose({ skill: "price", target: pr.id, params: {}, before: pr.price_cents, after: nc, rows: [[name, eur(pr.price_cents, en), eur(nc, en)]],
            warnings: [L("Quem já comprou um pacote mantém o preço que pagou.", "People who already bought a package keep the price they paid.")] }, L(`Aqui está o novo preço de ${name}.`, `Here’s the new price for ${name}.`));
        }
        if (!pr.active) return reply(L(`${name} já não está à venda.`, `${name} is already off sale.`));
        return propose({ skill: "product_active", target: pr.id, params: {}, before: true, after: false, rows: [[name, L("À venda", "On sale"), L("Retirado", "Off sale")]],
          warnings: [L("Quem já o comprou continua a usá-lo.", "People who already bought it keep using it.")] }, L("Deixa de aparecer para venda.", "It stops being offered for sale."));
      }
      case "hours": {
        const d = it.day as Day; const nv = it.closed ? null : it.open && it.close ? [str(it.open), str(it.close)] as [string, string] : undefined;
        if (nv === undefined) return reply(L("A que horas abre e fecha? Por exemplo: das 9:00 às 13:00.", "What time does it open and close? For example: 9:00 to 13:00."));
        const lbl = (v: [string, string] | null) => v ? `${v[0]}–${v[1]}` : L("Fechado", "Closed");
        return propose({ skill: "hours", target: null, params: { day: d }, before: hours[d], after: nv, rows: [[L(...DAY_NAMES[d]), lbl(hours[d]), lbl(nv)]],
          warnings: [L("Não mexe nas marcações que já existem.", "Appointments already booked aren’t changed.")] }, L("Este é o novo horário desse dia.", "Here are that day’s new hours."));
      }
      case "closure": {
        const from = str(it.from), to = str(it.to);
        if (from < today) return reply(L("Escolha dias a partir de hoje, com o último dia igual ou depois do primeiro.", "Choose days from today on, with the last day on or after the first."));
        const { booked } = span(from, to);
        const label = str(it.label);
        return propose({ skill: "closure", target: null, params: { from, to, label }, before: null, after: null,
          rows: [[from === to ? dmy(from) : `${dmy(from)} – ${dmy(to)}`, L("Aberta", "Open"), `${L("Fechada", "Closed")}${label ? ` — ${label}` : ""}`]],
          warnings: [L("Fica bloqueado na agenda durante o horário de funcionamento.", "It’s blocked in the schedule during opening hours."), ...(booked ? [L(`${booked} marcações já marcadas nesses dias continuam: remarque-as na Agenda.`, `${booked} appointments already booked then stay booked: reschedule them in Schedule.`)] : [])] },
          L("Fecho a clínica nesses dias.", "I’ll close the clinic on those days."));
      }
      case "staff_pay": case "staff_active": {
        const st = H.staff.find((s) => s.name === it.staff)!;
        if (it.skill === "staff_pay") {
          const nc = Math.round(Number(it.amount_eur) * 100);
          return propose({ skill: "staff_pay", target: st.id, params: {}, before: H.pay[st.id] ?? 0, after: nc, rows: [[`${st.name} — ${L("ordenado fixo (bruto)", "fixed pay (gross)")}`, eur(H.pay[st.id] ?? 0, en), eur(nc, en)]],
            warnings: [L("Conta a partir do próximo salário registado em Finanças › Salários.", "Counts from the next salary recorded in Finance › Salaries.")] }, L("Este é o novo ordenado.", "Here’s the new pay."));
        }
        if (st.role === "owner") return reply(L("Não posso desativar a conta da dona da clínica.", "I can’t deactivate the owner’s account."));
        return propose({ skill: "staff_active", target: st.id, params: {}, before: true, after: it.active, rows: [[st.name, L("Ativa", "Active"), it.active ? L("Ativa", "Active") : L("Desativada", "Deactivated")]],
          warnings: [L("Termina já a sessão em todos os computadores. Os dados e o histórico ficam.", "Signs them out of every computer now. Their data and history stay.")] }, L("Esta pessoa deixa de poder entrar.", "This person won’t be able to sign in."));
      }
      default:
        return reply(L("Posso mudar preços e pacotes, o horário e os encerramentos, o ordenado fixo e quem está ativo na equipa, ou resumir um mês. O que quer fazer?",
          "I can change prices and packages, opening hours and closures, staff fixed pay and who’s active, or summarise a month. What would you like to do?"));
    }
  }

  if (seg[0] === "assistant" && seg[1] === "changes") {
    owner();
    const c = changes.find((x) => x.id === seg[2]) ?? fail(404, "Not found");
    const set = (v: unknown) => {
      if (c.skill === "price") H.products.find((x) => x.id === c.target)!.price_cents = v as number;
      if (c.skill === "product_active") H.products.find((x) => x.id === c.target)!.active = v as boolean;
      if (c.skill === "hours") hours[c.params.day as Day] = v as [string, string] | null;
      if (c.skill === "staff_pay") H.pay[c.target!] = v as number;
    };
    if (seg[3] === "apply") {
      if (c.status !== "pending") fail(409, m("Esta alteração já foi aplicada ou cancelada.", "This change was already applied or cancelled."));
      if (c.skill === "closure") c.target = close(str(c.params.from), str(c.params.to), str(c.params.label)).id; else set(c.after);
      c.status = "applied"; S.audit("assistant.applied", null); return { ok: true, status: "applied" };
    }
    if (seg[3] === "cancel") { if (c.status !== "pending") fail(409, m("Esta alteração já foi aplicada ou cancelada.", "This change was already applied or cancelled.")); c.status = "cancelled"; return { ok: true, status: "cancelled" }; }
    if (seg[3] === "undo") {
      if (c.status !== "applied") fail(409, m("Só se pode anular uma alteração aplicada.", "Only an applied change can be undone."));
      if (c.skill === "closure") reopen(c.target!); else set(c.before);
      c.status = "undone"; S.audit("assistant.undone", null); return { ok: true, status: "undone" };
    }
  }
  return undefined;
}
