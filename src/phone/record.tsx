// Recording a session on the iPhone (phase 5): the same recorder as the desktop (16 kHz pieces sent to the clinic
// server every 30 s, nothing kept on the phone). Ask the patient first; start only on a yes. The local AI drafts
// the note, which is reviewed and signed on the computer.
//
// The screen is kept on while recording (Screen Wake Lock): iOS may pause the microphone of a locked phone. Whether
// recording keeps going with the screen locked is the risk the plan asks to test in the first build.
import { useEffect, useState, useSyncExternalStore } from "react";
import { Check, Mic, Pause, Play, Search, Square, Trash2, X, Sparkles } from "lucide-react";
import { useApp } from "../lib/ctx";
import { useApi, addDays, hm, range, NOTE_TEMPLATES, type Appointment, type PatientRow } from "../lib/data";
import { getRec, subscribeRec, startRecording, stopRecording, pauseRecording, resumeRecording, discardRecording, clearRecording } from "../lib/recorder";
import { clock } from "../components/Recording";
import { BackCircle, Group, PEmpty, Row, Screen, SheetHead } from "./ui";
import type { PhoneNav } from "./PhoneApp";

type Target = { patientId: string; name: string; appointmentId: string | null };

/** Keeps the screen on while a recording runs (iOS 16.4+). Released when it ends. */
function useWakeLock(on: boolean) {
  useEffect(() => {
    if (!on) return;
    let lockObj: { release: () => Promise<void> } | null = null;
    const wl = (navigator as unknown as { wakeLock?: { request: (t: "screen") => Promise<{ release: () => Promise<void> }> } }).wakeLock;
    const get = () => { wl?.request("screen").then((l) => { lockObj = l; }).catch(() => undefined); };
    get();
    const again = () => { if (document.visibilityState === "visible") get(); };
    document.addEventListener("visibilitychange", again);
    return () => { document.removeEventListener("visibilitychange", again); void lockObj?.release().catch(() => undefined); };
  }, [on]);
}

export function RecordScreen({ nav }: { nav: PhoneNav }) {
  const { L } = useApp();
  const r = useSyncExternalStore(subscribeRec, getRec);
  const active = r.state !== "idle";
  useWakeLock(r.state === "recording" || r.state === "paused" || r.state === "starting");
  const [sent, setSent] = useState<string | null>(null);
  return (
    <Screen title={L("Gravar sessão", "Record Session")} label={L("Gravar", "Record")} leading={<BackCircle label={L("Hoje", "Today")} onClick={() => nav.setTab("today")} />}>
      {active ? <Recording nav={nav} onSent={setSent} /> : <Start nav={nav} sent={sent} setSent={setSent} />}
    </Screen>
  );
}

function Start({ nav, sent, setSent }: { nav: PhoneNav; sent: string | null; setSent: (s: string | null) => void }) {
  const { L, lang, me, openSheet, toast } = useApp();
  const now = new Date();
  const day0 = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const { data: list } = useApi<Appointment[]>(`/appointments?${range(day0, addDays(day0, 1))}`);
  const [target, setTarget] = useState<Target | null>(nav.recordTarget);
  const [tpl, setTpl] = useState(NOTE_TEMPLATES[0].id);
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (nav.recordTarget) { setTarget(nav.recordTarget); nav.clearRecordTarget(); } }, [nav.recordTarget]); // eslint-disable-line react-hooks/exhaustive-deps
  // Today's sessions that can still be recorded: yours first (the owner sees everyone's).
  const sessions = (list ?? []).filter((x) => x.patient_id && x.status === "booked" && (x.kind === "session" || x.kind === "assessment" || x.kind === "home") && new Date(x.ends_at) > addDays(now, 0) && (me.role === "owner" || x.therapist_id === me.id))
    .sort((a, b) => a.starts_at.localeCompare(b.starts_at));
  const t = NOTE_TEMPLATES.find((x) => x.id === tpl) ?? NOTE_TEMPLATES[0];
  const start = async () => {
    if (!target) return;
    setBusy(true); setSent(null);
    try { await startRecording({ patientId: target.patientId, patientName: target.name, template: t.id, blocks: t.blocks(lang), appointmentId: target.appointmentId, lang, L }); }
    catch (e) { toast(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); }
  };
  return (
    <>
      {sent && <div className="p-banner ok" role="status"><Check aria-hidden /><span>{sent}</span><button className="p-icon sm" onClick={() => setSent(null)} aria-label={L("Fechar", "Close")}><X /></button></div>}
      <Group title={L("Utente", "Patient")}>
        {sessions.map((x) => {
          const sel = target?.appointmentId === x.id;
          return <Row key={x.id} label={x.patient_name ?? "—"} detail={`${hm(x.starts_at)} – ${hm(x.ends_at)}${me.role === "owner" && x.therapist_name ? ` · ${x.therapist_name}` : ""}`} selected={sel}
            value={sel ? <Check className="p-check" /> : undefined} onClick={() => setTarget({ patientId: x.patient_id!, name: x.patient_name ?? "", appointmentId: x.id })} />;
        })}
        {target && !target.appointmentId && <Row label={target.name} detail={L("Sem marcação associada", "No appointment linked")} selected value={<Check className="p-check" />} />}
        <Row label={L("Outro utente…", "Other Patient…")} tone="a" chevron onClick={() => openSheet(<PickPatient onPick={(p) => setTarget({ patientId: p.id, name: p.name, appointmentId: null })} />)} />
      </Group>

      <Group title={L("Modelo da nota", "Note template")}>
        {NOTE_TEMPLATES.map((x) => <Row key={x.id} label={L(x.name[0], x.name[1])} selected={tpl === x.id} value={tpl === x.id ? <Check className="p-check" /> : undefined} onClick={() => setTpl(x.id)} />)}
      </Group>

      <p className="p-ask">{target ? L(`${target.name.split(" ")[0]} aceitou ser gravado?`, `Did ${target.name.split(" ")[0]} agree to be recorded?`) : L("O utente aceitou ser gravado?", "Did the patient agree to be recorded?")}</p>
      <div className="p-actions">
        <button className="p-btn rec wide" disabled={!target || busy} onClick={() => void start()}><span className="p-rec-dot" aria-hidden />{L("Começar a gravar", "Start Recording")}</button>
      </div>
      <p className="p-note center">{L("Fica só no servidor da clínica. Apagado ao assinar a nota.", "Stays on the clinic server. Deleted when the note is signed.")}</p>
    </>
  );
}

function Recording({ nav, onSent }: { nav: PhoneNav; onSent: (s: string) => void }) {
  const { L, toast, confirm } = useApp();
  const r = useSyncExternalStore(subscribeRec, getRec);
  const [busy, setBusy] = useState(false);
  const live = r.state === "recording";
  const stop = async () => {
    setBusy(true);
    try {
      const done = await stopRecording();
      const mins = done.seconds < 60 ? `${done.seconds} s` : `${Math.round(done.seconds / 60)} min`;
      onSent(L(`Enviada (${mins}). O rascunho aparece na Evolução, no computador.`, `Sent (${mins}). The draft appears in Progress, on the computer.`));
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      toast(m === "offline" ? L("Não foi possível enviar o fim da gravação. Verifique a ligação; o que já foi enviado fica nas notas do utente.", "Couldn’t send the end of the recording. Check the connection; what was sent stays in the patient’s notes.") : m);
    } finally { setBusy(false); }
  };
  if (r.state === "error") {
    return (
      <PEmpty icon={<Mic />} title={L("A gravação parou", "Recording stopped")} text={r.error || L("O que chegou ao servidor fica nas notas do utente.", "What reached the server stays in the patient’s notes.")}
        action={<button className="p-btn" onClick={() => { const pid = r.patientId; clearRecording(); if (pid) nav.openPatient(pid); }}>{L("Abrir ficha", "Open File")}</button>} />
    );
  }
  return (
    <div className="p-recording">
      <div className={`p-rec-ring ${live ? "live" : ""}`} style={{ ["--lvl" as string]: String(Math.max(0.04, r.level)) }}>
        <span className="p-rec-time num" aria-live="off">{clock(r.seconds)}</span>
        <span className="p-rec-state">{r.state === "starting" ? L("A preparar o microfone…", "Getting the microphone ready…")
          : r.state === "finishing" ? L("A enviar o resto…", "Sending the rest…") : r.state === "paused" ? L("Em pausa", "Paused") : L("A gravar", "Recording")}</span>
      </div>
      <p className="p-rec-who">{r.patientName}</p>
      <p className="p-note center">{r.offline ? L("Sem ligação — continua a gravar", "Offline — still recording") : L("Mantenha a app aberta", "Keep the app open")}</p>
      {(r.state === "recording" || r.state === "paused") && (
        <div className="p-rec-ctl">
          <button className="p-round" onClick={() => confirm({ title: L("Apagar esta gravação?", "Delete this recording?"), text: L("O que foi gravado é apagado e não é transcrito.", "What was recorded is deleted and isn’t transcribed."),
            action: L("Apagar gravação", "Delete Recording"), hold: true, onConfirm: () => { void discardRecording(); toast(L("Gravação apagada", "Recording deleted")); } })}
            disabled={busy} aria-label={L("Apagar a gravação…", "Delete Recording…")}><Trash2 /></button>
          <button className="p-round p-stop" onClick={() => void stop()} disabled={busy} aria-label={L("Terminar e enviar", "Stop and Send")}><Square fill="currentColor" /></button>
          <button className="p-round" onClick={live ? pauseRecording : resumeRecording} disabled={busy} aria-label={live ? L("Pausa", "Pause") : L("Continuar", "Resume")}>{live ? <Pause /> : <Play />}</button>
        </div>
      )}
    </div>
  );
}

/** Any patient (for a session without an appointment). */
function PickPatient({ onPick }: { onPick: (p: PatientRow) => void }) {
  const { L, closeSheet } = useApp();
  const [q, setQ] = useState("");
  const [term, setTerm] = useState("");
  useEffect(() => { const t = setTimeout(() => setTerm(q.trim()), 250); return () => clearTimeout(t); }, [q]);
  const { data } = useApi<PatientRow[]>(term ? `/patients?limit=30&q=${encodeURIComponent(term)}` : null);
  return (
    <>
      <SheetHead title={L("Escolher utente", "Choose Patient")} onClose={closeSheet} closeLabel={L("Cancelar", "Cancel")} />
      <div className="p-sheet-body">
        <label className="p-search">
          <Search aria-hidden />
          <input type="search" autoFocus autoCorrect="off" value={q} onChange={(e) => setQ(e.target.value)} placeholder={L("Nome, número ou telefone", "Name, number or phone")} aria-label={L("Procurar utentes", "Search patients")} />
        </label>
        {data && <div className="p-group"><div className="p-list">
          {data.map((p) => <Row key={p.id} label={p.name} detail={`Nº ${String(p.number).padStart(4, "0")}`} onClick={() => { onPick(p); closeSheet(); }} />)}
          {!data.length && <Row label={L("Nenhum utente encontrado", "No patients found")} />}
        </div></div>}
      </div>
    </>
  );
}
