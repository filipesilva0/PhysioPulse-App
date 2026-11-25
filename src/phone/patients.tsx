// Patients on the iPhone (phase 5): search, then a read-only file. Nothing is written here: notes, assessments and
// tests are written on the computer (owner, 11 Nov 2026). Every file opened is written to the access log by the
// server, as on the desktop. Reception gets the reduced file (contacts, plan, consents), as on the desktop.
import { useEffect, useState } from "react";
import { FileText, Search, Users, X } from "lucide-react";
import { useApp } from "../lib/ctx";
import {
  useApi, age, fmtDate, hm, kindLabel, consentLabel, testName, blockSummary, toneFor,
  type PatientRow, type PatientFile, type Therapist, type Note,
} from "../lib/data";
import { Avatar, BackCircle, Group, PEmpty, Row, Screen } from "./ui";
import { searchNav } from "./schedule";
import type { PhoneNav } from "./PhoneApp";

const nr = (n: number) => String(n).padStart(4, "0");

export function PatientsScreen({ nav }: { nav: PhoneNav }) {
  return nav.patient ? <PatientFileScreen id={nav.patient} back={nav.closePatient} /> : <PatientList nav={nav} />;
}

function PatientList({ nav }: { nav: PhoneNav }) {
  const { L } = useApp();
  const [q, setQ] = useState("");
  const [term, setTerm] = useState("");
  // Opened from the search pill on Today: start in the field.
  const [focus] = useState(() => { const f = searchNav.focus; searchNav.focus = false; return f; });
  // Search as you type, a moment after the last key.
  useEffect(() => { const t = setTimeout(() => setTerm(q.trim()), 250); return () => clearTimeout(t); }, [q]);
  const { data, error } = useApi<PatientRow[]>(`/patients?limit=200${term ? `&q=${encodeURIComponent(term)}` : ""}`, { keep: true });
  const { data: ths } = useApi<Therapist[]>("/therapists");
  return (
    <Screen title={L("Utentes", "Patients")}>
      <label className="p-search">
        <Search aria-hidden />
        <input type="search" inputMode="search" enterKeyHint="search" autoFocus={focus} autoCorrect="off" autoCapitalize="words" value={q} onChange={(e) => setQ(e.target.value)}
          placeholder={L("Nome, número ou telefone", "Name, number or phone")} aria-label={L("Procurar utentes", "Search patients")} />
        {q && <button className="p-icon sm" onClick={() => setQ("")} aria-label={L("Limpar", "Clear")}><X /></button>}
      </label>
      {error && <p className="p-err">{error}</p>}
      {!data && !error && <div className="p-skel" />}
      {data && data.length === 0 && <PEmpty icon={<Users />} title={term ? L("Nenhum utente encontrado", "No patients found") : L("Ainda não há utentes", "No patients yet")}
        />}
      {data && data.length > 0 && (
        <div className="p-group"><div className="p-list">
          {data.map((p) => (
            <Row key={p.id} label={p.name} chevron onClick={() => nav.openPatient(p.id)}
              detail={<>Nº {nr(p.number)}{p.next_at ? L(` · próxima ${fmtDate(p.next_at, "pt", { day: "numeric", month: "short" })}, ${hm(p.next_at)}`, ` · next ${fmtDate(p.next_at, "en", { day: "numeric", month: "short" })}, ${hm(p.next_at)}`) : ""}</>} />
          ))}
        </div></div>
      )}
      {data && data.length >= 200 && <p className="p-note">{L("Primeiros 200 — procure para ver outros.", "First 200 — search for others.")}</p>}
    </Screen>
  );
}

function PatientFileScreen({ id, back }: { id: string; back: () => void }) {
  const { L, lang } = useApp();
  const { data: f, error } = useApi<PatientFile>(`/patients/${id}`);
  const { data: ths } = useApi<Therapist[]>("/therapists");
  const p = f?.patient;
  const now = new Date().toISOString();
  const upcoming = (f?.appointments ?? []).filter((a) => a.status === "booked" && a.starts_at > now).sort((a, b) => a.starts_at.localeCompare(b.starts_at)).slice(0, 4);
  const lastDone = (f?.appointments ?? []).filter((a) => a.status === "done").sort((a, b) => b.starts_at.localeCompare(a.starts_at))[0];
  const signed = (f?.notes ?? []).filter((n) => n.signed_at).sort((a, b) => (b.signed_at ?? "").localeCompare(a.signed_at ?? ""));
  const latestTests = Object.values((f?.tests ?? []).reduce<Record<string, NonNullable<PatientFile["tests"]>[number]>>((m, t) => {
    if (!m[t.test_code] || m[t.test_code].taken_on < t.taken_on) m[t.test_code] = t; return m; }, {}));
  const a = p ? age(p.birth_date) : null;
  return (
    <Screen title={L("Ficha", "File")} label={L("Ficha do utente", "Patient file")} leading={<BackCircle label={L("Utentes", "Patients")} onClick={back} />}>
      {error && <p className="p-err">{error}</p>}
      {!f && !error && <div className="p-skel" />}
      {f && p && <>
        <div className="p-profile">
          <Avatar name={p.name} tone={toneFor(ths, p.therapist_id)} size={56} />
          <span className="p-pinfo"><b>{p.name}</b><span>{[`Nº ${nr(p.number)}`, a != null ? L(`${a} anos`, `${a} years`) : ""].filter(Boolean).join(" · ")}</span>{p.therapist_name && <span>{p.therapist_name}</span>}</span>
        </div>

        <Group title={L("Contacto", "Contact")}>
          {p.phone && <Row label={L("Telefone", "Phone")} value={<span className="p-select num">{p.phone}</span>} />}
          {p.email && <Row label="Email" value={<span className="p-select">{p.email}</span>} />}
          {p.address && <Row label={L("Morada", "Address")} detail={<span className="p-select">{p.address}</span>} />}
          {p.guardian_name && <Row label={L("Representante", "Guardian")} detail={<span className="p-select">{p.guardian_name}{p.guardian_phone ? ` · ${p.guardian_phone}` : ""}</span>} />}
          {!p.phone && !p.email && !p.address && <Row label={L("Sem contactos na ficha", "No contact details on file")} />}
        </Group>
        {p.admin_note && <Group title={L("Nota da receção", "Front-desk note")}><Row label={<span className="p-select">{p.admin_note}</span>} /></Group>}

        <Group title={L("Plano", "Plan")}>
          {upcoming.map((x) => <Row key={x.id} label={`${cap(fmtDate(x.starts_at, lang, { weekday: "short", day: "numeric", month: "short" }))}, ${hm(x.starts_at)}`} detail={`${kindLabel(x.kind, lang)}${x.therapist_name ? ` · ${x.therapist_name}` : ""}`} />)}
          {!upcoming.length && <Row label={L("Nenhuma marcação futura", "No upcoming appointments")} />}
          {lastDone && <Row label={L("Última sessão", "Last session")} value={fmtDate(lastDone.starts_at, lang, { day: "numeric", month: "short" })} />}
        </Group>

        {f.prescriptions.length > 0 && (
          <Group title={L("Prescrições", "Prescriptions")}>
            {f.prescriptions.map((x) => <Row key={x.id} label={x.doctor || L("Prescrição", "Prescription")}
              detail={[x.specialty, x.valid_until ? L(`válida até ${fmtDate(x.valid_until, "pt", { day: "numeric", month: "short" })}`, `valid until ${fmtDate(x.valid_until, "en", { day: "numeric", month: "short" })}`) : ""].filter(Boolean).join(" · ")}
              value={<span className={x.remaining <= 2 ? "t-warn" : ""}>{L(`${x.remaining} de ${x.sessions_total}`, `${x.remaining} of ${x.sessions_total}`)}</span>} />)}
          </Group>
        )}

        <Group title={L("Consentimentos", "Consents")}>
          {f.consents.length ? f.consents.map((c) => <Row key={c.id} label={consentLabel(c.kind, lang).split(" (")[0]}
            value={<span className={`p-chip ${c.state === "valid" && c.granted ? "ok" : c.state === "refused" || !c.granted ? "" : "warn"}`}>
              {c.state === "valid" ? (c.granted ? L("Assinado", "Signed") : L("Recusado", "Refused")) : c.state === "expired" ? L("Expirado", "Expired") : c.state === "revoked" ? L("Retirado", "Withdrawn") : L("Recusado", "Refused")}</span>} />)
            : <Row label={L("Nenhum assinado", "None signed")} />}
        </Group>

        {f.view === "full" && f.anamnesis?.main_complaint && (
          <Group title={L("Queixa principal", "Main complaint")}><Row label={<span className="p-select p-prose">{f.anamnesis.main_complaint}</span>} /></Group>
        )}
        {f.view === "full" && latestTests.length > 0 && (
          <Group title={L("Testes (último resultado)", "Tests (latest score)")}>
            {latestTests.map((t) => <Row key={t.id} label={testName(t.test_code, lang)} detail={fmtDate(t.taken_on, lang, { day: "numeric", month: "short" })}
              value={<span className="num">{t.score}{t.max_score != null ? ` / ${t.max_score}` : ""}</span>} />)}
          </Group>
        )}
        {f.view === "full" && (
          <Group title={L("Notas assinadas", "Signed notes")}>
            {signed.length ? signed.slice(0, 10).map((n) => <NoteRow key={n.id} n={n} />) : <Row label={L("Ainda não há notas assinadas", "No signed notes yet")} />}
          </Group>
        )}
      </>}
    </Screen>
  );
}

/** A signed note: date and who signed, then the text (the first lines; tap for the rest). */
function NoteRow({ n }: { n: Note }) {
  const { L, lang } = useApp();
  const [open, setOpen] = useState(false);
  const text = n.text || blockSummary(n.blocks);
  return (
    <button className="p-row p-note-row" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
      <span className="p-row-main">
        <span className="p-row-l">{fmtDate(n.signed_at!, lang, { day: "numeric", month: "short", year: "numeric" })}<span className="p-row-d"> · {n.signed_by_name ?? ""}</span></span>
        <span className={`p-prose ${open ? "" : "clamp"}`}>{text || L("(sem texto)", "(no text)")}</span>
      </span>
    </button>
  );
}

const cap = (s: string) => s.replace(/^./, (c) => c.toUpperCase());
