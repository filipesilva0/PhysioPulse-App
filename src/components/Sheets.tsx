import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, Paperclip, Sparkles } from "lucide-react";
import { useApp } from "../lib/ctx";
import { api, isErr } from "../lib/api";
import { fileToBase64 } from "../lib/money";
import {
  useApi, ymd, hm, pad, fmtDate, addDays, age, weekdayShort, BLOCK_LABELS, TEST_LIBRARY, testName, kindLabel, DOC_ACCEPT,
  type Appointment, type ApptKind, type BookingRequest, type Overlap, type PatientRec, type PatientRow, type Prescription, type Therapist,
} from "../lib/data";
import { Field, Seg, Sheet, Badge, DateInput, TimeInput } from "./ui";
import { PatientPicker } from "./PatientPicker";
import { emailOk, patientNifOk, phoneOk } from "../lib/validate";

/** GET /patients/duplicates */
interface Dup { id: string; number: number; name: string; birth_date: string | null; phone: string | null; reason: "nif" | "phone" | "name_birth" }

/** Shows the clashes the server reported, so the person can choose "Book Anyway". */
function Clashes({ list }: { list: Overlap[] }) {
  const { L, lang } = useApp();
  return (
    <div className="card pad" role="alert" style={{ display: "flex", gap: 10, marginTop: 4, background: "color-mix(in srgb, var(--warn) 9%, var(--raise))" }}>
      <span style={{ color: "var(--warn)", display: "flex" }}><AlertTriangle size={16} /></span>
      <div className="small">
        <b>{L("Sobrepõe-se a outra marcação do mesmo fisioterapeuta.", "This overlaps another appointment for the same therapist.")}</b>
        {list.slice(0, 3).map((o, i) => <div key={i} className="muted">{fmtDate(o.starts_at, lang, { weekday: "short", day: "numeric", month: "short" })}, {hm(o.starts_at)}–{hm(o.ends_at)}{o.name ? ` — ${o.name}` : ""}</div>)}
        {list.length > 3 && <div className="muted">+{list.length - 3}</div>}
        <div style={{ marginTop: 4 }}>{L("Pode marcar mesmo assim.", "You can book it anyway.")}</div>
      </div>
    </div>
  );
}
const overlapOf = (e: unknown) => (isErr(e) && e.code === "overlap" ? (e.details as Overlap[]) ?? [] : null);
const iso = (date: string, time: string) => new Date(`${date}T${time}`).toISOString();

/** Book a session, assessment, class, home visit or blocked time; optionally a weekly series.
 *  `request` turns it into accepting an online booking request. */
export function NewAppointment({ patientId, prescriptions, date, request, therapistId }: { patientId?: string; prescriptions?: Prescription[]; date?: Date; request?: BookingRequest;
  /** Booked by clicking a therapist's column in the Schedule: keep that therapist even after choosing the patient. */
  therapistId?: string }) {
  const { L, lang, toast, me, role } = useApp();
  const { data: ths } = useApi<Therapist[]>("/therapists");
  const [prow, setProw] = useState<PatientRow | null>(null);
  const start = request ? new Date(request.preferred_at) : date ?? (() => { const d = new Date(); d.setHours(d.getHours() + 1, 0, 0, 0); return d; })();
  const [kind, setKind] = useState<ApptKind>(request?.service === "assessment" ? "assessment" : request?.service === "home" ? "home" : "session");
  const [pid, setPid] = useState(patientId ?? request?.patient_id ?? "");
  const [th, setTh] = useState(therapistId ?? "");
  const [day, setDay] = useState(ymd(start));
  const [time, setTime] = useState(`${pad(start.getHours())}:${pad(start.getMinutes())}`);
  const [dur, setDur] = useState(request?.service === "assessment" || request?.service === "home" ? 60 : 45);
  const [rx, setRx] = useState(prescriptions?.find((x) => x.remaining > 0)?.id ?? "");
  const [title, setTitle] = useState(L("Pilates clínico", "Clinical Pilates"));
  const [cap, setCap] = useState(6);
  const [address, setAddress] = useState("");
  const [travel, setTravel] = useState(15);
  const [block, setBlock] = useState("holiday");
  const [repeat, setRepeat] = useState<"none" | "weekly">("none");
  const [wdays, setWdays] = useState<number[]>([((start.getDay() + 6) % 7) + 1]);
  const [endBy, setEndBy] = useState<"count" | "until">("count");
  const [count, setCount] = useState(10);
  const [until, setUntil] = useState(ymd(addDays(start, 56)));
  const [clash, setClash] = useState<Overlap[] | null>(null);

  // Default therapist: the patient's own, else the person booking (if they treat), else the first.
  useEffect(() => {
    if (th || !ths?.length) return;
    setTh(prow?.therapist_id ?? (role !== "reception" ? ths.find((t) => t.id === me.id)?.id : undefined) ?? ths[0].id);
  }, [ths, prow, pid]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => setClash(null), [kind, pid, th, day, time, dur, repeat, wdays, count, until]);
  useEffect(() => { if (kind === "home" && dur < 60) setDur(60); }, [kind]); // eslint-disable-line react-hooks/exhaustive-deps

  const needsPatient = kind === "session" || kind === "assessment" || kind === "home";
  const ready = (!needsPatient || !!pid) && (kind === "block" || !!th) && !!day && !!time && (kind !== "class" || cap > 0) && (kind !== "home" || !!address.trim())
    && (repeat === "none" || (wdays.length > 0 && (endBy === "count" ? count > 0 : !!until)));
  const submit = async () => {
    const s = new Date(`${day}T${time}`), e = new Date(s.getTime() + dur * 60000);
    const body = {
      kind, starts_at: s.toISOString(), ends_at: e.toISOString(), therapist_id: th || null, patient_id: needsPatient ? pid : null,
      prescription_id: needsPatient && rx ? rx : null, title: kind === "class" ? title.trim() : kind === "block" ? block : "",
      color: kind === "block" ? BLOCK_LABELS.find((b) => b[0] === block)?.[2] ?? "" : "", address: kind === "home" ? address.trim() : "",
      travel_minutes: kind === "home" ? travel : 0, capacity: kind === "class" ? cap : null, allow_overlap: !!clash,
      repeat: repeat === "weekly" && !request ? { weekdays: wdays, ...(endBy === "count" ? { count } : { until }) } : undefined,
    };
    try {
      if (request) {
        await api("POST", `/booking-requests/${request.id}/accept`, body);
        toast(L("Pedido aceite e marcado", "Request accepted and booked"), () => { api("POST", `/booking-requests/${request.id}/reopen`).catch((x) => toast(isErr(x) ? x.message : String(x))); }, L("Aceitar pedido", "Accept Request"));
      } else {
        const r = await api<{ ids: string[] }>("POST", "/appointments", body);
        toast(r.ids.length > 1 ? L(`${r.ids.length} marcações criadas`, `${r.ids.length} appointments booked`) : L("Marcação criada", "Appointment booked"));
      }
    } catch (x) {
      const o = overlapOf(x);
      if (o) { setClash(o); return false; }
      throw x;
    }
  };
  const days = weekdayShort(lang);
  return (
    <Sheet title={request ? L("Aceitar pedido", "Accept Request") : L("Nova marcação", "New Appointment")} sub={request ? `${request.name}${request.message ? ` — ${request.message}` : ""}` : undefined}
      action={clash ? L("Marcar mesmo assim", "Book Anyway") : request ? L("Aceitar e marcar", "Accept and Book") : L("Marcar", "Book")} onAction={submit} disabled={!ready}>
      {!request && (
        <div style={{ marginBottom: 14, overflowX: "auto" }}>
          <Seg label={L("Tipo", "Type")} value={kind} onChange={setKind}
            options={[["session", L("Sessão", "Session")], ["assessment", L("Avaliação", "Assessment")], ["class", L("Aula", "Class")], ["home", L("Domicílio", "Home Visit")], ["block", L("Bloquear", "Block Time")]]} />
        </div>
      )}
      {kind === "block" ? (
        <Field label={L("Motivo", "Reason")}>
          <select className="input" value={block} onChange={(e) => setBlock(e.target.value)}>{BLOCK_LABELS.map(([k, t]) => <option key={k} value={k}>{L(t[0], t[1])}</option>)}</select>
        </Field>
      ) : kind === "class" ? (
        <div className="grid2">
          <Field label={L("Aula", "Class")}><input className="input" value={title} onChange={(e) => setTitle(e.target.value)} /></Field>
          <Field label={L("Lotação", "Capacity")}><input className="input" type="number" min={1} value={cap} onChange={(e) => setCap(+e.target.value)} /></Field>
        </div>
      ) : (
        <Field label={L("Utente", "Patient")}>
          <PatientPicker value={pid} label={L("Utente", "Patient")} autoFocus={!pid}
            onChange={(p) => { setPid(p?.id ?? ""); setProw(p); if (p?.therapist_id && !therapistId) setTh(p.therapist_id); }} />
        </Field>
      )}
      {kind === "home" && (
        <div className="grid2">
          <Field label={L("Morada", "Address")}><input className="input" value={address} onChange={(e) => setAddress(e.target.value)} placeholder={L("Rua, n.º, localidade", "Street, number, town")} /></Field>
          <Field label={L("Deslocação (min, cada sentido)", "Travel (min, each way)")}><input className="input" type="number" min={0} max={240} step={5} value={travel} onChange={(e) => setTravel(+e.target.value)} /></Field>
        </div>
      )}
      <div className="grid3">
        <Field label={L("Data", "Date")}><DateInput className="input" value={day} onChange={(e) => setDay(e.target.value)} /></Field>
        <Field label={L("Hora", "Time")}><TimeInput className="input" value={time} step={900} onChange={(e) => setTime(e.target.value)} /></Field>
        <Field label={L("Duração", "Duration")}>
          <select className="input" value={dur} onChange={(e) => setDur(+e.target.value)}>
            {(kind === "block" ? [30, 60, 120, 180, 240, 480] : [30, 45, 60, 90]).map((m) => <option key={m} value={m}>{m < 120 ? `${m} min` : `${m / 60} h`}</option>)}
          </select>
        </Field>
      </div>
      <div className="grid2">
        <Field label={L("Fisioterapeuta", "Therapist")}>
          <select className="input" value={th} onChange={(e) => setTh(e.target.value)}>
            {kind === "block" && <option value="">{L("Toda a clínica", "Whole clinic")}</option>}
            {ths?.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
          </select>
        </Field>
        {needsPatient && prescriptions && prescriptions.length > 0 && (
          <Field label={L("Prescrição", "Prescription")}>
            <select className="input" value={rx} onChange={(e) => setRx(e.target.value)}>
              <option value="">{L("Nenhuma", "None")}</option>
              {prescriptions.map((x) => <option key={x.id} value={x.id}>{x.payer || x.doctor} — {L(`restam ${x.remaining}`, `${x.remaining} left`)}</option>)}
            </select>
          </Field>
        )}
      </div>
      {!request && (
        <Field label={L("Repetir", "Repeat")}>
          <select className="input" value={repeat} onChange={(e) => setRepeat(e.target.value as "none" | "weekly")}>
            <option value="none">{L("Não repetir", "Does not repeat")}</option>
            <option value="weekly">{L("Todas as semanas, nos dias…", "Every week, on…")}</option>
          </select>
        </Field>
      )}
      {repeat === "weekly" && !request && (
        <>
          <div className="chips" role="group" aria-label={L("Dias da semana", "Days of the week")} style={{ marginBottom: 12 }}>
            {days.map((d, i) => <button key={d} className="chip" aria-pressed={wdays.includes(i + 1)} onClick={() => setWdays(wdays.includes(i + 1) ? wdays.filter((x) => x !== i + 1) : [...wdays, i + 1].sort())}>{d}</button>)}
          </div>
          <div className="grid2">
            <label className="check"><input type="radio" name="endby" checked={endBy === "count"} onChange={() => setEndBy("count")} />
              <input className="input" type="number" min={1} max={200} value={count} onChange={(e) => { setEndBy("count"); setCount(+e.target.value); }} style={{ width: 70 }} aria-label={L("Número de sessões", "Number of sessions")} />{L("sessões", "sessions")}</label>
            <label className="check"><input type="radio" name="endby" checked={endBy === "until"} onChange={() => setEndBy("until")} />{L("até", "until")}
              <DateInput className="input" value={until} onChange={(e) => { setEndBy("until"); setUntil(e.target.value); }} aria-label={L("Até", "Until")} /></label>
          </div>
        </>
      )}
      {clash && <Clashes list={clash} />}
    </Sheet>
  );
}

/** Move an appointment (or this and the following ones of a series), change its therapist or length. */
export function EditAppointment({ appt }: { appt: Appointment }) {
  const { L, toast } = useApp();
  const { data: ths } = useApi<Therapist[]>("/therapists");
  const s0 = new Date(appt.starts_at);
  const [day, setDay] = useState(ymd(s0));
  const [time, setTime] = useState(hm(appt.starts_at));
  const [dur, setDur] = useState(Math.round((new Date(appt.ends_at).getTime() - s0.getTime()) / 60000));
  const [th, setTh] = useState(appt.therapist_id ?? "");
  const [scope, setScope] = useState<"one" | "following" | "all">("one");
  const [clash, setClash] = useState<Overlap[] | null>(null);
  useEffect(() => setClash(null), [day, time, dur, th, scope]);
  const submit = async () => {
    const s = new Date(`${day}T${time}`);
    const before = { starts_at: appt.starts_at, ends_at: appt.ends_at, therapist_id: appt.therapist_id };
    try {
      await api("PATCH", `/appointments/${appt.id}`, { scope, starts_at: s.toISOString(), ends_at: new Date(s.getTime() + dur * 60000).toISOString(), therapist_id: th || null, allow_overlap: !!clash });
      toast(L("Marcação alterada", "Appointment changed"), () => {
        api("PATCH", `/appointments/${appt.id}`, { scope, ...before, allow_overlap: true }).catch((x) => toast(isErr(x) ? x.message : String(x)));
      }, L("Remarcar", "Reschedule"));
    } catch (x) {
      const o = overlapOf(x);
      if (o) { setClash(o); return false; }
      throw x;
    }
  };
  const mins = [30, 45, 60, 90, 120, 180, 240, 480];
  return (
    <Sheet title={L("Remarcar", "Reschedule")} sub={`${kindLabel(appt.kind, useApp().lang)}${appt.patient_name ? ` — ${appt.patient_name}` : appt.title ? ` — ${appt.title}` : ""}`}
      action={clash ? L("Marcar mesmo assim", "Book Anyway") : L("Guardar", "Save")} onAction={submit} disabled={!day || !time}>
      <div className="grid3">
        <Field label={L("Data", "Date")}><DateInput className="input" value={day} onChange={(e) => setDay(e.target.value)} /></Field>
        <Field label={L("Hora", "Time")}><TimeInput className="input" step={900} value={time} onChange={(e) => setTime(e.target.value)} /></Field>
        <Field label={L("Duração", "Duration")}>
          <select className="input" value={dur} onChange={(e) => setDur(+e.target.value)}>{(mins.includes(dur) ? mins : [dur, ...mins]).map((m) => <option key={m} value={m}>{m < 120 ? `${m} min` : `${m / 60} h`}</option>)}</select>
        </Field>
      </div>
      <Field label={L("Fisioterapeuta", "Therapist")}>
        <select className="input" value={th} onChange={(e) => setTh(e.target.value)}>
          {appt.kind === "block" && <option value="">{L("Toda a clínica", "Whole clinic")}</option>}
          {ths?.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
        </select>
      </Field>
      {appt.series_id && <SeriesScope value={scope} onChange={setScope} />}
      {clash && <Clashes list={clash} />}
    </Sheet>
  );
}

function SeriesScope({ value, onChange }: { value: "one" | "following" | "all"; onChange: (v: "one" | "following" | "all") => void }) {
  const { L } = useApp();
  return (
    <Field label={L("Esta marcação repete-se", "This appointment repeats")}>
      <div className="stack">
        {([["one", L("Só esta", "This one only")], ["following", L("Esta e as seguintes", "This and the following ones")], ["all", L("Todas da série", "All in the series")]] as const).map(([v, t]) => (
          <label key={v} className="check"><input type="radio" name="scope" checked={value === v} onChange={() => onChange(v)} />{t}</label>
        ))}
      </div>
    </Field>
  );
}

/** Cancel one appointment, or this and the following ones of a series. Offers the freed slot to the waitlist. */
export function CancelAppointment({ appt }: { appt: Appointment }) {
  const { L, lang, toast } = useApp();
  const [scope, setScope] = useState<"one" | "following" | "all">("one");
  const submit = async () => {
    const r = await api<{ ids: string[]; waitlist_matches: { name: string; phone: string | null }[] }>("PATCH", `/appointments/${appt.id}`, { scope, status: "cancelled" });
    const undo = () => { Promise.all(r.ids.map((id) => api("PATCH", `/appointments/${id}`, { scope: "one", status: "booked", allow_overlap: true }))).catch((x) => toast(isErr(x) ? x.message : String(x))); };
    const w = r.waitlist_matches ?? [];
    toast(w.length
      ? L(`Cancelada. Na lista de espera: ${w[0].name}${w[0].phone ? ` (${w[0].phone})` : ""}${w.length > 1 ? ` e mais ${w.length - 1}` : ""}`, `Cancelled. On the waitlist: ${w[0].name}${w[0].phone ? ` (${w[0].phone})` : ""}${w.length > 1 ? ` and ${w.length - 1} more` : ""}`)
      : r.ids.length > 1 ? L(`${r.ids.length} marcações canceladas`, `${r.ids.length} appointments cancelled`) : L("Marcação cancelada", "Appointment cancelled"), undo, L("Cancelar marcação", "Cancel Appointment"));
  };
  const block = appt.kind === "block";
  return (
    <Sheet title={block ? L("Remover bloqueio?", "Remove Blocked Time?") : L("Cancelar marcação?", "Cancel Appointment?")}
      sub={`${fmtDate(appt.starts_at, lang, { weekday: "long", day: "numeric", month: "long" })}, ${hm(appt.starts_at)}${appt.patient_name ? ` — ${appt.patient_name}` : ""}`}
      action={block ? L("Remover", "Remove") : L("Cancelar marcação", "Cancel Appointment")} destructive hold={block} onAction={submit}>
      {appt.series_id && <SeriesScope value={scope} onChange={setScope} />}
      {!block && <p className="small muted">{L("Se alguém na lista de espera puder vir a esta hora, aparece a seguir. O envio automático de SMS chega quando o fornecedor estiver escolhido.", "If someone on the waitlist fits this time, you’ll see them next. Automatic SMS offers arrive once the SMS provider is chosen.")}</p>}
    </Sheet>
  );
}

/** Register a patient (everyone), or change their details (pass `patient`). */
export function PatientDetails({ patient, onCreated }: { patient?: PatientRec; onCreated?: (id: string) => void }) {
  const { L, toast, openPatient, closeSheet } = useApp();
  const { data: ths } = useApi<Therapist[]>("/therapists");
  const [f, setF] = useState({
    name: patient?.name ?? "", birth_date: patient?.birth_date ?? "", sex: patient?.sex ?? "", phone: patient?.phone ?? "", email: patient?.email ?? "",
    nif: patient?.nif ?? "", sns_number: patient?.sns_number ?? "", address: patient?.address ?? "", guardian_name: patient?.guardian_name ?? "",
    guardian_phone: patient?.guardian_phone ?? "", lang: patient?.lang ?? "pt", reminders: patient?.reminders ?? "sms", therapist_id: patient?.therapist_id ?? "", admin_note: patient?.admin_note ?? "",
  });
  const [touched, setTouched] = useState<Record<string, boolean>>({});
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) => setF({ ...f, [k]: e.target.value });
  const blur = (k: string) => () => setTouched((t) => ({ ...t, [k]: true }));
  // Under 18 on the day (the old year subtraction asked 18-year-olds for a guardian).
  const years = age(f.birth_date || null);
  const minor = years !== null && years < 18;
  // Inline checks that say how to fix it (audit M11); the server checks the NIF too.
  const bad: Record<string, string> = {
    name: !f.name.trim() ? L("Escreva o nome do utente.", "Enter the patient’s name.") : "",
    nif: !patientNifOk(f.nif) ? L("Confirme os 9 dígitos. Se for estrangeiro, comece pelo código do país (ES…).", "Check the 9 digits. For a foreign number, start with its country code (ES…).") : "",
    phone: f.phone.trim() && !phoneOk(f.phone) ? L("Escreva um número com pelo menos 9 dígitos, como 912 345 678.", "Write a number with at least 9 digits, like 912 345 678.") : "",
    guardian_phone: f.guardian_phone.trim() && !phoneOk(f.guardian_phone) ? L("Escreva um número com pelo menos 9 dígitos.", "Write a number with at least 9 digits.") : "",
    email: f.email.trim() && !emailOk(f.email) ? L("Escreva um email como maria@exemplo.pt.", "Write an email like maria@example.com.") : "",
    birth_date: f.birth_date && f.birth_date > ymd(new Date()) ? L("A data de nascimento não pode ser no futuro.", "The date of birth can’t be in the future.") : "",
  };
  const err = (k: string) => (touched[k] ? bad[k] || undefined : undefined);
  const invalid = Object.values(bad).some(Boolean);
  // Possible duplicates: same NIF (refused), same phone, or same name and birth date (a warning; owner decision 10 Nov 2026).
  const [dupQ, setDupQ] = useState("");
  useEffect(() => {
    const qs = new URLSearchParams();
    if (f.nif.trim() && patientNifOk(f.nif)) qs.set("nif", f.nif.trim());
    if (phoneOk(f.phone)) qs.set("phone", f.phone.trim());
    if (f.name.trim() && f.birth_date) { qs.set("name", f.name.trim()); qs.set("birth_date", f.birth_date); }
    if (patient) qs.set("exclude", patient.id);
    const t = setTimeout(() => setDupQ([...qs.keys()].some((k) => k !== "exclude") ? qs.toString() : ""), 400);
    return () => clearTimeout(t);
  }, [f.nif, f.phone, f.name, f.birth_date]); // eslint-disable-line react-hooks/exhaustive-deps
  const { data: dups } = useApi<Dup[]>(dupQ ? `/patients/duplicates?${dupQ}` : null);
  const nifTaken = dups?.find((d) => d.reason === "nif");
  const warn = (dups ?? []).filter((d) => d.reason !== "nif");
  const submit = async () => {
    setTouched({ name: true, nif: true, phone: true, guardian_phone: true, email: true, birth_date: true });
    if (invalid || nifTaken) return false;
    if (patient) {
      const changed = Object.fromEntries(Object.entries(f).filter(([k, v]) => (patient as unknown as Record<string, unknown>)[k] !== v && !((patient as unknown as Record<string, unknown>)[k] == null && v === "")));
      if (Object.keys(changed).length) await api("PATCH", `/patients/${patient.id}`, changed);
      toast(L("Dados guardados", "Details saved"));
    } else {
      const r = await api<{ id: string; number: number }>("POST", "/patients", Object.fromEntries(Object.entries(f).filter(([, v]) => v !== "")));
      toast(L(`Utente criado — n.º ${r.number}`, `Patient created — no. ${r.number}`));
      onCreated ? onCreated(r.id) : openPatient(r.id);
    }
  };
  const reason = (d: Dup) => d.reason === "nif" ? L("mesmo NIF", "same NIF") : d.reason === "phone" ? L("mesmo telemóvel", "same phone") : L("mesmo nome e data de nascimento", "same name and date of birth");
  return (
    <Sheet wide title={patient ? L("Dados do utente", "Patient Details") : L("Novo utente", "New Patient")} sub={patient ? undefined : L("Só o nome é obrigatório. Pode completar a ficha depois.", "Only the name is required. You can complete the file later.")}
      action={patient ? L("Guardar", "Save") : warn.length ? L("Criar mesmo assim", "Create Anyway") : L("Criar utente", "Create Patient")} onAction={submit}
      disabled={!f.name.trim() || !!nifTaken || (invalid && Object.keys(bad).some((k) => bad[k] && touched[k]))}>
      {(nifTaken || warn.length > 0) && (
        <div className="card pad dup" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <div className="small">
            <b>{nifTaken ? L("Já existe um utente com este NIF.", "There’s already a patient with this NIF.") : L("Pode ser um utente que já tem ficha.", "This may be a patient who already has a file.")}</b>
            {(nifTaken ? [nifTaken] : warn).map((d) => (
              <div key={d.id} className="dup-row">
                <span>{d.name} — n.º {d.number} <span className="muted">({reason(d)})</span></span>
                <button type="button" className="btn sm" onClick={() => { closeSheet(); openPatient(d.id); }}>{L("Abrir ficha", "Open File")}</button>
              </div>
            ))}
            {!nifTaken && !patient && <div className="muted">{L("Se for outra pessoa, pode criar a ficha mesmo assim.", "If it’s someone else, you can still create the file.")}</div>}
          </div>
        </div>
      )}
      <div className="grid2">
        <Field label={L("Nome", "Name")} error={err("name")}><input className="input" value={f.name} onChange={set("name")} onBlur={blur("name")} placeholder="Maria Santos" autoFocus aria-invalid={!!err("name") || undefined} /></Field>
        <Field label={L("Data de nascimento", "Date of birth")} error={err("birth_date") ?? (f.birth_date && bad.birth_date ? bad.birth_date : undefined)}><DateInput className="input" value={f.birth_date} max={ymd(new Date())} onChange={set("birth_date")} /></Field>
        <Field label={L("Telemóvel", "Mobile")} error={err("phone")}><input className="input" value={f.phone} onChange={set("phone")} onBlur={blur("phone")} placeholder="912 345 678" inputMode="tel" aria-invalid={!!err("phone") || undefined} /></Field>
        <Field label="Email" error={err("email")}><input className="input" value={f.email} onChange={set("email")} onBlur={blur("email")} placeholder="maria@exemplo.pt" type="email" aria-invalid={!!err("email") || undefined} /></Field>
        <Field label="NIF" error={err("nif")}><input className="input" value={f.nif} onChange={set("nif")} onBlur={blur("nif")} placeholder="123 456 789" inputMode="numeric" aria-invalid={!!err("nif") || undefined} /></Field>
        <Field label={L("N.º de utente SNS", "SNS number")}><input className="input" value={f.sns_number} onChange={set("sns_number")} inputMode="numeric" /></Field>
        <Field label={L("Sexo", "Sex")}>
          <select className="input" value={f.sex} onChange={set("sex")}><option value="">—</option><option value="f">{L("Feminino", "Female")}</option><option value="m">{L("Masculino", "Male")}</option><option value="x">{L("Outro", "Other")}</option></select>
        </Field>
        <Field label={L("Fisioterapeuta", "Therapist")}>
          <select className="input" value={f.therapist_id} onChange={set("therapist_id")}><option value="">—</option>{ths?.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}</select>
        </Field>
      </div>
      <Field label={L("Morada", "Address")}><input className="input" value={f.address} onChange={set("address")} placeholder={L("Rua, n.º, código postal, localidade", "Street, number, postcode, town")} /></Field>
      {(minor || f.guardian_name) && (
        <div className="grid2">
          <Field label={L("Encarregado de educação", "Parent or guardian")}><input className="input" value={f.guardian_name} onChange={set("guardian_name")} /></Field>
          <Field label={L("Telemóvel do encarregado", "Guardian’s mobile")} error={err("guardian_phone")}><input className="input" value={f.guardian_phone} onChange={set("guardian_phone")} onBlur={blur("guardian_phone")} inputMode="tel" aria-invalid={!!err("guardian_phone") || undefined} /></Field>
        </div>
      )}
      <div className="grid2">
        <Field label={L("Lembretes por", "Reminders by")}>
          <select className="input" value={f.reminders} onChange={set("reminders")}><option value="sms">{L("Telemóvel (WhatsApp ou SMS)", "Phone (WhatsApp or SMS)")}</option><option value="email">Email</option><option value="none">{L("Sem lembretes", "No reminders")}</option></select>
        </Field>
        <Field label={L("Idioma das mensagens", "Message language")}>
          <select className="input" value={f.lang} onChange={set("lang")}><option value="pt">Português</option><option value="en">English</option></select>
        </Field>
      </div>
      <Field label={L("Nota administrativa (não clínica)", "Admin note (not clinical)")}><input className="input" value={f.admin_note} onChange={set("admin_note")} placeholder={L("Ex.: prefere manhãs; fatura em nome da empresa", "E.g. prefers mornings; invoice to their company")} /></Field>
    </Sheet>
  );
}
export const NewPatient = () => <PatientDetails />;

export { RecordPayment, SellPlan } from "./MoneySheets";

/** Phase 4 (archive and legal tools): shows the choice, exports nothing yet. */
export const LOG_LABEL: Record<string, [string, string]> = {
  "patient.viewed": ["Abriu a ficha", "Opened the file"], "patient.created": ["Criou a ficha", "Created the file"], "patient.updated": ["Alterou dados", "Changed details"],
  "anamnesis.updated": ["Alterou a anamnese", "Changed the anamnesis"], "note.created": ["Começou uma nota", "Started a note"], "note.updated": ["Editou um rascunho", "Edited a draft"],
  "note.signed": ["Assinou uma nota", "Signed a note"], "note.draft_deleted": ["Apagou um rascunho", "Deleted a draft"], "test.recorded": ["Registou um teste", "Recorded a test"],
  "test.deleted": ["Apagou um teste", "Deleted a test"], "prescription.created": ["Registou uma prescrição", "Added a prescription"], "prescription.updated": ["Alterou uma prescrição", "Changed a prescription"],
  "consent.signed": ["Registou um consentimento", "Recorded a consent"], "consent.revoked": ["Registou a retirada de um consentimento", "Recorded a withdrawn consent"],
  "appointment.created": ["Fez uma marcação", "Booked an appointment"], "appointment.updated": ["Alterou uma marcação", "Changed an appointment"], "waitlist.added": ["Pôs na lista de espera", "Added to the waitlist"],
  "booking_request.accepted": ["Aceitou um pedido", "Accepted a request"], "class.attendee_added": ["Inscreveu numa aula", "Added to a class"], "patients.listed": ["Viu a lista de utentes", "Viewed the patient list"],
  "booking_request.added": ["Registou um pedido", "Added a request"], "booking_request.received": ["Pedido recebido pelo site", "Request received on the website"],
  "website.published": ["Publicou o site", "Published the website"], "website.unpublished": ["Retirou o site", "Took the website down"], "website.changed": ["Alterou o site", "Changed the website"],
  "website.team_changed": ["Alterou a equipa no site", "Changed the team on the website"], "website.prices_changed": ["Alterou os preços no site", "Changed the prices on the website"], "booking_request.declined": ["Recusou um pedido", "Declined a request"], "booking_request.reopened": ["Desfez a resposta a um pedido", "Undid a request answer"],
  "waitlist.updated": ["Alterou a lista de espera", "Changed the waitlist"], "consent_text.created": ["Nova versão de consentimento", "New consent version"],
  "document.uploaded": ["Juntou um documento", "Added a document"], "document.opened": ["Abriu um documento", "Opened a document"],
  "document.saved": ["Guardou uma cópia de um documento", "Saved a copy of a document"], "document.updated": ["Alterou um documento", "Changed a document"],
  "document.removed": ["Removeu um documento", "Removed a document"], "note.pdf_saved": ["Guardou o PDF de uma nota", "Saved a note’s PDF"],
  "consent.pdf_saved": ["Guardou o PDF de um consentimento", "Saved a consent’s PDF"], "reminders.listed": ["Viu a lista de lembretes", "Viewed the reminders list"],
  "appointment.reminded": ["Enviou um lembrete", "Sent a reminder"], "appointment.reminder_undone": ["Desmarcou um lembrete", "Unmarked a reminder"], "reminder_text.updated": ["Alterou a mensagem dos lembretes", "Changed the reminder message"], "consent_text.approved": ["Aprovou um texto de consentimento", "Approved a consent text"],
  "patient.exported": ["Exportou o processo clínico", "Exported the clinical record"], "patient.anonymised": ["Anonimizou o utente", "Anonymised the patient"],
  "patient.deleted": ["Apagou o utente", "Deleted the patient"],
  "complaint.created": ["Registou uma reclamação", "Logged a complaint"], "complaint.updated": ["Alterou uma reclamação", "Changed a complaint"],
  "complaint.deleted": ["Apagou uma reclamação", "Deleted a complaint"], "day.deleted": ["Apagou um dia inteiro", "Deleted a whole day"],
  "patient.reimbursement_pack": ["Criou um pacote de reembolso", "Made a reimbursement pack"], "ai.letter_drafted": ["Pediu à IA local um rascunho de carta", "Asked the local AI for a letter draft"], "ai.settings_changed": ["Mudou o endereço ou o modelo da IA local", "Changed the local AI address or model"],
  "letter.created": ["Começou uma carta de alta", "Started a discharge letter"], "letter.updated": ["Editou uma carta de alta", "Edited a discharge letter"],
  "letter.draft_deleted": ["Apagou o rascunho de uma carta", "Discarded a letter draft"], "letter.approved": ["Aprovou uma carta de alta", "Approved a discharge letter"],
  "letter.pdf_saved": ["Guardou o PDF de uma carta", "Saved a letter’s PDF"],
  "ai.note_rewritten": ["Pediu à IA local para reescrever uma nota", "Asked the local AI to rewrite a note"], "ai.receipt_read": ["Pediu à IA local para ler um talão", "Asked the local AI to read a receipt"],
  "ai.report_drafted": ["Pediu à IA local um relatório mensal", "Asked the local AI for a monthly report"], "ai.report_saved": ["Guardou um relatório mensal", "Kept a monthly report"],
  "ai.report_deleted": ["Apagou um relatório mensal", "Deleted a monthly report"], "assistant.asked": ["Fez um pedido ao assistente", "Asked the assistant"],
  "assistant.applied": ["Aplicou uma alteração do assistente", "Applied an assistant change"], "assistant.undone": ["Anulou uma alteração do assistente", "Undid an assistant change"],
  "clinic.hours_changed": ["Mudou o horário da clínica", "Changed the opening hours"], "clinic.closure_added": ["Fechou a clínica em certos dias", "Closed the clinic on some days"],
  "clinic.closure_removed": ["Reabriu dias fechados", "Reopened closed days"],
};

/** Owner: who opened or changed this patient's file, from the server's access log. */
export function AccessLog({ patientId, name }: { patientId: string; name: string }) {
  const { L, lang } = useApp();
  const { data, error } = useApi<{ id: number; at: string; staff_name: string | null; action: string; device: string; detail: string }[]>(`/access-log?subject_type=patient&subject_id=${patientId}&limit=200`);
  return (
    <Sheet wide title={L("Registo de acessos", "Access Log")} sub={name}>
      {error ? <p className="err">{error}</p> : !data ? <p className="small muted">{L("A carregar…", "Loading…")}</p> : (
        <div className="tbl-wrap" style={{ maxHeight: 360, overflow: "auto" }}><table><thead><tr>{[L("Quando", "When"), L("Quem", "Who"), L("O quê", "What"), L("Computador", "Computer")].map((h) => <th key={h} className="plain">{h}</th>)}</tr></thead>
          <tbody>{data.map((r) => (
            <tr key={r.id}>
              <td className="num">{fmtDate(r.at, lang, { day: "2-digit", month: "2-digit", year: "numeric" })} {hm(r.at)}</td><td>{r.staff_name ?? "—"}</td>
              <td>{LOG_LABEL[r.action] ? L(...LOG_LABEL[r.action]) : r.action}{r.action === "patient.viewed" && r.detail === "reduced" ? L(" (vista reduzida)", " (reduced view)") : r.action === "patient.viewed" && r.detail === "consent" ? L(" (consentimento)", " (consent)") : ""}</td>
              <td>{r.device}</td>
            </tr>
          ))}</tbody></table></div>
      )}
    </Sheet>
  );
}

/** Record a test or questionnaire result. The server says whether it's worse than last time. */

/** Add a prescription (credencial): doctor, payer, sessions and validity. */
export function AddPrescription({ patientId }: { patientId: string }) {
  const { L, toast } = useApp();
  const [f, setF] = useState({ doctor: "", specialty: "", payer: "", issued_on: ymd(new Date()), sessions_total: "10", valid_until: "" });
  const [scan, setScan] = useState<File | null>(null);
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) => setF({ ...f, [k]: e.target.value });
  const ok = !!f.issued_on && +f.sessions_total > 0 && (!scan || scan.size <= 10 * 1024 * 1024);
  return (
    <Sheet title={L("Nova prescrição", "New Prescription")} action={L("Guardar", "Save")} disabled={!ok}
      onAction={async () => {
        // The scan goes in first, so a refused file doesn't leave a prescription without it.
        const doc = scan ? await api<{ id: string }>("POST", `/patients/${patientId}/documents`, { kind: "prescription", filename: scan.name, data: await fileToBase64(scan) }) : null;
        await api("POST", `/patients/${patientId}/prescriptions`, { ...f, sessions_total: +f.sessions_total, ...(doc ? { document_id: doc.id } : {}) });
        toast(L("Prescrição guardada", "Prescription saved"));
      }}>
      <div className="grid2">
        <Field label={L("Médico", "Doctor")}><input className="input" value={f.doctor} onChange={set("doctor")} placeholder="Dr. Paulo Matos" /></Field>
        <Field label={L("Especialidade", "Specialty")}><input className="input" value={f.specialty} onChange={set("specialty")} placeholder={L("Medicina física e reabilitação", "Physical medicine and rehabilitation")} /></Field>
        <Field label={L("Entidade", "Payer")}><input className="input" value={f.payer} onChange={set("payer")} placeholder={L("ADSE, seguradora…", "ADSE, insurer…")} list="payers" /></Field>
        <Field label={L("Data da prescrição", "Date issued")}><DateInput className="input" value={f.issued_on} onChange={set("issued_on")} /></Field>
        <Field label={L("Número de sessões", "Number of sessions")}><input className="input" type="number" min={1} value={f.sessions_total} onChange={set("sessions_total")} /></Field>
        <Field label={L("Válida até", "Valid until")}><DateInput className="input" value={f.valid_until} onChange={set("valid_until")} /></Field>
      </div>
      <datalist id="payers"><option value="ADSE" /><option value="SNS" /><option value="Médis" /><option value="Multicare" /><option value="AdvanceCare" /><option value="SAMS" /></datalist>
      <Field label={L("Digitalização (opcional)", "Scan (optional)")}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
          <label className="btn" style={{ cursor: "pointer", flex: "none" }}><Paperclip />{scan ? L("Trocar…", "Replace…") : L("Escolher PDF ou foto…", "Choose PDF or Photo…")}
            <input type="file" accept={DOC_ACCEPT} hidden onChange={(e) => { setScan(e.target.files?.[0] ?? null); e.target.value = ""; }} /></label>
          {scan && <span className="small muted ell">{scan.name}</span>}
        </div>
      </Field>
      {scan && scan.size > 10 * 1024 * 1024 && <p className="err" role="alert">{L("O ficheiro tem de ter menos de 10 MB.", "Files must be under 10 MB.")}</p>}
    </Sheet>
  );
}

/** Enter an online booking request by hand (phone or email), until the patient portal exists. */
export function NewRequest() {
  const { L, toast } = useApp();
  const t = addDays(new Date(), 1); t.setHours(10, 0, 0, 0);
  const [f, setF] = useState({ name: "", phone: "", email: "", service: "session", day: ymd(t), time: "10:00", message: "" });
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) => setF({ ...f, [k]: e.target.value });
  return (
    <Sheet title={L("Novo pedido de marcação", "New Booking Request")} sub={L("Para pedidos recebidos por telefone ou email. Depois aceita ou recusa na Agenda.", "For requests that come in by phone or email. Accept or decline them in the Schedule.")}
      action={L("Adicionar", "Add")} disabled={!f.name.trim() || (!f.phone.trim() && !f.email.trim())}
      onAction={async () => { await api("POST", "/booking-requests", { name: f.name, phone: f.phone, email: f.email, service: f.service, preferred_at: iso(f.day, f.time), message: f.message }); toast(L("Pedido adicionado", "Request added")); }}>
      <div className="grid2">
        <Field label={L("Nome", "Name")}><input className="input" value={f.name} onChange={set("name")} autoFocus /></Field>
        <Field label={L("Serviço", "Service")}><select className="input" value={f.service} onChange={set("service")}><option value="session">{L("Sessão", "Session")}</option><option value="assessment">{L("Avaliação inicial", "First assessment")}</option></select></Field>
        <Field label={L("Telemóvel", "Mobile")}><input className="input" value={f.phone} onChange={set("phone")} inputMode="tel" /></Field>
        <Field label="Email"><input className="input" value={f.email} onChange={set("email")} type="email" /></Field>
        <Field label={L("Dia preferido", "Preferred day")}><DateInput className="input" value={f.day} onChange={set("day")} /></Field>
        <Field label={L("Hora preferida", "Preferred time")}><TimeInput className="input" step={900} value={f.time} onChange={set("time")} /></Field>
      </div>
      <Field label={L("Motivo", "Reason")}><input className="input" value={f.message} onChange={set("message")} /></Field>
    </Sheet>
  );
}

/** Add someone (a patient, or a new person) to the waitlist, with an optional preferred window. */
export function AddWaitlist() {
  const { L, lang, toast } = useApp();
  const { data: ths } = useApi<Therapist[]>("/therapists");
  const [pid, setPid] = useState("");
  const [f, setF] = useState({ name: "", phone: "", therapist_id: "", from_time: "", to_time: "", note: "" });
  const [wd, setWd] = useState<number[]>([]);
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) => setF({ ...f, [k]: e.target.value });
  const ok = pid === "new" ? !!(f.name.trim() && f.phone.trim()) : !!pid;
  const days = weekdayShort(lang);
  return (
    <Sheet title={L("Adicionar à lista de espera", "Add to Waitlist")} action={L("Adicionar", "Add")} disabled={!ok}
      onAction={async () => {
        await api("POST", "/waitlist", { ...(pid === "new" ? { name: f.name, phone: f.phone } : { patient_id: pid }), therapist_id: f.therapist_id || null, weekdays: wd,
          from_time: f.from_time || null, to_time: f.to_time || null, note: f.note });
        toast(L("Adicionado à lista de espera", "Added to the waitlist"));
      }}>
      <div style={{ marginBottom: 12 }}>
        <Seg label={L("Quem", "Who")} value={pid === "new" ? "new" : "patient"} onChange={(v) => setPid(v === "new" ? "new" : "")}
          options={[["patient", L("Utente", "Patient")], ["new", L("Pessoa nova", "Someone New")]]} />
      </div>
      {pid !== "new" && <Field label={L("Utente", "Patient")}><PatientPicker value={pid} label={L("Utente", "Patient")} onChange={(p) => setPid(p?.id ?? "")} /></Field>}
      {pid === "new" && <div className="grid2">
        <Field label={L("Nome", "Name")}><input className="input" value={f.name} onChange={set("name")} /></Field>
        <Field label={L("Telemóvel", "Mobile")}><input className="input" value={f.phone} onChange={set("phone")} inputMode="tel" /></Field>
      </div>}
      <Field label={L("Dias preferidos (nenhum = qualquer dia)", "Preferred days (none = any day)")}>
        <div className="chips">{days.map((d, i) => <button key={d} type="button" className="chip" aria-pressed={wd.includes(i + 1)} onClick={() => setWd(wd.includes(i + 1) ? wd.filter((x) => x !== i + 1) : [...wd, i + 1])}>{d}</button>)}</div>
      </Field>
      <div className="grid3">
        <Field label={L("A partir de", "From")}><TimeInput className="input" step={1800} value={f.from_time} onChange={set("from_time")} /></Field>
        <Field label={L("Até", "Until")}><TimeInput className="input" step={1800} value={f.to_time} onChange={set("to_time")} /></Field>
        <Field label={L("Fisioterapeuta", "Therapist")}><select className="input" value={f.therapist_id} onChange={set("therapist_id")}><option value="">{L("Qualquer", "Any")}</option>{ths?.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}</select></Field>
      </div>
      <Field label={L("Nota", "Note")}><input className="input" value={f.note} onChange={set("note")} /></Field>
    </Sheet>
  );
}

/** Add a patient to a group class. */
export function AddAttendee({ appt }: { appt: Appointment }) {
  const { L, toast } = useApp();
  const [pid, setPid] = useState("");
  const taken = useMemo(() => new Set(appt.attendees?.map((a) => a.patient_id)), [appt]);
  return (
    <Sheet title={L("Inscrever na aula", "Add to Class")} sub={`${appt.title} — ${(appt.attendees?.length ?? 0)}/${appt.capacity}`} action={L("Inscrever", "Add")} disabled={!pid}
      onAction={async () => { await api("POST", `/appointments/${appt.id}/attendees`, { patient_id: pid }); toast(L("Inscrito", "Added")); }}>
      <Field label={L("Utente", "Patient")}>
        <PatientPicker value={pid} label={L("Utente", "Patient")} exclude={taken} autoFocus onChange={(p) => setPid(p?.id ?? "")} />
      </Field>
    </Sheet>
  );
}


