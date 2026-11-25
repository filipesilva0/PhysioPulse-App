// Today and Schedule on the iPhone (phase 5), and the sheet that opens when you tap an appointment.
// Owner extras on Today: new devices waiting for approval (your phone can be the first approver) and booking
// requests from the website or reception, each booked with one press like on the desktop.
import { useEffect, useRef, useState } from "react";
import { Bell, CalendarDays, CalendarCheck, Check, ChevronLeft, ChevronRight, FileSignature, Inbox, Mic, MonitorSmartphone, MoreHorizontal, Package, Pill, Search, SlidersHorizontal, Smartphone, TrendingDown, UserX, FolderOpen, Undo2 } from "lucide-react";
import { useApp } from "../lib/ctx";
import { api, isErr } from "../lib/api";
import {
  useApi, addDays, monday, sameDay, ymd, hm, range, fmtDate, fmtDay, weekdayShort, kindLabel, statusLabel, consentLabel, testName,
  type Alerts, type Appointment, type BookingRequest, type RequestSuggestion, type RequestSlot, type Therapist,
} from "../lib/data";
import { planName } from "../lib/money";
import { ApptCard, Circle, Group, PEmpty, Row, Screen, SecTitle, SheetHead, apptState, apptTitle } from "./ui";
import { canRecord, type PhoneNav } from "./PhoneApp";

const cap = (s: string) => s.replace(/^./, (c) => c.toUpperCase());

// ───────── Today (reference: the phone home) ─────────

/** Set by the search pill on Today, read by the patient list when it opens (focus the field). */
export const searchNav = { focus: false };

export function TodayScreen({ nav }: { nav: PhoneNav }) {
  const app = useApp();
  const { L, lang, me, openSheet } = app;
  const now = new Date();
  const day0 = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const { data: list, error } = useApi<Appointment[]>(`/appointments?${range(day0, addDays(day0, 1))}`);
  const { data: ths } = useApi<Therapist[]>("/therapists");
  const { data: alerts } = useApi<Alerts>("/alerts");
  const owner = me.role === "owner";
  const { data: reqs } = useApi<BookingRequest[]>(owner ? "/booking-requests?status=pending" : null);

  const today = (list ?? []).filter((x) => x.kind !== "block" && x.status !== "cancelled").sort((a, b) => a.starts_at.localeCompare(b.starts_at));
  // Therapists see their own day first; the owner and reception see everyone's.
  const mine = me.role === "therapist" ? today.filter((x) => x.therapist_id === me.id) : today;
  const others = me.role === "therapist" ? today.filter((x) => x.therapist_id !== me.id) : [];
  const first = me.name.split(" ")[0];
  const h = now.getHours();
  const hello = h < 13 ? L(`Bom dia, ${first}`, `Good morning, ${first}`) : h < 20 ? L(`Boa tarde, ${first}`, `Good afternoon, ${first}`) : L(`Boa noite, ${first}`, `Good evening, ${first}`);
  const open = (x: Appointment) => openSheet(<ApptSheet x={x} nav={nav} />);

  type A = { ic: JSX.Element; tone: string; t: string; pid: string };
  const rows: A[] = [];
  alerts?.worse_scores?.forEach((w) => rows.push({ ic: <TrendingDown />, tone: "var(--bad)", pid: w.patient_id, t: L(`${w.name}: ${testName(w.test_code, lang)} piorou (${w.previous} → ${w.score})`, `${w.name}: ${testName(w.test_code, lang)} worse (${w.previous} → ${w.score})`) }));
  alerts?.consents_missing.forEach((c) => rows.push({ ic: <FileSignature />, tone: "var(--warn)", pid: c.patient_id, t: L(`${c.name}: falta assinar ${c.missing.map((k) => consentLabel(k, "pt").split(" (")[0].toLowerCase()).join(", ")}`, `${c.name}: not signed (${c.missing.map((k) => consentLabel(k, "en").split(" (")[0].toLowerCase()).join(", ")})`) }));
  alerts?.prescriptions?.forEach((p) => rows.push({ ic: <Pill />, tone: "var(--p-sub)", pid: p.patient_id, t: p.remaining <= 2 ? L(`${p.name}: prescrição a acabar (${p.remaining === 1 ? "resta 1" : `restam ${p.remaining}`})`, `${p.name}: prescription ending (${p.remaining} left)`) : L(`${p.name}: prescrição expira a ${fmtDate(p.valid_until!, "pt", { day: "numeric", month: "short" })}`, `${p.name}: prescription expires ${fmtDate(p.valid_until!, "en", { day: "numeric", month: "short" })}`) }));
  alerts?.plans_low?.forEach((p) => rows.push({ ic: <Package />, tone: "var(--p-sub)", pid: p.patient_id, t: L(`${p.name}: ${planName(p, "pt")} a acabar (restam ${p.remaining})`, `${p.name}: ${planName(p, "en")} ending (${p.remaining} left)`) }));
  const attention = () => openSheet(<AttentionSheet rows={rows} nav={nav} />);

  const devices = alerts?.devices ?? [];
  const pending = devices.length + (reqs?.length ?? 0);
  return (
    <Screen home title={hello} sub={cap(fmtDay(now, lang))} label={L("Hoje", "Today")}
      trailing={<>
        <Circle label={L("A tratar", "Needs attention")} badge={rows.length} onClick={attention}><Bell /></Circle>
        <Circle label={L("Agenda", "Schedule")} onClick={() => nav.setTab("schedule")}><CalendarDays /></Circle>
      </>}>
      <button className="p-searchpill" onClick={() => { searchNav.focus = true; nav.setTab("patients"); }}>
        <Search aria-hidden /><span>{L("Procurar utente", "Search patients")}</span>
      </button>

      <DayCard mine={mine} loaded={!!list} onOpen={() => nav.setTab("schedule")} />

      {owner && pending > 0 && <>
        <SecTitle title={L("Para aprovar", "To approve")} action={<span className="p-count num">{pending}</span>} />
        <div className="p-hscroll">
          {devices.map((d) => <DeviceRow key={d.id} d={d} />)}
          {reqs?.map((r) => <RequestRow key={r.id} r={r} />)}
        </div>
      </>}

      <SecTitle title={L("Sessões de hoje", "Today’s sessions")} action={<button className="p-link" onClick={() => nav.setTab("schedule")}>{L("Ver tudo", "See All")}</button>} />
      {error && <p className="p-err">{error}</p>}
      {!list && !error && <div className="p-skel" />}
      {list && mine.length === 0 && <PEmpty icon={<CalendarDays />} title={L("Nada marcado para hoje", "Nothing booked today")} />}
      <div className="p-cards">{mine.map((x) => <ApptCard key={x.id} x={x} when="day" onClick={() => open(x)} />)}</div>
      {others.length > 0 && <>
        <SecTitle title={L("Restante equipa", "Rest of the team")} />
        <div className="p-cards">{others.map((x) => <ApptCard key={x.id} x={x} when="day" onClick={() => open(x)} />)}</div>
      </>}
    </Screen>
  );
}

/** "See your performance" in the reference, here your day: how many sessions, how many done, and a grid of the
 *  day from 08:00 to 20:00 — one column per hour, a filled square per quarter-hour that is booked. */
function DayCard({ mine, loaded, onOpen }: { mine: Appointment[]; loaded: boolean; onOpen: () => void }) {
  const { L } = useApp();
  const done = mine.filter((x) => x.status === "done").length;
  const H0 = 8, H1 = 20;
  const cols = Array.from({ length: H1 - H0 }, (_, i) => {
    let q = 0;
    for (let k = 0; k < 4; k++) {
      const t0 = (H0 + i) * 60 + k * 15;
      if (mine.some((x) => { const s = new Date(x.starts_at), e = new Date(x.ends_at); const a = s.getHours() * 60 + s.getMinutes(), b = e.getHours() * 60 + e.getMinutes(); return a < t0 + 15 && b > t0; })) q++;
    }
    return q;
  });
  const booked = cols.reduce((a, b) => a + b, 0);
  const pct = Math.round((booked / ((H1 - H0) * 4)) * 100);
  const nowCol = new Date().getHours() - H0;
  return (
    <div className="p-daycard">
      <div className="p-daycard-l">
        <b>{L("O seu dia", "Your day")}</b>
        <span>{loaded ? (mine.length ? L(`${done} de ${mine.length} feitas`, `${done} of ${mine.length} done`) : L("Livre", "Free")) : " "}</span>
        <button className="p-btn dark sm" onClick={onOpen}>{L("Ver agenda", "See Schedule")}</button>
      </div>
      <div className="p-grid" role="img" aria-label={L(`Dia ${pct}% ocupado entre as 8h e as 20h`, `Day ${pct}% booked between 8:00 and 20:00`)}>
        {cols.map((q, i) => (
          <span key={i} className={`p-grid-col ${i === nowCol ? "now" : ""}`}>
            {[3, 2, 1, 0].map((k) => <i key={k} className={k < q ? "on" : ""} />)}
          </span>
        ))}
      </div>
    </div>
  );
}

/** The bell: what needs attention (worse scores, consents, prescriptions and packages running out). */
function AttentionSheet({ rows, nav }: { rows: { ic: JSX.Element; tone: string; t: string; pid: string }[]; nav: PhoneNav }) {
  const { L, closeSheet } = useApp();
  return (
    <>
      <SheetHead title={L("A tratar", "Needs Attention")} onClose={closeSheet} />
      <div className="p-sheet-body">
        {rows.length ? <Group>{rows.map((a, i) => <Row key={i} label={a.t} chevron onClick={() => nav.openPatient(a.pid)} />)}</Group>
          : <PEmpty icon={<Bell />} title={L("Nada a tratar", "Nothing needs attention")} />}
      </div>
    </>
  );
}

/** A new computer or phone waiting for the owner (phase 3). The owner's phone can approve it. */
function DeviceRow({ d }: { d: NonNullable<Alerts["devices"]>[number] & { kind?: string } }) {
  const { L, toast, confirm } = useApp();
  const [busy, setBusy] = useState(false);
  const act = (what: "approve" | "revoke") => {
    setBusy(true);
    api("POST", `/devices/${d.id}/${what}`)
      .then(() => toast(what === "approve" ? L(`${d.name} aprovado`, `${d.name} approved`) : L(`${d.name} recusado`, `${d.name} declined`)))
      .catch((e) => toast(isErr(e) ? e.message : String(e))).finally(() => setBusy(false));
  };
  return (
    <div className="p-hcard">
      <span className="p-hcard-k">{L("Dispositivo novo", "New device")}</span>
      <b>{d.name}</b>
      <span className="p-hcard-s">{d.requested_by_name ?? "—"} · <b className="num">{d.code}</b></span>
        <span className="p-req-btns">
          <button className="p-btn sm" disabled={busy} onClick={() => confirm({ title: L(`Recusar ${d.name}?`, `Decline ${d.name}?`), text: L("Este dispositivo não consegue entrar até o aprovar em Definições › Dispositivos, no computador.", "This device can’t sign in until you approve it in Settings › Devices on the computer."), action: L("Recusar", "Decline"), destructive: true, onConfirm: () => act("revoke") })}>{L("Recusar…", "Decline…")}</button>
          <button className="p-btn sm dark" disabled={busy} onClick={() => act("approve")}>{L("Aprovar", "Approve")}</button>
        </span>
    </div>
  );
}

const SERVICE: Record<string, [string, string]> = { assessment: ["Avaliação inicial", "First assessment"], session: ["Sessão", "Session"], home: ["Domicílio", "Home visit"], class: ["Aula de grupo", "Group class"] };
const PART: Record<string, [string, string]> = { morning: ["de manhã", "morning"], afternoon: ["à tarde", "afternoon"], any: ["qualquer hora", "any time"] };
const slotText = (s: RequestSlot, lang: "pt" | "en") => `${cap(fmtDate(s.starts_at, lang, { weekday: "short", day: "numeric", month: "short" }))}, ${hm(s.starts_at)}`;

/** A booking request: who, what, when they'd like it; one press books the first free time that fits. */
function RequestRow({ r }: { r: BookingRequest }) {
  const { L, lang, toast, openSheet, confirm } = useApp();
  const svc = SERVICE[r.service] ? L(...SERVICE[r.service]) : r.service;
  const when = r.day_part ? `${cap(fmtDate(r.preferred_at, lang, { weekday: "short", day: "numeric", month: "short" }))}, ${L(...PART[r.day_part] ?? ["", ""])}` : `${cap(fmtDate(r.preferred_at, lang, { weekday: "short", day: "numeric", month: "short" }))}, ${hm(r.preferred_at)}`;
  const decline = () => api("POST", `/booking-requests/${r.id}/decline`)
    .then(() => toast(L("Pedido recusado", "Request declined"), () => { api("POST", `/booking-requests/${r.id}/reopen`).catch((e) => toast(isErr(e) ? e.message : String(e))); }))
    .catch((e) => toast(isErr(e) ? e.message : String(e)));
  return (
    <div className="p-hcard">
      <span className="p-hcard-k">{L("Pedido", "Request")}</span>
      <b>{r.name}</b>
      <span className="p-hcard-s">{svc} · {when}</span>
      {r.message && <span className="p-hcard-s p-quote">“{r.message}”</span>}
        <span className="p-req-btns">
          <button className="p-btn sm" onClick={() => confirm({ title: L(`Recusar o pedido de ${r.name}?`, `Decline ${r.name}’s request?`), text: L("Pode desfazer logo a seguir.", "You can undo it right after."), action: L("Recusar", "Decline"), destructive: true, onConfirm: decline })}>{L("Recusar…", "Decline…")}</button>
          <button className="p-btn sm dark" onClick={() => openSheet(<BookSheet r={r} />)}>{L("Marcar…", "Book…")}</button>
        </span>
    </div>
  );
}

/** One-press booking (same server route as the desktop): the suggested time, other times, and which file. */
function BookSheet({ r }: { r: BookingRequest }) {
  const { L, lang, toast, closeSheet } = useApp();
  const { data: sug, error } = useApi<RequestSuggestion>(`/booking-requests/${r.id}/suggest`);
  const [slot, setSlot] = useState<RequestSlot | null>(null);
  const [pid, setPid] = useState<string | null | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (sug) { setSlot(sug.slot ?? sug.alternatives[0] ?? null); setPid(sug.patient?.id ?? (sug.lookalikes.length ? undefined : null)); } }, [sug]);
  const slots = sug ? [sug.slot, ...sug.alternatives].filter((x): x is RequestSlot => !!x).filter((x, i, a) => a.findIndex((y) => y.starts_at === x.starts_at && y.therapist_id === x.therapist_id) === i).slice(0, 5) : [];
  const ready = !!slot && pid !== undefined;
  const book = () => {
    if (!slot) return;
    setBusy(true);
    api<{ appointment_id: string }>("POST", `/booking-requests/${r.id}/book`, { starts_at: slot.starts_at, ends_at: slot.ends_at, therapist_id: slot.therapist_id, patient_id: pid ?? null })
      .then(() => { closeSheet(); toast(L(`${r.name}: marcado ${slotText(slot, "pt")}`, `${r.name}: booked ${slotText(slot, "en")}`), () => { api("POST", `/booking-requests/${r.id}/reopen`).catch((e) => toast(isErr(e) ? e.message : String(e))); }); })
      .catch((e) => toast(isErr(e) ? e.message : String(e))).finally(() => setBusy(false));
  };
  return (
    <>
      <SheetHead title={L("Marcar pedido", "Book Request")} onClose={closeSheet} closeLabel={L("Cancelar", "Cancel")}
        action={<button className="p-link b" disabled={!ready || busy} onClick={book}>{L("Marcar", "Book")}</button>} />
      <div className="p-sheet-body">
        <p className="p-sheet-lead"><b>{r.name}</b> · {SERVICE[r.service] ? L(...SERVICE[r.service]) : r.service}{r.phone ? ` · ${r.phone}` : ""}</p>
        {error && <p className="p-err">{error}</p>}
        {!sug && !error && <div className="p-skel" />}
        {sug && (slots.length ? (
          <Group title={L("Hora", "Time")}>
            {slots.map((s) => <Row key={s.starts_at + s.therapist_id} label={slotText(s, lang)} detail={s.therapist_name} selected={slot === s}
              value={slot === s ? <Check className="p-check" /> : undefined} onClick={() => setSlot(s)} />)}
          </Group>
        ) : <PEmpty icon={<CalendarDays />} title={L("Sem horas livres nos próximos 14 dias", "No free times in the next 14 days")} text={L("Marque-o à mão no computador.", "Book it by hand on the computer.")} />)}
        {sug && !sug.patient && sug.lookalikes.length > 0 && (
          <Group title={L("Ficha", "File")}>
            {sug.lookalikes.map((p) => <Row key={p.id} label={p.name} detail={`Nº ${String(p.number).padStart(4, "0")} · ${p.reason === "phone" ? L("mesmo telefone", "same phone") : p.reason === "email" ? L("mesmo email", "same email") : L("mesmo nome", "same name")}`}
              selected={pid === p.id} value={pid === p.id ? <Check className="p-check" /> : undefined} onClick={() => setPid(p.id)} />)}
            <Row label={L("Ficha nova", "New file")} selected={pid === null} value={pid === null ? <Check className="p-check" /> : undefined} onClick={() => setPid(null)} />
          </Group>
        )}
      </div>
    </>
  );
}

// ───────── Schedule (reference: the phone schedule) ─────────

export function ScheduleScreen({ nav }: { nav: PhoneNav }) {
  const { L, lang, me, openSheet } = useApp();
  const now = new Date();
  const today0 = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const [day, setDay] = useState(today0);
  const [who, setWho] = useState<"me" | "all">(me.role === "therapist" ? "me" : "all");
  const wk = monday(day);
  const { data: week, error } = useApi<Appointment[]>(`/appointments?${range(wk, addDays(wk, 7))}`, { keep: true });
  const { data: ths } = useApi<Therapist[]>("/therapists");
  const visible = (week ?? []).filter((x) => x.status !== "cancelled" && (who === "all" || x.therapist_id === me.id || (x.kind === "block" && !x.therapist_id)));
  const ofDay = visible.filter((x) => sameDay(new Date(x.starts_at), day)).sort((a, b) => a.starts_at.localeCompare(b.starts_at));
  const busyDays = new Set(visible.filter((x) => x.kind !== "block").map((x) => ymd(new Date(x.starts_at))));
  const days = Array.from({ length: 7 }, (_, i) => addDays(wk, i));
  const wd = weekdayShort(lang);
  const shift = (n: number) => setDay((d) => addDays(d, n));
  const touch = useRef<number | null>(null);
  const isToday = sameDay(day, now);
  const month = cap(new Intl.DateTimeFormat(lang === "pt" ? "pt-PT" : "en-GB", { month: "long", year: "numeric" }).format(day));

  // Every hour from 8:00 (or earlier) to 19:00 (or later) gets a label; appointments sit under the hour they start.
  const hours = ofDay.map((x) => new Date(x.starts_at).getHours());
  const h0 = Math.min(8, ...hours), h1 = Math.max(19, ...hours);
  const nowH = isToday ? now.getHours() : -1;
  const options = () => openSheet(<ScheduleOptions who={who} setWho={setWho} canMine={me.role !== "reception"} goToday={() => setDay(today0)} />);
  return (
    <Screen title={L("Agenda", "Schedule")} label={L("Agenda", "Schedule")}
      leading={<Circle label={L("Hoje", "Today")} onClick={() => setDay(today0)}><CalendarCheck /></Circle>}
      trailing={<Circle label={L("Opções da agenda", "Schedule Options")} onClick={options}><MoreHorizontal /></Circle>}>
      <div className="p-month">
        <b>{month}</b>
        <span>
          <button className="p-mini" onClick={() => shift(-7)} aria-label={L("Semana anterior", "Previous week")}><ChevronLeft /></button>
          <button className="p-mini" onClick={() => shift(7)} aria-label={L("Semana seguinte", "Next week")}><ChevronRight /></button>
        </span>
      </div>
      <div className="p-days" role="tablist" aria-label={L("Dias da semana", "Days of the week")}
        onTouchStart={(e) => { touch.current = e.touches[0].clientX; }}
        onTouchEnd={(e) => { const s0 = touch.current; touch.current = null; if (s0 == null) return; const dx = e.changedTouches[0].clientX - s0; if (Math.abs(dx) > 50) shift(dx < 0 ? 7 : -7); }}>
        {days.map((d, i) => {
          const sel = sameDay(d, day), today = sameDay(d, now);
          return (
            <button key={i} role="tab" aria-selected={sel} className={`p-day ${sel ? "sel" : ""} ${today ? "today" : ""}`} onClick={() => setDay(d)}
              aria-label={cap(fmtDate(d, lang, { weekday: "long", day: "numeric", month: "long" }))}>
              <span>{wd[i]}</span><b className="num">{d.getDate()}</b><i className={busyDays.has(ymd(d)) ? "dot" : ""} />
            </button>
          );
        })}
      </div>

      {error && <p className="p-err">{error}</p>}
      {!week && !error && <div className="p-skel" />}
      {week && ofDay.length === 0 && <PEmpty icon={<CalendarCheck />} title={L("Nada marcado neste dia", "Nothing booked this day")} />}
      {week && ofDay.length > 0 && (
        <ol className="p-timeline">
          {Array.from({ length: h1 - h0 + 1 }, (_, i) => h0 + i).map((h) => {
            const here = ofDay.filter((x) => new Date(x.starts_at).getHours() === h);
            return (
              <li key={h} className={here.length ? "has" : ""}>
                <span className={`p-hour num ${h === nowH ? "now" : ""}`}>{`${String(h).padStart(2, "0")}:00`}</span>
                <div className="p-hour-cards">{here.map((x) => <ApptCard key={x.id} x={x} note onClick={() => openSheet(<ApptSheet x={x} nav={nav} />)} />)}</div>
              </li>
            );
          })}
        </ol>
      )}
    </Screen>
  );
}

/** ⋯ on the schedule: whose schedule, and back to today. */
function ScheduleOptions({ who, setWho, canMine, goToday }: { who: "me" | "all"; setWho: (w: "me" | "all") => void; canMine: boolean; goToday: () => void }) {
  const { L, closeSheet } = useApp();
  const pick = (w: "me" | "all") => { setWho(w); closeSheet(); };
  return (
    <>
      <SheetHead title={L("Agenda", "Schedule")} onClose={closeSheet} />
      <div className="p-sheet-body">
        <Group title={L("Mostrar", "Show")}>
          {canMine && <Row label={L("A minha agenda", "My schedule")} selected={who === "me"} value={who === "me" ? <Check className="p-check" /> : undefined} onClick={() => pick("me")} />}
          <Row label={L("Toda a equipa", "Everyone")} selected={who === "all"} value={who === "all" ? <Check className="p-check" /> : undefined} onClick={() => pick("all")} />
        </Group>
        <Group><Row label={L("Ir para hoje", "Go to Today")} chevron onClick={() => { goToday(); closeSheet(); }} /></Group>
      </div>
    </>
  );
}

// ───────── one appointment ─────────

/** What you can do with an appointment on the phone: see it, mark it done or missed, open the file, record it. */
export function ApptSheet({ x, nav }: { x: Appointment; nav: PhoneNav }) {
  const { L, lang, me, toast, closeSheet } = useApp();
  const [busy, setBusy] = useState(false);
  const st = apptState(x);
  const setStatus = (s: "done" | "no_show" | "booked") => {
    setBusy(true);
    api("PATCH", `/appointments/${x.id}`, { status: s, scope: "one" })
      .then(() => {
        closeSheet();
        if (s === "booked") return toast(L("Marcação reposta", "Appointment restored"));
        toast(s === "done" ? L("Marcada como realizada", "Marked as done") : L("Marcada como falta", "Marked as no-show"),
          () => { api("PATCH", `/appointments/${x.id}`, { status: "booked", scope: "one" }).catch((e) => toast(isErr(e) ? e.message : String(e))); });
      })
      .catch((e) => toast(isErr(e) ? e.message : String(e))).finally(() => setBusy(false));
  };
  const started = new Date(x.starts_at) <= new Date();
  const isToday = sameDay(new Date(x.starts_at), new Date());
  const patient = !!x.patient_id && x.kind !== "class" && x.kind !== "block";
  return (
    <>
      <SheetHead title={kindLabel(x.kind, lang)} onClose={closeSheet} />
      <div className="p-sheet-body">
        <h3 className="p-sheet-title">{apptTitle(x, lang)}</h3>
        <p className="p-sheet-lead">{cap(fmtDate(x.starts_at, lang, { weekday: "long", day: "numeric", month: "long" }))} · <span className="num">{hm(x.starts_at)} – {hm(x.ends_at)}</span></p>
        <Group>
          {x.therapist_name && <Row label={L("Fisioterapeuta", "Therapist")} value={x.therapist_name} />}
          <Row label={L("Estado", "Status")} value={st === "now" ? L("A decorrer", "In progress") : st === "past" ? L("Por fechar", "Not marked") : statusLabel(x.status, lang)} />
          {x.kind === "home" && x.address && <Row label={L("Morada", "Address")} detail={<span className="p-select">{x.address}</span>} />}
          {x.kind === "home" && x.travel_minutes > 0 && <Row label={L("Deslocação", "Travel")} value={`${x.travel_minutes} min`} />}
          {x.kind === "class" && <Row label={L("Inscritos", "Booked")} value={`${x.attendees?.length ?? 0} / ${x.capacity ?? "—"}`} />}
          {x.admin_note && <Row label={L("Nota", "Note")} detail={<span className="p-select">{x.admin_note}</span>} />}
        </Group>
        {x.kind === "class" && (x.attendees?.length ?? 0) > 0 && (
          <Group title={L("Inscritos", "Attendees")}>{x.attendees!.map((a) => <Row key={a.patient_id} label={a.name} chevron onClick={() => nav.openPatient(a.patient_id)} />)}</Group>
        )}
        <div className="p-actions">
          {patient && x.status === "booked" && started && <button className="p-btn pri wide" disabled={busy} onClick={() => setStatus("done")}><Check />{L("Marcar como realizada", "Mark as Done")}</button>}
          {patient && x.status === "booked" && canRecord(me) && isToday && st !== "past" && (
            <button className="p-btn wide" onClick={() => nav.recordFor({ patientId: x.patient_id!, name: x.patient_name ?? "", appointmentId: x.id })}><Mic />{L("Gravar esta sessão…", "Record This Session…")}</button>
          )}
          {patient && <button className="p-btn wide" onClick={() => nav.openPatient(x.patient_id!)}><FolderOpen />{L("Abrir ficha", "Open File")}</button>}
          {patient && x.status === "booked" && started && <button className="p-btn wide" disabled={busy} onClick={() => setStatus("no_show")}><UserX />{L("Marcar falta", "Mark as No-Show")}</button>}
          {patient && (x.status === "done" || x.status === "no_show") && <button className="p-btn wide" disabled={busy} onClick={() => setStatus("booked")}><Undo2 />{L("Repor marcação", "Restore Appointment")}</button>}
        </div>
      </div>
    </>
  );
}
