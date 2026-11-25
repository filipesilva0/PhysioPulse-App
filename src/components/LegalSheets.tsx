// Legal tools on the patient file (owner decision, 7 Nov 2026):
// export the clinical record (owner and therapists), anonymise and delete (owner only).
// The server enforces the roles and checks the typed name again (server/src/routes/legal.rs).
import { useState } from "react";
import { useApp, eur } from "../lib/ctx";
import { api, downloadRecord, isErr } from "../lib/api";
import { useApi } from "../lib/data";
import { HoldButton, Mark, Sheet } from "./ui";

interface LegalCheck {
  number: number; name: string; anonymised: boolean;
  notes_signed: number; notes_draft: number; documents: number; tests: number; consents: number;
  upcoming: number; visits: number; payments: number; plans_active: number; owed_cents: number;
  /** Last visit, note or test (YYYY-MM-DD), or null. */
  last_contact: string | null;
}

const sameName = (a: string, b: string) => { const n = (x: string) => x.trim().split(/\s+/).join(" ").toLowerCase(); return a.trim() !== "" && n(a) === n(b); };
const today = () => { const d = new Date(); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };

/** Export the clinical record as a zip, straight to Downloads. */
export function ExportRecord({ pid, name, number }: { pid: string; name: string; number?: number }) {
  const { L, toast } = useApp();
  const [kind, setKind] = useState<"full" | "summary">("full");
  const file = `Processo clínico ${number ? `${String(number).padStart(4, "0")} ` : ""}${kind === "summary" ? "resumo " : ""}${today()}.zip`;
  return (
    <Sheet title={L("Exportar processo clínico", "Export Clinical Record")} sub={name} action={L("Exportar", "Export")}
      onAction={async () => { const where = await downloadRecord(pid, kind, file); toast(L(`Guardado em ${where}`, `Saved to ${where}`)); }}>
      <div className="stack" role="radiogroup" aria-label={L("O que exportar", "What to export")} style={{ marginBottom: 12 }}>
        <label className="check" style={{ alignItems: "flex-start" }}><input type="radio" name="exp" checked={kind === "full"} onChange={() => setKind("full")} style={{ marginTop: 3 }} />
          <span><b>{L("Completo", "Full")}</b><span className="muted"> — {L("tudo, incluindo o texto de cada nota assinada, todas as marcações e os ficheiros originais (documentos e PDFs das notas)", "everything, including the text of every signed note, every appointment and the original files (documents and note PDFs)")}</span></span></label>
        <label className="check" style={{ alignItems: "flex-start" }}><input type="radio" name="exp" checked={kind === "summary"} onChange={() => setKind("summary")} style={{ marginTop: 3 }} />
          <span><b>{L("Resumo", "Summary")}</b><span className="muted"> — {L("identificação, anamnese, testes, prescrições, consentimentos, totais de marcações e a lista de notas assinadas", "identity, anamnesis, tests, prescriptions, consents, appointment totals and the list of signed notes")}</span></span></label>
      </div>
      <p className="small muted">{L("Um ficheiro zip vai para a pasta Transferências. Contém dados de saúde: guarde-o e envie-o com cuidado. A exportação fica no registo de acessos.",
        "A zip file goes to your Downloads folder. It holds health data: keep and send it with care. The export is recorded in the access log.")}</p>
    </Sheet>
  );
}

/** "3 notas" / "1 note". */
function useCount() {
  const { lang } = useApp();
  return (n: number, pt: [string, string], en: [string, string]) => `${n} ${(lang === "pt" ? pt : en)[n === 1 ? 0 : 1]}`;
}

/** An alert in the app's own style (frosted, centered, app mark, stacked buttons), plus the name to type.
 *  Nothing happens until the name matches; Esc or Cancel closes it. */
function LegalAlert({ pid, name, number, title, action, text, note, check, run, done, warn }: {
  pid: string; name: string; number: number; title: string; action: string; warn?: boolean;
  text: (c: LegalCheck) => string; note?: (c: LegalCheck) => string | null; check: { data?: LegalCheck; error?: string };
  run: (confirm: string) => Promise<unknown>; done: string;
}) {
  const { L, lang, toast, openSheet, closeSheet, cancelSheet, go } = useApp();
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string>();
  const c = check.data;
  const ok = !!c && sameName(typed, name) && !busy;
  const go_ = async () => {
    if (!ok) return;
    setBusy(true); setErr(undefined);
    try { await run(typed); closeSheet(); go("patients"); toast(done); }
    catch (e) { setErr(isErr(e) ? e.message : String(e)); }
    finally { setBusy(false); }
  };
  const extra = c && note ? note(c) : null;
  return (
    <div className="alert frosted legal-alert" role="alertdialog" aria-modal="true" aria-labelledby="legal-title" aria-describedby="legal-text">
      <Mark />
      <h2 id="legal-title">{title}</h2>
      {check.error ? <p className="err">{check.error}</p> : !c ? <div className="skel" style={{ height: 48 }} /> : (
        <>
          <p id="legal-text">{text(c)}</p>
          {c.owed_cents > 0 && <p className="owe">{L(`Ainda deve ${eur(c.owed_cents / 100, lang)}. Se já pagou, registe o pagamento primeiro.`, `Still owes ${eur(c.owed_cents / 100, lang)}. If they’ve paid, record the payment first.`)}</p>}
          {extra && <p className={warn ? "owe" : undefined}>{extra}</p>}
          <input className="confirm" autoFocus value={typed} onChange={(e) => setTyped(e.target.value)}
            placeholder={name} autoComplete="off" spellCheck={false} aria-label={L("Escreva o nome do utente para confirmar", "Type the patient’s name to confirm")} />
          <p className="hint">{L("Escreva o nome e mantenha o botão premido 5 segundos. Não é possível desfazer.", "Type the name, then hold the button for 5 seconds. This can’t be undone.")}</p>
        </>
      )}
      {err && <p className="err" role="alert">{err}</p>}
      <div className="btns">
        <HoldButton label={action} disabled={!ok} onDone={go_} />
        <button className="btn wide" onClick={cancelSheet}>{L("Cancelar", "Cancel")}</button>
      </div>
      <button className="linkbtn" onClick={() => openSheet(<ExportRecord pid={pid} name={name} number={number} />)}>{L("Exportar processo primeiro…", "Export Record First…")}</button>
    </div>
  );
}

/** Owner: anonymise a patient. */
export function AnonymisePatient({ pid, name, number }: { pid: string; name: string; number: number }) {
  const { L } = useApp();
  const n = useCount();
  const check = useApi<LegalCheck>(`/patients/${pid}/legal`);
  return (
    <LegalAlert pid={pid} name={name} number={number} check={check}
      title={L(`Anonimizar ${name}?`, `Anonymise ${name}?`)} action={L("Anonimizar", "Anonymise")}
      text={(c) => L(
        `Nome, contactos, NIF, assinaturas${c.documents ? ` e ${n(c.documents, ["documento", "documentos"], ["", ""])}` : ""} são apagados${c.upcoming ? `, e ${n(c.upcoming, ["marcação futura é desmarcada", "marcações futuras são desmarcadas"], ["", ""])}` : ""}. A anamnese, as notas, os testes e os pagamentos ficam, sem identificar a pessoa.`,
        `Name, contacts, tax number, signatures${c.documents ? ` and ${n(c.documents, ["", ""], ["document", "documents"])}` : ""} are removed${c.upcoming ? `, and ${n(c.upcoming, ["", ""], ["upcoming appointment is", "upcoming appointments are"])} cancelled` : ""}. Anamnesis, notes, tests and payments stay, without identifying the person.`)}
      note={(c) => c.notes_signed ? L("O texto das notas fica como foi escrito.", "Note text stays as written.") : null}
      run={(confirm_name) => api("POST", `/patients/${pid}/anonymise`, { confirm_name })}
      done={L(`Utente n.º ${number} anonimizado`, `Patient no. ${number} anonymised`)} />
  );
}

/** Clinical records are kept 20 years after the last contact (LEGAL.md). Warn, don't block. */
const KEEP_YEARS = 20;

/** Owner: delete a patient. Payments stay, without a name (fiscal records). */
export function DeletePatient({ pid, name, number }: { pid: string; name: string; number: number }) {
  const { L, lang } = useApp();
  const n = useCount();
  const check = useApi<LegalCheck>(`/patients/${pid}/legal`);
  const keepWarning = (last: string | null) => {
    if (!last) return null;
    const until = new Date(`${last}T12:00:00`); until.setFullYear(until.getFullYear() + KEEP_YEARS);
    if (until <= new Date()) return null;
    const f = (d: Date) => d.toLocaleDateString(lang === "pt" ? "pt-PT" : "en-GB", { day: "numeric", month: "long", year: "numeric" });
    return L(`Último contacto a ${f(new Date(`${last}T12:00:00`))}. O processo clínico deve ser guardado ${KEEP_YEARS} anos depois do último contacto, até ${f(until)}.`,
      `Last contact on ${f(new Date(`${last}T12:00:00`))}. Clinical records should be kept for ${KEEP_YEARS} years after the last contact, until ${f(until)}.`);
  };
  return (
    <LegalAlert pid={pid} name={name} number={number} check={check} warn
      title={L(`Apagar ${name}?`, `Delete ${name}?`)} action={L("Apagar", "Delete")}
      text={(c) => {
        const notes = c.notes_signed + c.notes_draft;
        const parts = L(
          [notes ? n(notes, ["nota", "notas"], ["", ""]) : "", c.documents ? n(c.documents, ["documento", "documentos"], ["", ""]) : ""].filter(Boolean).join(", "),
          [notes ? n(notes, ["", ""], ["note", "notes"]) : "", c.documents ? n(c.documents, ["", ""], ["document", "documents"]) : ""].filter(Boolean).join(", "));
        return L(
          `A ficha inteira é apagada${parts ? `, com ${parts}` : ""} e todas as marcações.${c.payments ? ` ${c.payments === 1 ? "O pagamento fica" : `Os ${c.payments} pagamentos ficam`} nas Finanças, sem nome.` : ""}`,
          `The whole file is deleted${parts ? `, with ${parts}` : ""} and every appointment.${c.payments ? ` ${c.payments === 1 ? "The payment stays" : `The ${c.payments} payments stay`} in Finance, without a name.` : ""}`);
      }}
      note={(c) => keepWarning(c.last_contact)}
      run={(confirm_name) => api("POST", `/patients/${pid}/delete`, { confirm_name })}
      done={L(`Utente n.º ${number} apagado`, `Patient no. ${number} deleted`)} />
  );
}
