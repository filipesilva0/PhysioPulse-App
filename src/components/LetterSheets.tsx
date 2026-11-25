// Reimbursement pack and discharge letters (owner decisions, 8 Nov 2026).
// Server: server/src/routes/letters.rs. Roles are enforced there: packs and letter drafts for the owner and
// therapists (never reception); the local AI and approving a letter for the owner only.
//
// Layout (8 Nov, owner asked for "more pleasing, easier to read, no clutter"): grouped form rows like macOS
// settings, details behind disclosure, one footer with a quiet status on the left and at most two buttons on
// the right. Letters save themselves as drafts; the letter reads like a page, not a form.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, ChevronRight, FileText, Sparkles } from "lucide-react";
import { useApp, eur } from "../lib/ctx";
import { api, isErr } from "../lib/api";
import { fmtDate, useApi, kindLabel, type ApptKind, type DocumentRow } from "../lib/data";
import { Mark, Seg, DateInput } from "./ui";
import { DocumentViewer } from "./DocSheets";

/** A textarea that grows with its text (no inner scroll bar). */
function Grow(props: React.TextareaHTMLAttributes<HTMLTextAreaElement> & { minRows?: number; inputRef?: React.RefObject<HTMLTextAreaElement> }) {
  const { minRows = 3, inputRef, ...rest } = props;
  const own = useRef<HTMLTextAreaElement>(null);
  const ref = inputRef ?? own;
  useLayoutEffect(() => {
    const t = ref.current; if (!t) return;
    t.style.height = "auto"; t.style.height = `${t.scrollHeight + 2}px`;
  }, [props.value, ref]);
  return <textarea ref={ref} rows={minRows} {...rest} />;
}

const errText = (e: unknown) => (isErr(e) ? e.message : String(e));

// ───────── reimbursement pack ─────────

interface PackRx { id: string; doctor: string; specialty: string; payer: string; issued: string; issued_iso: string; total: number; used: number; document_id: string | null }
interface PackSession { id: string; on: string; time: string; kind: ApptKind; title: string; therapist: string | null; minutes: number }
interface PackPayment { id: string; number: number; received_on: string; amount_cents: number; description: string; invoice_number: string; has_file: boolean; file_mime: string | null }
interface PackPreview {
  from: string; to: string; prescriptions: PackRx[]; prescription_id: string | null; payer: string;
  sessions: PackSession[]; payments: PackPayment[]; report: string; signer: string; professional_id: string;
}
interface PackDone { document_id: string; filename: string; pages: number; attached: string[]; skipped: { what: string; reason: string }[] }

/** One PDF for ADSE or an insurer: report, sessions and invoices, then the prescription scan and the invoice files. */
export function ReimbursementPack({ pid, name }: { pid: string; name: string }) {
  const { L, lang, openSheet, closeSheet, cancelSheet, toast } = useApp();
  const [rx, setRx] = useState<string | null | undefined>(undefined); // undefined = the server chooses
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const qs = new URLSearchParams();
  if (rx) qs.set("prescription_id", rx);
  if (from) qs.set("from", from);
  if (to) qs.set("to", to);
  const pv = useApi<PackPreview>(`/patients/${pid}/reimbursement${qs.toString() ? `?${qs}` : ""}`);
  const d = pv.data;
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [report, setReport] = useState("");
  const [edited, setEdited] = useState(false);
  const [payer, setPayer] = useState("");
  const [pro, setPro] = useState("");
  const [showSessions, setShowSessions] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string>();
  const first = useRef(true);
  useEffect(() => {
    if (!d) return;
    if (first.current) {
      first.current = false;
      setPro(d.professional_id);
      setRx(d.prescription_id ?? (d.prescriptions[0]?.id ?? null));
      if (!d.prescription_id && d.prescriptions[0]) return; // asks again with the latest prescription and its period
    }
    if (!from) setFrom(d.from);
    if (!to) setTo(d.to);
    setPicked(new Set(d.payments.filter((p) => p.invoice_number || p.has_file).map((p) => p.id)));
    if (!edited) setReport(d.report);
    setPayer(d.payer);
  }, [d]); // eslint-disable-line react-hooks/exhaustive-deps

  const chosen = d?.payments.filter((p) => picked.has(p.id)) ?? [];
  const total = chosen.reduce((t, p) => t + p.amount_cents, 0);
  const rxRow = d?.prescriptions.find((r) => r.id === rx);
  const toggle = (id: string) => setPicked((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const ready = !!d && !!report.trim() && !!from && !!to && !busy;

  const make = async () => {
    if (!ready) return;
    setBusy(true); setErr(undefined);
    try {
      const r = await api<PackDone>("POST", `/patients/${pid}/reimbursement`, {
        prescription_id: rx || null, from, to, payment_ids: chosen.map((p) => p.id), payer: payer.trim(), report, professional_id: pro.trim(),
      });
      const doc: DocumentRow = { id: r.document_id, kind: "report", filename: r.filename, mime: "application/pdf", bytes: 0, uploaded_at: new Date().toISOString(), uploaded_by: null, uploaded_by_name: null, note_id: null };
      openSheet(<DocumentViewer doc={doc} />);
      const n = r.skipped.length;
      toast(n === 0 ? L(`Pacote criado (${r.pages} páginas)`, `Pack made (${r.pages} pages)`)
        : L(`Pacote criado. ${n} ${n === 1 ? "ficheiro fica" : "ficheiros ficam"} para juntar à parte — estão na 1.ª página.`, `Pack made. ${n} ${n === 1 ? "file needs" : "files need"} attaching separately — listed on page 1.`));
    } catch (e) { setErr(errText(e)); setBusy(false); }
  };

  return (
    <div className="sheet ls-sheet" role="dialog" aria-modal="true" aria-label={L("Pacote de reembolso", "Reimbursement Pack")}>
      <div className="sheet-body">
        <h2>{L("Pacote de reembolso", "Reimbursement Pack")}</h2>
        <p className="sub small">{name}</p>

        {pv.error ? <p className="err" role="alert">{pv.error}</p> : !d ? <div className="ls-loading"><div className="skel" /><div className="skel" style={{ width: "70%" }} /><div className="skel" style={{ width: "85%" }} /></div> : (
          <>
            <div className="ls-group">
              <label className="ls-row">
                <span className="ls-label">{L("Prescrição", "Prescription")}</span>
                <select className="ls-ctl" value={rx ?? ""} onChange={(e) => { setRx(e.target.value || null); setFrom(""); setTo(""); setEdited(false); }}>
                  <option value="">{L("Nenhuma", "None")}</option>
                  {d.prescriptions.map((r) => <option key={r.id} value={r.id}>{[r.doctor || L("Prescrição", "Prescription"), r.issued].join(" · ")}</option>)}
                </select>
              </label>
              <label className="ls-row">
                <span className="ls-label">{L("Entidade", "Payer")}</span>
                <input className="ls-ctl" value={payer} onChange={(e) => setPayer(e.target.value)} placeholder={L("ADSE, Médis…", "ADSE, Médis…")} maxLength={120} />
              </label>
              <div className="ls-row">
                <span className="ls-label">{L("Período", "Period")}</span>
                <span className="ls-dates">
                  <DateInput className="ls-ctl" value={from} max={to || undefined} onChange={(e) => setFrom(e.target.value)} aria-label={L("Desde", "From")} />
                  <span className="muted" aria-hidden="true">–</span>
                  <DateInput className="ls-ctl" value={to} min={from || undefined} onChange={(e) => setTo(e.target.value)} aria-label={L("Até", "To")} />
                </span>
              </div>
              <button className="ls-row ls-disclose" aria-expanded={showSessions} onClick={() => setShowSessions(!showSessions)} disabled={d.sessions.length === 0}>
                <span className="ls-label">{L("Sessões", "Sessions")}</span>
                <span className="ls-value">{d.sessions.length === 0 ? L("Nenhuma neste período", "None in this period") : L(`${d.sessions.length} realizadas`, `${d.sessions.length} done`)}</span>
                {d.sessions.length > 0 && <ChevronRight size={14} className="ls-chev" aria-hidden="true" />}
              </button>
              {showSessions && (
                <div className="ls-sub">
                  {d.sessions.map((s) => (
                    <div key={s.id} className="ls-subrow"><span className="tnum">{s.on} · {s.time}</span><span className="muted">{s.title || kindLabel(s.kind, lang)}{s.therapist ? ` · ${s.therapist}` : ""}</span></div>
                  ))}
                </div>
              )}
            </div>
            {rxRow && !rxRow.document_id && (
              <p className="ls-foot"><AlertTriangle size={12} />{L("Esta prescrição não tem digitalização, por isso não vai no pacote.", "This prescription has no scan, so it won’t be in the pack.")}</p>
            )}

            <div className="ls-head"><span>{L("Faturas", "Invoices")}</span>{chosen.length > 0 && <span className="tnum">{eur(total / 100, lang)}</span>}</div>
            {d.payments.length === 0 ? <p className="ls-foot" style={{ marginTop: 0 }}>{L("Sem pagamentos neste período.", "No payments in this period.")}</p> : (
              <div className="ls-group">
                {d.payments.map((p) => (
                  <label key={p.id} className="ls-row ls-check">
                    <input type="checkbox" checked={picked.has(p.id)} onChange={() => toggle(p.id)} />
                    <span className="ls-main">
                      <span>{p.invoice_number || L(`Recibo n.º ${p.number}`, `Receipt no. ${p.number}`)}</span>
                      <span className="ls-meta">{fmtDate(p.received_on, lang, { day: "numeric", month: "short" })}{!p.has_file && <> · <span className="ls-warn">{L("sem ficheiro", "no file")}</span></>}</span>
                    </span>
                    <span className="tnum">{eur(p.amount_cents / 100, lang)}</span>
                  </label>
                ))}
              </div>
            )}

            <div className="ls-head"><span>{L("Relatório clínico", "Clinical report")}</span></div>
            <div className="ls-group">
              <Grow className="ls-text" minRows={5} value={report} onChange={(e) => { setReport(e.target.value); setEdited(true); }} maxLength={8000} aria-label={L("Relatório clínico", "Clinical report")} />
              <label className="ls-row">
                <span className="ls-label">{d.signer}</span>
                <input className="ls-ctl" value={pro} onChange={(e) => setPro(e.target.value)} placeholder={L("N.º da cédula profissional", "Licence no.")} maxLength={40} />
              </label>
            </div>
          </>
        )}
        {err && <p className="err" role="alert" style={{ marginTop: 10 }}>{err}</p>}
      </div>
      <div className="sheet-foot">
        <span className="ls-status">{L("Fica guardado nos documentos do utente.", "Saved in the patient’s documents.")}</span>
        <button className="btn" onClick={cancelSheet}>{L("Cancelar", "Cancel")}</button>
        <button className="btn pri" onClick={make} disabled={!ready} aria-busy={busy}>{busy ? L("A criar…", "Making…") : L("Criar PDF", "Make PDF")}</button>
      </div>
    </div>
  );
}

// ───────── discharge letters ─────────

interface Letter {
  id: string; lang: "pt" | "en"; addressee: string; body: string; ai_drafted: boolean; created_by: string | null; created_by_name: string | null;
  created_at: string; updated_at: string; approved_by_name: string | null; approved_at: string | null; document_id: string | null;
}
interface AiStatus { available: boolean; model: string; url?: string; reason?: "offline" | "no_model" | "not_local" | "error" }

const GAP = /\[(a completar|to complete)\]/;

/** Write a discharge letter. It saves itself as a draft; the owner can ask the local AI for a draft and is the only one who approves. */
export function DischargeLetter({ pid, name, lang: patientLang }: { pid: string; name: string; lang?: "pt" | "en" }) {
  const { L, lang, role, me, closeSheet, cancelSheet, openSheet, toast, confirm } = useApp();
  const owner = role === "owner";
  const list = useApi<{ letters: Letter[]; professional_id: string }>(`/patients/${pid}/letters`);
  const ai = useApi<AiStatus>(owner ? "/ai/status" : null);
  const [lg, setLg] = useState<"pt" | "en">(patientLang ?? "pt");
  const [to, setTo] = useState("");
  const [body, setBody] = useState("");
  const [aiUsed, setAiUsed] = useState(false);
  const [ready, setReady] = useState(false);
  const [save, setSave] = useState<"idle" | "saving" | "saved">("idle");
  const [discharge, setDischarge] = useState(true);
  const [pro, setPro] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string>();
  const [asking, setAsking] = useState(false);
  const [draft, setDraft] = useState<{ state: "idle" | "working" | "done" | "error"; text?: string; message?: string }>({ state: "idle" });
  const box = useRef<HTMLTextAreaElement>(null);
  // Autosave state lives in refs so the save on close sees the latest text.
  const idRef = useRef<string | null>(null);
  const dirty = useRef(false);
  const latest = useRef({ lg, to, body, aiUsed });
  latest.current = { lg, to, body, aiUsed };

  const approved = (list.data?.letters ?? []).filter((x) => x.approved_at);
  const mine = (list.data?.letters ?? []).find((x) => !x.approved_at && (owner || x.created_by === me.id));

  // Open the latest draft, or start from the letter pre-filled from the file.
  useEffect(() => {
    if (!list.data || ready) return;
    setPro(list.data.professional_id);
    if (mine) {
      idRef.current = mine.id; setLg(mine.lang); setTo(mine.addressee); setBody(mine.body); setAiUsed(mine.ai_drafted); setSave("saved"); setReady(true);
    } else {
      api<{ addressee: string; body: string }>("GET", `/patients/${pid}/letters/template?lang=${lg}`)
        .then((t) => { setTo(t.addressee); setBody(t.body); setReady(true); })
        .catch((e) => setErr(errText(e)));
    }
  }, [list.data]); // eslint-disable-line react-hooks/exhaustive-deps

  const persist = async (): Promise<string | null> => {
    if (!dirty.current && idRef.current) return idRef.current;
    const c = latest.current;
    if (!c.body.trim()) return idRef.current;
    const payload = { lang: c.lg, addressee: c.to, body: c.body, ai_drafted: c.aiUsed };
    dirty.current = false;
    setSave("saving");
    if (idRef.current) await api("PATCH", `/letters/${idRef.current}`, payload);
    else idRef.current = (await api<{ id: string }>("POST", `/patients/${pid}/letters`, payload)).id;
    setSave("saved");
    return idRef.current;
  };
  // Save a moment after typing stops, and when the sheet closes.
  const edit = (f: () => void) => { f(); dirty.current = true; setSave("idle"); };
  useEffect(() => {
    if (!ready || !dirty.current) return;
    const t = setTimeout(() => { persist().catch((e) => setErr(errText(e))); }, 900);
    return () => clearTimeout(t);
  }, [to, body, lg, aiUsed]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => { if (dirty.current) persist().catch(() => undefined); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const switchLang = async (l: "pt" | "en") => {
    // Untouched pre-filled text follows the language; written text stays as it is.
    if (!idRef.current && !dirty.current) {
      const t = await api<{ addressee: string; body: string }>("GET", `/patients/${pid}/letters/template?lang=${l}`);
      setLg(l); setTo(t.addressee); setBody(t.body);
    } else edit(() => setLg(l));
  };
  /** Delete the saved draft: an alert with a 5-second hold button (owner, 8 Nov 2026). Recent typing is saved first,
   *  so Cancel keeps everything. */
  const discard = async () => {
    const lid = idRef.current;
    if (!lid) return;
    if (dirty.current) await persist().catch(() => undefined);
    dirty.current = false;
    confirm({
      title: L("Apagar este rascunho?", "Delete this draft?"), text: L("O rascunho da carta de alta é apagado.", "The discharge letter draft is deleted."),
      action: L("Apagar", "Delete"), destructive: true, hold: true,
      onConfirm: () => { api("DELETE", `/letters/${lid}`).then(() => toast(L("Rascunho apagado", "Draft deleted"))).catch((e) => toast(errText(e))); },
    });
  };
  const approve = async () => {
    setBusy(true); setErr(undefined);
    try {
      const lid = await persist();
      if (!lid) throw { message: L("Escreva a carta primeiro.", "Write the letter first.") };
      const r = await api<{ document_id: string | null; discharged: boolean }>("POST", `/letters/${lid}/approve`, { discharge, professional_id: pro.trim() });
      closeSheet();
      toast(!r.document_id ? L("Carta aprovada, mas o PDF não foi guardado. Abra a carta de novo para tentar outra vez.", "Letter approved, but the PDF wasn’t saved. Open the letter again to retry.")
        : r.discharged ? L("Carta aprovada. O utente teve alta.", "Letter approved. The patient is discharged.") : L("Carta aprovada", "Letter approved"));
    } catch (e) { setAsking(false); setErr(errText(e)); }
    finally { setBusy(false); }
  };

  const askAi = async () => {
    setDraft({ state: "working" });
    try { const r = await api<{ text: string }>("POST", `/patients/${pid}/letters/ai-draft`, { lang: lg }); setDraft({ state: "done", text: r.text }); }
    catch (e) { setDraft({ state: "error", message: errText(e) }); }
  };
  const useAi = () => { edit(() => { setBody(draft.text ?? ""); setAiUsed(true); }); setDraft({ state: "idle" }); };

  const gaps = useMemo(() => (body.match(new RegExp(GAP, "g")) ?? []).length, [body]);
  const nextGap = () => {
    const t = box.current; if (!t) return;
    const from = t.selectionEnd ?? 0;
    const after = body.slice(from).search(GAP);
    const at = after >= 0 ? from + after : body.search(GAP);
    if (at < 0) return;
    t.focus(); t.setSelectionRange(at, at + (body.slice(at).match(GAP)?.[0] ?? "").length);
  };

  // Approving can't be undone: the app's own alert, with the two choices that belong to that moment.
  if (asking) {
    return (
      <div className="alert frosted ls-alert" role="alertdialog" aria-modal="true" aria-labelledby="ap-title" aria-describedby="ap-text">
        <Mark />
        <h2 id="ap-title">{L("Aprovar a carta de alta?", "Approve the discharge letter?")}</h2>
        <p id="ap-text">{L("Fica guardada em PDF na ficha e já não pode ser alterada.", "It’s saved as a PDF in the file and can’t be changed afterwards.")}</p>
        <label className="check ls-alert-check"><input type="checkbox" checked={discharge} onChange={(e) => setDischarge(e.target.checked)} />{L("Dar alta ao utente", "Discharge the patient")}</label>
        <input className="input ls-alert-input" value={pro} onChange={(e) => setPro(e.target.value)} placeholder={L("N.º da cédula profissional (opcional)", "Licence no. (optional)")} maxLength={40} aria-label={L("N.º da cédula profissional", "Professional licence no.")} />
        {err && <p className="err" role="alert">{err}</p>}
        <div className="btns">
          <button className="btn wide pri" disabled={busy} aria-busy={busy} onClick={approve}>{L("Aprovar", "Approve")}</button>
          <button className="btn wide" autoFocus onClick={() => setAsking(false)}>{L("Cancelar", "Cancel")}</button>
        </div>
      </div>
    );
  }

  const aiOff = !!ai.data && !ai.data.available;
  const status = gaps > 0
    ? <span className="ls-status warn"><AlertTriangle size={12} />{L(`${gaps} ${gaps === 1 ? "parte" : "partes"} por completar`, `${gaps} ${gaps === 1 ? "part" : "parts"} to complete`)} · <button className="linkbtn" onClick={nextGap}>{L("Mostrar", "Show")}</button></span>
    : <span className="ls-status">{save === "saving" ? L("A guardar…", "Saving…") : save === "saved" ? (owner ? L("Rascunho guardado", "Draft saved") : L("Guardado para o proprietário aprovar", "Saved for the owner to approve")) : ""}
        {idRef.current && save !== "saving" && <> · <button className="linkbtn" onClick={discard}>{L("Apagar…", "Delete…")}</button></>}</span>;

  return (
    <div className="sheet wide ls-sheet" role="dialog" aria-modal="true" aria-label={L("Carta de alta", "Discharge Letter")}>
      <div className="sheet-body">
        <div className="ls-title">
          <div><h2>{L("Carta de alta", "Discharge Letter")}</h2><p className="sub small">{name}</p></div>
          {ready && <Seg label={L("Língua da carta", "Letter language")} value={lg} onChange={switchLang} options={[["pt", "PT"], ["en", "EN"]]} />}
        </div>

        {approved.length > 0 && (
          <div className="ls-group">
            {approved.map((x) => (
              <button key={x.id} className="ls-row ls-disclose" disabled={!x.document_id}
                onClick={() => x.document_id && openSheet(<DocumentViewer doc={{ id: x.document_id, kind: "letter", letter_id: x.id, filename: L("Carta de alta.pdf", "Discharge letter.pdf"), mime: "application/pdf", bytes: 0, uploaded_at: x.approved_at!, uploaded_by: null, uploaded_by_name: x.approved_by_name, note_id: null }} />)}>
                <FileText size={14} className="ls-ic" aria-hidden="true" />
                <span className="ls-label">{L("Carta aprovada", "Approved letter")}</span>
                <span className="ls-value">{fmtDate(x.approved_at!, lang)}</span>
                <ChevronRight size={14} className="ls-chev" aria-hidden="true" />
              </button>
            ))}
          </div>
        )}

        {!ready ? (!err && <div className="ls-loading"><div className="skel" /><div className="skel" style={{ width: "80%" }} /><div className="skel" style={{ width: "90%" }} /></div>) : (
          <>
            {owner && draft.state === "idle" && (
              <div className="ls-ai-line">
                <button className="btn sm ai" onClick={askAi} disabled={aiOff}><Sparkles />{L("Escrever com a IA local", "Draft with Local AI")}</button>
                {aiOff && <span className="small muted">{ai.data!.reason === "no_model"
                  ? L(`O modelo ${ai.data!.model} não está instalado no computador da IA. Escolha outro em Definições › IA local.`, `The model ${ai.data!.model} isn’t installed on the AI computer. Choose another in Settings › Local AI.`)
                  : L(`Sem resposta da IA local${ai.data!.url ? ` (${ai.data!.url.replace(/^http:\/\//, "")})` : ""}. Verifique se o computador está ligado, ou o endereço em Definições › IA local.`, `No answer from the local AI${ai.data!.url ? ` (${ai.data!.url.replace(/^http:\/\//, "")})` : ""}. Check the computer is on, or the address in Settings › Local AI.`)}</span>}
              </div>
            )}
            {draft.state !== "idle" && (
              <div className="ai-card ls-ai" aria-live="polite">
                <div className="ai-head"><Sparkles size={14} />{draft.state === "working"
                  ? L("A escrever a partir da anamnese, das notas e dos testes… pode demorar alguns minutos.", "Writing from the anamnesis, notes and test scores… this can take a few minutes.")
                  : draft.state === "error" ? L("A IA não conseguiu escrever o rascunho", "The AI couldn’t write the draft") : L("Rascunho da IA — leia antes de usar", "AI draft — read before using")}</div>
                {draft.state === "working" && <><div className="skel" style={{ width: "95%" }} /><div className="skel" style={{ width: "88%" }} /><div className="skel" style={{ width: "60%" }} /></>}
                {draft.state === "error" && <p className="small">{draft.message}</p>}
                {draft.state === "done" && <p className="ai-text">{draft.text}</p>}
                {draft.state !== "working" && (
                  <div className="ai-actions">
                    {draft.state === "done" && <button className="btn sm pri" onClick={useAi}>{L("Usar este texto", "Use This Text")}</button>}
                    <button className="btn sm" onClick={askAi}>{L("Tentar de novo", "Retry")}</button>
                    <button className="btn sm" onClick={() => setDraft({ state: "idle" })}>{L("Descartar", "Discard")}</button>
                  </div>
                )}
              </div>
            )}

            <div className="ls-paper">
              <input className="ls-to" value={to} onChange={(e) => edit(() => setTo(e.target.value))} maxLength={300} placeholder={L("Destinatário", "To")} aria-label={L("Destinatário", "To")} />
              <Grow inputRef={box} className="ls-body" minRows={10} value={body} onChange={(e) => edit(() => setBody(e.target.value))} maxLength={20000}
                placeholder={L("Escreva a carta…", "Write the letter…")} aria-label={L("Texto da carta", "Letter")} />
              {aiUsed && <p className="ls-ai-note"><Sparkles size={11} />{L("Começou com um rascunho da IA local", "Started from a local AI draft")}</p>}
            </div>
          </>
        )}
        {err && <p className="err" role="alert" style={{ marginTop: 10 }}>{err}</p>}
      </div>
      <div className="sheet-foot">
        {status}
        <button className="btn" onClick={cancelSheet}>{L("Fechar", "Close")}</button>
        {owner && <button className="btn pri" onClick={() => setAsking(true)} disabled={!ready || !body.trim() || gaps > 0 || busy}
          title={gaps > 0 ? L("Complete primeiro as partes marcadas", "Fill in the marked parts first") : undefined}>{L("Aprovar…", "Approve…")}</button>}
      </div>
    </div>
  );
}
