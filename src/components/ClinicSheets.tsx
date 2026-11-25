// Clinic-wide legal tools in the File menu (owner only; the server checks the role):
// the complaints log (ERS deadline of 10 working days) and "delete everything from one day".
// Server: server/src/routes/complaints.rs, server/src/routes/days.rs. Law: claude/LEGAL.md.
import { PatientPicker } from "./PatientPicker";
import { useEffect, useState } from "react";
import { AlertTriangle, ChevronRight } from "lucide-react";
import { useApp } from "../lib/ctx";
import { api, isErr } from "../lib/api";
import { fmtDate, useApi, ymd } from "../lib/data";
import { HoldButton, DateInput } from "./ui";

const errText = (e: unknown) => (isErr(e) ? e.message : String(e));

import { addWorkingDays, workingDaysBetween } from "../lib/workdays";

export const ERS_DAYS = 10;

// ───────── complaints log ─────────

type Source = "paper" | "electronic" | "other";
export interface Complaint {
  id: string; received_on: string; source: Source; book_ref: string; patient_id: string | null; patient_name: string | null; patient_number: number | null;
  complainant: string; summary: string; reply: string; replied_on: string | null; sent_to_ers_on: string | null; due_on: string;
  created_by_name: string | null; created_at: string; updated_at: string;
}

/** Done = sent to the ERS (paper book) or answered (electronic book, other ways). */
export const complaintDone = (c: Pick<Complaint, "source" | "sent_to_ers_on" | "replied_on">) => !!(c.source === "paper" ? c.sent_to_ers_on : c.replied_on);

function useSourceLabel() {
  const { L } = useApp();
  return (s: Source) => s === "paper" ? L("Livro em papel", "Paper book") : s === "electronic" ? L("Livro eletrónico", "Electronic book") : L("Outra via", "Other");
}

/** The status shown on a complaint: a badge tone and a short text. */
function useStatus() {
  const { L, lang } = useApp();
  return (c: Complaint): [string, string] => {
    if (complaintDone(c)) return ["ok", c.source === "paper"
      ? L(`Enviada à ERS a ${fmtDate(c.sent_to_ers_on!, lang, { day: "numeric", month: "short" })}`, `Sent to ERS ${fmtDate(c.sent_to_ers_on!, lang, { day: "numeric", month: "short" })}`)
      : L("Respondida", "Answered")];
    const left = workingDaysBetween(ymd(new Date()), c.due_on);
    if (left < 0) return ["bad", L(`Em atraso ${-left} ${-left === 1 ? "dia útil" : "dias úteis"}`, `${-left} working ${-left === 1 ? "day" : "days"} late`)];
    if (left === 0) return ["bad", L("Termina hoje", "Due today")];
    return [left <= 3 ? "warn" : "n", L(`${left === 1 ? "Falta 1 dia útil" : `Faltam ${left} dias úteis`}`, `${left} working ${left === 1 ? "day" : "days"} left`)];
  };
}

/** File › Complaints Log… The list, and one complaint at a time in the same sheet. */
export function ComplaintsLog({ open }: { open?: string | "new" }) {
  const { L, lang, closeSheet, cancelSheet } = useApp();
  const list = useApi<Complaint[]>("/complaints");
  const [editing, setEditing] = useState<string | "new" | null>(open ?? null);
  const source = useSourceLabel();
  const status = useStatus();
  const rows = list.data ?? [];
  const current = editing && editing !== "new" ? rows.find((c) => c.id === editing) : undefined;
  // A complaint that's gone (deleted elsewhere) drops back to the list.
  useEffect(() => { if (editing && editing !== "new" && list.data && !current) setEditing(null); }, [editing, list.data, current]);
  if (editing === "new" || (editing && current)) return <ComplaintForm c={current} onDone={() => setEditing(null)} />;
  const open_ = rows.filter((c) => !complaintDone(c));
  return (
    <div className="sheet wide ls-sheet" role="dialog" aria-modal="true" aria-label={L("Livro de reclamações", "Complaints Log")}>
      <div className="sheet-body">
        <h2>{L("Livro de reclamações", "Complaints Log")}</h2>
        <p className="sub small">{L("As reclamações do livro em papel têm de ser registadas na ERS, com a resposta, em 10 dias úteis.", "Paper-book complaints must be entered with the ERS, with the reply, within 10 working days.")}</p>
        {list.error ? <p className="err" role="alert">{list.error}</p> : !list.data ? <div className="ls-loading"><div className="skel" /><div className="skel" style={{ width: "70%" }} /></div>
          : rows.length === 0 ? <p className="muted" style={{ padding: "18px 4px 26px" }}>{L("Nenhuma reclamação registada.", "No complaints logged.")}</p> : (
          <div className="ls-group">
            {rows.map((c) => {
              const [tone, text] = status(c);
              return (
                <button key={c.id} className="ls-row ls-disclose" onClick={() => setEditing(c.id)}>
                  <span className="ls-main">
                    <span className="tnum">{fmtDate(c.received_on, lang)}</span>
                    <span className="ls-name">{c.complainant || c.patient_name || L("Sem nome", "No name")}</span>
                    <span className="ls-meta">{source(c.source)}{c.book_ref ? ` · ${c.book_ref}` : ""}</span>
                  </span>
                  <span className={`badge b-${tone}`}>{text}</span>
                  <ChevronRight size={14} className="ls-chev" aria-hidden="true" />
                </button>
              );
            })}
          </div>
        )}
      </div>
      <div className="sheet-foot">
        <span className="ls-status">{rows.length > 0 && (open_.length ? L(`${open_.length} em aberto`, `${open_.length} open`) : L("Todas tratadas", "All dealt with"))}</span>
        <button className="btn" onClick={cancelSheet}>{L("Fechar", "Close")}</button>
        <button className="btn pri" onClick={() => setEditing("new")}>{L("Nova reclamação…", "New Complaint…")}</button>
      </div>
    </div>
  );
}

function ComplaintForm({ c, onDone }: { c?: Complaint; onDone: () => void }) {
  const { L, lang, toast, confirm, openSheet } = useApp();
  const source = useSourceLabel();
  const [v, setV] = useState(() => ({
    received_on: c?.received_on ?? ymd(new Date()), source: (c?.source ?? "paper") as Source, book_ref: c?.book_ref ?? "", patient_id: c?.patient_id ?? "",
    complainant: c?.complainant ?? "", summary: c?.summary ?? "", reply: c?.reply ?? "", replied_on: c?.replied_on ?? "", sent_to_ers_on: c?.sent_to_ers_on ?? "",
  }));
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string>();
  const set = (k: keyof typeof v) => (e: { target: { value: string } }) => setV({ ...v, [k]: e.target.value });
  const due = v.received_on ? addWorkingDays(v.received_on, ERS_DAYS) : "";
  const left = due ? workingDaysBetween(ymd(new Date()), due) : 0;
  const done = complaintDone({ source: v.source, sent_to_ers_on: v.sent_to_ers_on || null, replied_on: v.replied_on || null });
  const save = async () => {
    setBusy(true); setErr(undefined);
    try {
      const body = { ...v, patient_id: v.patient_id || null, replied_on: v.replied_on || null, sent_to_ers_on: v.sent_to_ers_on || null };
      if (c) await api("PATCH", `/complaints/${c.id}`, body); else await api("POST", "/complaints", body);
      toast(c ? L("Reclamação guardada", "Complaint saved") : L(`Reclamação registada. Prazo: ${fmtDate(due, lang)}`, `Complaint logged. Due ${fmtDate(due, lang)}`));
      onDone();
    } catch (e) { setErr(errText(e)); setBusy(false); }
  };
  const remove = () => confirm({
    title: L("Apagar esta reclamação?", "Delete this complaint?"),
    text: L("Use só para uma reclamação registada por engano. As reclamações devem ser guardadas pelo menos 3 anos.", "Only for a complaint logged by mistake. Complaints should be kept for at least 3 years."),
    action: L("Apagar", "Delete"), destructive: true, hold: true,
    // The alert replaces the sheet; go back to the log afterwards.
    onConfirm: () => { api("DELETE", `/complaints/${c!.id}`).then(() => { toast(L("Reclamação apagada", "Complaint deleted")); openSheet(<ComplaintsLog />); }).catch((e) => toast(errText(e))); },
  });
  return (
    <div className="sheet wide ls-sheet" role="dialog" aria-modal="true" aria-label={c ? L("Reclamação", "Complaint") : L("Nova reclamação", "New Complaint")}>
      <div className="sheet-body">
        <h2>{c ? L("Reclamação", "Complaint") : L("Nova reclamação", "New Complaint")}</h2>
        <p className="sub small">{L("Fica só no servidor da clínica. Só o proprietário a vê.", "Kept on the clinic server only. Only the owner sees it.")}</p>
        <div className="ls-head"><span>{L("Reclamação", "Complaint")}</span></div>
        <div className="ls-group">
          <label className="ls-row"><span className="ls-label">{L("Recebida a", "Received")}</span>
            <DateInput className="ls-ctl" value={v.received_on} max={ymd(new Date())} onChange={set("received_on")} style={{ maxWidth: 150 }} /></label>
          <label className="ls-row"><span className="ls-label">{L("Origem", "Source")}</span>
            <select className="ls-ctl" value={v.source} onChange={set("source")} style={{ maxWidth: 200 }}>
              {(["paper", "electronic", "other"] as Source[]).map((s) => <option key={s} value={s}>{source(s)}</option>)}
            </select></label>
          <label className="ls-row"><span className="ls-label">{v.source === "paper" ? L("N.º da folha", "Sheet no.") : L("Referência", "Reference")}</span>
            <input className="ls-ctl" value={v.book_ref} onChange={set("book_ref")} maxLength={60} placeholder={L("Opcional", "Optional")} /></label>
          <div className="ls-row"><span className="ls-label">{L("Utente", "Patient")}</span>
            <span className="ls-ctl" style={{ flex: 1 }}><PatientPicker value={v.patient_id} label={L("Utente", "Patient")} emptyLabel={L("Nenhum", "None")}
              placeholder={L("Nenhum (não é utente) — procure para ligar", "None (not a patient) — search to link")}
              onChange={(p) => setV((x) => ({ ...x, patient_id: p?.id ?? "", complainant: x.complainant || p?.name || "" }))} /></span></div>
          <label className="ls-row"><span className="ls-label">{L("Quem reclama", "Complainant")}</span>
            <input className="ls-ctl" value={v.complainant} onChange={set("complainant")} maxLength={120} /></label>
          <textarea className="ls-text" rows={3} value={v.summary} onChange={set("summary")} placeholder={L("Resumo do que foi reclamado", "Summary of the complaint")} aria-label={L("Resumo", "Summary")} style={{ borderTop: ".5px solid var(--line)" }} />
        </div>
        <div className="ls-head"><span>{L("Resposta", "Reply")}</span>
          {due && !done && <span className={left < 0 ? "ls-warn" : ""}>{L(`Prazo: ${fmtDate(due, lang)}`, `Due ${fmtDate(due, lang)}`)}</span>}</div>
        <div className="ls-group">
          <textarea className="ls-text" rows={3} value={v.reply} onChange={set("reply")} aria-label={L("Resposta", "Reply")}
            placeholder={L("O que a clínica respondeu: se aceita os factos e, se não, porquê", "What the clinic answered: whether it accepts the facts and, if not, why")} />
          <label className="ls-row"><span className="ls-label">{L("Respondida a", "Answered")}</span>
            <DateInput className="ls-ctl" value={v.replied_on} min={v.received_on} onChange={set("replied_on")} style={{ maxWidth: 150 }} /></label>
          {v.source === "paper" && <label className="ls-row"><span className="ls-label">{L("Registada na ERS a", "Entered with ERS")}</span>
            <DateInput className="ls-ctl" value={v.sent_to_ers_on} min={v.received_on} onChange={set("sent_to_ers_on")} style={{ maxWidth: 150 }} /></label>}
        </div>
        {v.source === "paper" && !v.sent_to_ers_on && <p className="ls-foot"><AlertTriangle size={12} />{L("Registe-a no portal da ERS (SGREC) com a resposta e envie o original.", "Enter it on the ERS portal (SGREC) with the reply and send the original.")}</p>}
        {err && <p className="err" role="alert">{err}</p>}
      </div>
      <div className="sheet-foot">
        <span className="ls-status">{c && <button className="linkbtn danger" onClick={remove}>{L("Apagar…", "Delete…")}</button>}</span>
        <button className="btn" onClick={onDone}>{L("Cancelar", "Cancel")}</button>
        <button className="btn pri" disabled={busy || !v.received_on} aria-busy={busy} onClick={save}>{c ? L("Guardar", "Save") : L("Registar", "Log Complaint")}</button>
      </div>
    </div>
  );
}

// ───────── delete everything from one day ─────────

interface DayCheck {
  day: string; appointments: number; blocks: number; attendees: number; notes_signed: number; notes_draft: number;
  tests: number; documents: number; payments_kept: number; patients: number;
}

/** File › Delete a Day… Owner only. What goes and what stays, then a 5-second hold. */
export function DeleteDay() {
  const { L, lang, closeSheet, cancelSheet, toast } = useApp();
  const [day, setDay] = useState(() => ymd(new Date()));
  const check = useApi<DayCheck>(day ? `/days/${day}` : null);
  const [err, setErr] = useState<string>();
  const c = check.data && check.data.day === day ? check.data : undefined;
  const n = (k: number, pt: [string, string], en: [string, string]) => `${k} ${(lang === "pt" ? pt : en)[k === 1 ? 0 : 1]}`;
  const notes = c ? c.notes_signed + c.notes_draft : 0;
  const total = c ? c.appointments + c.blocks + notes + c.tests + c.documents : 0;
  const rows: [string, string][] = c ? ([
    [L("Marcações", "Appointments"), c.appointments ? `${c.appointments}${c.attendees ? L(` · ${c.attendees} inscritos em aulas`, ` · ${c.attendees} in classes`) : ""}` : ""],
    [L("Bloqueios", "Blocked time"), c.blocks ? String(c.blocks) : ""],
    [L("Notas de evolução", "Progress notes"), notes ? `${notes}${c.notes_signed ? L(` · ${c.notes_signed} assinada${c.notes_signed === 1 ? "" : "s"}, com PDF`, ` · ${c.notes_signed} signed, with PDF`) : ""}` : ""],
    [L("Testes", "Tests"), c.tests ? String(c.tests) : ""],
    [L("Documentos adicionados", "Documents added"), c.documents ? String(c.documents) : ""],
  ] as [string, string][]).filter((r) => r[1]) : [];
  const run = async () => {
    setErr(undefined);
    try {
      await api("POST", `/days/${day}/delete`, { confirm: day });
      closeSheet();
      toast(L(`${fmtDate(day, lang, { day: "numeric", month: "long", year: "numeric" })} apagado`, `${fmtDate(day, lang, { day: "numeric", month: "long", year: "numeric" })} deleted`));
    } catch (e) { setErr(errText(e)); }
  };
  return (
    <div className="sheet ls-sheet" role="dialog" aria-modal="true" aria-label={L("Apagar um dia", "Delete a Day")}>
      <div className="sheet-body">
        <h2>{L("Apagar um dia", "Delete a Day")}</h2>
        <p className="sub small">{L("Apaga tudo o que foi marcado ou escrito nesse dia. Os documentos fiscais ficam, como a lei exige.", "Deletes everything booked or written that day. Tax records stay, as the law requires.")}</p>
        <div className="ls-group">
          <label className="ls-row"><span className="ls-label">{L("Dia", "Day")}</span>
            <DateInput className="ls-ctl" value={day} onChange={(e) => setDay(e.target.value)} style={{ maxWidth: 160 }} /></label>
        </div>
        {check.error ? <p className="err" role="alert">{check.error}</p> : !c ? <div className="ls-loading"><div className="skel" /><div className="skel" style={{ width: "60%" }} /></div> : total === 0 ? (
          <p className="muted" style={{ padding: "4px 4px 18px" }}>{L("Não há nada para apagar neste dia.", "There’s nothing to delete on this day.")}</p>
        ) : (
          <>
            <div className="ls-head"><span>{L("É apagado", "Deleted")}</span><span>{n(c.patients, ["utente", "utentes"], ["patient", "patients"])}</span></div>
            <div className="ls-group">
              {rows.map(([k, val]) => <div key={k} className="ls-row"><span className="ls-label">{k}</span><span className="ls-value tnum">{val}</span></div>)}
            </div>
            <div className="ls-head"><span>{L("Fica", "Kept")}</span></div>
            <div className="ls-group">
              <div className="ls-row"><span className="ls-label">{L("Pagamentos e faturas", "Payments and invoices")}</span><span className="ls-value tnum">{c.payments_kept ? String(c.payments_kept) : L("Nenhum", "None")}</span></div>
              <div className="ls-row"><span className="ls-label">{L("Também ficam", "Also kept")}</span><span className="ls-value">{L("utentes, consentimentos, cartas, despesas, registo de acessos", "patients, consents, letters, expenses, access log")}</span></div>
            </div>
            {c.notes_signed > 0 && <p className="ls-foot"><AlertTriangle size={12} />{L("Notas assinadas fazem parte do processo clínico. Apague-as só se foram feitas por engano.", "Signed notes are part of the clinical record. Delete them only if they were made by mistake.")}</p>}
          </>
        )}
        {err && <p className="err" role="alert">{err}</p>}
      </div>
      <div className="sheet-foot">
        <span className="ls-status">{total > 0 && L("Não é possível desfazer.", "This can’t be undone.")}</span>
        <button className="btn" onClick={cancelSheet} autoFocus>{L("Cancelar", "Cancel")}</button>
        <HoldButton label={L("Apagar o dia", "Delete Day")} wide={false} disabled={!c || total === 0} onDone={run} />
      </div>
    </div>
  );
}
