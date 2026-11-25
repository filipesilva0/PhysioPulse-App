import { useEffect, useMemo, useRef, useState } from "react";
import {
  CalendarPlus, Check, ChevronDown, ChevronLeft, ChevronRight, ClipboardList, FileDown, FileSignature, FileText, History, Lock,
  MoreHorizontal, Mic, PenLine, Globe, Plus, Receipt, Search, Sparkles, Trash2, Upload, UserPlus, X, Pill, Package, Eye, EyeOff, Mail, UserPen, Users, TrendingDown, TriangleAlert,
} from "lucide-react";
import { useApp, eur } from "../lib/ctx";
import { c2e, methodLabel, owedKey, owedLabel, planName, planStatusLabel, type Payment, type PatientMoney, type Plan } from "../lib/money";
import { InvoiceSheet, InvoiceViewer, SellPlan, VoidPayment } from "../components/MoneySheets";
import { openMenu } from "../components/menus";
import { api, download, isErr } from "../lib/api";
import {
  useApi, age, fmtDate, hm, toneFor, blockSummary, REGIONS, SYMPTOMS, RED_FLAGS, CONSENT_KINDS, REFUSABLE, NOTE_TEMPLATES, TEST_LIBRARY, testName,
  regionLabel, symptomLabel, consentLabel, kindLabel, statusLabel, ymd, docKindLabel, fileSize, DOC_ACCEPT, serverMade,
  type Anamnesis, type Block, type DocKind, type DocumentRow, type ConsentKind, type ConsentRow, type ConsentText, type ConsentFull, type Note, type PatientFile,
  type PatientRow, type Prescription, type Region, type TestResult, type Therapist,
} from "../lib/data";
import { Layout } from "../components/Layout";
import { Avatar, Badge, Empty, Field, Head, Seg, Sheet, Sparkline } from "../components/ui";
import { AddDocuments, DocIcon, DocumentDetails, DocumentViewer } from "../components/DocSheets";
import { NewAppointment, NewPatient, PatientDetails, RecordPayment, AccessLog, AddPrescription } from "../components/Sheets";
import { RecordTest, TestHistory } from "../components/TestForms";
import { SUB_LABEL, tx } from "../lib/forms";
import { DischargeLetter, ReimbursementPack } from "../components/LetterSheets";
import { AnonymisePatient, DeletePatient, ExportRecord } from "../components/LegalSheets";
import { AiCard, aiOffText, useAiStatus, type AiState } from "../components/Ai";
import { DraftCard, RecordSheet, Recordings, useRec, type RecordingDraft } from "../components/Recording";
import { PortalAccess } from "../components/PortalSheets";
import { anamnesisDrafts, noteDrafts, type NoteDraft } from "../lib/drafts";

/** Another page can ask for a tab when it opens a file (Today › A seguir › Nota de evolução). */
export const patientNav: { tab: Tab | null } = { tab: null };
export type Tab = "summary" | "anamnese" | "notes" | "tests" | "rx" | "consents" | "docs" | "pay";

export default function Patients() {
  const { L, lang, role, patientId, openPatient, openSheet, setPatientName, go } = useApp();
  const [q, setQ] = useState("");
  const [dq, setDq] = useState("");
  useEffect(() => { const t = setTimeout(() => setDq(q.trim()), 250); return () => clearTimeout(t); }, [q]);
  const [filter, setFilter] = useState<"all" | "booked">("all");
  const clinical = role !== "reception";
  const allowed: Tab[] = clinical ? ["summary", "anamnese", "notes", "tests", "rx", "consents", "docs", "pay"] : ["summary", "rx", "consents", "docs", "pay"];
  const [tab, setTab] = useState<Tab>(allowed[0]);
  useEffect(() => { if (!allowed.includes(tab)) setTab(allowed[0]); }, [role]); // eslint-disable-line

  // Paged (audit M7): 100 at a time, "Show More" adds the next 100. One extra row says whether there are more.
  const PAGE = 100;
  const [pages, setPages] = useState(1);
  useEffect(() => setPages(1), [dq]);
  const list = useApi<PatientRow[]>(`/patients?limit=${pages * PAGE + 1}${dq ? `&q=${encodeURIComponent(dq)}` : ""}`, { keep: true });
  const more = (list.data?.length ?? 0) > pages * PAGE;
  const { data: ths } = useApi<Therapist[]>("/therapists");
  const rows = (list.data ?? []).slice(0, pages * PAGE).filter((x) => filter === "all" || x.next_at);
  const file = useApi<PatientFile>(patientId ? `/patients/${patientId}` : null);
  const f = file.data && file.data.patient.id === patientId ? file.data : undefined;
  useEffect(() => { setPatientName(f?.patient.name ?? ""); }, [f?.patient.name]); // eslint-disable-line react-hooks/exhaustive-deps
  // Every file opens on its summary (23 Nov 2026 overhaul), not on a form.
  useEffect(() => { setTab(patientNav.tab && allowed.includes(patientNav.tab) ? patientNav.tab : "summary"); patientNav.tab = null; }, [patientId]); // eslint-disable-line react-hooks/exhaustive-deps

  const tabName: Record<Tab, string> = {
    summary: L("Resumo", "Summary"), anamnese: L("Anamnese", "Anamnesis"), notes: L("Evolução", "Progress"), tests: L("Testes", "Tests"), rx: L("Prescrições", "Prescriptions"),
    consents: L("Consentimentos", "Consents"), docs: L("Documentos", "Documents"), pay: L("Pagamentos", "Payments"),
  };
  const when = (iso: string) => `${fmtDate(iso, lang, { weekday: "short", day: "numeric", month: "short" })}, ${hm(iso)}`;

  // ── The list: every patient, searchable. Opening one shows the file (Back returns here). ──
  if (!patientId) {
    const total = Math.min(list.data?.length ?? 0, pages * PAGE);
    return (
      <Layout label={L("Utentes", "Patients")}>
        <div className="wrap">
          <Head title={L("Utentes", "Patients")} sub={list.data && !dq ? more ? L(`Mais de ${total} utentes`, `More than ${total} patients`) : L(`${total} utente${total === 1 ? "" : "s"}`, `${total} patient${total === 1 ? "" : "s"}`) : undefined}>
            <button className="btn pri" onClick={() => openSheet(<NewPatient />)}><UserPlus />{L("Novo utente…", "New Patient…")}</button>
          </Head>
          {list.data && total === 0 && !dq ? (
            <Empty icon={<Users />} title={L("Ainda não há utentes", "No patients yet")} text={L("Registe o primeiro utente para abrir a ficha, marcar sessões e recolher consentimentos.", "Register the first patient to open their file, book sessions and collect consents.")}
              action={<button className="btn pri" onClick={() => openSheet(<NewPatient />)}><UserPlus />{L("Novo utente…", "New Patient…")}</button>} />
          ) : (
            <>
              <div className="toolrow">
                <label className="search"><Search /><input placeholder={L("Nome, n.º, telefone ou NIF", "Name, number, phone or NIF")} value={q} onChange={(e) => setQ(e.target.value)} aria-label={L("Procurar utentes", "Search patients")} /></label>
                <Seg label={L("Mostrar", "Show")} value={filter} onChange={setFilter} options={[["all", L("Todos", "All")], ["booked", L("Com marcação", "Booked")]]} />
              </div>
              {list.error && <p className="err">{list.error}</p>}
              {!list.data && <div className="skel" style={{ height: 160 }} />}
              {list.data && rows.length === 0 && <p className="small muted">{dq ? L(`Nenhum utente encontrado para “${dq}”.`, `No patients found for “${dq}”.`) : L("Ninguém com marcação.", "No one is booked.")}</p>}
              {rows.length > 0 && (
                <div className="rows plist">
                  {rows.map((x) => (
                    <button key={x.id} className="row" onClick={() => openPatient(x.id)}>
                      <Avatar name={x.name} color={toneFor(ths, x.therapist_id)} />
                      <span className="grow"><span className="t" style={{ display: "block" }}>{x.name}</span><span className="s" style={{ display: "block" }}>n.º {x.number}{x.phone ? ` · ${x.phone}` : ""}</span></span>
                      <span className="nx s">{x.next_at ? <>{L("Próxima", "Next")} {when(x.next_at)}</> : <span className="muted">{L("Sem marcações", "Nothing booked")}</span>}</span>
                      <ChevronRight size={14} className="chev" />
                    </button>
                  ))}
                </div>
              )}
              {more && (
                <div style={{ display: "flex", justifyContent: "center", gap: 10, alignItems: "center", marginTop: 12 }}>
                  <span className="small muted">{L(`A mostrar os primeiros ${pages * PAGE}.`, `Showing the first ${pages * PAGE}.`)}</span>
                  <button className="btn" disabled={list.loading} aria-busy={list.loading} onClick={() => setPages((n) => n + 1)}>{L("Mostrar mais", "Show More")}</button>
                </div>
              )}
            </>
          )}
        </div>
      </Layout>
    );
  }

  const back = <button className="backlink" onClick={() => go("patients")}><ChevronLeft size={14} />{L("Utentes", "Patients")}</button>;
  if (!f) {
    return (
      <Layout label={L("Utentes", "Patients")}>
        <div className="wrap"><div className="crumb">{back}</div>{file.error && !file.loading ? <Empty icon={<Users />} title={L("Não foi possível abrir a ficha", "Couldn’t open the file")} text={file.error} />
          : <><div className="skel" style={{ width: 280, height: 26, margin: "12px 0 14px" }} /><div className="skel" style={{ width: "70%" }} /><div className="skel" style={{ width: "50%" }} /></>}</div>
      </Layout>
    );
  }

  const p = f.patient;
  const a = age(p.birth_date);
  const now = new Date().toISOString();
  const next = f.appointments.filter((x) => x.status === "booked" && x.starts_at > now).reverse();
  const last = f.appointments.find((x) => x.status === "done");
  const missing = (["treatment", "data"] as ConsentKind[]).filter((k) => !f.consents.some((c) => c.kind === k && c.state !== "revoked" && c.state !== "expired" && (k !== "treatment" || c.granted)));
  const lowRx = f.prescriptions.filter((x) => x.remaining > 0 && (x.remaining <= 2 || (x.valid_until && x.valid_until <= ymd(new Date(Date.now() + 14 * 864e5)))));

  return (
    <Layout label={L("Utentes", "Patients")}>
      <div className="wrap">
        <div className="crumb">{back}</div>
        <Head title={<span style={{ display: "flex", gap: 12, alignItems: "center" }}><Avatar name={p.name} size="md" color={toneFor(ths, p.therapist_id)} />{p.name}</span>}
          sub={<span style={{ marginLeft: 52, display: "block" }}>n.º {p.number}{a != null ? ` · ${a} ${L("anos", "years")}` : ""}{clinical && f.anamnesis?.main_complaint ? ` · ${f.anamnesis.main_complaint.split(/[.,;]/)[0]}` : ""}</span>}>
          <button className="btn" disabled={!!p.anonymised_at} onClick={() => openSheet(<NewAppointment patientId={p.id} prescriptions={f.prescriptions} />)}><CalendarPlus />{L("Nova marcação…", "New Appointment…")}</button>
          {clinical ? <button className="btn pri" onClick={() => { setTab("notes"); setTimeout(() => { const t = document.querySelector<HTMLTextAreaElement>(".main textarea"); t?.scrollIntoView({ block: "center", behavior: "smooth" }); t?.focus({ preventScroll: true }); }, 60); }}><PenLine />{L("Nota de evolução", "Progress Note")}</button>
            : <button className="btn pri" onClick={() => setTab("consents")}><FileSignature />{L("Consentimentos", "Consents")}</button>}
          <PatientActions f={f} />
        </Head>
        <dl className="meta-line">
          <div><dt>{L("Próxima", "Next")}</dt><dd>{next[0] ? `${when(next[0].starts_at)}${next.length > 1 ? L(` (+${next.length - 1})`, ` (+${next.length - 1})`) : ""}` : "—"}</dd></div>
          {last && <div><dt>{L("Última", "Last")}</dt><dd>{fmtDate(last.starts_at, lang, { day: "numeric", month: "short" })}</dd></div>}
          <div><dt>{L("Telemóvel", "Mobile")}</dt><dd>{p.phone || "—"}</dd></div>
          {p.email && <div><dt>Email</dt><dd>{p.email}</dd></div>}
          <div><dt>NIF</dt><dd>{p.nif || "—"}</dd></div>
          <div><dt>{L("Fisioterapeuta", "Therapist")}</dt><dd>{p.therapist_name || "—"}</dd></div>
          {p.guardian_name && <div><dt>{L("Encarregado", "Guardian")}</dt><dd>{p.guardian_name}{p.guardian_phone ? `, ${p.guardian_phone}` : ""}</dd></div>}
        </dl>
        {p.admin_note && <p className="small muted" style={{ margin: "-6px 0 14px" }}>{p.admin_note}</p>}
        {p.anonymised_at && <div className="notices"><div className="notice info" role="note"><EyeOff />{L(`Anonimizado a ${fmtDate(p.anonymised_at, lang)}. A ficha já não identifica a pessoa e não pode receber novos contactos, documentos ou marcações.`, `Anonymised on ${fmtDate(p.anonymised_at, lang)}. The file no longer identifies the person and can’t take new contacts, documents or appointments.`)}</div></div>}
        {(missing.length > 0 || lowRx.length > 0) && (
          <div className="notices">
            {missing.map((k) => <button key={k} className="notice" onClick={() => setTab("consents")}><FileSignature />{L(`Falta assinar: ${consentLabel(k, lang)}`, `Not signed yet: ${consentLabel(k, lang)}`)}</button>)}
            {lowRx.map((x) => <button key={x.id} className="notice info" onClick={() => setTab("rx")}><Pill />{L(`Prescrição: ${x.remaining === 1 ? "resta 1 sessão" : `restam ${x.remaining} sessões`}`, `Prescription: ${x.remaining} session${x.remaining === 1 ? "" : "s"} left`)}{x.valid_until ? L(`, válida até ${fmtDate(x.valid_until, lang)}`, `, valid until ${fmtDate(x.valid_until, lang)}`) : ""}</button>)}
          </div>
        )}
        <div className="tabs" role="tablist" aria-label={L("Ficha do utente", "Patient file")}>
          {allowed.map((k) => {
            // A dot on tabs with unsaved typed work (kept while you look elsewhere; audit C2).
            const unsaved = (k === "anamnese" && anamnesisDrafts.has(p.id)) || (k === "notes" && !!noteDrafts.get(p.id) && noteDrafts.get(p.id)!.saved !== noteJson(noteDrafts.get(p.id)!));
            return <button key={k} role="tab" aria-selected={tab === k} onClick={() => setTab(k)}>{tabName[k]}{unsaved && <span className="tab-dot" aria-label={L("alterações por guardar", "unsaved changes")} />}</button>;
          })}
        </div>
        {!clinical && (
          <p className="small muted" style={{ marginBottom: 14, display: "flex", gap: 6, alignItems: "center" }}><Lock size={13} />{L("Só profissionais de saúde veem a informação clínica.", "Only health professionals can see clinical information.")}</p>
        )}
        {tab === "summary" && <Summary f={f} clinical={clinical} go={setTab} />}
        {tab === "anamnese" && <Anamnese key={p.id} pid={p.id} an={f.anamnesis ?? null} />}
        {tab === "notes" && <Notes key={p.id} pid={p.id} name={p.name} notes={f.notes ?? []} docs={f.documents ?? []} todayAppt={f.appointments.find((x) => x.status !== "cancelled" && ymd(new Date(x.starts_at)) === ymd(new Date()))?.id ?? null} />}
        {tab === "tests" && <Tests pid={p.id} tests={f.tests ?? []} />}
        {tab === "rx" && <Prescriptions pid={p.id} name={p.name} list={f.prescriptions} docs={f.documents ?? []} />}
        {tab === "consents" && <Consents key={p.id} f={f} />}
        {tab === "docs" && <Documents f={f} />}
        {tab === "pay" && <Payments pid={p.id} />}
      </div>
    </Layout>
  );
}

/** Resumo (23 Nov 2026 overhaul): what you need before the session, read-only, each part leading to its tab.
 *  Built only from the file the server already sent (no extra request, no extra access-log entry). */
function Summary({ f, clinical, go }: { f: PatientFile; clinical: boolean; go: (t: Tab) => void }) {
  const { L, lang } = useApp();
  const now = new Date().toISOString();
  const an = f.anamnesis;
  const flags = (an?.red_flags ?? []).map((k) => RED_FLAGS.find((r) => r[0] === k)?.[1][lang === "pt" ? 0 : 1] ?? k);
  const notes = (f.notes ?? []).filter((n) => n.signed_at).slice(0, 2);
  const rx = f.prescriptions.filter((x) => x.remaining > 0);
  const upcoming = f.appointments.filter((x) => x.status === "booked" && x.starts_at > now).reverse().slice(0, 3);
  const latest = new Map<string, TestResult>();
  for (const t of f.tests ?? []) if (!latest.has(t.test_code)) latest.set(t.test_code, t);
  const consents = CONSENT_KINDS.map(([k]) => [k, f.consents.find((c) => c.kind === k)] as const).filter(([k, c]) => c || k === "treatment" || k === "data");
  const more = (t: Tab, label: string) => <button className="btn sm" onClick={() => go(t)}>{label}<ChevronRight size={14} /></button>;
  const day = (iso: string) => `${fmtDate(iso, lang, { weekday: "short", day: "numeric", month: "short" })}, ${hm(iso)}`;
  return (
    <div className="sum-grid">
      {clinical && (
        <section className="tile sum-wide" aria-label={L("Queixa e objetivos", "Complaint and goals")}>
          <div className="tile-head"><h2>{L("Queixa e objetivos", "Complaint and goals")}</h2>{more("anamnese", L("Anamnese", "Anamnesis"))}</div>
          {flags.length > 0 && <p className="sum-flags" role="note"><TriangleAlert size={15} />{L("Sinais de alerta: ", "Red flags: ")}{flags.join(", ")}</p>}
          {an?.main_complaint || an?.goals ? (
            <dl className="sum-dl">
              {an?.main_complaint && <div><dt>{L("Queixa principal", "Main complaint")}</dt><dd>{an.main_complaint}</dd></div>}
              {an?.goals && <div><dt>{L("Objetivos", "Goals")}</dt><dd>{an.goals}</dd></div>}
              {(an?.regions ?? []).length > 0 && <div><dt>{L("Regiões", "Regions")}</dt><dd>{an!.regions.map((r) => `${regionLabel(r.region, lang)}${r.symptoms.length ? ` (${r.symptoms.map((x) => symptomLabel(x, lang).toLowerCase()).join(", ")})` : ""}`).join(" · ")}</dd></div>}
            </dl>
          ) : <p className="muted">{L("Anamnese ainda por preencher.", "Anamnesis not filled in yet.")}</p>}
        </section>
      )}
      {clinical && (
        <section className="tile sum-wide" aria-label={L("Últimas notas", "Latest notes")}>
          <div className="tile-head"><h2>{L("Últimas notas", "Latest notes")}</h2>{more("notes", L("Evolução", "Progress"))}</div>
          {notes.length ? notes.map((n) => (
            <div key={n.id} className="sum-note">
              <p className="sum-when">{fmtDate(n.signed_at!, lang, { day: "numeric", month: "short" })} · {n.signed_by_name}</p>
              {blockSummary(n.blocks) && <p className="sum-blocks">{blockSummary(n.blocks)}</p>}
              {n.text && <p className="sum-text">{n.text}</p>}
            </div>
          )) : <p className="muted">{L("Ainda sem notas assinadas.", "No signed notes yet.")}</p>}
        </section>
      )}
      <section className="tile" aria-label={L("Marcações", "Appointments")}>
        <div className="tile-head"><h2>{L("Próximas marcações", "Upcoming")}</h2></div>
        {upcoming.length ? <ul className="sum-list">{upcoming.map((x) => <li key={x.id}><span className="num">{day(x.starts_at)}</span><span className="muted">{x.therapist_name ?? ""}</span></li>)}</ul>
          : <p className="muted">{L("Nada marcado.", "Nothing booked.")}</p>}
      </section>
      <section className="tile" aria-label={L("Prescrições", "Prescriptions")}>
        <div className="tile-head"><h2>{L("Prescrições", "Prescriptions")}</h2>{more("rx", L("Ver", "View"))}</div>
        {rx.length ? <ul className="sum-list">{rx.map((x) => <li key={x.id}><span>{x.payer || x.doctor}</span><span className={x.remaining <= 2 ? "warn-text" : "muted"}>{L(x.remaining === 1 ? "resta 1 sessão" : `restam ${x.remaining}`, `${x.remaining} left`)}</span></li>)}</ul>
          : <p className="muted">{L("Sem prescrições ativas.", "No active prescriptions.")}</p>}
      </section>
      <section className="tile" aria-label={L("Consentimentos", "Consents")}>
        <div className="tile-head"><h2>{L("Consentimentos", "Consents")}</h2>{more("consents", L("Ver", "View"))}</div>
        <ul className="sum-list">{consents.map(([k, c]) => {
          const ok = c && c.state === "valid" && c.granted;
          return <li key={k}><span>{consentLabel(k, lang)}</span><span className={ok ? "muted" : "warn-text"}>{!c ? L("Por assinar", "Not signed") : c.state === "valid" ? (c.granted ? L("Assinado", "Signed") : L("Recusado", "Refused")) : c.state === "expired" ? L("Expirado", "Expired") : c.state === "revoked" ? L("Retirado", "Withdrawn") : L("Recusado", "Refused")}</span></li>;
        })}</ul>
      </section>
      {clinical && latest.size > 0 && (
        <section className="tile" aria-label={L("Testes", "Tests")}>
          <div className="tile-head"><h2>{L("Testes", "Tests")}</h2>{more("tests", L("Ver", "View"))}</div>
          <ul className="sum-list">{[...latest.values()].slice(0, 4).map((t) => <li key={t.id}><span>{testName(t.test_code, lang)}</span><span className="num">{Number(t.score).toLocaleString(lang === "pt" ? "pt-PT" : "en-GB", { maximumFractionDigits: 1 })}{t.max_score ? `/${t.max_score}` : ""} <span className="muted">· {fmtDate(t.taken_on, lang, { day: "numeric", month: "short" })}</span></span></li>)}</ul>
        </section>
      )}
    </div>
  );
}

/** The "patient card" button: details, and the legal tools (export, reimbursement pack, access log, anonymise, delete). */
function PatientActions({ f }: { f: PatientFile }) {
  const { L, role, openSheet, toast } = useApp();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const p = f.patient;
  useEffect(() => {
    const close = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);
  const item = (icon: JSX.Element, label: string, fn: () => void, danger = false, disabled = false) => (
    <button className={`mi ${danger ? "danger" : ""}`} role="menuitem" disabled={disabled} onClick={() => { setOpen(false); fn(); }}><span className="chk">{icon}</span>{label}</button>
  );
  const owner = role === "owner";
  const anon = !!p.anonymised_at;
  return (
    <div ref={ref} style={{ position: "relative" }}>
      <button className="btn icon round" aria-haspopup="menu" aria-expanded={open} aria-label={L("Mais ações do utente", "More patient actions")} title={L("Mais ações", "More actions")} onClick={() => setOpen(!open)}><MoreHorizontal /></button>
      {open && (
        <div className="menu frosted right" role="menu">
          {item(<UserPen size={14} />, L("Editar dados…", "Edit Details…"), () => openSheet(<PatientDetails patient={p} />), false, anon)}
          {item(<Globe size={14} />, L("Área de utente…", "Patient Area…"), () => openSheet(<PortalAccess pid={p.id} />), false, anon)}
          <div className="msep" />
          {item(<FileDown size={14} />, L("Exportar processo clínico…", "Export Clinical Record…"), () => openSheet(<ExportRecord pid={p.id} name={p.name} number={p.number} />), false, role === "reception")}
          {item(<Package size={14} />, L("Pacote de reembolso…", "Reimbursement Pack…"), () => openSheet(<ReimbursementPack pid={p.id} name={p.name} />), false, role === "reception" || anon)}
          {item(<Mail size={14} />, L("Carta de alta…", "Discharge Letter…"), () => openSheet(<DischargeLetter pid={p.id} name={p.name} lang={p.lang} />), false, role === "reception" || anon)}
          {item(<History size={14} />, L("Registo de acessos", "Access Log"), () => openSheet(<AccessLog patientId={p.id} name={p.name} />), false, !owner)}
          <div className="msep" />
          {item(<EyeOff size={14} />, L("Anonimizar utente…", "Anonymise Patient…"), () => openSheet(<AnonymisePatient pid={p.id} name={p.name} number={p.number} />), true, !owner || anon)}
          {item(<Trash2 size={14} />, L("Apagar utente…", "Delete Patient…"), () => openSheet(<DeletePatient pid={p.id} name={p.name} number={p.number} />), true, !owner)}
        </div>
      )}
    </div>
  );
}

// ───────── anamnesis ─────────

const EMPTY_AN = { main_complaint: "", history: "", medication: "", medical_history: "", goals: "", regions: [] as Region[], red_flags: [] as string[] };

function Anamnese({ pid, an }: { pid: string; an: Anamnesis | null }) {
  const { L, lang, toast } = useApp();
  const init = useMemo(() => ({ ...EMPTY_AN, ...(an ? { main_complaint: an.main_complaint, history: an.history, medication: an.medication, medical_history: an.medical_history, goals: an.goals, regions: an.regions, red_flags: an.red_flags } : {}) }), [an]);
  // Unsaved changes survive switching tabs or pages (audit C2): kept in memory until saved or reverted.
  const [v, setVState] = useState<typeof init>(() => (anamnesisDrafts.get(pid) as typeof init | undefined) ?? init);
  const setV = (n: typeof init) => { setVState(n); if (JSON.stringify(n) === JSON.stringify(init)) anamnesisDrafts.delete(pid); else anamnesisDrafts.set(pid, n); };
  const [open, setOpen] = useState<string[]>(v.regions.slice(0, 1).map((r) => r.region));
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string>();
  const dirty = JSON.stringify(v) !== JSON.stringify(init);
  // A newer version from the server (after saving) replaces the form only if nothing is being typed.
  useEffect(() => { if (!anamnesisDrafts.has(pid)) setVState(init); }, [init]); // eslint-disable-line react-hooks/exhaustive-deps
  const fields: [keyof typeof EMPTY_AN, string, string][] = [
    ["main_complaint", L("Queixa principal", "Main complaint"), L("Ex.: dor cervical à direita há 3 meses", "E.g. right-sided neck pain for 3 months")],
    ["history", L("História da condição atual", "History of current condition"), L("Início, evolução, episódios anteriores", "Onset, course, earlier episodes")],
    ["medication", L("Medicação", "Medication"), ""], ["medical_history", L("Antecedentes", "Medical history"), ""],
    ["goals", L("Objetivos do utente", "Patient goals"), L("Ex.: trabalhar 8 horas sem dor", "E.g. work 8 hours without pain")],
  ];
  const used = v.regions.map((x) => x.region);
  const setRegion = (k: string, ch: Partial<Region>) => setV({ ...v, regions: v.regions.map((r) => r.region === k ? { ...r, ...ch } : r) });
  const save = async () => {
    setBusy(true); setErr(undefined);
    try { await api("PUT", `/patients/${pid}/anamnesis`, v); anamnesisDrafts.delete(pid); toast(L("Anamnese guardada", "Anamnesis saved")); }
    catch (e) { setErr(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  return (
    <>
      <div className="grid2">
        {fields.map(([k, label, ph], i) => (
          <Field key={k} label={label}><textarea className="input" rows={i < 2 ? 3 : 2} value={v[k] as string} placeholder={ph} onChange={(e) => setV({ ...v, [k]: e.target.value })} /></Field>
        ))}
      </div>
      <div className="section-title">
        <h2>{L("Regiões e sintomas", "Regions and symptoms")}</h2>
        <div style={{ position: "relative" }}>
          <button className="btn" onClick={() => setAdding(!adding)} aria-expanded={adding} aria-haspopup="menu"><Plus />{L("Adicionar região", "Add Region")}</button>
          {adding && (
            <div className="menu frosted right" role="menu" style={{ maxHeight: 300, overflow: "auto" }}>
              {REGIONS.map(([k]) => (
                <button key={k} className="mi" role="menuitem" disabled={used.includes(k)} onClick={() => { setV({ ...v, regions: [...v.regions, { region: k, symptoms: [], notes: "" }] }); setOpen([...open, k]); setAdding(false); }}>
                  <span className="chk">{used.includes(k) && <Check size={13} />}</span>{regionLabel(k, lang)}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
      {v.regions.length === 0 && <p className="small muted" style={{ marginBottom: 10 }}>{L("Nenhuma região adicionada. Use “Adicionar região” para escolher onde dói.", "No regions yet. Use “Add Region” to choose where it hurts.")}</p>}
      {v.regions.map((reg) => {
        const isOpen = open.includes(reg.region);
        return (
          <div key={reg.region} className="region">
            <div className="region-head">
              <button className="iconbtn" aria-expanded={isOpen} aria-label={isOpen ? L("Recolher", "Collapse") : L("Expandir", "Expand")}
                onClick={() => setOpen(isOpen ? open.filter((x) => x !== reg.region) : [...open, reg.region])}>{isOpen ? <ChevronDown /> : <ChevronRight />}</button>
              <span className="grow">{regionLabel(reg.region, lang)}</span>
              {!isOpen && <span className="small muted">{reg.symptoms.map((s) => symptomLabel(s, lang)).join(", ") || L("Sem sintomas", "No symptoms")}</span>}
              <button className="iconbtn" aria-label={L("Remover região", "Remove region")} onClick={() => setV({ ...v, regions: v.regions.filter((r) => r.region !== reg.region) })}><X /></button>
            </div>
            {isOpen && (
              <div className="region-body">
                <div>
                  <div className="small muted" style={{ marginBottom: 6, marginTop: 8 }}>{L("Sintomas", "Symptoms")}</div>
                  <div className="chips">
                    {SYMPTOMS.map(([s]) => (
                      <button key={s} className="chip" aria-pressed={reg.symptoms.includes(s)}
                        onClick={() => setRegion(reg.region, { symptoms: reg.symptoms.includes(s) ? reg.symptoms.filter((y) => y !== s) : [...reg.symptoms, s] })}>{symptomLabel(s, lang)}</button>
                    ))}
                  </div>
                </div>
                <Field label={L("Notas", "Notes")}><input className="input" value={reg.notes} onChange={(e) => setRegion(reg.region, { notes: e.target.value })} placeholder={L("Quando piora, quando alivia", "What makes it worse or better")} /></Field>
              </div>
            )}
          </div>
        );
      })}
      <div className="section-title"><h2>{L("Sinais de alerta", "Red flags")}</h2></div>
      <div className="radio-row" style={{ marginBottom: 18 }}>
        {RED_FLAGS.map(([k, t]) => (
          <label key={k} className="check"><input type="checkbox" checked={v.red_flags.includes(k)} onChange={(e) => setV({ ...v, red_flags: e.target.checked ? [...v.red_flags, k] : v.red_flags.filter((x) => x !== k) })} />{L(t[0], t[1])}</label>
        ))}
      </div>
      <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap" }}>
        <button className="btn pri" onClick={save} disabled={!dirty || busy}><Check />{L("Guardar anamnese", "Save Anamnesis")}</button>
        {dirty && <button className="btn" onClick={() => { anamnesisDrafts.delete(pid); setVState(init); }}>{L("Repor", "Revert")}</button>}
        <span className="small muted">{dirty ? L("Alterações por guardar.", "Unsaved changes.") : an?.updated_by_name ? L(`Guardada por ${an.updated_by_name}, ${fmtDate(an.updated_at, lang)}.`, `Saved by ${an.updated_by_name}, ${fmtDate(an.updated_at, lang)}.`) : ""}</span>
      </div>
      {err && <div className="err" role="alert">{err}</div>}
    </>
  );
}

// ───────── progress notes ─────────

function BlockEditor({ b, onChange, onRemove }: { b: Block; onChange: (b: Block) => void; onRemove: () => void }) {
  const { L } = useApp();
  const custom = b.key.startsWith("custom");
  const label = custom
    ? <input className="input" value={b.label} onChange={(e) => onChange({ ...b, label: e.target.value })} aria-label={L("Nome do bloco", "Block name")} placeholder={L("Nome do bloco", "Block name")} style={{ height: 26 }} />
    : <label htmlFor={`b-${b.key}`}>{b.label}</label>;
  const rm = <button className="iconbtn" aria-label={L(`Remover ${b.label}`, `Remove ${b.label}`)} onClick={onRemove}><X size={14} /></button>;
  // A scale starts unset ("—") and is only part of the note once it's touched (audit C3: no invented "Dor 5").
  if (b.type === "slider") {
    const unset = b.value == null;
    const set = (el: HTMLInputElement) => onChange({ ...b, value: el.valueAsNumber });
    return (
      <div className={`slider ${unset ? "unset" : ""}`} style={{ gridTemplateColumns: "160px minmax(0,1fr) 84px 28px" }}>
        {label}<input id={`b-${b.key}`} type="range" min={b.min} max={b.max} step={b.step ?? 1} value={b.value ?? Math.round((b.min + b.max) / 2 / (b.step ?? 1)) * (b.step ?? 1)}
          aria-valuetext={unset ? L("Não medido", "Not measured") : undefined}
          onChange={(e) => set(e.currentTarget)} onPointerUp={(e) => { if (unset) set(e.currentTarget); }}
          onKeyDown={(e) => { if (unset && (e.key === " " || e.key === "Enter")) { e.preventDefault(); set(e.currentTarget); } }} />
        {unset ? <output className="muted small" style={{ fontWeight: 500 }} title={L("Não medido: toque na escala para dar um valor", "Not measured: touch the scale to give it a value")}>{L("Por medir", "Not set")}</output>
          : <button type="button" className="out-btn" title={L("Limpar (não medido)", "Clear (not measured)")} aria-label={L(`${b.label}: ${b.value}${b.unit ?? ""}. Limpar`, `${b.label}: ${b.value}${b.unit ?? ""}. Clear`)} onClick={() => onChange({ ...b, value: null })}>{b.value}{b.unit ?? ""}</button>}
        {rm}
      </div>
    );
  }
  if (b.type === "choice") return (
    <div style={{ padding: "6px 0", display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap" }}>
      <span style={{ width: 160 }}>{label}</span>
      <div className="chips" role="radiogroup" aria-label={b.label} style={{ flex: 1 }}>{b.options.map((o) => <button key={o} className="chip" role="radio" aria-checked={b.value === o} aria-pressed={b.value === o} onClick={() => onChange({ ...b, value: b.value === o ? "" : o })}>{o}</button>)}</div>{rm}
    </div>
  );
  if (b.type === "checklist") return (
    <div style={{ padding: "6px 0" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}><span className="small muted">{b.label}</span>{rm}</div>
      <div className="chips">{b.options.map((o) => <button key={o} className="chip" aria-pressed={b.value.includes(o)} onClick={() => onChange({ ...b, value: b.value.includes(o) ? b.value.filter((x) => x !== o) : [...b.value, o] })}>{o}</button>)}</div>
    </div>
  );
  return (
    <div style={{ padding: "6px 0" }}>
      <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 4 }}>{label}<span style={{ flex: 1 }} />{rm}</div>
      <input className="input" value={b.value} onChange={(e) => onChange({ ...b, value: e.target.value })} aria-label={b.label} />
    </div>
  );
}

/** What a note draft would save (to tell if anything changed since the last save). */
const noteJson = (d: Pick<NoteDraft, "tpl" | "blocks" | "text" | "aiUsed"> & { recordingId?: string | null }) => JSON.stringify([d.tpl, d.blocks, d.text, d.aiUsed, d.recordingId ?? null]);
const noteEmpty = (blocks: Block[], text: string) => !text.trim() && blocks.every((b) => b.type === "slider" ? b.value == null : Array.isArray(b.value) ? !b.value.length : !b.value);
/** How long typing has to pause before the draft is saved on the server. */
const AUTOSAVE_MS = 8000;

/** Saves a note draft on the server (creates it the first time). One save at a time per patient. */
async function persistNote(pid: string, d: NoteDraft, appointmentId: string | null, quiet: boolean): Promise<string> {
  if (d.saving) await d.saving.catch(() => undefined);
  const snap = noteJson(d);
  // recording_id: the note was written from a recording's AI draft; signing it deletes that recording.
  const body = { template: d.tpl, blocks: d.blocks, text: d.text, appointment_id: appointmentId, ai_assisted: d.aiUsed, ...(d.recordingId ? { recording_id: d.recordingId } : {}) };
  const run = (async () => {
    if (d.draftId) { await api("PATCH", `/notes/${d.draftId}`, body, { quiet }); return d.draftId; }
    const r = await api<{ id: string }>("POST", `/patients/${pid}/notes`, body, { quiet });
    d.draftId = r.id; return r.id;
  })();
  d.saving = run;
  try { const id = await run; d.saved = snap; d.savedAt = Date.now(); return id; }
  finally { if (d.saving === run) d.saving = undefined; }
}

function Notes({ pid, name, notes, docs, todayAppt }: { pid: string; name: string; notes: Note[]; docs: DocumentRow[]; todayAppt: string | null }) {
  const { L, lang, role, me, toast, confirm, openSheet } = useApp();
  // The note being written lives in memory per patient (lib/drafts.ts), so it survives switching tabs or
  // pages, and is saved as a server draft a few seconds after typing stops (audit C2).
  const fresh = (): NoteDraft => { const b0 = NOTE_TEMPLATES[0].blocks(lang); const d = { tpl: NOTE_TEMPLATES[0].id, blocks: b0, text: "", aiUsed: false, draftId: null, saved: "", savedAt: null }; d.saved = noteJson(d); return d; };
  const store = useRef<NoteDraft>(noteDrafts.get(pid) ?? fresh());
  useEffect(() => { noteDrafts.set(pid, store.current); }, [pid]);
  const d0 = store.current;
  const [tpl, setTplS] = useState(d0.tpl);
  const [blocks, setBlocksS] = useState<Block[]>(d0.blocks);
  const [text, setTextS] = useState(d0.text);
  const [draftId, setDraftId] = useState<string | null>(d0.draftId);
  const [savedAt, setSavedAt] = useState<number | null>(d0.savedAt);
  const setTpl = (v: string) => { store.current.tpl = v; setTplS(v); };
  const setBlocks = (v: Block[]) => { store.current.blocks = v; setBlocksS(v); };
  const setText = (v: string) => { store.current.text = v; setTextS(v); };
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string>();
  const [adding, setAdding] = useState(false);
  const [aiUsed, setAiUsedS] = useState(d0.aiUsed);
  const setAiUsed = (v: boolean) => { store.current.aiUsed = v; setAiUsedS(v); };
  const [rw, setRw] = useState<AiState<string>>({ state: "idle" });
  const ai = useAiStatus();
  const owner = role === "owner";
  // The local AI (rewrite, recording): the owner, and therapists whose AI switch is on (Team).
  const canAi = owner || (role === "therapist" && !!me.ai_enabled);
  const rec = useRec();
  const [recDraft, setRecDraft] = useState<RecordingDraft | null>(null);
  const mine = (n: Note) => owner || n.created_by === me.id;
  const empty = noteEmpty(blocks, text);
  const unsaved = !empty && store.current.saved !== noteJson({ tpl, blocks, text, aiUsed });
  const reset = () => {
    const d = fresh(); d.tpl = tpl; d.blocks = NOTE_TEMPLATES.find((t) => t.id === tpl)!.blocks(lang); d.saved = noteJson(d);
    store.current = d; noteDrafts.set(pid, d);
    setDraftId(null); setTextS(""); setAiUsedS(false); setRw({ state: "idle" }); setBlocksS(d.blocks); setSavedAt(null); setRecDraft(null);
  };
  /** Puts a recording's AI draft in the note: its template, the proposed values, the report and the plan. Undo puts back what was there. */
  const applyRecDraft = (d: RecordingDraft) => {
    const t = NOTE_TEMPLATES.find((x) => x.id === d.template) ?? NOTE_TEMPLATES.find((x) => x.id === tpl)!;
    const prev = { ...store.current };
    const nb = (draftId ? blocks : t.blocks(lang)).map((b): Block => {
      const v = d.values[b.key];
      if (v === undefined) return b;
      if (b.type === "slider" && typeof v === "number") return { ...b, value: v };
      if (b.type === "choice" && typeof v === "string") return { ...b, value: v };
      if (b.type === "checklist" && Array.isArray(v)) return { ...b, value: v };
      if (b.type === "text" && typeof v === "string") return { ...b, value: v };
      return b;
    });
    if (!draftId) setTpl(t.id);
    setBlocks(nb); setText([d.report, d.plan].filter(Boolean).join("\n\n")); setAiUsed(true);
    store.current.recordingId = d.id; setRecDraft(null);
    toast(L("Rascunho da gravação colocado na nota", "Recording draft put in the note"), () => {
      setTpl(prev.tpl); setBlocks(prev.blocks); setText(prev.text); setAiUsed(prev.aiUsed); store.current.recordingId = prev.recordingId;
    }, L("usar rascunho da gravação", "Use Recording Draft"));
  };
  // Autosave: a few seconds after typing stops, and when leaving this tab or the patient.
  const appt = useRef(todayAppt); appt.current = todayAppt;
  const autosave = (quiet: boolean) => {
    const d = store.current;
    if (noteEmpty(d.blocks, d.text) || d.saved === noteJson(d)) return;
    persistNote(pid, d, appt.current, quiet).then((id) => { if (store.current === d) { setDraftId(id); setSavedAt(d.savedAt); } }).catch(() => { /* kept in memory; Save Draft shows the error */ });
  };
  useEffect(() => { const t = setTimeout(() => autosave(true), AUTOSAVE_MS); return () => clearTimeout(t); }, [tpl, blocks, text, aiUsed]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => autosave(true), []); // eslint-disable-line react-hooks/exhaustive-deps
  /** Owner: a plain-text draft (no titles, no SOAP) from what's filled in. Nothing is saved; Use puts it in Free notes. */
  const rewrite = async () => {
    setRw({ state: "working" });
    try { const r = await api<{ text: string }>("POST", "/ai/rewrite-note", { patient_id: pid, template: tname(tpl), blocks, text, lang }); setRw({ state: "done", result: r.text }); }
    catch (e) { setRw({ state: "error", message: isErr(e) ? e.message : String(e) }); }
  };
  const pickTemplate = (id: string) => { setTpl(id); setBlocks(NOTE_TEMPLATES.find((t) => t.id === id)!.blocks(lang)); };
  const addBlock = (type: "slider" | "text") => {
    const key = `custom${Date.now()}`;
    setBlocks([...blocks, type === "slider" ? { type, key, label: "", value: 5, min: 0, max: 10 } : { type, key, label: "", value: "" }]);
    setAdding(false);
  };
  const saveDraft = async (): Promise<string> => {
    const id = await persistNote(pid, store.current, todayAppt, false);
    setDraftId(id); setSavedAt(store.current.savedAt); return id;
  };
  const run = async (fn: () => Promise<void>) => { setBusy(true); setErr(undefined); try { await fn(); } catch (e) { setErr(isErr(e) ? e.message : String(e)); } finally { setBusy(false); } };
  const sign = () => confirm({
    title: L("Assinar esta nota?", "Sign this note?"),
    text: L("Depois de assinada, a nota fica fechada e não pode ser alterada nem apagada. Para corrigir, acrescenta-se uma nota nova.", "Once signed, the note is locked and can’t be changed or deleted. To correct it, add a new note."),
    action: L("Assinar", "Sign"),
    onConfirm: () => run(async () => {
      const id = await saveDraft();
      const r = await api<{ document_id: string | null }>("POST", `/notes/${id}/sign`);
      reset();
      if (r.document_id) toast(L("Nota assinada e guardada em PDF no arquivo", "Note signed and saved as PDF in the archive"));
      else toast(L("Nota assinada. O PDF não foi guardado no arquivo do servidor.", "Note signed. The PDF couldn’t be saved in the server archive."),
        () => { api("POST", `/notes/${id}/pdf`).then(() => toast(L("PDF guardado no arquivo", "PDF saved in the archive"))).catch((e) => toast(isErr(e) ? e.message : String(e))); },
        L("Tentar outra vez", "Try Again"));
    }),
  });
  const edit = (n: Note) => {
    const d: NoteDraft = { tpl: n.template || "blank", blocks: n.blocks, text: n.text, aiUsed: !!n.ai_assisted, draftId: n.id, saved: "", savedAt: null, recordingId: null };
    d.saved = noteJson(d); store.current = d; noteDrafts.set(pid, d);
    setDraftId(n.id); setTplS(d.tpl); setBlocksS(d.blocks); setTextS(d.text); setAiUsedS(d.aiUsed); setSavedAt(null); setRw({ state: "idle" }); window.scrollTo?.(0, 0);
  };
  const tname = (id: string) => { const t = NOTE_TEMPLATES.find((x) => x.id === id); return t ? L(t.name[0], t.name[1]) : id; };
  return (
    <>
      <div className="card pad" style={{ marginBottom: 18 }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12, marginBottom: 10, flexWrap: "wrap" }}>
          <h2>{draftId ? L("Rascunho", "Draft") : L(`Nova nota — ${fmtDate(new Date(), "pt", { day: "numeric", month: "short" })}`, `New note — ${fmtDate(new Date(), "en", { day: "numeric", month: "short" })}`)}</h2>
          <select className="input" style={{ width: "auto" }} aria-label={L("Modelo", "Template")} value={tpl} onChange={(e) => pickTemplate(e.target.value)} disabled={!!draftId}>
            {NOTE_TEMPLATES.map((t) => <option key={t.id} value={t.id}>{L("Modelo: ", "Template: ")}{L(t.name[0], t.name[1])}</option>)}
          </select>
        </div>
        {blocks.map((b, i) => <BlockEditor key={b.key} b={b} onChange={(nb) => setBlocks(blocks.map((x, j) => j === i ? nb : x))} onRemove={() => setBlocks(blocks.filter((_, j) => j !== i))} />)}
        <div style={{ position: "relative", margin: "6px 0 12px" }}>
          <button className="chip" onClick={() => setAdding(!adding)} aria-haspopup="menu" aria-expanded={adding}><Plus size={12} style={{ verticalAlign: -2 }} /> {L("Bloco", "Block")}</button>
          {adding && (
            <div className="menu frosted" role="menu" style={{ left: 0 }}>
              <button className="mi" role="menuitem" onClick={() => addBlock("slider")}><span className="chk" />{L("Escala 0–10", "Scale 0–10")}</button>
              <button className="mi" role="menuitem" onClick={() => addBlock("text")}><span className="chk" />{L("Campo de texto", "Text field")}</button>
            </div>
          )}
        </div>
        <Field label={L("Notas livres", "Free notes")}>
          <textarea className="input" value={text} onChange={(e) => setText(e.target.value)} placeholder={L("O que foi feito, como respondeu, plano para a próxima sessão", "What you did, how they responded, plan for next session")} />
        </Field>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
          <button className="btn pri" onClick={sign} disabled={busy || empty}><Check />{L("Assinar nota…", "Sign Note…")}</button>
          <button className="btn" onClick={() => run(async () => { await saveDraft(); toast(L("Rascunho guardado", "Draft saved")); })} disabled={busy || empty}>{L("Guardar rascunho", "Save Draft")}</button>
          {draftId && <button className="btn" onClick={reset} disabled={busy}>{L("Nova nota", "New Note")}</button>}
          {canAi && <button className="btn ai" onClick={rewrite} disabled={busy || empty || rw.state === "working" || (!!ai.data && !ai.data.available)}><Sparkles />{L("Reescrever com IA", "Rewrite with AI")}</button>}
          {canAi && <button className="btn mic" onClick={() => openSheet(<RecordSheet pid={pid} name={name} template={tpl} templateName={tname(tpl)} blocks={blocks} appointmentId={todayAppt} />)}
            disabled={rec.state !== "idle"} title={rec.state !== "idle" ? L("Já está a gravar uma sessão", "A session is already being recorded") : undefined}>
            <Mic />{rec.state !== "idle" && rec.patientId === pid ? L("A gravar…", "Recording…") : L("Gravar sessão…", "Record Session…")}</button>}
          {canAi && ai.data && !ai.data.available && <span className="small muted">{aiOffText(ai.data, L)}</span>}
        </div>
        {rw.state !== "idle" && (
          <div style={{ marginTop: 12 }}>
            <AiCard title={rw.state === "error" ? L("A IA não conseguiu reescrever a nota", "The AI couldn’t rewrite the note") : L("Rascunho da IA local — confirme antes de assinar", "Draft from the local AI — check it before signing")}
              working={rw.state === "working" ? L("A escrever a nota a partir das escalas, escolhas e notas livres…", "Writing the note from the scales, choices and free notes…") : undefined}
              error={rw.state === "error" ? rw.message : undefined}
              actions={<>
                {rw.state === "done" && <button className="btn sm pri" onClick={() => { const prev = text, prevAi = aiUsed; setText(rw.result); setAiUsed(true); setRw({ state: "idle" });
                  toast(L("Texto da IA colocado nas notas livres", "AI text put in Free notes"), () => { setText(prev); setAiUsed(prevAi); }, L("usar texto da IA", "Use AI Text")); }}>{L("Usar este texto", "Use This Text")}</button>}
                <button className="btn sm" onClick={rewrite}>{L("Tentar de novo", "Retry")}</button>
                <button className="btn sm" onClick={() => setRw({ state: "idle" })}>{L("Descartar", "Discard")}</button>
              </>}>
              {rw.state === "done" && <p className="ai-text">{rw.result}</p>}
              {rw.state === "done" && <p className="small muted" style={{ marginTop: 8 }}>{L("Usar substitui as notas livres; as escalas e escolhas ficam como estão. A IA pode errar: só fica guardado quando guardar ou assinar.", "Use replaces Free notes; scales and choices stay as they are. The AI can make mistakes: nothing is kept until you save or sign.")}</p>}
            </AiCard>
          </div>
        )}
        {recDraft && <div style={{ marginTop: 12 }}><DraftCard d={recDraft} blocks={draftId ? blocks : (NOTE_TEMPLATES.find((t) => t.id === recDraft.template) ?? NOTE_TEMPLATES[0]).blocks(lang)} onUse={() => applyRecDraft(recDraft)} onDiscard={() => setRecDraft(null)} /></div>}
        {aiUsed && <p className="ls-ai-note"><Sparkles size={11} />{store.current.recordingId
          ? L("O texto começou com um rascunho da IA local a partir da gravação. Ao assinar, a gravação é apagada.", "The text started from a local AI draft of the recording. Signing deletes the recording.")
          : L("O texto começou com um rascunho da IA local", "The text started from a local AI draft")}</p>}
        {!empty && <p className="small muted" role="status" style={{ marginTop: 8 }}>{unsaved
          ? L("Alterações por guardar — o rascunho é guardado sozinho daqui a uns segundos.", "Unsaved changes — the draft saves itself in a few seconds.")
          : savedAt ? L(`Rascunho guardado às ${hm(new Date(savedAt).toISOString())}.`, `Draft saved at ${hm(new Date(savedAt).toISOString())}.`) : ""}</p>}
        {todayAppt && <p className="small muted" style={{ marginTop: 8 }}>{L("Ligada à sessão de hoje.", "Linked to today’s session.")}</p>}
        {err && <div className="err" role="alert">{err}</div>}
      </div>
      {canAi && <Recordings pid={pid} onReview={(d) => { setRecDraft(d); window.scrollTo?.(0, 0); }} />}
      <h2 style={{ marginBottom: 10 }}>{L("Notas anteriores", "Earlier notes")}</h2>
      {notes.length === 0 ? <p className="small muted">{L("Ainda não há notas.", "No notes yet.")}</p> : (
        <div className="rows">
          {notes.map((n) => (
            <div key={n.id} className={`row ${n.id === draftId ? "sel" : ""}`} style={{ alignItems: "flex-start" }}>
              <span className="time" style={{ width: 64 }}>{fmtDate(n.created_at, lang, { day: "numeric", month: "short" })}</span>
              <span className="grow">
                {blockSummary(n.blocks) && <span style={{ display: "block", fontWeight: 500 }}>{blockSummary(n.blocks)}</span>}
                {n.text && <span style={{ display: "block", whiteSpace: "pre-wrap" }}>{n.text}</span>}
                <span className="s">{n.ai_assisted && <><Badge tone="ai"><Sparkles size={10} style={{ verticalAlign: -1 }} /> {L("IA", "AI")}</Badge> </>}{tname(n.template)} — {n.signed_at ? L(`assinada por ${n.signed_by_name}, ${fmtDate(n.signed_at, lang)} ${hm(n.signed_at)}`, `signed by ${n.signed_by_name}, ${fmtDate(n.signed_at, lang)} ${hm(n.signed_at)}`) : L(`rascunho de ${n.created_by_name}`, `draft by ${n.created_by_name}`)}</span>
              </span>
              {n.signed_at ? (() => {
                const pdf = docs.find((d) => d.note_id === n.id);
                return (
                  <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <Lock size={13} aria-label={L("Assinada", "Signed")} style={{ color: "var(--text3)" }} />
                    {pdf && <button className="btn sm" aria-label={L("Abrir o PDF da nota assinada", "Open the signed note’s PDF")} onClick={() => openSheet(<DocumentViewer doc={pdf} />)}><FileText />PDF</button>}
                  </span>
                );
              })() : mine(n) && (
                <span style={{ display: "flex", gap: 6 }}>
                  <button className="btn sm" onClick={() => edit(n)}>{L("Continuar", "Continue")}</button>
                  <button className="btn sm" onClick={() => confirm({ title: L("Apagar este rascunho?", "Delete this draft?"), action: L("Apagar", "Delete"), destructive: true, hold: true,
                    onConfirm: () => api("DELETE", `/notes/${n.id}`).then(() => { if (draftId === n.id) reset(); toast(L("Rascunho apagado", "Draft deleted")); }).catch((e) => toast(isErr(e) ? e.message : String(e))) })}>{L("Apagar…", "Delete…")}</button>
                </span>
              )}
            </div>
          ))}
        </div>
      )}
    </>
  );
}

// ───────── tests ─────────

function Tests({ pid, tests }: { pid: string; tests: TestResult[] }) {
  const { L, lang, openSheet } = useApp();
  const groups = Object.values(tests.reduce<Record<string, TestResult[]>>((acc, t) => { (acc[t.test_code] ??= []).push(t); return acc; }, {}));
  return (
    <>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12, marginBottom: 14, flexWrap: "wrap" }}>
        <p className="sub">{L("Evolução das pontuações. Avisa quando uma pontuação piora.", "Score history. Alerts you when a score gets worse.")}</p>
        <button className="btn" onClick={() => openSheet(<RecordTest patientId={pid} history={tests} />)}><ClipboardList />{L("Registar teste…", "Record Test…")}</button>
      </div>
      {groups.length === 0 ? <Empty icon={<ClipboardList />} title={L("Sem testes", "No tests yet")} text={L("Registe um teste para começar a acompanhar a evolução.", "Record a test to start tracking progress.")}
        action={<button className="btn pri" onClick={() => openSheet(<RecordTest patientId={pid} history={tests} />)}><ClipboardList />{L("Registar teste…", "Record Test…")}</button>} /> : (
        <div className="grid3">
          {groups.map((g) => {
            const t = g[g.length - 1], first = g[0], prev = g[g.length - 2];
            const hiw = t.higher_is_worse;
            const worse = prev && (hiw ? t.score > prev.score : t.score < prev.score);
            const better = hiw ? t.score < first.score : t.score > first.score;
            const max = t.max_score ?? TEST_LIBRARY.find((x) => x.code === t.test_code)?.max ?? Math.max(...g.map((x) => x.score));
            const d = Math.round((t.score - first.score) * 10) / 10;
            return (
              <div key={t.test_code} className="card pad">
                <div style={{ display: "flex", justifyContent: "space-between", gap: 8 }}>
                  <h3>{testName(t.test_code, lang)}</h3>
                  {worse ? <Badge tone="bad"><TrendingDown size={11} style={{ verticalAlign: -1 }} /> {L("Piorou", "Worse")}</Badge> : g.length > 1 ? <Badge tone={better ? "ok" : "n"}>{d > 0 ? "+" : ""}{d}</Badge> : null}
                </div>
                <div className="num" style={{ fontSize: "var(--fs-title)", fontWeight: 700, margin: "4px 0" }}>{Number(t.score).toLocaleString(lang === "pt" ? "pt-PT" : "en-GB", { maximumFractionDigits: 1 })}<span className="muted" style={{ fontSize: "var(--fs-sub)" }}>/{max}</span></div>
                {(() => { const sub = (t.answers as { sub?: Record<string, number> } | null)?.sub; return sub ? <p className="small muted" style={{ marginBottom: 4 }}>{Object.entries(sub).map(([k, v]) => `${tx(SUB_LABEL[k] ?? [k, k], lang)} ${Number(v).toLocaleString(lang === "pt" ? "pt-PT" : "en-GB", { maximumFractionDigits: 1 })}`).join(" · ")}</p> : null; })()}
                {g.length > 1 && <Sparkline pts={g.map((x) => x.score)} max={max} label={`${testName(t.test_code, lang)}: ${g.map((x) => x.score).join(", ")}`} />}
                <div className="small muted" style={{ display: "flex", justifyContent: "space-between", gap: 8 }}>
                  <span>{g.length > 1 ? `${fmtDate(first.taken_on, lang, { day: "numeric", month: "short" })} → ${fmtDate(t.taken_on, lang, { day: "numeric", month: "short" })}` : fmtDate(t.taken_on, lang)}</span>
                  <span style={{ display: "flex", gap: 10 }}>
                    <button className="linkbtn" onClick={() => openSheet(<TestHistory code={t.test_code} list={g} />)}>{L("Histórico", "History")}</button>
                    <button className="linkbtn" onClick={() => openSheet(<RecordTest patientId={pid} code={t.test_code} history={tests} />)}>{L("Registar", "Record")}</button>
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </>
  );
}

// ───────── prescriptions ─────────

function Prescriptions({ pid, name, list, docs }: { pid: string; name: string; list: Prescription[]; docs: DocumentRow[] }) {
  const { L, lang, role, openSheet } = useApp();
  const clinical = role !== "reception";
  const today = ymd(new Date());
  return (
    <>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12, marginBottom: 14, flexWrap: "wrap" }}>
        <p className="sub">{L("As sessões contam quando a marcação ligada à prescrição fica realizada.", "Sessions count once an appointment linked to the prescription is marked done.")}</p>
        {clinical && <button className="btn" onClick={() => openSheet(<AddPrescription patientId={pid} />)}><Plus />{L("Nova prescrição…", "New Prescription…")}</button>}
      </div>
      {list.length === 0 ? <Empty icon={<Pill />} title={L("Sem prescrições", "No prescriptions")} text={clinical ? L("Registe a prescrição (credencial) para contar as sessões e avisar antes de acabar.", "Add the prescription to count sessions and get a warning before it runs out.") : L("Um fisioterapeuta regista as prescrições.", "A therapist adds prescriptions.")} /> : (
        <div className="stack">
          {list.map((rx) => {
            const expired = rx.valid_until && rx.valid_until < today;
            const low = rx.remaining <= 2 || (rx.valid_until && rx.valid_until <= ymd(new Date(Date.now() + 14 * 864e5)));
            return (
              <div key={rx.id} className="card pad">
                <div style={{ display: "flex", justifyContent: "space-between", gap: 12, flexWrap: "wrap" }}>
                  <div><h3>{rx.doctor || L("Médico não indicado", "Doctor not given")}{rx.payer ? ` — ${rx.payer}` : ""}</h3><p className="small muted">{rx.specialty ? `${rx.specialty} — ` : ""}{L("emitida a", "issued")} {fmtDate(rx.issued_on, lang)}</p></div>
                  {expired ? <Badge tone="bad">{L("Expirada", "Expired")}</Badge> : rx.remaining <= 0 ? <Badge>{L("Concluída", "Used up")}</Badge>
                    : <Badge tone={low ? "warn" : "ok"}>{L(`Restam ${rx.remaining} sessões`, `${rx.remaining} sessions left`)}</Badge>}
                </div>
                <div style={{ display: "flex", justifyContent: "space-between", margin: "14px 0 6px" }}><span>{L("Sessões realizadas", "Sessions done")}</span><span className="num">{rx.used}/{rx.sessions_total}</span></div>
                <div className={`bar ${low ? "warn" : ""}`}><i style={{ width: `${Math.min(100, (rx.used / rx.sessions_total) * 100)}%` }} /></div>
                <p className="small muted" style={{ marginTop: 8 }}>
                  {rx.booked > 0 ? L(`${rx.booked} marcada(s). `, `${rx.booked} booked. `) : ""}
                  {rx.valid_until ? L(`Válida até ${fmtDate(rx.valid_until, lang)}. `, `Valid until ${fmtDate(rx.valid_until, lang)}. `) : ""}
                  {L("Avisa com 2 sessões ou 14 dias de antecedência.", "Alerts at 2 sessions left or 14 days before.")}
                </p>
                {(() => {
                  const scan = rx.document_id ? docs.find((d) => d.id === rx.document_id) : undefined;
                  if (scan) return <button className="btn sm" style={{ marginTop: 10 }} onClick={() => openSheet(<DocumentViewer doc={scan} />)}><FileText />{L("Ver digitalização", "View Scan")}</button>;
                  if (!clinical) return null;
                  return (
                    <label className="btn sm" style={{ marginTop: 10, cursor: "pointer" }}>
                      <Upload />{L("Juntar digitalização…", "Attach Scan…")}
                      <input type="file" accept={DOC_ACCEPT} hidden onChange={(e) => { const fl = [...(e.target.files ?? [])].slice(0, 1); e.target.value = ""; if (fl.length) openSheet(<AddDocuments patientId={pid} patientName={name} files={fl} linkPrescription={rx.id} />); }} />
                    </label>
                  );
                })()}
              </div>
            );
          })}
        </div>
      )}
    </>
  );
}

// ───────── consents ─────────

function SignaturePad({ onChange }: { onChange: (png: string | null) => void }) {
  const { L } = useApp();
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current!;
    const r = c.getBoundingClientRect(), dpr = window.devicePixelRatio || 1;
    c.width = r.width * dpr; c.height = r.height * dpr;
    const x = c.getContext("2d")!;
    x.scale(dpr, dpr); x.lineWidth = 2.2; x.lineCap = "round"; x.lineJoin = "round"; x.strokeStyle = "#111111";
    let down = false;
    const pt = (e: PointerEvent) => { const b = c.getBoundingClientRect(); return [e.clientX - b.left, e.clientY - b.top] as const; };
    c.onpointerdown = (e) => { down = true; c.setPointerCapture(e.pointerId); x.beginPath(); x.moveTo(...pt(e)); x.lineTo(...pt(e)); x.stroke(); };
    c.onpointermove = (e) => { if (down) { x.lineTo(...pt(e)); x.stroke(); } };
    c.onpointerup = () => { if (!down) return; down = false; onChange(c.toDataURL("image/png")); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <>
      <canvas ref={ref} className="sig" aria-label={L("Área de assinatura: assine com o rato, o dedo ou a caneta", "Signature area: sign with the mouse, a finger or a pen")} />
      <button className="btn sm" style={{ marginTop: 8 }} onClick={() => { const c = ref.current!; c.getContext("2d")!.clearRect(0, 0, c.width, c.height); onChange(null); }}>{L("Limpar", "Clear")}</button>
    </>
  );
}

function Consents({ f }: { f: PatientFile }) {
  const { L, lang, toast, openSheet, role } = useApp();
  const { data: texts, error } = useApi<ConsentText[]>("/consent-texts");
  const p = f.patient;
  const [pad, setPad] = useState<ConsentKind | null>(null);
  const [who, setWho] = useState(p.name);
  const [granted, setGranted] = useState(true);
  const [sig, setSig] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string>();
  const a = age(p.birth_date);
  const textFor = (k: ConsentKind) => texts?.find((t) => t.kind === k && t.current && t.lang === p.lang) ?? texts?.find((t) => t.kind === k && t.current);
  const awaiting = (k: ConsentKind) => !!texts?.some((t) => t.kind === k && t.draft);
  const latest = (k: ConsentKind) => f.consents.find((c) => c.kind === k);
  const docs = f.documents ?? [];
  const pdfOf = (id: string | null) => (id ? docs.find((d) => d.id === id) : undefined);
  const open = (k: ConsentKind) => { setPad(pad === k ? null : k); setWho(k === "guardian" && p.guardian_name ? p.guardian_name : p.name); setGranted(true); setSig(null); setErr(undefined); };
  const submit = async (k: ConsentKind) => {
    setBusy(true); setErr(undefined);
    try {
      const yes = granted || !REFUSABLE.includes(k);
      const r = await api<{ id: string; document_id: string | null }>("POST", `/patients/${p.id}/consents`, { text_id: textFor(k)!.id, granted: REFUSABLE.includes(k) ? granted : true, signed_by_name: who.trim(), signature_png: sig });
      setPad(null);
      if (r.document_id) toast(yes ? L("Consentimento assinado e guardado em PDF", "Consent signed and saved as PDF") : L("Recusa registada e guardada em PDF", "Refusal recorded and saved as PDF"));
      else toast(L("Assinatura registada. O PDF não foi guardado no arquivo do servidor.", "Signature recorded. The PDF couldn’t be saved in the server archive."),
        () => { retryPdf(r.id); }, L("Tentar outra vez", "Try Again"));
    } catch (e) { setErr(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  const retryPdf = (id: string) => api("POST", `/consents/${id}/pdf`).then(() => toast(L("PDF guardado no arquivo", "PDF saved in the archive"))).catch((e) => toast(isErr(e) ? e.message : String(e)));
  const stateText = (c: ConsentRow) => c.state === "revoked" ? L(`Retirado a ${fmtDate(c.revoked_at!, lang)}`, `Withdrawn on ${fmtDate(c.revoked_at!, lang)}`)
    : c.state === "expired" ? L(`Expirou a ${fmtDate(c.expires_on!, lang)}`, `Expired on ${fmtDate(c.expires_on!, lang)}`)
      : c.state === "refused" ? L(`Recusado a ${fmtDate(c.signed_at, lang)}`, `Refused on ${fmtDate(c.signed_at, lang)}`)
        : c.expires_on ? L(`Assinado a ${fmtDate(c.signed_at, lang)}, válido até ${fmtDate(c.expires_on, lang)}`, `Signed on ${fmtDate(c.signed_at, lang)}, valid until ${fmtDate(c.expires_on, lang)}`)
          : L(`Assinado a ${fmtDate(c.signed_at, lang)} por ${c.signed_by_name}`, `Signed on ${fmtDate(c.signed_at, lang)} by ${c.signed_by_name}`);
  if (error) return <p className="err">{error}</p>;
  return (
    <div className="rows">
      {CONSENT_KINDS.map(([k]) => {
        const c = latest(k);
        const ok = c?.state === "valid";
        const t = textFor(k);
        const notApplicable = k === "guardian" && (a == null || a >= 18) && !c;
        return (
          <div key={k} className="row" style={{ flexWrap: "wrap" }}>
            <span className="ic" style={{ color: ok ? "var(--ok)" : c?.state === "expired" || c?.state === "revoked" ? "var(--bad)" : notApplicable ? "var(--text3)" : "var(--warn)" }}>{ok ? <Check /> : <FileSignature />}</span>
            <span className="grow">
              <span className="t" style={{ display: "block" }}>{consentLabel(k, lang)}</span>
              <span className="s" style={{ display: "block" }}>{c ? stateText(c) : notApplicable ? L("Só para menores de 18", "Only for patients under 18") : !t && awaiting(k) ? L("Texto por aprovar — o proprietário revê-o em Definições › Consentimentos", "Text waiting for approval — the owner reviews it in Settings › Consents") : !t && texts ? L("Sem texto ainda — o proprietário escreve-o em Definições › Consentimentos", "No text yet — the owner writes it in Settings › Consents") : L("Por assinar", "Not signed")}</span>
            </span>
            {ok && <Badge tone="ok">{L("Assinado", "Signed")}</Badge>}
            {c?.state === "refused" && <Badge>{L("Recusado", "Refused")}</Badge>}
            {c && <button className="btn sm" onClick={() => openSheet(<ConsentView id={c.id} canRevoke={c.state === "valid" || c.state === "refused"} />)}>{L("Ver", "View")}</button>}
            {c && (pdfOf(c.state === "revoked" ? c.revoked_pdf_id : c.pdf_id)
              ? <button className="btn sm" aria-label={c.state === "revoked" ? L("Abrir o PDF da retirada", "Open the withdrawal PDF") : L("Abrir o PDF do consentimento assinado", "Open the signed consent’s PDF")}
                  onClick={() => openSheet(<DocumentViewer doc={pdfOf(c.state === "revoked" ? c.revoked_pdf_id : c.pdf_id)!} />)}><FileText />PDF</button>
              : <button className="btn sm" title={L("O PDF não foi guardado no arquivo. Tentar outra vez.", "The PDF wasn’t saved in the archive. Try again.")} onClick={() => retryPdf(c.id)}><FileText />{L("Guardar PDF", "Save PDF")}</button>)}
            {t && !notApplicable && !(ok && !c?.expires_on) && <button className="btn sm" aria-expanded={pad === k} onClick={() => open(k)}><PenLine size={13} />{c?.state === "expired" ? L("Renovar", "Renew") : c ? L("Assinar de novo", "Sign Again") : L("Assinar", "Sign")}</button>}
            {pad === k && t && (
              <div style={{ flexBasis: "100%", paddingTop: 10 }}>
                <div className="card pad" style={{ marginBottom: 10, maxHeight: 180, overflow: "auto" }}>
                  <h3 style={{ marginBottom: 6 }}>{t.title} <span className="small muted">v{t.version}</span></h3>
                  <p className="small" style={{ whiteSpace: "pre-wrap", lineHeight: 1.55 }}>{t.body}</p>
                </div>
                {REFUSABLE.includes(k) && (
                  <div className="radio-row" style={{ marginBottom: 8 }}>
                    <label className="check"><input type="radio" name={`g-${k}`} checked={granted} onChange={() => setGranted(true)} />{L("Aceita", "Agrees")}</label>
                    <label className="check"><input type="radio" name={`g-${k}`} checked={!granted} onChange={() => setGranted(false)} />{L("Não aceita", "Doesn’t agree")}</label>
                  </div>
                )}
                <Field label={L("Nome de quem assina", "Name of the person signing")}><input className="input" value={who} onChange={(e) => setWho(e.target.value)} /></Field>
                <SignaturePad onChange={setSig} />
                <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                  <button className="btn" onClick={() => setPad(null)}>{L("Cancelar", "Cancel")}</button>
                  <button className="btn pri" disabled={!sig || !who.trim() || busy} onClick={() => submit(k)}><Check size={13} />{L("Confirmar assinatura", "Confirm Signature")}</button>
                </div>
                {err && <div className="err" role="alert">{err}</div>}
                {role === "reception" && <p className="small muted" style={{ marginTop: 6 }}>{L("Fica registado quem recolheu a assinatura.", "Who collected the signature is recorded.")}</p>}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

/** One signed consent: the exact text version, who signed and the signature. Opening it is logged. */
function ConsentView({ id, canRevoke }: { id: string; canRevoke: boolean }) {
  const { L, lang, confirm, toast } = useApp();
  const { data: c, error } = useApi<ConsentFull>(`/consents/${id}`);
  return (
    <Sheet wide title={c ? c.title : L("Consentimento", "Consent")} sub={c ? `v${c.version} — ${c.granted ? L("aceite", "agreed") : L("recusado", "refused")} ${L("por", "by")} ${c.signed_by_name}, ${fmtDate(c.signed_at, lang)} ${hm(c.signed_at)}` : undefined}
      action={canRevoke && c ? L("Registar retirada…", "Record Withdrawal…") : undefined} destructive
      onAction={() => setTimeout(() => confirm({
        title: L("Registar que o utente retirou este consentimento?", "Record that the patient withdrew this consent?"),
        text: L("O registo assinado fica guardado, marcado como retirado, e a retirada é guardada em PDF no processo.", "The signed record is kept and marked as withdrawn, and the withdrawal is saved as PDF in the record."),
        action: L("Registar retirada", "Record Withdrawal"), destructive: true,
        onConfirm: () => api<{ document_id: string | null }>("POST", `/consents/${id}/revoke`)
          .then((r) => r.document_id ? toast(L("Retirada registada e guardada em PDF", "Withdrawal recorded and saved as PDF"))
            : toast(L("Retirada registada. O PDF não foi guardado no arquivo do servidor.", "Withdrawal recorded. The PDF couldn’t be saved in the server archive."),
              () => { api("POST", `/consents/${id}/pdf`).then(() => toast(L("PDF guardado no arquivo", "PDF saved in the archive"))).catch((e) => toast(isErr(e) ? e.message : String(e))); }, L("Tentar outra vez", "Try Again")))
          .catch((e) => toast(isErr(e) ? e.message : String(e))),
      }), 0)}>
      {error ? <p className="err">{error}</p> : !c ? <p className="small muted">{L("A carregar…", "Loading…")}</p> : (
        <>
          <p style={{ whiteSpace: "pre-wrap", lineHeight: 1.55, marginBottom: 12 }}>{c.body}</p>
          <img src={c.signature_png} alt={L(`Assinatura de ${c.signed_by_name}`, `Signature of ${c.signed_by_name}`)} style={{ width: "100%", maxHeight: 160, objectFit: "contain", background: "#fff", borderRadius: 8, boxShadow: "inset 0 0 0 .5px var(--line2)" }} />
          {c.expires_on && <p className="small muted" style={{ marginTop: 8 }}>{L("Válido até", "Valid until")} {fmtDate(c.expires_on, lang)}.</p>}
          {c.revoked_at && <p className="small" style={{ marginTop: 8, color: "var(--bad)" }}>{L("Retirado a", "Withdrawn on")} {fmtDate(c.revoked_at, lang)}.</p>}
        </>
      )}
    </Sheet>
  );
}

// ───────── documents, payments ─────────

function Documents({ f }: { f: PatientFile }) {
  const ctx = useApp();
  const { L, lang, role, me, openSheet, confirm, toast } = ctx;
  const docs = f.documents ?? [];
  const reception = role === "reception";
  const [kind, setKind] = useState<DocKind | "all">("all");
  const [over, setOver] = useState(false);
  const kinds = [...new Set(docs.map((d) => d.kind))];
  const shown = docs.filter((d) => kind === "all" || d.kind === kind);
  useEffect(() => { if (kind !== "all" && !kinds.includes(kind)) setKind("all"); }, [docs.length]); // eslint-disable-line react-hooks/exhaustive-deps
  const add = (list: FileList | null | undefined) => { const files = [...(list ?? [])]; if (files.length) openSheet(<AddDocuments patientId={f.patient.id} patientName={f.patient.name} files={files} />); };
  const today = (iso: string) => new Date(iso).toDateString() === new Date().toDateString();
  const canRemove = (d: DocumentRow) => !serverMade(d) && (role === "owner" || (d.uploaded_by === me.id && today(d.uploaded_at)));
  const save = (d: DocumentRow) => download(d.id, d.filename, "document").then((w) => toast(L(`Guardado em ${w}`, `Saved to ${w}`))).catch((e) => toast(isErr(e) ? e.message : String(e)));
  const remove = (d: DocumentRow) => confirm({
    title: L(`Remover “${d.filename}”?`, `Remove “${d.filename}”?`),
    text: L("O ficheiro é apagado da pasta do utente no servidor. Não é possível desfazer.", "The file is deleted from the patient’s folder on the server. This can’t be undone."),
    action: L("Remover", "Remove"), destructive: true, hold: true,
    onConfirm: () => api("DELETE", `/documents/${d.id}`).then(() => toast(L("Documento removido", "Document removed"))).catch((e) => toast(isErr(e) ? e.message : String(e))),
  });
  const menu = (e: React.MouseEvent, d: DocumentRow) => openMenu(ctx, e, [
    { label: L("Abrir", "Open"), fn: () => openSheet(<DocumentViewer doc={d} />) },
    { label: L("Guardar uma cópia…", "Save a Copy…"), fn: () => save(d) },
    ...(!serverMade(d) ? [{ label: L("Informações…", "Get Info…"), fn: () => openSheet(<DocumentDetails doc={d} />) }] : []),
    ...(canRemove(d) ? ["-" as const, { label: L("Remover…", "Remove…"), danger: true, fn: () => remove(d) }] : []),
  ]);
  const picker = (label: string, primary = false) => (
    <label className={`btn ${primary ? "pri" : ""}`} style={{ cursor: "pointer" }}>
      <Upload />{label}
      <input type="file" multiple accept={DOC_ACCEPT} hidden onChange={(e) => { add(e.target.files); e.target.value = ""; }} />
    </label>
  );
  return (
    <div className={`docs-drop ${over ? "over" : ""}`}
      onDragOver={(e) => { if (e.dataTransfer.types.includes("Files")) { e.preventDefault(); e.dataTransfer.dropEffect = "copy"; setOver(true); } }}
      onDragLeave={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node)) setOver(false); }}
      onDrop={(e) => { e.preventDefault(); setOver(false); add(e.dataTransfer.files); }}>
      {docs.length === 0 ? (
        <Empty icon={<Upload />} title={reception ? L("Sem prescrições digitalizadas", "No scanned prescriptions") : L("Sem documentos", "No documents")}
          text={reception ? L("Arraste a digitalização da credencial para aqui. Os consentimentos assinados aparecem aqui em PDF.", "Drag the scanned prescription here. Signed consents appear here as PDF.") : L("Arraste para aqui relatórios, exames e fotografias, em PDF ou imagem. As notas e os consentimentos assinados aparecem aqui em PDF.", "Drag reports, imaging and photos here, as PDF or pictures. Signed notes and consents appear here as PDF.")}
          action={picker(L("Juntar ficheiros…", "Add Files…"), true)} />
      ) : (
        <>
          <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 14, flexWrap: "wrap" }}>
            {kinds.length > 1 ? (
              <div className="chips" style={{ flex: 1 }} role="group" aria-label={L("Mostrar", "Show")}>
                <button className="chip" aria-pressed={kind === "all"} onClick={() => setKind("all")}>{L("Todos", "All")} {docs.length}</button>
                {kinds.map((k) => <button key={k} className="chip" aria-pressed={kind === k} onClick={() => setKind(k)}>{docKindLabel(k, lang)} {docs.filter((d) => d.kind === k).length}</button>)}
              </div>
            ) : <p className="small muted" style={{ flex: 1 }}>{docs.length === 1 ? L("1 documento", "1 document") : L(`${docs.length} documentos`, `${docs.length} documents`)}</p>}
            {picker(L("Juntar ficheiros…", "Add Files…"))}
          </div>
          <div className="doc-grid">
            {shown.map((d) => (
              <div key={d.id} className={`card doc k-${d.kind}`} onContextMenu={(e) => menu(e, d)}>
                <button className="doc-open" onClick={() => openSheet(<DocumentViewer doc={d} />)} aria-label={L(`Abrir ${d.filename}`, `Open ${d.filename}`)}>
                  <span className="doc-ic" aria-hidden="true"><DocIcon d={d} /></span>
                  <span style={{ minWidth: 0, flex: 1 }}>
                    <span className="ell" style={{ display: "block", fontWeight: 500 }}>{d.filename}</span>
                    <span className="small muted ell" style={{ display: "block" }}>{docKindLabel(d.kind, lang)} · {fmtDate(d.uploaded_at, lang, { day: "numeric", month: "short", year: "numeric" })} · {fileSize(d.bytes, lang)}</span>
                  </span>
                </button>
                <button className="btn icon sm more" aria-label={L(`Mais ações para ${d.filename}`, `More actions for ${d.filename}`)} onClick={(e) => menu(e, d)}><MoreHorizontal /></button>
              </div>
            ))}
          </div>
          <p className="small muted" style={{ marginTop: 12 }}>{L("Também pode arrastar ficheiros para aqui. Ficam na pasta do utente no servidor da clínica; cada abertura fica no registo de acessos.", "You can also drag files here. They’re kept in the patient’s folder on the clinic server; every time one is opened, it’s written to the access log.")}</p>
        </>
      )}
      {over && <div className="docs-over" aria-hidden="true"><Upload /><span>{L("Largue para juntar à ficha", "Drop to add to the file")}</span></div>}
    </div>
  );
}

/** Packages and plans, what's owed and the payments of one patient (everyone; the server logs the read). */
function Payments({ pid }: { pid: string }) {
  const ctx = useApp();
  const { L, lang, role, openSheet, toast, confirm } = ctx;
  const { data, error } = useApi<PatientMoney>(`/patients/${pid}/money`);
  if (error) return <Empty icon={<Receipt />} title={L("Não foi possível carregar os pagamentos", "Couldn’t load payments")} text={error} />;
  if (!data) return <div className="skel" style={{ height: 120 }} />;
  const live = data.plans.filter((x) => x.status === "active" || x.status === "paused");
  const old = data.plans.filter((x) => x.status === "ended" || x.status === "cancelled");
  const planAct = (pl: Plan, status: Plan["status"], done: string) => api("PATCH", `/plans/${pl.id}`, { status }).then(() => toast(done)).catch((x) => toast(isErr(x) ? x.message : String(x)));
  const planMenu = (e: React.MouseEvent, pl: Plan) => openMenu(ctx, e, [
    ...(pl.kind === "monthly" && pl.status === "active" ? [{ label: L("Pausar este mês", "Pause This Month"), fn: () => planAct(pl, "paused", L("Plano em pausa", "Plan paused")) }] : []),
    ...(pl.status === "paused" ? [{ label: L("Retomar", "Resume"), fn: () => planAct(pl, "active", L("Plano retomado", "Plan resumed")) }] : []),
    { label: L("Terminar…", "End…"), fn: () => confirm({ title: L(`Terminar ${planName(pl, "pt")}?`, `End ${planName(pl, "en")}?`), text: pl.kind === "monthly" ? L("Deixa de ser cobrado a partir do próximo mês.", "It won’t be charged from next month.") : L("As sessões que restam deixam de poder ser usadas.", "Any sessions left can no longer be used."), action: L("Terminar", "End"), onConfirm: () => planAct(pl, "ended", L("Terminado", "Ended")) }) },
    ...(pl.used === 0 && pl.paid_cents === 0 ? ["-" as const, { label: L("Anular venda…", "Cancel Sale…"), danger: true, fn: () => confirm({ title: L("Anular esta venda?", "Cancel this sale?"), text: L("Use quando foi vendido por engano. Não fica nada em dívida.", "Use this when it was sold by mistake. Nothing will be owed."), action: L("Anular venda", "Cancel Sale"), destructive: true, onConfirm: () => planAct(pl, "cancelled", L("Venda anulada", "Sale cancelled")) }) }] : []),
  ]);
  const payMenu = (e: React.MouseEvent, py: Payment) => openMenu(ctx, e, [
    ...(py.invoice_file_id ? [{ label: L("Ver fatura", "View Invoice"), icon: <Eye />, fn: () => openSheet(<InvoiceViewer payment={py} />) }] : []),
    { label: py.invoice_number ? L("Editar fatura…", "Edit Invoice…") : L("Registar fatura…", "Add Invoice…"), icon: <FileText />, fn: () => openSheet(<InvoiceSheet payment={py} />) },
    ...(role === "owner" || (py.created_by === ctx.me.id && ymd(new Date(py.created_at)) === ymd(new Date())) ? ["-" as const, { label: L("Anular pagamento…", "Void Payment…"), danger: true, fn: () => openSheet(<VoidPayment payment={py} />) }] : []),
  ]);
  const planLine = (pl: Plan) => pl.kind === "pack"
    ? L(`${pl.remaining} de ${pl.sessions} por usar${pl.expires_on ? ` · válido até ${fmtDate(pl.expires_on, lang)}` : ""}`, `${pl.remaining} of ${pl.sessions} left${pl.expires_on ? ` · valid until ${fmtDate(pl.expires_on, lang)}` : ""}`)
    : L(`${pl.used_month}${pl.sessions ? ` de ${pl.sessions}` : ""} este mês · ${eur(c2e(pl.price_cents), lang)}/mês`, `${pl.used_month}${pl.sessions ? ` of ${pl.sessions}` : ""} this month · ${eur(c2e(pl.price_cents), lang)}/month`);
  return (
    <>
      <div className="grid2" style={{ marginBottom: 18 }}>
        <div className={`card pad ${data.due_cents ? "hl" : ""}`}>
          <div className="small" style={{ opacity: .8 }}>{L("Em dívida", "Owed")}</div>
          <div style={{ fontSize: "var(--fs-title)", fontWeight: 600 }} className="num">{eur(c2e(data.due_cents), lang)}</div>
          {data.owed.slice(0, 4).map((o) => <div key={owedKey(o)} className="small" style={{ display: "flex", justifyContent: "space-between", opacity: .9 }}><span>{owedLabel(o, lang)}</span><span className="num">{eur(c2e(o.due_cents), lang)}</span></div>)}
          {data.owed.length > 4 && <div className="small" style={{ opacity: .8 }}>+{data.owed.length - 4}</div>}
          <button className="btn" style={{ marginTop: 10 }} onClick={() => openSheet(<RecordPayment patientId={pid} />)}><Receipt />{L("Registar pagamento…", "Record Payment…")}</button>
        </div>
        <div className="card pad">
          <div style={{ display: "flex", alignItems: "center", marginBottom: 8 }}><h3 style={{ flex: 1 }}>{L("Pacotes e planos", "Packages and plans")}</h3>
            <button className="btn sm" onClick={() => openSheet(<SellPlan patientId={pid} />)}><Plus />{L("Vender…", "Sell…")}</button></div>
          {live.length === 0 && <p className="small muted">{L("Sem pacote ativo: cada sessão é paga à parte.", "No active package: each session is paid on its own.")}</p>}
          {live.map((pl) => (
            <div key={pl.id} style={{ display: "flex", gap: 8, alignItems: "flex-start", padding: "6px 0", borderTop: "1px solid var(--line)" }}>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontWeight: 500 }}>{planName(pl, lang)} {pl.status === "paused" && <Badge tone="warn">{planStatusLabel(pl.status, lang)}</Badge>}
                  {pl.patient_id !== pid && <Badge>{L(`de ${pl.patient_name}`, `${pl.patient_name}’s`)}</Badge>}</div>
                <div className="small muted">{planLine(pl)}{pl.shared_with_name && pl.patient_id === pid ? L(` · partilhado com ${pl.shared_with_name}`, ` · shared with ${pl.shared_with_name}`) : ""}</div>
                {pl.kind === "pack" && pl.sessions && <div className="bar" style={{ marginTop: 6 }} role="img" aria-label={planLine(pl)}><i style={{ width: `${(pl.used / pl.sessions) * 100}%` }} /></div>}
                {pl.kind === "pack" && pl.paid_cents < pl.price_cents && pl.patient_id === pid && <div className="small" style={{ color: "var(--warn)", marginTop: 4 }}>{L(`Pago ${eur(c2e(pl.paid_cents), lang)} de ${eur(c2e(pl.price_cents), lang)}`, `Paid ${eur(c2e(pl.paid_cents), lang)} of ${eur(c2e(pl.price_cents), lang)}`)}</div>}
              </div>
              {pl.patient_id === pid && <button className="btn icon sm more" aria-label={L("Mais ações", "More actions")} onClick={(e) => planMenu(e, pl)}><MoreHorizontal /></button>}
            </div>
          ))}
          {old.length > 0 && <p className="small muted" style={{ marginTop: 8 }}>{L(`${old.length} anterior(es): `, `${old.length} earlier: `)}{old.map((x) => `${planName(x, lang)} (${planStatusLabel(x.status, lang).toLowerCase()})`).join(", ")}</p>}
        </div>
      </div>
      <h3 style={{ marginBottom: 8 }}>{L("Pagamentos", "Payments")}</h3>
      {data.payments.length === 0 ? <p className="small muted">{L("Ainda sem pagamentos.", "No payments yet.")}</p> : (
        <div className="rows">{data.payments.map((py) => (
          <div key={py.id} className="row" style={py.voided_at ? { opacity: .6 } : undefined}>
            <span className="num muted" style={{ width: 54 }}>{fmtDate(py.received_on, lang, { day: "2-digit", month: "2-digit" })}</span>
            <span className="grow"><span className="t" style={{ display: "block", textDecoration: py.voided_at ? "line-through" : undefined }}>{py.description}</span>
              <span className="s">{methodLabel(py.method, lang)}{py.created_by_name ? ` · ${py.created_by_name}` : ""}{py.voided_at ? ` · ${L("anulado", "voided")}: ${py.void_reason}` : ""}</span></span>
            {!py.voided_at && (py.invoice_number ? (py.invoice_file_id
              ? <button className="badge-btn" onClick={() => openSheet(<InvoiceViewer payment={py} />)} aria-label={L(`Ver fatura ${py.invoice_number}`, `View invoice ${py.invoice_number}`)}><Badge tone="ok"><FileText size={11} />{py.invoice_number}</Badge></button>
              : <Badge tone="ok">{py.invoice_number}</Badge>) : <Badge tone="warn">{L("Sem fatura", "Not invoiced")}</Badge>)}
            <span className="amt" style={{ width: 80, textAlign: "right" }}>{eur(c2e(py.amount_cents), lang)}</span>
            {!py.voided_at ? <button className="btn icon sm more" aria-label={L("Mais ações", "More actions")} onClick={(e) => payMenu(e, py)}><MoreHorizontal /></button> : <span style={{ width: 28 }} />}
          </div>
        ))}</div>
      )}
    </>
  );
}
