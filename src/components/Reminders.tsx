// Appointment reminders sent by hand (owner decision 7 Nov 2026: no SMS provider for now).
// The app fills in the clinic's message and opens Messages (SMS), WhatsApp or email on this computer with it
// ready; the person presses Send there. The server only records that it went out — never the text.
// Nothing clinical ever goes in a reminder.
import { useState } from "react";
import { Check, ChevronLeft, ChevronRight, Copy, Mail, MessageCircle, MessageSquare, MoreHorizontal, Phone, BellRing } from "lucide-react";
import { useApp, type AppCtx } from "../lib/ctx";
import { api, isErr, openMessage } from "../lib/api";
import { useApi, addDays, fmtDate, hm, ymd, sameDay, kindLabel } from "../lib/data";
import { isMac } from "../lib/platform";
import { openMenu } from "./menus";
import { Badge, Empty, Head } from "./ui";

/** Set before going to the Schedule to open its Reminders section (Today's "to send" row). */
export const remindersNav = { open: false };

export type Channel = "sms" | "whatsapp" | "email" | "call" | "other";
export interface ReminderItem {
  appointment_id: string; patient_id: string; name: string; number: number; phone: string | null; email: string | null;
  lang: "pt" | "en"; prefers: "sms" | "email" | "none"; kind: string; title: string; starts_at: string; therapist_name: string | null; home: boolean;
  sent_at: string | null; sent_channel: Channel | null; sent_by_name: string | null;
}
export interface ReminderDay { day: string; clinic: string; template_pt: string; template_en: string; items: ReminderItem[] }

/** International number for links: Portuguese numbers without a code get +351. Null when there's no usable number. */
export function intlPhone(raw: string | null): string | null {
  if (!raw) return null;
  let d = raw.replace(/[^\d+]/g, "");
  if (d.startsWith("00")) d = "+" + d.slice(2);
  const digits = d.replace(/\D/g, "");
  if (!d.startsWith("+") && digits.length === 9 && /^[29]/.test(digits)) return "351" + digits;
  return digits.length >= 8 ? digits : null;
}

/** "amanhã" / "hoje" / "sexta, 8 nov." in the patient's language. */
function dayWords(iso: string, l: "pt" | "en") {
  const d = new Date(iso), today = new Date();
  if (sameDay(d, today)) return l === "pt" ? "hoje" : "today";
  if (sameDay(d, addDays(today, 1))) return l === "pt" ? "amanhã" : "tomorrow";
  return (l === "pt" ? "no dia " : "on ") + fmtDate(d, l, { weekday: "long", day: "numeric", month: "long" });
}

/** The message for one person, in their language. */
export function reminderText(day: ReminderDay, it: ReminderItem): string {
  const l = it.lang === "en" ? "en" : "pt";
  const first = it.name.trim().split(/\s+/)[0] ?? it.name;
  const vals: Record<string, string> = {
    nome: first, name: first, dia: dayWords(it.starts_at, l), day: dayWords(it.starts_at, l), hora: hm(it.starts_at), time: hm(it.starts_at),
    clinica: day.clinic, clinic: day.clinic, terapeuta: it.therapist_name ?? "", therapist: it.therapist_name ?? "",
  };
  return (l === "en" ? day.template_en : day.template_pt).replace(/\{(\w+)\}/g, (m, k: string) => vals[k.toLowerCase()] ?? m);
}

/** The link that opens the right app with the message filled in. */
export function messageLink(ch: "sms" | "whatsapp" | "email", it: ReminderItem, text: string, clinic: string): string | null {
  const body = encodeURIComponent(text);
  if (ch === "email") return it.email ? `mailto:${it.email}?subject=${encodeURIComponent(it.lang === "en" ? `Reminder — ${clinic}` : `Lembrete — ${clinic}`)}&body=${body}` : null;
  const p = intlPhone(it.phone);
  if (!p) return null;
  // Messages on the Mac takes "&body=", Windows (Phone Link) the standard "?body=".
  if (ch === "sms") return `sms:+${p}${isMac ? "&" : "?"}body=${body}`;
  return `https://wa.me/${p}?text=${body}`;
}

const CH: Record<Channel, [string, string]> = { sms: ["SMS", "SMS"], whatsapp: ["WhatsApp", "WhatsApp"], email: ["Email", "Email"], call: ["Chamada", "Call"], other: ["Outro", "Other"] };

/** Marks a reminder as sent (or not), with Undo. */
function useMark() {
  const { L, toast } = useApp();
  return (it: ReminderItem, ch: Channel | null, quiet = false) =>
    api("POST", "/reminders/sent", { appointment_id: it.appointment_id, patient_id: it.patient_id, channel: ch ?? undefined, sent: !!ch })
      .then(() => {
        if (quiet) return;
        if (ch) toast(L(`Lembrete de ${it.name} marcado como enviado`, `Reminder to ${it.name} marked as sent`),
          () => { api("POST", "/reminders/sent", { appointment_id: it.appointment_id, patient_id: it.patient_id, sent: false }).catch(() => {}); }, L("Marcar como enviado", "Mark as Sent"));
        else toast(L("Marcado como por enviar", "Marked as not sent"));
      })
      .catch((e) => toast(isErr(e) ? e.message : String(e)));
}

function send(ctx: AppCtx, mark: ReturnType<typeof useMark>, day: ReminderDay, it: ReminderItem, ch: "sms" | "whatsapp" | "email") {
  const url = messageLink(ch, it, reminderText(day, it), day.clinic);
  if (!url) return ctx.toast(ch === "email" ? ctx.L("Este utente não tem email.", "This patient has no email.") : ctx.L("Este utente não tem um número de telemóvel válido.", "This patient has no valid mobile number."));
  openMessage(url).then(() => mark(it, ch)).catch((e) => ctx.toast(isErr(e) ? e.message : String(e)));
}

function copyText(ctx: AppCtx, text: string) {
  navigator.clipboard?.writeText(text).then(() => ctx.toast(ctx.L("Mensagem copiada", "Message copied"))).catch(() => ctx.toast(ctx.L("Não foi possível copiar.", "Couldn’t copy.")));
}

/** One person to remind. The main button uses their preferred channel; ⋯ has the others. */
function ReminderRow({ day, it }: { day: ReminderDay; it: ReminderItem }) {
  const ctx = useApp();
  const { L, lang, openPatient } = ctx;
  const mark = useMark();
  const hasPhone = !!intlPhone(it.phone);
  const main: "sms" | "whatsapp" | "email" | null = it.prefers === "email" && it.email ? "email" : hasPhone ? "whatsapp" : it.email ? "email" : null;
  const what = it.kind === "class" ? (it.title || kindLabel("class", lang)) : kindLabel(it.kind as never, lang);
  const menu = (e: React.MouseEvent) => openMenu(ctx, e, [
    ...(hasPhone ? [{ label: L("Enviar por WhatsApp", "Send by WhatsApp"), icon: <MessageCircle />, fn: () => send(ctx, mark, day, it, "whatsapp") },
                    { label: L("Enviar por SMS", "Send by SMS"), icon: <MessageSquare />, fn: () => send(ctx, mark, day, it, "sms") }] : []),
    ...(it.email ? [{ label: L("Enviar por email", "Send by Email"), icon: <Mail />, fn: () => send(ctx, mark, day, it, "email") }] : []),
    { label: L("Copiar mensagem", "Copy Message"), icon: <Copy />, fn: () => copyText(ctx, reminderText(day, it)) },
    "-" as const,
    ...(it.sent_at ? [{ label: L("Marcar como por enviar", "Mark as Not Sent"), fn: () => mark(it, null) }]
      : [{ label: L("Avisado por chamada", "Reminded by Phone Call"), icon: <Phone />, fn: () => mark(it, "call") },
         { label: L("Marcar como enviado", "Mark as Sent"), icon: <Check />, fn: () => mark(it, "other") }]),
    { label: L("Abrir ficha", "Open File"), fn: () => openPatient(it.patient_id) },
  ]);
  const contact = [it.phone, it.prefers === "email" ? it.email : null].filter(Boolean).join(" · ");
  return (
    <div className="row" onContextMenu={menu}>
      <span className="time">{hm(it.starts_at)}</span>
      <span className="grow" style={{ minWidth: 0 }}>
        <span className="t ell" style={{ display: "block" }}>{it.name}</span>
        <span className="s ell" style={{ display: "block" }}>
          {what}{it.therapist_name ? ` · ${it.therapist_name}` : ""}{contact ? ` · ${contact}` : ` · ${L("sem contacto", "no contact")}`}
        </span>
      </span>
      {it.prefers === "none" && !it.sent_at && <Badge>{L("Não quer lembretes", "No reminders")}</Badge>}
      {it.sent_at
        ? <span className="small muted" style={{ display: "flex", alignItems: "center", gap: 6, whiteSpace: "nowrap" }}><Check size={14} style={{ color: "var(--ok)" }} aria-hidden="true" />
            {L(`${CH[it.sent_channel ?? "other"][0]}, ${hm(it.sent_at)}`, `${CH[it.sent_channel ?? "other"][1]}, ${hm(it.sent_at)}`)}</span>
        : it.prefers !== "none" && main && (
          <button className="btn sm" onClick={() => send(ctx, mark, day, it, main)}>
            {main === "email" ? <Mail /> : <MessageCircle />}{main === "email" ? L("Email", "Email") : "WhatsApp"}
          </button>
        )}
      <button className="btn icon sm more" aria-label={L(`Mais opções para ${it.name}`, `More options for ${it.name}`)} onClick={menu}><MoreHorizontal /></button>
    </div>
  );
}

/** Schedule › Reminders: one day's list (default tomorrow), with day navigation. */
export function RemindersPanel() {
  const { L, lang } = useApp();
  const [day, setDay] = useState(() => addDays(new Date(new Date().setHours(0, 0, 0, 0)), 1));
  const { data, error } = useApi<ReminderDay>(`/reminders?day=${ymd(day)}`);
  const items = data?.items ?? [];
  const todo = items.filter((i) => !i.sent_at && i.prefers !== "none").length;
  const label = sameDay(day, addDays(new Date(), 1)) ? L("Amanhã", "Tomorrow") : sameDay(day, new Date()) ? L("Hoje", "Today") : null;
  const title = `${label ? `${label}, ` : ""}${fmtDate(day, lang, { weekday: label ? undefined : "long", day: "numeric", month: "long" })}`.replace(/^./, (c) => c.toUpperCase());
  return (
    <div className="wrap">
      <Head title={L("Lembretes", "Reminders")} sub={error ? <span className="err">{error}</span> : !data ? L("A carregar…", "Loading…")
        : items.length === 0 ? title : todo === 0 ? L(`${title} — todos enviados`, `${title} — all sent`) : L(`${title} — ${todo} por enviar`, `${title} — ${todo} to send`)}>
        <button className="btn" aria-label={L("Dia anterior", "Previous day")} onClick={() => setDay(addDays(day, -1))}><ChevronLeft /></button>
        <button className="btn" onClick={() => setDay(addDays(new Date(new Date().setHours(0, 0, 0, 0)), 1))}>{L("Amanhã", "Tomorrow")}</button>
        <button className="btn" aria-label={L("Dia seguinte", "Next day")} onClick={() => setDay(addDays(day, 1))}><ChevronRight /></button>
      </Head>
      {!data ? <div className="skel" style={{ height: 160 }} /> : items.length === 0
        ? <Empty icon={<BellRing />} title={L("Sem marcações neste dia", "No appointments on this day")} text={L("Os lembretes aparecem aqui para cada sessão marcada.", "Reminders appear here for every booked session.")} />
        : <div className="rows">{items.map((it) => <ReminderRow key={`${it.appointment_id}-${it.patient_id}`} day={data} it={it} />)}</div>}
      <p className="small muted" style={{ marginTop: 12, maxWidth: 640 }}>{L(
        "O botão abre o WhatsApp, as Mensagens ou o email deste computador com a mensagem pronta; só falta carregar em Enviar. A mensagem nunca tem informação clínica. O envio automático por SMS chega mais tarde.",
        "The button opens WhatsApp, Messages or email on this computer with the message ready; you just press Send. The message never contains clinical information. Automatic SMS sending comes later.")}</p>
    </div>
  );
}

/** Owner, Settings › Reminders: the message in both languages, with a live example. */
export function ReminderTemplate() {
  const { L, toast } = useApp();
  const { data } = useApi<ReminderDay>("/reminders");
  const [pt, setPt] = useState<string | null>(null);
  const [en, setEn] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string>();
  if (!data) return <div className="skel" style={{ height: 120 }} />;
  const vPt = pt ?? data.template_pt, vEn = en ?? data.template_en;
  const changed = vPt !== data.template_pt || vEn !== data.template_en;
  const sample = (l: "pt" | "en", tpl: string): string => reminderText({ ...data, template_pt: tpl, template_en: tpl }, {
    appointment_id: "", patient_id: "", name: l === "pt" ? "Maria Santos" : "John Smith", number: 1, phone: null, email: null, lang: l, prefers: "sms", kind: "session", title: "",
    starts_at: (() => { const d = addDays(new Date(), 1); d.setHours(9, 30, 0, 0); return d.toISOString(); })(), therapist_name: "Ana", home: false, sent_at: null, sent_channel: null, sent_by_name: null,
  });
  const save = async () => {
    setBusy(true); setErr(undefined);
    try { await api("PUT", "/reminders/template", { pt: vPt, en: vEn }); setPt(null); setEn(null); toast(L("Mensagem guardada", "Message saved")); }
    catch (e) { setErr(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  return (
    <>
      <div className="group-title">{L("Mensagem em português", "Message in Portuguese")}</div>
      <textarea className="input" style={{ minHeight: 76 }} value={vPt} onChange={(e) => setPt(e.target.value)} aria-label={L("Mensagem em português", "Message in Portuguese")} />
      <p className="small muted" style={{ margin: "6px 0 14px" }}>{L("Exemplo: ", "Example: ")}{sample("pt", vPt)}</p>
      <div className="group-title">{L("Mensagem em inglês", "Message in English")}</div>
      <textarea className="input" style={{ minHeight: 76 }} value={vEn} onChange={(e) => setEn(e.target.value)} aria-label={L("Mensagem em inglês", "Message in English")} />
      <p className="small muted" style={{ margin: "6px 0 10px" }}>{L("Exemplo: ", "Example: ")}{sample("en", vEn)}</p>
      <p className="small muted">{L("Pode usar {nome}, {dia}, {hora}, {clinica} e {terapeuta} (em inglês {name}, {day}, {time}, {clinic}, {therapist}). Não escreva nada clínico.", "You can use {nome}, {dia}, {hora}, {clinica} and {terapeuta} (in English {name}, {day}, {time}, {clinic}, {therapist}). Don’t write anything clinical.")}</p>
      {err && <div className="err" role="alert">{err}</div>}
      <div style={{ display: "flex", gap: 8, marginTop: 12 }}>
        <button className="btn" disabled={!changed || busy} onClick={() => { setPt(null); setEn(null); }}>{L("Repor", "Revert")}</button>
        <button className="btn pri" disabled={!changed || busy || !vPt.trim() || !vEn.trim()} onClick={save}>{L("Guardar", "Save")}</button>
      </div>
    </>
  );
}
