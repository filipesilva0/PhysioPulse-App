import { useLayoutEffect, useRef, useState } from "react";
import { CalendarDays, PenLine, FileText, MoreHorizontal, CalendarPlus, ChevronRight, FileSignature, Inbox, Pill, Receipt, TrendingDown, Users, Home, Check, CircleCheck, Package, HandCoins, BellRing, MessageSquareWarning, HardDrive, MonitorSmartphone } from "lucide-react";
import { useApp, eur } from "../lib/ctx";
import { c2e, planName } from "../lib/money";
import {
  useApi, addDays, monday, sameDay, hm, range, fmtDate, fmtDay, toneFor, kindLabel, statusLabel, consentLabel, testName, blockLabel,
  type Alerts, type Appointment, type Therapist, type Waiting, type BookingRequest,
} from "../lib/data";
import { Layout } from "../components/Layout";
import { Badge, Empty, Head, Sheet } from "../components/ui";
import { NewAppointment, RecordPayment } from "../components/Sheets";
import { ComplaintsLog } from "../components/ClinicSheets";
import { apptItems, openMenu } from "../components/menus";
import { remindersNav, type ReminderDay } from "../components/Reminders";
import { settingsNav, whenLabel } from "./Settings";
import { patientNav } from "./Patients";

export default function Today() {
  const app = useApp();
  const { L, lang, me, openSheet, openPatient, go } = app;
  const now = new Date();
  const day0 = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  // Today and tomorrow (tomorrow fills the agenda tile's spare room; 23 Nov 2026 overhaul).
  const { data: week, error } = useApi<Appointment[]>(`/appointments?${range(day0, addDays(day0, 2))}`);
  const { data: alerts } = useApi<Alerts>("/alerts");
  const { data: rem } = useApi<ReminderDay>("/reminders");
  const remTodo = rem?.items.filter((i) => !i.sent_at && i.prefers !== "none").length ?? 0;
  const { data: ths } = useApi<Therapist[]>("/therapists");
  const { data: wait } = useApi<Waiting[]>("/waitlist");
  const { data: reqs } = useApi<BookingRequest[]>("/booking-requests?status=pending");

  const today = (week ?? []).filter((x) => sameDay(new Date(x.starts_at), day0)).sort((a, b) => a.starts_at.localeCompare(b.starts_at));
  const sessions = today.filter((x) => x.kind !== "block");
  const day1 = addDays(day0, 1);
  const tomorrow = (week ?? []).filter((x) => x.kind !== "block" && x.status !== "cancelled" && sameDay(new Date(x.starts_at), day1)).sort((a, b) => a.starts_at.localeCompare(b.starts_at));
  const title = (x: Appointment) => x.kind === "class" ? x.title || kindLabel("class", lang) : x.kind === "block" ? blockLabel(x.title, lang) : x.patient_name ?? "—";
  const state = (x: Appointment) => x.status !== "booked" ? x.status : new Date(x.ends_at) <= now ? "past" : new Date(x.starts_at) <= now ? "now" : "next";

  type Row = { ic: JSX.Element; tone: string; t: string; act: () => void };
  const n = (k: number, one: string, many: string) => `${k} ${k === 1 ? one : many}`;
  const rows: Row[] = [];
  if (alerts) {
    // Owner: the nightly backup stopped, failed or can't be seen (audit C1).
    if (alerts.backup) {
      const bk = alerts.backup;
      rows.push({ ic: <HardDrive />, tone: bk.state === "failed" ? "var(--bad)" : "var(--warn)", act: () => { settingsNav.sec = "server"; go("settings"); },
        t: bk.state === "failed" ? L("A última cópia de segurança falhou", "The last backup failed")
          : bk.state === "unknown" ? L("Não há notícias das cópias de segurança", "No news from the backups")
          : bk.last_ok ? L(`Última cópia de segurança: ${whenLabel(bk.last_ok, L)}`, `Last backup: ${whenLabel(bk.last_ok, L)}`) : L("Ainda não há cópias de segurança", "No backups yet") });
    }
    // Owner: a new computer or phone waits for approval (phase 3).
    alerts.devices?.forEach((d) => rows.push({ ic: <MonitorSmartphone />, tone: "var(--warn)", act: () => { settingsNav.sec = "devices"; go("settings"); },
      t: L(`${d.requested_by_name ?? "Alguém"} pede para entrar num dispositivo novo (${d.code})`, `${d.requested_by_name ?? "Someone"} asks to sign in on a new device (${d.code})`) }));
    if (remTodo) rows.push({ ic: <BellRing />, tone: "var(--a)", t: L(n(remTodo, "lembrete para amanhã por enviar", "lembretes para amanhã por enviar"), n(remTodo, "reminder for tomorrow to send", "reminders for tomorrow to send")),
      act: () => { remindersNav.open = true; go("schedule"); } });
    alerts.complaints?.forEach((c) => rows.push({ ic: <MessageSquareWarning />, tone: c.working_days_left <= 0 ? "var(--bad)" : "var(--warn)", act: () => openSheet(<ComplaintsLog open={c.id} />),
      t: c.working_days_left < 0
        ? L(`Reclamação${c.complainant ? ` de ${c.complainant}` : ""}: prazo da ERS passou a ${fmtDate(c.due_on, "pt", { day: "numeric", month: "short" })}`, `Complaint${c.complainant ? ` from ${c.complainant}` : ""}: ERS deadline passed on ${fmtDate(c.due_on, "en", { day: "numeric", month: "short" })}`)
        : c.working_days_left === 0
          ? L(`Reclamação${c.complainant ? ` de ${c.complainant}` : ""}: o prazo da ERS termina hoje`, `Complaint${c.complainant ? ` from ${c.complainant}` : ""}: ERS deadline is today`)
          : L(`Reclamação${c.complainant ? ` de ${c.complainant}` : ""}: ${c.working_days_left === 1 ? "falta 1 dia útil" : `faltam ${c.working_days_left} dias úteis`} (ERS)`, `Complaint${c.complainant ? ` from ${c.complainant}` : ""}: ${c.working_days_left} working ${c.working_days_left === 1 ? "day" : "days"} left (ERS)`) }));
    if (alerts.booking_requests) rows.push({ ic: <Inbox />, tone: "var(--a)", t: L(n(alerts.booking_requests, "pedido de marcação", "pedidos de marcação"), n(alerts.booking_requests, "booking request", "booking requests")), act: () => go("schedule") });
    alerts.worse_scores?.forEach((w) => rows.push({ ic: <TrendingDown />, tone: "var(--bad)", t: L(`${w.name}: ${testName(w.test_code, lang)} piorou (${w.previous} → ${w.score})`, `${w.name}: ${testName(w.test_code, lang)} worse (${w.previous} → ${w.score})`), act: () => openPatient(w.patient_id) }));
    alerts.consents_missing.forEach((c) => rows.push({ ic: <FileSignature />, tone: "var(--warn)", t: L(`${c.name}: falta assinar ${c.missing.map((k) => consentLabel(k, "pt").split(" (")[0].toLowerCase()).join(", ")}`, `${c.name}: not signed (${c.missing.map((k) => consentLabel(k, "en").split(" (")[0].toLowerCase()).join(", ")})`), act: () => openPatient(c.patient_id) }));
    alerts.consents_expiring.forEach((c) => rows.push({ ic: <FileSignature />, tone: "var(--a)", t: L(`${c.name}: consentimento expira a ${fmtDate(c.expires_on, "pt", { day: "numeric", month: "short" })}`, `${c.name}: consent expires ${fmtDate(c.expires_on, "en", { day: "numeric", month: "short" })}`), act: () => openPatient(c.patient_id) }));
    alerts.prescriptions?.forEach((p) => rows.push({ ic: <Pill />, tone: "var(--a)", act: () => openPatient(p.patient_id),
      t: p.remaining <= 2 ? L(`${p.name}: prescrição a acabar (${p.remaining === 1 ? "resta 1" : `restam ${p.remaining}`})`, `${p.name}: prescription ending (${p.remaining} left)`)
        : L(`${p.name}: prescrição expira a ${fmtDate(p.valid_until!, "pt", { day: "numeric", month: "short" })}`, `${p.name}: prescription expires ${fmtDate(p.valid_until!, "en", { day: "numeric", month: "short" })}`) }));
    alerts.plans_low?.forEach((p) => rows.push({ ic: <Package />, tone: "var(--a)", act: () => openPatient(p.patient_id),
      t: p.remaining <= 1 ? L(`${p.name}: ${planName(p, "pt")} a acabar (resta ${p.remaining})`, `${p.name}: ${planName(p, "en")} ending (${p.remaining} left)`)
        : L(`${p.name}: pacote expira a ${fmtDate(p.expires_on!, "pt", { day: "numeric", month: "short" })} (restam ${p.remaining})`, `${p.name}: package expires ${fmtDate(p.expires_on!, "en", { day: "numeric", month: "short" })} (${p.remaining} left)`) }));
    if (alerts.money?.owed_cents) rows.push({ ic: <HandCoins />, tone: "var(--warn)", act: () => go("finance"),
      t: L(`Por receber: ${eur(c2e(alerts.money.owed_cents), "pt")} de ${n(alerts.money.owed_patients, "utente", "utentes")}`, `To collect: ${eur(c2e(alerts.money.owed_cents), "en")} from ${n(alerts.money.owed_patients, "patient", "patients")}`) });
  }
  /** Payment state of a done appointment: package, paid, or still owed. */
  const pay = (x: Appointment) => x.status !== "done" || x.kind === "class" || x.kind === "block" ? null
    : x.plan_id || x.charge_waived || !x.charge_cents || (x.paid_cents ?? 0) >= x.charge_cents ? null
      : <Badge tone="warn">{L("Por pagar", "Unpaid")}</Badge>;

  const h = now.getHours();
  const first = me.name.split(" ")[0];
  const hello = h < 13 ? L(`Bom dia, ${first}`, `Good morning, ${first}`) : h < 20 ? L(`Boa tarde, ${first}`, `Good afternoon, ${first}`) : L(`Boa noite, ${first}`, `Good evening, ${first}`);
  const classes = sessions.filter((x) => x.kind === "class");
  // "Sessões hoje" counts one-to-one sessions; group classes have their own figure (audit phase C: the class was counted twice).
  const single = sessions.filter((x) => x.kind !== "class");
  const done = single.filter((x) => x.status === "done").length;
  const homes = sessions.filter((x) => x.kind === "home");
  /** Clicking a session opens what you most likely want: the patient's file (classes and blocks open their menu). */
  const open = (x: Appointment, e: React.MouseEvent | React.KeyboardEvent) => x.patient_id ? openPatient(x.patient_id) : openMenu(app, e as React.MouseEvent, apptItems(app, x));
  // The day as squares, one per quarter-hour from 08:00 to 20:00 (as on the phone): filled = booked.
  const quarters = Array.from({ length: 48 }, (_, i) => {
    const t0 = new Date(day0); t0.setHours(8, i * 15); const t1 = new Date(t0.getTime() + 15 * 60000);
    return sessions.some((x) => x.status !== "cancelled" && new Date(x.starts_at) < t1 && new Date(x.ends_at) > t0);
  });
  // Only what fits is shown (owner, 12 Nov 2026: "there should not be a scrollable page, but a view more").
  const [agRef, agFit] = useFit(54);
  const [atRef, atFit] = useFit(48);
  const agShown = sessions.length > agFit ? sessions.slice(0, Math.max(1, agFit - 1)) : sessions;
  const atShown = rows.length > atFit ? rows.slice(0, Math.max(1, atFit - 1)) : rows;
  const allAttention = () => openSheet(
    <Sheet title={L("A tratar", "Needs attention")}>
      <div className="rows">{rows.map((a, i) => (
        <button key={i} className="row" onClick={() => { app.closeSheet(); a.act(); }}><span className="ic" style={{ color: a.tone }}>{a.ic}</span><span className="grow">{a.t}</span><ChevronRight size={14} className="chev" /></button>
      ))}</div>
    </Sheet>);
  const doneAll = sessions.filter((x) => x.status === "done").length;
  const upNext = sessions.find((x) => x.status === "booked" && x.kind !== "block" && new Date(x.ends_at) > now);
  const nowQ = Math.floor(((now.getHours() - 8) * 60 + now.getMinutes()) / 15);
  return (
    <Layout label={L("Hoje", "Today")}>
      <div className="wrap today">
        <Head title={hello} sub={fmtDay(now, lang).replace(/^./, (c) => c.toUpperCase())}>
          <button className="btn" onClick={() => openSheet(<RecordPayment />)}><Receipt />{L("Registar pagamento…", "Record Payment…")}</button>
          <button className="btn pri" onClick={() => openSheet(<NewAppointment />)}><CalendarPlus />{L("Nova marcação…", "New Appointment…")}</button>
        </Head>
        <div className="bento">
          {/* A seguir (23 Nov 2026 overhaul): the next person, with what you do before they come in. The day's
              quarter-hours stay underneath as context. */}
          <section className="tile day-tile" aria-label={L("A seguir", "Up next")}>
            {upNext ? (
              <>
                <p className="tile-sub up-when">{state(upNext) === "now" ? L(`Agora · até às ${hm(upNext.ends_at)}`, `Now · until ${hm(upNext.ends_at)}`) : L(`A seguir · ${hm(upNext.starts_at)}`, `Up next · ${hm(upNext.starts_at)}`)}</p>
                <h2 className="up-name">{title(upNext)}</h2>
                <p className="tile-sub">{[upNext.therapist_name, kindLabel(upNext.kind, lang)].filter(Boolean).join(" · ")}</p>
                {upNext.patient_id && (
                  <div className="up-actions">
                    <button className="btn sm on-dark" onClick={() => openPatient(upNext.patient_id!)}><FileText />{L("Abrir ficha", "Open File")}</button>
                    {me.role !== "reception" && <button className="btn sm on-dark ghost" onClick={() => { patientNav.tab = "notes"; openPatient(upNext.patient_id!); }}><PenLine />{L("Nota de evolução", "Progress Note")}</button>}
                  </div>
                )}
              </>
            ) : (
              <>
                <p className="tile-sub up-when">{L("O seu dia", "Your day")}</p>
                <h2 className="up-name">{sessions.length ? L("Mais nada hoje", "Nothing else today") : L("Dia livre", "Nothing booked")}</h2>
                <p className="tile-sub">{L(`${doneAll} de ${sessions.length} feitas`, `${doneAll} of ${sessions.length} done`)}</p>
              </>
            )}
            <div className="quarters" role="img" aria-label={L(`${sessions.length} sessões entre as 8 e as 20 horas`, `${sessions.length} sessions between 8 am and 8 pm`)}>
              {quarters.map((on, i) => <i key={i} className={`${on ? "on" : ""} ${i === nowQ ? "now" : ""}`} />)}
            </div>
            <div className="q-axis" aria-hidden="true"><span>08</span><span>11</span><span>14</span><span>17</span><span>20</span></div>
          </section>

          <section className="tile facts-tile" aria-label={L("Resumo", "Summary")}>
            <Fact label={L("Sessões hoje", "Sessions today")} value={String(single.length)} note={single.length ? L(`${done} realizada${done === 1 ? "" : "s"}`, `${done} done`) : undefined} />
            {(classes.length > 0 || homes.length > 0) && <Fact label={classes.length ? L("Aulas de grupo", "Group classes") : L("Domicílios", "Home visits")} value={String(classes.length || homes.length)} note={hm((classes[0] ?? homes[0]).starts_at)} />}
            <Fact label={L("Lista de espera", "Waitlist")} value={String(wait?.length ?? "—")} note={reqs?.length ? L(n(reqs.length, "pedido", "pedidos"), n(reqs.length, "request", "requests")) : undefined} onClick={() => go("schedule")} />
            {alerts?.money && <Fact label={L("Por receber", "To collect")} value={eur(c2e(alerts.money.owed_cents), lang)} note={L(`hoje entrou ${eur(c2e(alerts.money.today_cents), lang)}`, `${eur(c2e(alerts.money.today_cents), lang)} in today`)} onClick={() => go("finance")} />}
          </section>

          <section className="tile agenda-tile" aria-label={L("Agenda de hoje", "Today’s schedule")}>
            <div className="tile-head">
              <div><h2>{L("Agenda de hoje", "Today’s schedule")}</h2><p className="tile-sub">{L(n(sessions.length, "marcação", "marcações"), n(sessions.length, "appointment", "appointments"))}</p></div>
              <button className="btn sm" onClick={() => go("schedule")}>{L("Ver agenda", "See Schedule")}<ChevronRight size={14} /></button>
            </div>
            <div className="tile-body" ref={agRef}>
              {error && <p className="err">{error}</p>}
              {!week && !error && <div className="skel" style={{ height: 120 }} />}
              {week && sessions.length === 0 ? <Empty icon={<CalendarDays />} title={L("Nada marcado para hoje", "Nothing booked today")}
                action={<button className="btn pri" onClick={() => openSheet(<NewAppointment />)}><CalendarPlus />{L("Nova marcação…", "New Appointment…")}</button>} /> : sessions.length > 0 && (
                <div className="rows agenda">
                  {agShown.map((x) => {
                    const st = state(x);
                    const isClass = x.kind === "class";
                    return (
                      <div key={x.id} role="button" tabIndex={0} className={`row ${st === "now" ? "is-now" : st === "done" ? "is-done" : ""}`} onClick={(e) => open(x, e)}
                        onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(x, e); } }}
                        onContextMenu={(e) => openMenu(app, e, apptItems(app, x))}>
                        <span className="time"><b>{hm(x.starts_at)}</b><span>{hm(x.ends_at)}</span></span>
                        <span className="tbar" style={{ ["--c" as string]: toneFor(ths, x.therapist_id) }} aria-hidden="true" />
                        <span className="grow">
                          <span className="t" style={{ display: "flex", gap: 6, alignItems: "center" }}>{isClass ? <Users size={14} /> : x.kind === "home" ? <Home size={14} /> : null}{title(x)}</span>
                          <span className="s" style={{ display: "block" }}>
                            {x.therapist_name} · {isClass ? L(`${x.attendees?.length ?? 0} de ${x.capacity} inscritos`, `${x.attendees?.length ?? 0} of ${x.capacity} booked`) : kindLabel(x.kind, lang)}
                          </span>
                        </span>
                        <span className="end">
                          {st === "now" ? <span className="now-pill">{L("Agora", "Now")}</span>
                            : st === "done" ? (pay(x) ?? <span className="done-mark" title={statusLabel("done", lang)} aria-label={statusLabel("done", lang)}><Check size={15} /></span>)
                              : st === "no_show" ? <Badge tone="warn">{statusLabel("no_show", lang)}</Badge>
                                : st === "past" ? <Badge>{L("Por fechar", "Not marked")}</Badge> : null}
                          <button className="btn icon sm more" aria-label={L("Mais ações", "More actions")} aria-haspopup="menu" onClick={(e) => openMenu(app, e, apptItems(app, x))}><MoreHorizontal /></button>
                        </span>
                      </div>
                    );
                  })}
                  {agShown.length === sessions.length && tomorrow.length > 0 && agFit - sessions.length >= 3 && (() => {
                    const room = agFit - sessions.length - 1;
                    const tm = tomorrow.length > room ? tomorrow.slice(0, Math.max(1, room - 1)) : tomorrow;
                    return <>
                      <p className="agenda-next">{L(`Amanhã · ${n(tomorrow.length, "marcação", "marcações")}`, `Tomorrow · ${n(tomorrow.length, "appointment", "appointments")}`)}</p>
                      {tm.map((x) => (
                        <div key={x.id} role="button" tabIndex={0} className="row is-tomorrow" onClick={(e) => open(x, e)} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(x, e); } }} onContextMenu={(e) => openMenu(app, e, apptItems(app, x))}>
                          <span className="time"><b>{hm(x.starts_at)}</b><span>{hm(x.ends_at)}</span></span>
                          <span className="tbar" style={{ ["--c" as string]: toneFor(ths, x.therapist_id) }} aria-hidden="true" />
                          <span className="grow"><span className="t" style={{ display: "block" }}>{title(x)}</span><span className="s" style={{ display: "block" }}>{x.therapist_name} · {kindLabel(x.kind, lang)}</span></span>
                        </div>
                      ))}
                      {tm.length < tomorrow.length && <button className="row more-row" onClick={() => go("schedule")}><span className="grow">{L(`Mais ${tomorrow.length - tm.length} amanhã`, `${tomorrow.length - tm.length} more tomorrow`)}</span><span className="linkish">{L("Ver agenda", "See Schedule")}</span><ChevronRight size={14} className="chev" /></button>}
                    </>;
                  })()}
                  {agShown.length < sessions.length && (
                    <button className="row more-row" onClick={() => go("schedule")}><span className="grow">{L(`Mais ${sessions.length - agShown.length} hoje`, `${sessions.length - agShown.length} more today`)}</span><span className="linkish">{L("Ver agenda", "See Schedule")}</span><ChevronRight size={14} className="chev" /></button>
                  )}
                </div>
              )}
            </div>
          </section>

          <section className="tile attention-tile" aria-label={L("A tratar", "Needs attention")}>
            <div className="tile-head">
              <div><h2>{L("A tratar", "Needs attention")}</h2><p className="tile-sub">{alerts ? (rows.length ? L(n(rows.length, "assunto", "assuntos"), n(rows.length, "item", "items")) : L("Nada a tratar", "Nothing to do")) : "…"}</p></div>
              {rows.length > 0 && <button className="btn sm" onClick={allAttention}>{L("Ver tudo", "See All")}<ChevronRight size={14} /></button>}
            </div>
            <div className="tile-body" ref={atRef}>
              {!alerts ? <div className="skel" /> : rows.length === 0 ? (
                <p className="small muted" style={{ display: "flex", gap: 6, alignItems: "center", padding: "4px 2px" }}><CircleCheck size={14} style={{ color: "var(--ok)" }} />{L("Está tudo em dia.", "All up to date.")}</p>
              ) : (
                <div className="rows">
                  {atShown.map((a, i) => (
                    <button key={i} className="row" onClick={a.act}><span className="ic" style={{ color: a.tone === "var(--bad)" ? a.tone : undefined }}>{a.ic}</span><span className="grow ell">{a.t}</span><ChevronRight size={14} className="chev" /></button>
                  ))}
                  {atShown.length < rows.length && (
                    <button className="row more-row" onClick={allAttention}><span className="grow">{L(`Mais ${rows.length - atShown.length}`, `${rows.length - atShown.length} more`)}</span><span className="linkish">{L("Ver tudo", "See All")}</span><ChevronRight size={14} className="chev" /></button>
                  )}
                </div>
              )}
            </div>
          </section>
        </div>
      </div>
    </Layout>
  );
}

/** How many rows of a fixed height fit in a box right now (follows the window size). */
function useFit(rowH: number) {
  const ref = useRef<HTMLDivElement>(null);
  const [n, setN] = useState(6);
  useLayoutEffect(() => {
    const el = ref.current; if (!el) return;
    const f = () => setN(Math.max(2, Math.floor((el.clientHeight + 1) / rowH)));
    f();
    const ro = new ResizeObserver(f); ro.observe(el);
    return () => ro.disconnect();
  }, [rowH]);
  return [ref, n] as const;
}

/** A figure in the summary tile: the number and what it counts. */
function Fact({ label, value, note, onClick }: { label: string; value: string; note?: string; onClick?: () => void }) {
  const inner = <><span className="l">{label}</span><span className="v">{value}</span>{note && <span className="d">{note}</span>}</>;
  return onClick ? <button className="fact" onClick={onClick}>{inner}</button> : <div className="fact">{inner}</div>;
}
