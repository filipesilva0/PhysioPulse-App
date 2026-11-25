// Patient documents: add files, open one inside the app, rename or re-file it.
// Files go to the patient's folder on the clinic server (never to a cloud service). Opening shows the file
// in the app; "Save a Copy…" is the only way it lands on the computer (Downloads), and the server logs both.
import { useEffect, useMemo, useState } from "react";
import { Download, FileSignature, FileText, Image as ImageIcon, Lock, Mail, X } from "lucide-react";
import { useApp } from "../lib/ctx";
import { api, download, isErr } from "../lib/api";
import { fileToBase64 } from "../lib/money";
import { DOC_KINDS, docKindLabel, fileSize, fmtDate, type DocKind, type DocumentFull, type DocumentRow } from "../lib/data";
import { Field, Sheet } from "./ui";

const MAX = 10 * 1024 * 1024;
const OK_TYPE = /^(application\/pdf|image\/(jpeg|png|heic|heif|webp))$/;
const okFile = (f: File) => OK_TYPE.test(f.type) || /\.(pdf|jpe?g|png|heic|webp)$/i.test(f.name);

/** A first guess at what a file is, from its name and type. */
function guess(f: File, reception: boolean): DocKind {
  if (reception) return "prescription";
  const n = f.name.toLowerCase();
  if (/credencial|prescri|receita/.test(n)) return "prescription";
  if (/rx|raio|rm\b|ressonan|ecografia|tac\b|mri|x-?ray|scan/.test(n)) return "imaging";
  if (/carta|letter/.test(n)) return "letter";
  if (f.type.startsWith("image/")) return "photo";
  return "report";
}

export const DocIcon = ({ d }: { d: Pick<DocumentRow, "kind" | "mime" | "letter_id"> }) =>
  d.kind === "note" ? <Lock /> : d.kind === "consent" ? <FileSignature /> : d.letter_id ? <Mail /> : d.mime.startsWith("image/") ? <ImageIcon /> : <FileText />;

/** Add one or more files to a patient (picked or dropped). Optionally links the first one to a prescription. */
export function AddDocuments({ patientId, patientName, files, linkPrescription }: { patientId: string; patientName: string; files: File[]; linkPrescription?: string }) {
  const { L, lang, role, toast } = useApp();
  const reception = role === "reception";
  const usable = files.filter((f) => okFile(f) && f.size <= MAX);
  const skipped = files.filter((f) => !usable.includes(f));
  const [kinds, setKinds] = useState<DocKind[]>(() => usable.map((f) => linkPrescription ? "prescription" : guess(f, reception)));
  const [done, setDone] = useState(0);
  const choices = DOC_KINDS.filter(([k]) => k !== "note" && k !== "consent" && (!reception || k === "prescription"));
  const n = usable.length;
  return (
    <Sheet title={linkPrescription ? L("Juntar digitalização", "Attach Scan") : n === 1 ? L("Juntar documento", "Add Document") : L(`Juntar ${n} documentos`, `Add ${n} Documents`)}
      sub={L(`Ficam na pasta de ${patientName} no servidor da clínica.`, `They go into ${patientName}’s folder on the clinic server.`)}
      action={done ? L(`A enviar ${done} de ${n}…`, `Uploading ${done} of ${n}…`) : n === 1 ? L("Juntar", "Add") : L(`Juntar ${n}`, `Add ${n}`)} disabled={!n}
      onAction={async () => {
        for (let i = 0; i < n; i++) {
          setDone(i + 1);
          try {
            const r = await api<{ id: string }>("POST", `/patients/${patientId}/documents`, { kind: kinds[i], filename: usable[i].name, data: await fileToBase64(usable[i]) });
            if (linkPrescription && i === 0) await api("PATCH", `/prescriptions/${linkPrescription}`, { document_id: r.id });
          } catch (e) {
            setDone(0);
            throw { message: `${usable[i].name}: ${isErr(e) ? e.message : String(e)}${i ? L(` (os ${i} anteriores já foram juntos)`, ` (the ${i} before it were added)`) : ""}` };
          }
        }
        toast(n === 1 ? L("Documento junto", "Document added") : L(`${n} documentos juntos`, `${n} documents added`));
      }}>
      {usable.length > 0 && (
        <div className="rows" style={{ marginBottom: 12 }}>
          {usable.map((f, i) => (
            <div key={i} className="row">
              <span className="doc-ic" aria-hidden="true">{f.type.startsWith("image/") ? <ImageIcon /> : <FileText />}</span>
              <span className="grow" style={{ minWidth: 0 }}><span className="t ell" style={{ display: "block" }}>{f.name}</span><span className="s">{fileSize(f.size, lang)}</span></span>
              {choices.length > 1
                ? <select className="input" style={{ width: "auto" }} aria-label={L(`Tipo de ${f.name}`, `Type of ${f.name}`)} value={kinds[i]} onChange={(e) => setKinds(kinds.map((k, j) => j === i ? e.target.value as DocKind : k))}>
                    {choices.map(([k]) => <option key={k} value={k}>{docKindLabel(k, lang)}</option>)}
                  </select>
                : <span className="small muted">{docKindLabel(choices[0][0], lang)}</span>}
            </div>
          ))}
        </div>
      )}
      {skipped.length > 0 && (
        <p className="err" role="alert">{L("Não é possível juntar: ", "Can’t add: ")}{skipped.map((f) => f.name).join(", ")}. {L("Use PDF ou fotografias (JPEG, PNG, HEIC) com menos de 10 MB.", "Use PDFs or photos (JPEG, PNG, HEIC) under 10 MB.")}</p>
      )}
      {reception && !linkPrescription && <p className="small muted">{L("A receção junta prescrições digitalizadas. Os outros documentos são juntos por um fisioterapeuta.", "Reception adds scanned prescriptions. A therapist adds other documents.")}</p>}
    </Sheet>
  );
}

/** Open a document inside the app (picture or PDF). Nothing is saved on the computer unless "Save a Copy…". */
export function DocumentViewer({ doc }: { doc: DocumentRow }) {
  const { L, lang, toast, closeSheet, cancelSheet } = useApp();
  const [full, setFull] = useState<DocumentFull | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => { api<DocumentFull>("GET", `/documents/${doc.id}`).then(setFull).catch((e) => setErr(isErr(e) ? e.message : String(e))); }, [doc.id]);
  // PDFs show in a frame from a blob: URL, freed when the sheet closes.
  const url = useMemo(() => {
    if (!full) return "";
    const bin = atob(full.data); const u8 = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
    return URL.createObjectURL(new Blob([u8], { type: full.mime }));
  }, [full]);
  useEffect(() => () => { if (url) URL.revokeObjectURL(url); }, [url]);
  const canShow = full && (full.mime === "application/pdf" || /^image\/(jpeg|png|webp)$/.test(full.mime));
  const save = () => download(doc.id, doc.filename, "document").then((w) => toast(L(`Guardado em ${w}`, `Saved to ${w}`))).catch((e) => toast(isErr(e) ? e.message : String(e)));
  return (
    <div className="sheet viewer" role="dialog" aria-modal="true" aria-label={doc.filename}>
      <div className="viewer-bar">
        <div style={{ minWidth: 0, flex: 1 }}>
          <h2 className="ell">{doc.filename}</h2>
          <p className="small muted ell">{docKindLabel(doc.kind, lang)} · {fmtDate(doc.uploaded_at, lang)}{doc.uploaded_by_name ? ` · ${doc.uploaded_by_name}` : ""} · {fileSize(doc.bytes, lang)}</p>
        </div>
        <button className="btn" onClick={save}><Download />{L("Guardar uma cópia…", "Save a Copy…")}</button>
        <button className="btn icon" onClick={cancelSheet} aria-label={L("Fechar", "Close")}><X /></button>
      </div>
      <div className="viewer-body">
        {err ? <p className="err" role="alert" style={{ margin: 20 }}>{err}</p>
          : !full ? <div className="skel" style={{ height: "100%", borderRadius: 0 }} aria-label={L("A abrir…", "Opening…")} />
          : !canShow ? <div className="viewer-none"><FileText /><p>{L("Este tipo de ficheiro não pode ser mostrado na app.", "This kind of file can’t be shown in the app.")}</p><p className="small muted">{L("Guarde uma cópia para o abrir noutra aplicação.", "Save a copy to open it in another app.")}</p></div>
          : full.mime === "application/pdf" ? <iframe title={doc.filename} src={url} />
          : <img src={url} alt={doc.filename} />}
      </div>
      {doc.kind === "note" && <p className="small muted viewer-foot"><Lock size={12} style={{ verticalAlign: -1 }} /> {L("Cópia da nota assinada, guardada no arquivo do utente. Não pode ser alterada nem removida.", "Copy of the signed note, kept in the patient’s archive. It can’t be changed or removed.")}</p>}
    </div>
  );
}

/** Rename a document or change what it is. */
export function DocumentDetails({ doc }: { doc: DocumentRow }) {
  const { L, lang, role, toast } = useApp();
  const dot = doc.filename.lastIndexOf(".");
  const ext = dot > 0 ? doc.filename.slice(dot) : "";
  const [name, setName] = useState(dot > 0 ? doc.filename.slice(0, dot) : doc.filename);
  const [kind, setKind] = useState<DocKind>(doc.kind);
  const choices = DOC_KINDS.filter(([k]) => k !== "note" && k !== "consent" && (role !== "reception" || k === "prescription"));
  return (
    <Sheet title={L("Informações do documento", "Document Info")} action={L("Guardar", "Save")} disabled={!name.trim()}
      onAction={async () => { await api("PATCH", `/documents/${doc.id}`, { kind, filename: name.trim() + ext }); toast(L("Documento atualizado", "Document updated")); }}>
      <Field label={L("Nome", "Name")}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}><input className="input" value={name} onChange={(e) => setName(e.target.value)} autoFocus /><span className="muted">{ext}</span></div>
      </Field>
      <Field label={L("Tipo", "Type")}>
        <select className="input" value={kind} onChange={(e) => setKind(e.target.value as DocKind)} disabled={choices.length < 2}>
          {choices.map(([k]) => <option key={k} value={k}>{docKindLabel(k, lang)}</option>)}
        </select>
      </Field>
    </Sheet>
  );
}
