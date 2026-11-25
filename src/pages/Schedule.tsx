import { useEffect, useMemo, useState } from "react";
import { BellRing, Clock, CalendarClock, CalendarPlus, ChevronLeft, ChevronRight, FileText, X, CalendarDays, Check, UserX, RotateCcw, Plus, Trash2, Sparkles, UserPlus, AlertTriangle } from "lucide-react";
import { useApp } from "../lib/ctx";
import { api, isErr } from "../lib/api";
import {
  useApi, addDays, monday, ymd, sameDay, hm, range, fmtDate, fmtDay, weekdayShort, toneFor, kindLabel, statusLabel, blockLabel,
  type Appointment, type BookingRequest, type Overlap, type RequestSlot, type RequestSuggestion, type Therapist, type Waiting,
} from "../lib/data";
import { Layout, SubItem } from "../components/Layout";
import { Badge, DayStrip, Empty, Head, Seg, Sheet } from "../components/ui";
import { AddAttendee, AddWaitlist, CancelAppointment, EditAppointment, NewAppointment, NewRequest } from "../components/Sheets";
import { setStatus } from "../components/menus";
import { TimeGrid, hNum, type GridCol } from "../components/TimeGrid";
import { RemindersPanel, remindersNav, type ReminderDay } from "../components/Reminders";

type View = "day" | "week" | "month";
const START = 8;
const DAY_KEYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/** A website request asks for a time of day, not an hour (phase 4a). */
const partLabel = (p: string, L: (pt: string, en: string) => string) =>
  p === "morning" ? L("de manhã", "morning") : p === "afternoon" ? L("à tarde", "afternoon") : L("a qualquer hora", "any time");

export default function Schedule() {
  const app = useApp();
  const { L, lang, openSheet, openPatient, toast } = app;
  const [view, setView] = useState<View>("day");
  const [cur, setCur] = useState(() => { const d = new Date(); d.setHours(0, 0, 0, 0); return d; });
  const [sel, setSel] = useState<string | null>(null);
  const [hidden, setHidden] = useState<string[]>([]);
  
  const wk = monday(cur);
  const monthStart = monday(new Date(cur.getFullYear(), cur.getMonth(), 1));
  const [from, to] = view === "month" ? [monthStart, addDays(monthStart, 42)] : [wk, addDays(wk, 7)];
  const { data: list, error } = useApi<Appointment[]>(`/appointments?${range(from, to)}&include_cancelled=true`);
  const { data: ths } = useApi<Therapist[]>("/therapists");
  const { data: reqs } = useApi<BookingRequest[]>("/booking-requests?status=pending");
  const { data: wait } = useApi<Waiting[]>("/waitlist");
  const { data: rem } = useApi<ReminderDay>("/reminders");
  const remTodo = rem?.items.filter((i) => !i.sent_at && i.prefers !== "none").length;

  const { data: hours } = useApi<{ hours: Record<string, [string, string] | null> }>("/clinic/hours");
  // Moves made by dragging show at once, until the list reloads from the server.
  const [pending, setPending] = useState<Record<string, Pick<Appointment, "starts_at" | "ends_at" | "therapist_id">>>({});
  useEffect(() => setPending({}), [list]);
  const all = list ?? [];
  const visible = all.filter((x) => x.status !== "cancelled" && !(x.therapist_id && hidden.includes(x.therapist_id)));
  const shown = visible.map((x) => pending[x.id] ? { ...x, ...pending[x.id] } : x);
  const color = (x: Appointment) => x.kind === "block" ? (x.color || "var(--text3)") : toneFor(ths, x.therapist_id);
  const title = (x: Appointment) => x.kind === "block" ? blockLabel(x.title, lang) : x.kind === "class" ? x.title || kindLabel("class", lang) : x.patient_name ?? "—";
  const today = new Date();

  const step = (n: number) => {
    const d = view === "day" ? addDays(cur, n) : view === "week" ? addDays(cur, 7 * n) : new Date(cur.getFullYear(), cur.getMonth() + n, 1);
    setCur(d);
  };
  const goDay = (d: Date) => setCur(d);

  const decide = (r: BookingRequest, ok: boolean) => {
    if (ok) return openSheet(<NewAppointment request={r} patientId={r.patient_id ?? undefined} />);
    api("POST", `/booking-requests/${r.id}/decline`)
      .then(() => toast(L("Pedido recusado", "Request declined"), () => { api("POST", `/booking-requests/${r.id}/reopen`).catch((e) => toast(isErr(e) ? e.message : String(e))); }, L("Recusar pedido", "Decline Request")))
      .catch((e) => toast(isErr(e) ? e.message : String(e)));
  };
  const removeWait = (w: Waiting) => api("PATCH", `/waitlist/${w.id}`, { status: "removed" })
    .then(() => toast(L("Removido da lista de espera", "Removed from the waitlist"), () => { api("PATCH", `/waitlist/${w.id}`, { status: "waiting" }).catch(() => {}); }, L("Remover da lista", "Remove from Waitlist")))
    .catch((e) => toast(isErr(e) ? e.message : String(e)));
  const waitPref = (w: Waiting) => {
    const d = weekdayShort(lang);
    const days = w.weekdays.length ? w.weekdays.map((n) => d[n - 1]).join(", ") : L("Qualquer dia", "Any day");
    const t = w.from_time || w.to_time ? `, ${w.from_time?.slice(0, 5) ?? ""}–${w.to_time?.slice(0, 5) ?? ""}` : "";
    return `${days}${t}${w.therapist_name ? ` · ${w.therapist_name}` : ""}`;
  };

  const [sec, setSec] = useState<"agenda" | "requests" | "waitlist" | "reminders">(() => { const r = remindersNav.open; remindersNav.open = false; return r ? "reminders" : "agenda"; });
  const sub = (
    <>
      <SubItem icon={<CalendarDays />} on={sec === "agenda"} onClick={() => setSec("agenda")}>{L("Calendário", "Calendar")}</SubItem>
      <SubItem icon={<CalendarPlus />} on={sec === "requests"} onClick={() => setSec("requests")} count={reqs?.length}>{L("Pedidos de marcação", "Booking Requests")}</SubItem>
      <SubItem icon={<Clock />} on={sec === "waitlist"} onClick={() => setSec("waitlist")} count={wait?.length}>{L("Lista de espera", "Waitlist")}</SubItem>
      <SubItem icon={<BellRing />} on={sec === "reminders"} onClick={() => setSec("reminders")} count={remTodo}>{L("Lembretes", "Reminders")}</SubItem>
    </>
  );
  /** Clicking an appointment shows its details and actions in a sheet. */
  const pick = (id: string) => { setSel(id); const x = all.find((y) => y.id === id); if (x) openSheet(<Inspector a={x} color={color(x)} title={title(x)} />); };
  const newAt = () => openSheet(<NewAppointment date={view === "day" ? (() => { const d = new Date(cur); d.setHours(Math.max(START, new Date().getHours() + 1), 0, 0, 0); return d; })() : undefined} />);

  if (sec === "reminders") return <Layout label={L("Agenda", "Schedule")} sub={sub}><RemindersPanel /></Layout>;
  if (sec === "requests") {
    return (
      <Layout label={L("Agenda", "Schedule")} sub={sub}>
        <div className="wrap">
          <Head title={L("Pedidos de marcação", "Booking Requests")} sub={L("Pedidos feitos no site, por telefone ou por email.", "Requests made on the website, by phone or by email.")}>
            <button className="btn" onClick={() => openSheet(<NewRequest />)}><Plus />{L("Pedido por telefone ou email…", "Phone or Email Request…")}</button>
          </Head>
          {!reqs ? <div className="skel" /> : reqs.length === 0 ? <Empty icon={<CalendarDays />} title={L("Sem pedidos pendentes", "No pending requests")} text={L("Os novos pedidos aparecem aqui para aceitar ou recusar.", "New requests appear here to accept or decline.")} /> : (
            <div className="rows">
              {reqs.map((r) => <RequestRow key={r.id} r={r} onDecline={() => decide(r, false)} onOther={() => decide(r, true)} />)}
            </div>
          )}
        </div>
      </Layout>
    );
  }
  if (sec === "waitlist") {
    return (
      <Layout label={L("Agenda", "Schedule")} sub={sub}>
        <div className="wrap">
          <Head title={L("Lista de espera", "Waitlist")} sub={L("Ao cancelar uma marcação, a Agenda mostra quem pode vir a essa hora.", "When you cancel an appointment, the Schedule shows who can come at that time.")}>
            <button className="btn" onClick={() => openSheet(<AddWaitlist />)}><Plus />{L("Adicionar à lista…", "Add to Waitlist…")}</button>
          </Head>
          {!wait ? <div className="skel" /> : wait.length === 0 ? <Empty icon={<CalendarDays />} title={L("Ninguém em espera", "No one is waiting")} text={L("Junte utentes que querem uma vaga mais cedo.", "Add patients who want an earlier slot.")} /> : (
            <div className="rows">
              {wait.map((w) => (
                <div key={w.id} className="row" role={w.patient_id ? "button" : undefined} tabIndex={w.patient_id ? 0 : undefined} onClick={() => w.patient_id && openPatient(w.patient_id)}>
                  <span className="grow"><span className="t" style={{ display: "block" }}>{w.name}</span><span className="s" style={{ display: "block" }}>{waitPref(w)}</span></span>
                  <button className="btn icon sm more" aria-label={L(`Remover ${w.name} da lista de espera`, `Remove ${w.name} from the waitlist`)} title={L("Remover da lista", "Remove from waitlist")} onClick={(e) => { e.stopPropagation(); removeWait(w); }}><X size={14} /></button>
                </div>
              ))}
            </div>
          )}
          <p className="small muted" style={{ marginTop: 10 }}>{L("O envio automático de vagas por SMS chega com o fornecedor de SMS.", "Automatic SMS offers arrive with the SMS provider.")}</p>
        </div>
      </Layout>
    );
  }

  const days7 = Array.from({ length: 7 }, (_, i) => addDays(wk, i));
  const dayList = (d: Date) => shown.filter((x) => sameDay(new Date(x.starts_at), d));
  const heading = view === "month" ? fmtDate(cur, lang, { month: "long", year: "numeric" })
    : view === "day" ? fmtDay(cur, lang).replace(/^./, (c) => c.toUpperCase())
      : L(`Semana de ${fmtDate(wk, "pt", { day: "numeric", month: "long" })}`, `Week of ${fmtDate(wk, "en", { day: "numeric", month: "long" })}`);
  const n = view === "day" ? dayList(cur).filter((x) => x.kind !== "block").length : visible.filter((x) => x.kind !== "block").length;
  const unit = view === "month" ? L("neste período", "in this period") : view === "week" ? L("nesta semana", "this week") : "";

  // ── the time grid (day and week) ──
  const openOf = (d: Date): [number, number] | null => {
    const v = hours?.hours?.[DAY_KEYS[(d.getDay() + 6) % 7]];
    return v ? [hNum(v[0]), hNum(v[1])] : hours ? null : [8, 20];
  };
  const now = new Date();
  const stLabel = (a: Appointment) => a.status !== "booked" ? statusLabel(a.status, lang)
    : new Date(a.ends_at) <= now ? L("Por fechar", "Not marked yet") : new Date(a.starts_at) <= now ? L("A decorrer", "In progress") : sameDay(new Date(a.starts_at), now) ? L("A seguir", "Upcoming") : L("Marcada", "Booked");
  // Day view: one column per therapist shown (audit M6), plus whole-clinic blocked time in every column.
  const dayCols = (): GridCol[] => {
    const list = dayList(cur), o = openOf(cur), isT = sameDay(cur, today);
    const tl = (ths ?? []).filter((t) => !hidden.includes(t.id));
    if (tl.length <= 1) return [{ key: "all", date: cur, therapistId: tl[0]?.id ?? null, appts: list, open: o, isToday: isT }];
    const known = new Set(tl.map((t) => t.id));
    const cols: GridCol[] = tl.map((t) => {
      const mine = list.filter((x) => x.therapist_id === t.id || (x.kind === "block" && !x.therapist_id));
      return { key: t.id, date: cur, therapistId: t.id, appts: mine, open: o, isToday: isT,
        head: <><i style={{ background: toneFor(ths, t.id) }} />{t.name}<span className="muted num">{mine.filter((x) => x.kind !== "block").length || ""}</span></> };
    });
    const other = list.filter((x) => x.therapist_id && !known.has(x.therapist_id));
    if (other.length) cols.push({ key: "other", date: cur, therapistId: undefined, appts: other, open: o, isToday: isT, head: <>{L("Outros", "Others")}</> });
    return cols;
  };
  const weekCols = (): GridCol[] => days7.map((d, i) => ({
    key: ymd(d), date: d, appts: dayList(d), open: openOf(d), isToday: sameDay(d, today),
    head: <>{weekdayShort(lang)[i]}<b>{d.getDate()}</b></>, headLabel: fmtDay(d, lang), onHead: () => { goDay(d); setView("day"); },
  }));
  const book = (at: Date, therapistId?: string | null) => openSheet(<NewAppointment date={at} therapistId={therapistId ?? undefined} />);
  /** Only booked sessions move by dragging; done, missed and cancelled ones keep their time (they carry charges). */
  const canDrag = (a: Appointment) => a.status === "booked";
  const moveTo = async (a: Appointment, s: Date, e: Date, col: GridCol, allow = false) => {
    const th = col.therapistId !== undefined && a.kind !== "block" ? col.therapistId : a.therapist_id;
    setPending((p) => ({ ...p, [a.id]: { starts_at: s.toISOString(), ends_at: e.toISOString(), therapist_id: th } }));
    const before = { starts_at: a.starts_at, ends_at: a.ends_at, therapist_id: a.therapist_id };
    try {
      await api("PATCH", `/appointments/${a.id}`, { scope: "one", starts_at: s.toISOString(), ends_at: e.toISOString(), therapist_id: th, allow_overlap: allow });
      const who = th !== a.therapist_id ? ths?.find((t) => t.id === th)?.name : undefined;
      const what = `${fmtDate(s, lang, { weekday: "short", day: "numeric", month: "short" })}, ${hm(s.toISOString())}–${hm(e.toISOString())}${who ? ` · ${who}` : ""}`;
      toast(a.kind === "block" ? L(`Bloqueio mudado: ${what}`, `Blocked time moved: ${what}`)
        : a.series_id ? L(`Só esta marcação mudou: ${what}`, `Only this appointment moved: ${what}`) : L(`Marcação mudada: ${what}`, `Appointment moved: ${what}`),
        () => { api("PATCH", `/appointments/${a.id}`, { scope: "one", ...before, allow_overlap: true }).catch((x) => toast(isErr(x) ? x.message : String(x))); }, L("Mover marcação", "Move Appointment"));
    } catch (x) {
      setPending((p) => { const q = { ...p }; delete q[a.id]; return q; });
      if (isErr(x) && x.code === "overlap") {
        const o = ((x.details as Overlap[]) ?? [])[0];
        app.confirm({ title: L("Sobrepõe-se a outra marcação", "This overlaps another appointment"),
          text: o ? L(`${o.name ?? "Outra marcação"}, ${hm(o.starts_at)}–${hm(o.ends_at)}, com o mesmo fisioterapeuta.`, `${o.name ?? "Another appointment"}, ${hm(o.starts_at)}–${hm(o.ends_at)}, with the same therapist.`) : undefined,
          action: L("Mover mesmo assim", "Move Anyway"), onConfirm: () => { moveTo(a, s, e, col, true); } });
      } else toast(isErr(x) ? x.message : String(x));
    }
  };
  const grid = (cols: GridCol[], week: boolean) => (
    <TimeGrid cols={cols} H={week ? 56 : 72} compactWeek={week} sel={sel} onSel={pick} title={title} color={color} status={stLabel}
      onBook={book} onMove={(a, s, e, col) => { moveTo(a, s, e, col); }} canDrag={canDrag} />
  );
  const dayClosed = view === "day" && !openOf(cur) && !dayList(cur).length;
  return (
    <Layout label={L("Agenda", "Schedule")} sub={sub}>
      <div className="wrap wide">
        <Head title={heading} sub={error ? <span className="err">{error}</span> : !list ? L("A carregar…", "Loading…") : L(`${n} marcações ${unit}`, `${n} appointments ${unit}`).trim()}>
          <button className="btn pri" onClick={newAt}><CalendarPlus />{L("Nova marcação…", "New Appointment…")}</button>
        </Head>
        {/* One toolbar row (23 Nov 2026 overhaul): moving through time, the view, and whose appointments show. */}
        <div className="toolrow sched-bar">
          <div className="sched-nav" role="group" aria-label={L("Navegar", "Navigate")}>
          <button className="btn" aria-label={view === "day" ? L("Dia anterior", "Previous day") : view === "week" ? L("Semana anterior", "Previous week") : L("Mês anterior", "Previous month")} onClick={() => step(-1)}><ChevronLeft /></button>
          <button className="btn" onClick={() => goDay((() => { const d = new Date(); d.setHours(0, 0, 0, 0); return d; })())}>{L("Hoje", "Today")}</button>
          <button className="btn" aria-label={view === "day" ? L("Dia seguinte", "Next day") : view === "week" ? L("Semana seguinte", "Next week") : L("Mês seguinte", "Next month")} onClick={() => step(1)}><ChevronRight /></button>
          </div>
          <Seg label={L("Vista", "View")} value={view} onChange={setView} options={[["day", L("Dia", "Day")], ["week", L("Semana", "Week")], ["month", L("Mês", "Month")]]} />
          {(ths?.length ?? 0) > 1 && (
            <div className="sched-who" role="group" aria-label={L("Mostrar fisioterapeutas", "Show therapists")}>
              {ths!.map((t) => (
                <button key={t.id} className="legend-chip" aria-pressed={!hidden.includes(t.id)} style={{ ["--c" as string]: toneFor(ths, t.id) }}
                  onClick={() => setHidden(hidden.includes(t.id) ? hidden.filter((x) => x !== t.id) : [...hidden, t.id])}><i />{t.name}</button>
              ))}
            </div>
          )}
        </div>
        {view === "month" ? (
          <MonthView start={monthStart} month={cur.getMonth()} appts={visible} title={title} color={color} onDay={(d) => { goDay(d); setView("day"); }} />
        ) : view === "day" ? (
          <>
            <div style={{ marginBottom: 16 }}>
              <DayStrip value={(cur.getDay() + 6) % 7} onChange={(i) => goDay(days7[i])}
                days={days7.map((d, i) => ({ label: weekdayShort(lang)[i], num: d.getDate(), dots: dayList(d).filter((x) => x.kind !== "block").length }))} />
            </div>
            {dayClosed ? (
              <Empty icon={<CalendarDays />} title={L("Clínica fechada", "Clinic closed")} text={L("Este dia está fora do horário de funcionamento. Pode marcar mesmo assim.", "This day is outside the opening hours. You can still book it.")}
                action={<button className="btn pri" onClick={newAt}><CalendarPlus />{L("Nova marcação…", "New Appointment…")}</button>} />
            ) : grid(dayCols(), false)}
          </>
        ) : <div className="week-wrap">{grid(weekCols(), true)}</div>}
        {view !== "month" && !dayClosed && <p className="small muted tg-hint">{L("Clique numa hora livre para marcar. Arraste uma marcação para a mudar de hora, de dia ou de fisioterapeuta; arraste a margem de baixo para mudar a duração.", "Click a free time to book it. Drag an appointment to change its time, day or therapist; drag its bottom edge to change its length.")}</p>}
      </div>
    </Layout>
  );
}

export function Inspector({ a, color, title }: { a: Appointment; color: string; title: string }) {
  const app = useApp();
  const { L, lang, openSheet, openPatient, toast, closeSheet } = app;
  const status = (st: Appointment["status"]) => { closeSheet(); setStatus(app, a, st); };
  const mins = Math.round((new Date(a.ends_at).getTime() - new Date(a.starts_at).getTime()) / 60000);
  // The sheet shows a snapshot, so it keeps its own copy of the class list as you change it.
  const [people, setPeople] = useState(a.attendees ?? []);
  const attend = (pid: string, v: boolean) => (setPeople((l) => l.map((p) => p.patient_id === pid ? { ...p, attended: v } : p)), api("PATCH", `/appointments/${a.id}/attendees/${pid}`, { attended: v }).catch((e) => toast(isErr(e) ? e.message : String(e))));
  const remove = (pid: string, name: string) => api("DELETE", `/appointments/${a.id}/attendees/${pid}`)
    .then(() => setPeople((l) => l.filter((p) => p.patient_id !== pid)))
    .then(() => toast(L(`${name} saiu da aula`, `${name} removed from the class`), () => { api("POST", `/appointments/${a.id}/attendees`, { patient_id: pid }).catch(() => {}); }, L("Remover da aula", "Remove from Class")))
    .catch((e) => toast(isErr(e) ? e.message : String(e)));
  return (
    <Sheet title={title} sub={`${fmtDate(a.starts_at, lang, { weekday: "long", day: "numeric", month: "long" })}, ${hm(a.starts_at)}–${hm(a.ends_at)}`}>
      <div className="insp-sec">
        <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 10 }}>
          <span style={{ width: 10, height: 10, borderRadius: 3, background: color, flex: "none" }} aria-hidden="true" />
          <Badge tone={a.status === "cancelled" ? "bad" : a.status === "done" ? "ok" : a.status === "no_show" ? "warn" : "a"}>{statusLabel(a.status, lang)}</Badge>
        </div>
        <dl className="kv">
          <dt>{L("Tipo", "Type")}</dt><dd>{kindLabel(a.kind, lang)}</dd>
          <dt>{L("Fisioterapeuta", "Therapist")}</dt><dd>{a.therapist_name ?? L("Toda a clínica", "Whole clinic")}</dd>
          <dt>{L("Duração", "Duration")}</dt><dd>{mins} min</dd>
          <dt>{L("Repete", "Repeats")}</dt><dd>{a.series_id ? L("Semanal", "Weekly") : "—"}</dd>
          {a.patient_number != null && <><dt>{L("N.º utente", "Patient no.")}</dt><dd className="num">{a.patient_number}</dd></>}
          {a.kind === "home" && <><dt>{L("Morada", "Address")}</dt><dd>{a.address}</dd><dt>{L("Deslocação", "Travel")}</dt><dd>{a.travel_minutes} min</dd></>}
          {a.prescription_id && <><dt>{L("Prescrição", "Prescription")}</dt><dd>{L("Conta para a prescrição", "Counts toward the prescription")}</dd></>}
        </dl>
      </div>
      {a.kind === "class" && (
        <div className="insp-sec">
          <h3 style={{ display: "flex", justifyContent: "space-between" }}>{L("Inscritos", "Attendees")}<span className="muted num">{people.length}/{a.capacity}</span></h3>
          {people.length ? people.map((p) => (
            <div key={p.patient_id} style={{ display: "flex", alignItems: "center", gap: 8, padding: "3px 0" }}>
              <label className="check" style={{ flex: 1 }}><input type="checkbox" checked={!!p.attended} onChange={(e) => attend(p.patient_id, e.target.checked)} aria-label={L(`${p.name} esteve presente`, `${p.name} attended`)} />{p.name}</label>
              <button className="iconbtn" aria-label={L(`Remover ${p.name}`, `Remove ${p.name}`)} onClick={() => remove(p.patient_id, p.name)}><Trash2 size={13} /></button>
            </div>
          )) : <p className="small muted">{L("Ainda ninguém inscrito.", "Nobody signed up yet.")}</p>}
          {people.length < (a.capacity ?? 0) && a.status !== "cancelled" && <button className="btn sm" style={{ marginTop: 8 }} onClick={() => openSheet(<AddAttendee appt={a} />)}><Plus size={13} />{L("Inscrever…", "Add…")}</button>}
          <p className="small muted" style={{ marginTop: 6 }}>{L("Marque quem esteve presente.", "Tick who attended.")}</p>
        </div>
      )}
      <div className="insp-sec btnrow">
        {a.patient_id && <button className="btn" onClick={() => { closeSheet(); openPatient(a.patient_id!); }}><FileText />{L("Abrir ficha", "Open File")}</button>}
        {a.status === "cancelled" ? <button className="btn" onClick={() => status("booked")}><RotateCcw />{L("Repor marcação", "Restore Appointment")}</button> : (
          <>
            {a.kind !== "block" && a.status === "booked" && <button className="btn" onClick={() => status("done")}><Check />{L("Marcar como realizada", "Mark as Done")}</button>}
            {a.patient_id && a.status === "booked" && <button className="btn" onClick={() => status("no_show")}><UserX />{L("Marcar falta", "Mark as No-Show")}</button>}
            {a.kind !== "block" && a.status !== "booked" && <button className="btn" onClick={() => status("booked")}><RotateCcw />{L("Voltar a marcada", "Mark as Booked")}</button>}
            <button className="btn" onClick={() => openSheet(<EditAppointment appt={a} />)}><CalendarClock />{L("Remarcar…", "Reschedule…")}</button>
            <button className="btn danger" onClick={() => openSheet(<CancelAppointment appt={a} />)}><X />{a.kind === "block" ? L("Remover bloqueio…", "Remove Blocked Time…") : L("Cancelar marcação…", "Cancel Appointment…")}</button>
          </>
        )}
      </div>
    </Sheet>
  );
}

/** Month view: six weeks from the Monday before the 1st. */
function MonthView({ start, month, appts, title, color, onDay }: { start: Date; month: number; appts: Appointment[]; title: (a: Appointment) => string; color: (a: Appointment) => string; onDay: (d: Date) => void }) {
  const { L, lang } = useApp();
  const byDay = useMemo(() => {
    const m: Record<string, Appointment[]> = {};
    for (const a of appts) (m[ymd(new Date(a.starts_at))] ??= []).push(a);
    return m;
  }, [appts]);
  const today = new Date();
  return (
    <div className="month">
      {weekdayShort(lang).map((h) => <div key={h} className="mh">{h}</div>)}
      {Array.from({ length: 42 }, (_, i) => addDays(start, i)).map((d) => {
        const inMonth = d.getMonth() === month;
        const list = (byDay[ymd(d)] ?? []).sort((a, b) => a.starts_at.localeCompare(b.starts_at));
        const real = list.filter((a) => a.kind !== "block");
        const blocks = list.filter((a) => a.kind === "block");
        return (
          <button key={ymd(d)} className={`${inMonth ? "" : "out"} ${sameDay(d, today) ? "today" : ""}`} onClick={() => onDay(d)}
            aria-label={`${fmtDay(d, lang)}: ${real.length} ${L("marcações", "appointments")}`}>
            <span className="n">{d.getDate()}</span>
            {blocks.slice(0, 1).map((b) => <span key={b.id} className="closed">{title(b)}</span>)}
            {real.slice(0, 2).map((a) => <span key={a.id} className="ev1" style={{ ["--tc" as string]: color(a) }}>{hm(a.starts_at)} {title(a)}</span>)}
            {real.length > 2 && <span className="more">+{real.length - 2} {L("mais", "more")}</span>}
          </button>
        );
      })}
    </div>
  );
}


/** Opens one appointment's details sheet from elsewhere (Find, audit phase C), with the same color and title as the Schedule. */
export function openAppointment(app: ReturnType<typeof useApp>, a: Appointment, ths: Therapist[] | undefined) {
  const color = a.kind === "block" ? (a.color || "var(--text3)") : toneFor(ths, a.therapist_id);
  const title = a.kind === "block" ? blockLabel(a.title, app.lang) : a.kind === "class" ? a.title || kindLabel("class", app.lang) : a.patient_name ?? "—";
  app.openSheet(<Inspector a={a} color={color} title={title} />);
}

/** One booking request: what was asked, and the time one press would book (owner, 11 Nov 2026: "book it automatically
 *  with a press of a button"). The server picks the first free time that fits the day, part of the day and service. */
function RequestRow({ r, onDecline, onOther }: { r: BookingRequest; onDecline: () => void; onOther: () => void }) {
  const { L, lang, toast, openSheet, openPatient } = useApp();
  const { data: sug, error } = useApi<RequestSuggestion>(`/booking-requests/${r.id}/suggest`);
  const [busy, setBusy] = useState(false);
  const slot = sug?.slot ?? null;
  const looks = sug?.lookalikes ?? [];
  const when = (x: RequestSlot) => `${fmtDate(x.starts_at, lang, { weekday: "short", day: "numeric", month: "short" })}, ${hm(x.starts_at)}`;
  const press = (x: RequestSlot, patientId?: string) => {
    setBusy(true);
    api<{ appointment_id: string; patient_id: string; patient_created: boolean }>("POST", `/booking-requests/${r.id}/book`, { starts_at: x.starts_at, ends_at: x.ends_at, therapist_id: x.therapist_id, patient_id: patientId ?? null })
      .then((res) => toast(res.patient_created ? L(`Marcado: ${when(x)}. Ficha nova criada.`, `Booked: ${when(x)}. New patient file made.`) : L(`Marcado: ${when(x)}`, `Booked: ${when(x)}`),
        () => { api("POST", `/booking-requests/${r.id}/reopen`).catch((e) => toast(isErr(e) ? e.message : String(e))); }, L("Marcar pedido", "Book Request")))
      .catch((e) => toast(isErr(e) ? e.message : String(e)))
      .finally(() => setBusy(false));
  };
  const book = () => {
    if (!slot) return;
    // Looks like someone with a file: ask which, instead of guessing.
    if (!sug?.patient && looks.length) return openSheet(<WhichFile r={r} slot={slot} looks={looks} when={when(slot)} onPick={(pid) => press(slot, pid)} />);
    press(slot);
  };
  // Phase 4b: the patient cancelled on the website (already done) — reception and the therapist only need to see it.
  if (r.kind === "cancelled") return (
    <div className="row req">
      <span className="grow">
        <span className="t" style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>{r.name}<Badge tone="warn">{L("Cancelou no site", "Cancelled on the website")}</Badge></span>
        <span className="s" style={{ display: "block" }}>{L("Sessão de", "Session on")} {fmtDate(r.preferred_at, lang, { weekday: "short", day: "numeric", month: "short" })}, {hm(r.preferred_at)}{[r.phone, r.email].filter(Boolean).length ? ` · ${[r.phone, r.email].filter(Boolean).join(" · ")}` : ""}</span>
      </span>
      <span className="end">
        {r.patient_id && <button className="btn sm" onClick={() => openPatient(r.patient_id!)}>{L("Abrir ficha", "Open File")}</button>}
        <button className="btn sm pri" onClick={onDecline}>{L("Visto", "Seen")}</button>
      </span>
    </div>
  );
  return (
    <div className="row req">
      <span className="grow">
        <span className="t" style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>{r.name}
          <Badge tone="a">{r.service === "assessment" ? L("Avaliação", "Assessment") : r.service === "home" ? L("Domicílio", "Home visit") : L("Sessão", "Session")}</Badge>
          {r.source === "website" && <Badge>{r.lang === "en" ? L("Site · inglês", "Website · English") : L("Site", "Website")}</Badge>}
          {r.source === "portal" && <Badge>{L("Área de utente", "Patient area")}</Badge>}
          {r.kind === "move" && <Badge tone="warn">{r.about_starts_at ? L(`Mudar a sessão de ${fmtDate(r.about_starts_at, lang, { day: "numeric", month: "short" })}, ${hm(r.about_starts_at)}`, `Move the session on ${fmtDate(r.about_starts_at, lang, { day: "numeric", month: "short" })}, ${hm(r.about_starts_at)}`) : L("Mudar uma sessão", "Move a session")}</Badge>}</span>
        <span className="s" style={{ display: "block" }}>{L("Pediu", "Asked for")} {fmtDate(r.preferred_at, lang, { weekday: "short", day: "numeric", month: "short" })}, {r.source !== "staff" && r.day_part ? partLabel(r.day_part, L) : hm(r.preferred_at)}
          {[r.phone, r.email].filter(Boolean).length ? ` · ${[r.phone, r.email].filter(Boolean).join(" · ")}` : ""}</span>
        {r.address && <span className="s" style={{ display: "block" }}>{r.address}</span>}
        {r.message && <span className="s" style={{ display: "block" }}>{r.message}</span>}
        <span className="suggest" aria-live="polite">
          {error ? <span className="muted">{error}</span>
            : !sug ? <span className="muted">{L("A procurar uma hora livre…", "Finding a free time…")}</span>
            : !slot ? <span className="muted">{L("Sem horas livres nas próximas duas semanas. Escolha outra hora.", "No free times in the next two weeks. Choose another time.")}</span>
            : <>
                <Sparkles size={13} aria-hidden style={{ color: "var(--a)", flex: "none" }} />
                <span><b>{when(slot)}</b>–{hm(slot.ends_at)} · {slot.therapist_name}</span>
                {sug.patient
                  ? <button className="linkbtn" onClick={() => openPatient(sug.patient!.id)}>{sug.patient.name} ({L("nº", "no.")} {sug.patient.number})</button>
                  : looks.length
                    ? <span className="warn-t"><AlertTriangle size={12} aria-hidden style={{ verticalAlign: -1 }} /> {L(`Parecido com ${looks[0].name} (nº ${looks[0].number})`, `Looks like ${looks[0].name} (no. ${looks[0].number})`)}</span>
                    : <span className="muted"><UserPlus size={12} aria-hidden style={{ verticalAlign: -1 }} /> {L("Ficha nova", "New patient file")}</span>}
              </>}
        </span>
      </span>
      <span className="end">
        <button className="btn sm" onClick={onDecline}>{L("Recusar", "Decline")}</button>
        <button className="btn sm" onClick={onOther}>{L("Outra hora…", "Other Time…")}</button>
        <button className="btn sm pri" disabled={!slot || busy} aria-busy={busy} onClick={book}
          title={slot ? L(`Marca ${when(slot)} com ${slot.therapist_name}`, `Books ${when(slot)} with ${slot.therapist_name}`) : undefined}>{looks.length && !sug?.patient ? L("Marcar…", "Book…") : L("Marcar", "Book")}</button>
      </span>
    </div>
  );
}

/** The request looks like a patient who already has a file: use that file, or make a new one. */
function WhichFile({ r, slot, looks, when, onPick }: { r: BookingRequest; slot: RequestSlot; looks: RequestSuggestion["lookalikes"]; when: string; onPick: (patientId?: string) => void }) {
  const { L, closeSheet } = useApp();
  const [pick, setPick] = useState<string>(looks[0].id);
  const why = (x: RequestSuggestion["lookalikes"][number]) => x.reason === "phone" ? L("mesmo telemóvel", "same mobile") : x.reason === "email" ? L("mesmo email", "same email") : L("mesmo nome", "same name");
  return (
    <Sheet title={L("Marcar pedido", "Book Request")} sub={L(`${r.name} · ${when} com ${slot.therapist_name}`, `${r.name} · ${when} with ${slot.therapist_name}`)}
      action={L("Marcar", "Book")} onAction={() => { closeSheet(); onPick(pick === "new" ? undefined : pick); }}>
      <p className="small muted" style={{ marginTop: 0 }}>{L("Este pedido parece ser de alguém que já tem ficha.", "This request looks like someone who already has a file.")}</p>
      <div className="group" role="radiogroup" aria-label={L("Ficha a usar", "File to use")}>
        {looks.map((x) => (
          <label key={x.id} className="setrow" style={{ cursor: "default" }}>
            <input type="radio" name="file" checked={pick === x.id} onChange={() => setPick(x.id)} />
            <span className="grow">{x.name} ({L("nº", "no.")} {x.number})<span className="d" style={{ display: "block" }}>{why(x)}</span></span>
          </label>
        ))}
        <label className="setrow" style={{ cursor: "default" }}>
          <input type="radio" name="file" checked={pick === "new"} onChange={() => setPick("new")} />
          <span className="grow">{L("Criar ficha nova", "Make a new file")}<span className="d" style={{ display: "block" }}>{L("Com o nome e os contactos do pedido.", "With the request’s name and contacts.")}</span></span>
        </label>
      </div>
    </Sheet>
  );
}
