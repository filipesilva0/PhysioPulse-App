// Session recording on the desktop (PhysioPulse plan, phase 2): the sheet that starts it (the patient is asked
// first), the pill in the title bar while it runs, the patient's recordings in the Notes tab with their status,
// the AI draft to review, and playback. Owner, and therapists whose AI switch is on (Team).
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { Mic, Pause, Play, Square, Sparkles, X, RotateCcw, Headphones, Trash2, ChevronDown, ChevronRight } from "lucide-react";
import { useApp } from "../lib/ctx";
import { api, isErr, recordingAudio } from "../lib/api";
import { useApi, type Block } from "../lib/data";
import { getRec, subscribeRec, startRecording, stopRecording, pauseRecording, resumeRecording, discardRecording, clearRecording, isRecording } from "../lib/recorder";
import { Sheet } from "./ui";
import { AiCard } from "./Ai";

export interface RecordingRow {
  id: string; status: "recording" | "queued" | "transcribing" | "drafting" | "ready" | "failed";
  seconds: number; template: string; lang: "pt" | "en"; error: string; note_id: string | null;
  created_by: string; created_by_name: string; created_at: string; ready_at: string | null; delete_after: string;
}
export interface RecordingDraft {
  id: string; template: string; lang: "pt" | "en"; report: string; plan: string;
  values: Record<string, number | string | string[]>; model: string; transcript: string;
}

export const useRec = () => useSyncExternalStore(subscribeRec, getRec);

/** 754 → "12:34"; 3725 → "1:02:05". */
export const clock = (s: number) => {
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(x).padStart(2, "0")}` : `${m}:${String(x).padStart(2, "0")}`;
};
/** 840 → "14 min"; 40 → "40 s". */
const mins = (s: number) => s < 60 ? `${s} s` : `${Math.round(s / 60)} min`;

/** Asks first, then starts. The note template decides which blocks the AI may fill in. */
export function RecordSheet({ pid, name, template, templateName, blocks, appointmentId }: {
  pid: string; name: string; template: string; templateName: string; blocks: Block[]; appointmentId: string | null;
}) {
  const { L, lang } = useApp();
  return (
    <Sheet title={L("Gravar a sessão", "Record the Session")} action={L("Começar a gravar", "Start Recording")}
      onAction={() => startRecording({ patientId: pid, patientName: name, template, blocks, appointmentId, lang, L })}>
      <div className="rec-ask">
        <span className="rec-ask-ico" aria-hidden><Mic size={18} /></span>
        <p><strong>{L(`Pergunte primeiro a ${name} se aceita que a sessão seja gravada.`, `First ask ${name} whether they agree to the session being recorded.`)}</strong>{" "}
          {L("Comece só se disser que sim. Se não, escreva a nota como habitualmente.", "Only start if they say yes. If not, write the note as usual.")}</p>
      </div>
      <p className="small muted" style={{ marginTop: 12 }}>
        {L(`O áudio vai só para o servidor da clínica. A IA local transcreve a conversa e escreve um rascunho da nota (modelo «${templateName}») para rever e assinar. O áudio, a transcrição e o rascunho são apagados quando assinar a nota, ou ao fim de 30 dias.`,
          `The audio only goes to the clinic server. The local AI transcribes the conversation and drafts the note (template “${templateName}”) for you to review and sign. The audio, the transcript and the draft are deleted when you sign the note, or after 30 days.`)}
      </p>
      <p className="small muted" style={{ marginTop: 8 }}>{L("Pode continuar a usar a aplicação enquanto grava: a gravação fica na barra de cima.", "You can keep using the app while recording: the recording stays in the bar at the top.")}</p>
    </Sheet>
  );
}

/** In the title bar while a session is recorded: who, how long, the level, Pause and Stop. */
export function RecordingPill() {
  const { L, toast, openPatient, confirm } = useApp();
  const r = useRec();
  const [busy, setBusy] = useState(false);
  if (r.state === "idle") return null;
  const stop = async () => {
    setBusy(true);
    try {
      const done = await stopRecording();
      toast(L(`Gravação enviada (${mins(done.seconds)}). O rascunho aparece na Evolução do utente quando a IA local terminar.`, `Recording sent (${mins(done.seconds)}). The draft appears in the patient’s Progress tab when the local AI is done.`));
      if (done.patientId) openPatient(done.patientId);
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      toast(m === "offline" ? L("Não foi possível enviar o fim da gravação. Verifique a ligação ao servidor; o que já foi enviado está nas notas do utente.", "Couldn’t send the end of the recording. Check the connection to the server; what was sent is in the patient’s notes.") : m);
    } finally { setBusy(false); }
  };
  const discard = () => confirm({
    title: L("Apagar esta gravação?", "Delete this recording?"),
    text: L("O que foi gravado é apagado e não é transcrito.", "What was recorded is deleted and isn’t transcribed."),
    action: L("Apagar", "Delete"), hold: true, onConfirm: () => { void discardRecording(); toast(L("Gravação apagada", "Recording deleted")); },
  });
  if (r.state === "error") {
    return (
      <div className="rec-pill err" role="status">
        <span className="rec-dot off" aria-hidden />
        <span className="rec-txt" title={r.error}>{L("Gravação interrompida", "Recording stopped")}{r.error ? ` — ${r.error}` : ""}</span>
        <button className="iconbtn sm" onClick={() => { clearRecording(); if (r.patientId) openPatient(r.patientId); }} aria-label={L("Fechar", "Close")} title={L("Fechar", "Close")}><X size={14} /></button>
      </div>
    );
  }
  const live = r.state === "recording";
  return (
    <div className={`rec-pill ${live ? "live" : ""}`} role="status" aria-live="off" aria-label={L(`A gravar a sessão de ${r.patientName}, ${clock(r.seconds)}`, `Recording ${r.patientName}’s session, ${clock(r.seconds)}`)}>
      <span className={`rec-dot ${live ? "" : "off"}`} aria-hidden />
      <span className="rec-txt">
        {r.state === "starting" ? L("A preparar o microfone…", "Getting the microphone ready…")
          : r.state === "finishing" ? L("A enviar o resto da gravação…", "Sending the rest of the recording…")
            : <>{r.state === "paused" ? L("Em pausa", "Paused") : r.patientName} <span className="rec-time">{clock(r.seconds)}</span></>}
      </span>
      {live && <span className="rec-level" aria-hidden><span style={{ transform: `scaleX(${Math.max(0.04, r.level)})` }} /></span>}
      {r.offline && <span className="rec-warn" title={L("Sem ligação ao servidor: a gravação continua e é enviada quando voltar.", "No connection to the server: recording continues and is sent when it’s back.")}>{L("sem ligação", "offline")}</span>}
      {(r.state === "recording" || r.state === "paused") && <>
        <button className="iconbtn sm" onClick={live ? pauseRecording : resumeRecording} disabled={busy} aria-label={live ? L("Pausa", "Pause") : L("Continuar", "Resume")} title={live ? L("Pausa", "Pause") : L("Continuar", "Resume")}>{live ? <Pause size={14} /> : <Play size={14} />}</button>
        <button className="btn sm rec-stop" onClick={stop} disabled={busy}><Square size={11} fill="currentColor" />{L("Terminar", "Stop")}</button>
        <button className="iconbtn sm" onClick={discard} disabled={busy} aria-label={L("Apagar a gravação…", "Delete Recording…")} title={L("Apagar a gravação…", "Delete Recording…")}><Trash2 size={13} /></button>
      </>}
    </div>
  );
}

/** What the status means, in words that say what happens next. */
function statusText(r: RecordingRow, speechOn: boolean, L: (pt: string, en: string) => string): string {
  switch (r.status) {
    case "recording": return L("Gravação interrompida antes de terminar", "Recording stopped before it finished");
    case "queued": return speechOn ? L("Na fila para a IA local…", "Waiting in line for the local AI…") : L("À espera do computador da IA (desligado ou sem resposta)", "Waiting for the AI computer (off or not answering)");
    case "transcribing": return L("A transcrever a conversa…", "Transcribing the conversation…");
    case "drafting": return L("A escrever o rascunho da nota a partir da conversa…", "Writing the draft note from the conversation…");
    case "ready": return L("Rascunho pronto para rever", "Draft ready to review");
    case "failed": return r.error === "no_speech" ? L("Não se ouviu conversa nesta gravação", "No conversation could be heard in this recording")
      : r.error === "stt_failed" ? L("A transcrição falhou no computador da IA", "Transcription failed on the AI computer")
        : r.error === "ai_no_model" ? L("O modelo de IA escolhido não está instalado", "The chosen AI model isn’t installed")
          : L("A IA não conseguiu escrever um rascunho simples", "The AI couldn’t write a plain draft");
  }
}

/** The patient's recordings (own; the owner sees all), above the earlier notes. Refreshes while work is going on. */
export function Recordings({ pid, onReview }: { pid: string; onReview: (d: RecordingDraft) => void }) {
  const { L, lang, toast, confirm, openSheet } = useApp();
  const live = useRec();
  const q = useApi<{ recordings: RecordingRow[]; speech: { available: boolean } }>(`/patients/${pid}/recordings`, { keep: true });
  const rows = (q.data?.recordings ?? []).filter((r) => !(r.id === live.id && isRecording()));
  const working = rows.some((r) => ["queued", "transcribing", "drafting"].includes(r.status));
  const reload = useRef(q.reload); reload.current = q.reload;
  useEffect(() => {
    if (!working) return;
    const t = setInterval(() => reload.current(), 8000);
    return () => clearInterval(t);
  }, [working]);
  useEffect(() => { if (live.state === "idle") reload.current(); }, [live.state]);
  const [opening, setOpening] = useState<string | null>(null);
  if (!rows.length) return null;
  const act = (fn: () => Promise<unknown>, done?: string) => fn().then(() => { if (done) toast(done); reload.current(); }).catch((e) => toast(isErr(e) ? e.message : String(e)));
  const review = async (r: RecordingRow) => {
    setOpening(r.id);
    try { onReview(await api<RecordingDraft>("GET", `/recordings/${r.id}`)); }
    catch (e) { toast(isErr(e) ? e.message : String(e)); }
    finally { setOpening(null); }
  };
  const day = (iso: string) => new Date(iso).toLocaleDateString(lang === "pt" ? "pt-PT" : "en-GB", { day: "numeric", month: "short" });
  const hm = (iso: string) => new Date(iso).toLocaleTimeString(lang === "pt" ? "pt-PT" : "en-GB", { hour: "2-digit", minute: "2-digit" });
  return (
    <section aria-labelledby="recs-h" style={{ marginBottom: 18 }}>
      <h2 id="recs-h" style={{ marginBottom: 10 }}>{L("Sessões gravadas", "Recorded sessions")}</h2>
      <div className="rows">
        {rows.map((r) => {
          const busy = ["queued", "transcribing", "drafting"].includes(r.status);
          return (
            <div key={r.id} className="row rec-row">
              <span className={`rec-ico ${r.status === "ready" ? "ai" : r.status === "failed" ? "bad" : ""}`} aria-hidden>{r.status === "ready" ? <Sparkles size={14} /> : <Mic size={14} />}</span>
              <span className="grow">
                <span style={{ display: "block", fontWeight: 500 }}>{L(`${day(r.created_at)}, ${hm(r.created_at)} · ${mins(r.seconds)}`, `${day(r.created_at)}, ${hm(r.created_at)} · ${mins(r.seconds)}`)}</span>
                <span className="s">{busy && <span className="spin" aria-hidden />}{statusText(r, !!q.data?.speech.available, L)}
                  {" · "}{r.note_id ? L("apagada quando a nota for assinada", "deleted when the note is signed") : L(`apagada sozinha a ${day(r.delete_after)}`, `deletes itself on ${day(r.delete_after)}`)}</span>
              </span>
              <span style={{ display: "flex", gap: 6, flexWrap: "wrap", justifyContent: "flex-end" }}>
                {r.status === "ready" && <button className="btn sm ai" onClick={() => review(r)} disabled={opening === r.id} aria-busy={opening === r.id}><Sparkles />{L("Rever rascunho", "Review Draft")}</button>}
                {r.status === "failed" && <button className="btn sm" onClick={() => act(() => api("POST", `/recordings/${r.id}/retry`), L("De volta à fila", "Back in line"))}><RotateCcw />{L("Tentar de novo", "Try Again")}</button>}
                {r.status === "recording" && r.seconds >= 10 && <button className="btn sm" onClick={() => act(() => api("POST", `/recordings/${r.id}/finish`), L("Enviada para a IA local", "Sent to the local AI"))}>{L("Usar o que foi gravado", "Use What Was Recorded")}</button>}
                {r.status !== "recording" && <button className="btn sm" onClick={() => openSheet(<ListenSheet id={r.id} seconds={r.seconds} />)}><Headphones />{L("Ouvir…", "Listen…")}</button>}
                <button className="btn sm" onClick={() => confirm({
                  title: L("Apagar esta gravação?", "Delete this recording?"),
                  text: L("O áudio, a transcrição e o rascunho são apagados do servidor. As notas já escritas ficam.", "The audio, the transcript and the draft are deleted from the server. Notes already written stay."),
                  action: L("Apagar", "Delete"), hold: true,
                  onConfirm: () => act(() => api("DELETE", `/recordings/${r.id}`), L("Gravação apagada", "Recording deleted")),
                })}>{L("Apagar…", "Delete…")}</button>
              </span>
            </div>
          );
        })}
      </div>
    </section>
  );
}

/** The AI's draft from a recording, to use in the note being written (nothing is saved until the note is). */
export function DraftCard({ d, blocks, onUse, onDiscard }: { d: RecordingDraft; blocks: Block[]; onUse: () => void; onDiscard: () => void }) {
  const { L } = useApp();
  const [showText, setShowText] = useState(false);
  const proposed = blocks.filter((b) => d.values[b.key] !== undefined).map((b) => {
    const v = d.values[b.key];
    const unit = b.type === "slider" ? (b.unit ?? (b.max === 10 ? "/10" : "")) : "";
    return `${b.label.replace(/ \(.*\)$/, "")}: ${Array.isArray(v) ? v.join(", ") : `${v}${unit}`}`;
  });
  return (
    <AiCard title={L("Rascunho da IA local a partir da gravação — reveja antes de assinar", "Draft from the local AI, from the recording — check it before signing")}
      actions={<>
        <button className="btn sm pri" onClick={onUse}>{L("Usar na nota", "Use in Note")}</button>
        <button className="btn sm" onClick={onDiscard}>{L("Fechar", "Close")}</button>
      </>}>
      <p className="ai-text">{d.report}</p>
      {d.plan && <p className="ai-text" style={{ marginTop: 10 }}>{d.plan}</p>}
      {proposed.length > 0 && <p className="small" style={{ marginTop: 10 }}><span className="muted">{L("Valores propostos: ", "Proposed values: ")}</span>{proposed.join(" · ")}</p>}
      <button className="linkbtn small" style={{ marginTop: 10 }} aria-expanded={showText} onClick={() => setShowText(!showText)}>
        {showText ? <ChevronDown size={12} /> : <ChevronRight size={12} />}{L("Transcrição automática", "Automatic transcript")}
      </button>
      {showText && <p className="rec-transcript">{d.transcript}</p>}
      <p className="small muted" style={{ marginTop: 8 }}>{L("Usar põe o texto nas notas livres e os valores nas escalas e escolhas do modelo. A IA pode errar e não sabe com certeza quem falou: confirme tudo antes de assinar.",
        "Use puts the text in Free notes and the values in the template’s scales and choices. The AI can make mistakes and can’t be sure who spoke: check everything before signing.")}</p>
    </AiCard>
  );
}

/** Plays a recording inside the app (logged on the server). Nothing is saved on this computer. */
export function ListenSheet({ id, seconds }: { id: string; seconds: number }) {
  const { L } = useApp();
  const [url, setUrl] = useState<string>();
  const [err, setErr] = useState<string>();
  useEffect(() => {
    let u: string | undefined;
    recordingAudio(id).then((b) => { u = URL.createObjectURL(b); setUrl(u); }).catch((e) => setErr(isErr(e) ? e.message : String(e)));
    return () => { if (u) URL.revokeObjectURL(u); };
  }, [id]);
  return (
    <Sheet title={L("Ouvir a gravação", "Listen to the Recording")} sub={L(`${mins(seconds)}. Cada vez que é ouvida fica no registo de acessos.`, `${mins(seconds)}. Each time it’s played is in the access log.`)}>
      {url ? <audio controls src={url} style={{ width: "100%" }} autoFocus /> : err ? <div className="err" role="alert">{err}</div> : <p className="small muted">{L("A carregar o áudio…", "Loading the audio…")}</p>}
    </Sheet>
  );
}
