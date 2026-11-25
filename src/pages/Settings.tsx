import { Fragment, useRef, useState } from "react";
import type { ReactNode } from "react";
import {
  Bell, Building2, CalendarClock, Cpu, CreditCard, FileSignature, Globe, HardDrive, KeyRound, LogOut, MessageSquare,
  Receipt, Server, ShieldCheck, SlidersHorizontal, Clock, Plug, Mail, Copy, ImagePlus, Trash2, Monitor, RefreshCw,
  ChevronRight, Languages, Lock, SunMoon, UserRound, ScrollText, Search, Download, MonitorSmartphone, Smartphone, Check, X, Pencil,
  CalendarPlus, House, MapPin, Euro, Link2, UserRoundCheck,
} from "lucide-react";
import { useEffect } from "react";
import { api, downloadAccessLog, forgetRemembered, isErr, type DeviceRow, type LogRow, type Me } from "../lib/api";
import { METHODS } from "../lib/money";
import { emailOk, nifOk, phoneOk } from "../lib/validate";
import { eur, useApp } from "../lib/ctx";
import { useApi, CONSENT_KINDS, consentLabel, type ConsentKind, type ConsentText, type WebsiteInfo, type WebsitePerson, type WebsiteSite } from "../lib/data";
import { Layout, SubItem } from "../components/Layout";
import { LOG_LABEL } from "../components/Sheets";
import { Badge, DateInput, Field, Head, Seg, Sheet, Switch, TimeInput } from "../components/ui";
import { ReminderTemplate } from "../components/Reminders";
import { getTheme, setTheme, type Theme } from "../lib/theme";
import { checkForUpdate, installUpdate, onUpdateState, updateState, type UpdateState } from "../lib/updates";
import { isRecording } from "../lib/recorder";
import { version as APP_VERSION } from "../../package.json";

type Sec = "general" | "account" | "clinic" | "schedule" | "reminders" | "consents" | "website" | "security" | "devices" | "server" | "ai" | "invoicing" | "payments";

/** Set before going to Settings to open one section (e.g. Today's backup alert → "server"). */
export const settingsNav: { sec: string } = { sec: "" };

export default function Settings() {
  const { L, role } = useApp();
  const owner = role === "owner";
  const [sec, setSec] = useState<Sec>(() => { const x = settingsNav.sec as Sec; settingsNav.sec = ""; return x || "general"; });
  const all: [Sec, JSX.Element, string][] = [
    ["general", <SlidersHorizontal />, L("Visão geral", "Overview")], ["account", <UserRound />, L("Conta", "Account")], ["clinic", <Building2 />, L("Dados da clínica", "Clinic Details")], ["schedule", <CalendarClock />, L("Agenda", "Schedule")],
    ["reminders", <Bell />, L("Lembretes", "Reminders")], ["consents", <FileSignature />, L("Consentimentos", "Consents")], ["website", <Globe />, L("Site e portal", "Website and Portal")],
    ["security", <ShieldCheck />, L("Segurança", "Security")], ["devices", <MonitorSmartphone />, L("Dispositivos", "Devices")], ["server", <Server />, L("Servidor e cópias", "Server and Backups")], ["ai", <Cpu />, L("IA local", "Local AI")],
    ["invoicing", <Receipt />, L("Faturação", "Invoicing")], ["payments", <CreditCard />, L("Métodos de pagamento", "Payment Methods")],
  ];
  const secs = owner ? all : all.slice(0, 2);
  // The column follows the overview's groups and order (23 Nov 2026: it listed the same sections in another order).
  const groups: [string, Sec[]][] = [
    ["", ["general", "account"]],
    [L("Clínica", "Clinic"), ["clinic", "schedule", "reminders", "consents", "payments", "invoicing", "website"]],
    [L("Sistema", "System"), ["security", "devices", "server", "ai"]],
  ];
  const waiting = useApi<{ devices: unknown[] | null }>(owner ? "/alerts" : null).data?.devices?.length ?? 0;
  const side = groups.flatMap(([h, ks]) => {
    const items = ks.map((k) => all.find(([x]) => x === k)!).filter(([k]) => secs.some(([x]) => x === k));
    if (!items.length) return [];
    return [...(h ? [<p key={h} className="side-head">{h}</p>] : []),
      ...items.map(([k, ic, t]) => <SubItem key={k} icon={ic} on={sec === k} count={k === "devices" ? waiting : undefined} onClick={() => setSec(k)}>{t}</SubItem>)];
  });
  // Only sections this role can use (a link to an owner section opens the overview instead).
  useEffect(() => { if (!secs.some(([k]) => k === sec)) setSec("general"); }, [owner, sec]); // eslint-disable-line react-hooks/exhaustive-deps
  const title = all.find(([k]) => k === sec)![2];
  return (
    <Layout label={L("Definições", "Settings")} sub={secs.length > 1 ? side : undefined}>
      <div className="wrap" style={{ maxWidth: 720 }}>
        <Head title={title} />
        {sec === "general" && <General goTo={setSec} />}
        {sec === "account" && <Account />}
        {sec === "clinic" && <Clinic />}
        {sec === "schedule" && <ScheduleSet />}
        {sec === "reminders" && <Reminders />}
        {sec === "consents" && <ConsentsLib />}
        {sec === "website" && <Website />}
        {sec === "security" && <Security />}
        {sec === "devices" && <Devices />}
        {sec === "server" && <ServerSet />}
        {sec === "ai" && <AI />}
        {sec === "invoicing" && <Invoicing />}
        {sec === "payments" && <PayMethods />}
      </div>
    </Layout>
  );
}

function Row({ icon, title, desc, children }: { icon?: JSX.Element; title: string; desc?: string; children?: ReactNode }) {
  return <div className="setrow">{icon}<div className="grow"><div>{title}</div>{desc && <div className="d">{desc}</div>}</div>{children}</div>;
}
const roleName = (r: string, L: (pt: string, en: string) => string) => r === "owner" ? L("Proprietário", "Owner") : r === "therapist" ? L("Fisioterapeuta", "Therapist") : L("Receção", "Reception");
const roleColor = (r: string) => r === "owner" ? "var(--t1)" : r === "therapist" ? "var(--t2)" : "var(--t3)";

/** Settings home, grouped like the reference: profile card, then Account, Clinic, System.
 *  Only real values are shown (audit M1, C6): the signed-in person, and no made-up summaries. */
function General({ goTo }: { goTo: (s: Sec) => void }) {
  const { L, lang, setLang, role, me, openSheet } = useApp();
  const [theme, setThemeState] = useState<Theme>(getTheme);
  const owner = role === "owner";
  const alerts = useApi<{ devices: unknown[] | null }>(owner ? "/alerts" : null);
  const pendingDevices = alerts.data?.devices?.length ?? 0;
  const site = useApi<WebsiteInfo>(owner ? "/website" : null);
  const R = ({ icon, label, val, to }: { icon: JSX.Element; label: string; val?: string; to: Sec }) => (
    <button className="lrow" onClick={() => goTo(to)}>{icon}<span className="grow">{label}</span>{val && <span className="val">{val}</span>}<ChevronRight className="chev" /></button>
  );
  return (
    <>
      <div className="profile-card">
        <span className="avatar" style={{ ["--c" as string]: roleColor(me.role) }}>{me.name.split(" ").filter(Boolean).map((w) => w[0]).join("").slice(0, 2).toUpperCase()}</span>
        <div style={{ flex: 1 }}><h2>{me.name}</h2><p className="small muted">{me.email} · {roleName(me.role, L)}</p></div>
        <button className="btn sm" onClick={() => openSheet(<NameSheet />)}>{L("Alterar nome…", "Change Name…")}</button>
      </div>
      <p className="list-head">{L("Conta", "Account")}</p>
      <div className="group">
        <R icon={<Lock />} label={L("Palavra-passe e segurança", "Password and Security")} to="account" />
        <div className="lrow"><Languages /><span className="grow">{L("Idioma", "Language")}</span><Seg label={L("Idioma", "Language")} value={lang} onChange={setLang} options={[["pt", "Português"], ["en", "English"]]} /></div>
        <div className="lrow"><SunMoon /><span className="grow">{L("Aparência", "Appearance")}</span><Seg label={L("Aparência", "Appearance")} value={theme} onChange={(t) => { setThemeState(t); setTheme(t); }} options={[["system", L("Automático", "Automatic")], ["light", L("Claro", "Light")], ["dark", L("Escuro", "Dark")]]} /></div>
      </div>
      <p className="list-head">{L("Aplicação", "App")}</p>
      <div className="group"><AppUpdates /></div>
      {owner && (
        <>
          <p className="list-head">{L("Clínica", "Clinic")}</p>
          <div className="group">
            <R icon={<Building2 />} label={L("Dados da clínica", "Clinic Details")} to="clinic" />
            <R icon={<CalendarClock />} label={L("Agenda e encerramentos", "Schedule and Closures")} to="schedule" />
            <R icon={<Bell />} label={L("Lembretes aos utentes", "Patient Reminders")} to="reminders" />
            <R icon={<FileSignature />} label={L("Consentimentos", "Consents")} to="consents" />
            <R icon={<CreditCard />} label={L("Métodos de pagamento", "Payment Methods")} to="payments" />
            <R icon={<Receipt />} label={L("Faturação", "Invoicing")} to="invoicing" />
            <R icon={<Globe />} label={L("Site e portal", "Website and Portal")} val={site.data ? (site.data.site.published ? L("Publicado", "Published") : L("Não publicado", "Not Published")) : undefined} to="website" />
          </div>
          <p className="list-head">{L("Sistema", "System")}</p>
          <div className="group">
            <R icon={<ShieldCheck />} label={L("Segurança e registo de acessos", "Security and Access Log")} to="security" />
            <R icon={<MonitorSmartphone />} label={L("Dispositivos", "Devices")} val={pendingDevices ? L(`${pendingDevices} à espera`, `${pendingDevices} waiting`) : undefined} to="devices" />
            <R icon={<Server />} label={L("Servidor e cópias", "Server and Backups")} to="server" />
            <R icon={<Cpu />} label={L("IA local", "Local AI")} to="ai" />
          </div>
        </>
      )}
    </>
  );
}

/** Everyone: this computer's copy of the app and its updates (owner, 25 Nov 2026: "update the app through an
 *  in app button"). Releases come from GitHub and must carry the owner's signature (claude/RELEASING.md). */
export function updateErrorText(message: string, L: (pt: string, en: string) => string) {
  return /signature|minisign|verify/i.test(message)
    ? L("A atualização não tem uma assinatura válida, por isso não foi instalada.", "The update doesn’t have a valid signature, so it wasn’t installed.")
    : L("Não foi possível procurar atualizações. Verifique a ligação à internet e tente de novo.", "Couldn’t check for updates. Check the internet connection and try again.");
}

function AppUpdates() {
  const { L, confirm, toast } = useApp();
  const [st, setSt] = useState<UpdateState>(updateState);
  useEffect(() => onUpdateState(() => setSt(updateState())), []);
  const install = () => {
    if (st.kind !== "available") return;
    if (isRecording()) return toast(L("Termine a gravação da sessão antes de atualizar.", "Stop the session recording before updating."));
    confirm({
      title: L(`Atualizar para a versão ${st.info.version}?`, `Update to version ${st.info.version}?`),
      text: L("A aplicação descarrega a nova versão, confirma a assinatura, instala-a e abre de novo. Depois tem de entrar outra vez.",
        "The app downloads the new version, checks its signature, installs it and opens again. You’ll need to sign in again."),
      action: L("Atualizar e reiniciar", "Update and Restart"),
      onConfirm: () => { installUpdate(); },
    });
  };
  const desc = st.kind === "checking" ? L("A procurar…", "Checking…")
    : st.kind === "latest" ? L("Tem a versão mais recente.", "You have the latest version.")
    : st.kind === "available" ? (st.info.notes ? `${L(`Versão ${st.info.version} disponível.`, `Version ${st.info.version} is available.`)} ${st.info.notes}` : L(`Versão ${st.info.version} disponível.`, `Version ${st.info.version} is available.`))
    : st.kind === "installing" ? (st.pct === null ? L(`A descarregar a versão ${st.info.version}…`, `Downloading version ${st.info.version}…`) : L(`A descarregar a versão ${st.info.version}… ${st.pct}%`, `Downloading version ${st.info.version}… ${st.pct}%`))
    : st.kind === "error" ? updateErrorText(st.message, L)
    : L("As novas versões chegam do GitHub, assinadas pela clínica.", "New versions come from GitHub, signed by the clinic.");
  return (
    <div className="setrow">
      <RefreshCw />
      <div className="grow">
        <div>PhysioPulse {APP_VERSION}{st.kind === "available" && <> <Badge tone="a">{L("Nova versão", "New Version")}</Badge></>}</div>
        <div className="d" role="status" aria-live="polite" title={st.kind === "error" ? st.message : undefined}>{desc}</div>
        {st.kind === "installing" && <div className="bar" style={{ marginTop: 6 }} role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={st.pct ?? undefined}><i style={{ width: `${st.pct ?? 5}%` }} /></div>}
      </div>
      {st.kind === "available"
        ? <button className="btn sm pri" onClick={install}>{L("Atualizar…", "Update…")}</button>
        : <button className="btn sm" disabled={st.kind === "checking" || st.kind === "installing"} onClick={() => { checkForUpdate(); }}>{L("Procurar atualizações", "Check for Updates")}</button>}
    </div>
  );
}

/** Everyone: change their own name (how it shows in the schedule, notes and documents). */
function NameSheet() {
  const { L, me, setMe, toast } = useApp();
  const [name, setName] = useState(me.name);
  const clean = name.trim().replace(/\s+/g, " ");
  return (
    <Sheet title={L("Alterar nome", "Change Name")} sub={L("É o nome que aparece na agenda, nas notas que assina e nos documentos.", "It’s the name shown in the schedule, on the notes you sign and on documents.")}
      action={L("Guardar", "Save")} disabled={!clean || clean === me.name}
      onAction={async () => { const r = await api<Me>("PATCH", "/me", { name: clean }); setMe({ id: r.id, name: r.name, email: r.email, role: r.role }); toast(L("Nome alterado", "Name changed")); }}>
      <Field label={L("Nome", "Name")}><input className="input" value={name} onChange={(e) => setName(e.target.value)} maxLength={80} autoComplete="name" autoFocus /></Field>
    </Sheet>
  );
}

/** Everyone: change their own password. The other computers they're signed in on are signed out. */
function PasswordSheet() {
  const { L, toast } = useApp();
  const [cur, setCur] = useState("");
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const short = pw.length > 0 && pw.length < 12;
  const differ = pw2.length > 0 && pw !== pw2;
  return (
    <Sheet title={L("Mudar palavra-passe", "Change Password")} sub={L("Os outros computadores onde tem sessão aberta são desligados. Este fica.", "Other computers where you’re signed in are signed out. This one stays.")}
      action={L("Mudar palavra-passe", "Change Password")} disabled={!cur || pw.length < 12 || pw !== pw2}
      onAction={async () => { await api("POST", "/me/password", { current: cur, new: pw }); toast(L("Palavra-passe alterada", "Password changed")); }}>
      <input type="email" autoComplete="username" hidden readOnly value="" />
      <Field label={L("Palavra-passe atual", "Current password")}><input className="input" type="password" autoComplete="current-password" value={cur} onChange={(e) => setCur(e.target.value)} autoFocus /></Field>
      <Field label={L("Nova palavra-passe", "New password")}><input className="input" type="password" autoComplete="new-password" value={pw} onChange={(e) => setPw(e.target.value)} aria-invalid={short || undefined} aria-describedby="pw-hint" /></Field>
      <p id="pw-hint" className="small" style={{ margin: "-6px 0 10px", color: short ? "var(--bad)" : "var(--text3)" }}>{short ? L(`Faltam ${12 - pw.length} caracteres (mínimo 12).`, `${12 - pw.length} more characters needed (12 minimum).`) : L("Pelo menos 12 caracteres. Uma frase curta é fácil de lembrar.", "At least 12 characters. A short phrase is easy to remember.")}</p>
      <Field label={L("Confirmar nova palavra-passe", "Confirm new password")}><input className="input" type="password" autoComplete="new-password" value={pw2} onChange={(e) => setPw2(e.target.value)} aria-invalid={differ || undefined} /></Field>
      {differ && <p className="small" style={{ margin: "-6px 0 0", color: "var(--bad)" }}>{L("As palavras-passe não coincidem.", "The passwords don’t match.")}</p>}
    </Sheet>
  );
}

/** Owner: ten new recovery codes (the old ones stop working). Asks for the password, then shows them once. */
function RecoveryCodesSheet({ onDone }: { onDone: () => void }) {
  const { L, toast } = useApp();
  const [pw, setPw] = useState("");
  const [codes, setCodes] = useState<string[] | null>(null);
  if (codes) return (
    <Sheet title={L("Códigos de recuperação", "Recovery Codes")} sub={L("Cada código serve uma vez, se perder o telemóvel ou esquecer a palavra-passe. Imprima-os ou escreva-os e guarde-os longe do computador. Só são mostrados agora.", "Each code works once, if you lose your phone or forget your password. Print or write them down and keep them away from the computer. They’re shown only now.")}>
      <div className="codes-grid num">{codes.map((c) => <span key={c}>{c}</span>)}</div>
      <button className="btn" onClick={() => { navigator.clipboard?.writeText(codes.join("\n")); toast(L("Códigos copiados", "Codes copied")); }}><Copy />{L("Copiar códigos", "Copy Codes")}</button>
    </Sheet>
  );
  return (
    <Sheet title={L("Criar códigos de recuperação", "Make Recovery Codes")} sub={L("Cria 10 códigos novos; os anteriores deixam de funcionar.", "Makes 10 new codes; the old ones stop working.")}
      action={L("Criar códigos", "Make Codes")} disabled={!pw}
      onAction={async () => { const r = await api<{ codes: string[] }>("POST", "/me/recovery-codes", { password: pw }); setCodes(r.codes); onDone(); return false; }}>
      <Field label={L("A sua palavra-passe", "Your password")}><input className="input" type="password" autoComplete="current-password" value={pw} onChange={(e) => setPw(e.target.value)} autoFocus /></Field>
    </Sheet>
  );
}

/** Settings › Account: everyone's own name, password and two-step; the owner's recovery codes. */
function Account() {
  const { L, role, me, openSheet, toast } = useApp();
  const owner = role === "owner";
  const rc = useApi<{ left: number; total: number; created_at: string | null }>(owner ? "/me/recovery-codes" : null);
  return (
    <>
      <div className="group">
        <Row icon={<UserRound />} title={L("Nome", "Name")} desc={me.name}><button className="btn sm" onClick={() => openSheet(<NameSheet />)}>{L("Alterar…", "Change…")}</button></Row>
        <Row icon={<Mail />} title="Email" desc={`${me.email} — ${L("é com ele que entra.", "you sign in with it.")}`} />
        <Row icon={<Lock />} title={L("Palavra-passe", "Password")} desc={L("Pelo menos 12 caracteres.", "At least 12 characters.")}><button className="btn sm" onClick={() => openSheet(<PasswordSheet />)}>{L("Mudar…", "Change…")}</button></Row>
        <Row icon={<KeyRound />} title={L("Verificação em dois passos", "Two-step verification")} desc={owner
          ? L("Sempre ligada. Se perder o telemóvel, entre com um código de recuperação.", "Always on. If you lose your phone, sign in with a recovery code.")
          : L("Sempre ligada. Se perder o telemóvel, o proprietário repõe-na em Equipa.", "Always on. If you lose your phone, the owner resets it in Team.")}><Badge tone="ok">{L("Ligada", "On")}</Badge></Row>
      </div>
      {owner && (
        <div className="group">
          <Row icon={<ShieldCheck />} title={L("Códigos de recuperação", "Recovery codes")}
            desc={!rc.data ? "…" : rc.data.total === 0 ? L("Ainda não tem. Sem eles, perder o telemóvel ou a palavra-passe obriga a mexer no servidor.", "You don’t have any yet. Without them, losing your phone or password means fixing it on the server.")
              : L(`${rc.data.left} de ${rc.data.total} por usar. Cada um serve uma vez para entrar sem o telemóvel ou para repor a palavra-passe.`, `${rc.data.left} of ${rc.data.total} unused. Each works once to sign in without your phone or to reset your password.`)}>
            {rc.data && rc.data.left <= 3 && <Badge tone="warn">{rc.data.left === 0 ? L("Nenhum", "None left") : L("Poucos", "Few left")}</Badge>}
            <button className="btn sm" onClick={() => openSheet(<RecoveryCodesSheet onDone={rc.reload} />)}>{L("Criar novos…", "Make New…")}</button>
          </Row>
        </div>
      )}
      <div className="group">
        <Row icon={<Monitor />} title={L("Este computador", "This computer")} desc={L("Depois do código uma vez, este computador só pede a palavra-passe durante 30 dias.", "After the code once, this computer only asks for your password for 30 days.")}>
          <button className="btn sm" onClick={async () => { await forgetRemembered(me.email); toast(L("Este computador vai pedir o código na próxima entrada.", "This computer will ask for the code next time you sign in.")); }}>{L("Esquecer este computador", "Forget This Computer")}</button>
        </Row>
      </div>
    </>
  );
}

/** Owner: the clinic's details, printed at the top of every PDF (audit C5). */
interface ClinicInfo { name: string; nif: string; ers_number: string; address: string; phone: string; email: string; logo: string | null }
function readImage(file: File): Promise<string> {
  // Any picture → a JPEG on white, at most 600 × 240 px (the server keeps JPEG only, under 512 KB).
  return new Promise((ok, bad) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      const k = Math.min(1, 600 / img.width, 240 / img.height);
      const c = document.createElement("canvas"); c.width = Math.max(1, Math.round(img.width * k)); c.height = Math.max(1, Math.round(img.height * k));
      const g = c.getContext("2d")!; g.fillStyle = "#fff"; g.fillRect(0, 0, c.width, c.height); g.drawImage(img, 0, 0, c.width, c.height);
      URL.revokeObjectURL(url); ok(c.toDataURL("image/jpeg", 0.9));
    };
    img.onerror = () => { URL.revokeObjectURL(url); bad(new Error("image")); };
    img.src = url;
  });
}
function Clinic() {
  const { L, toast } = useApp();
  const cur = useApi<ClinicInfo>("/clinic");
  const [v, setV] = useState<ClinicInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [touched, setTouched] = useState<Record<string, boolean>>({});
  useEffect(() => { if (cur.data && !v) setV(cur.data); }, [cur.data]); // eslint-disable-line react-hooks/exhaustive-deps
  const file = useRef<HTMLInputElement>(null);
  if (cur.error) return <p className="err" role="alert">{cur.error}</p>;
  if (!v || !cur.data) return <div className="skel" style={{ height: 300 }} />;
  const set = (k: keyof ClinicInfo) => (e: { target: { value: string } }) => { setV({ ...v, [k]: e.target.value }); setErr(""); };
  const blur = (k: string) => () => setTouched({ ...touched, [k]: true });
  const nif = v.nif.replace(/\s/g, "");
  const bad = {
    name: !v.name.trim() ? L("Escreva o nome da clínica.", "Write the clinic’s name.") : "",
    nif: nif && !nifOk(nif) ? L("Esse NIF não é válido: confirme os 9 dígitos.", "That NIF isn’t valid: check the 9 digits.") : "",
    email: v.email.trim() && !emailOk(v.email) ? L("Escreva um email como geral@clinica.pt.", "Write an email like info@clinic.pt.") : "",
    phone: v.phone.trim() && !phoneOk(v.phone) ? L("Escreva um número com pelo menos 9 dígitos.", "Write a number with at least 9 digits.") : "",
  };
  const changed = JSON.stringify(v) !== JSON.stringify(cur.data);
  const invalid = Object.values(bad).some(Boolean);
  const hint = (k: keyof typeof bad) => (touched[k] || k === "name") && bad[k] ? <p className="small" role="alert" style={{ margin: "-6px 0 10px", color: "var(--bad)" }}>{bad[k]}</p> : null;
  const save = async () => {
    setBusy(true); setErr("");
    try {
      const body: Record<string, unknown> = { name: v.name, nif, ers_number: v.ers_number, address: v.address, phone: v.phone, email: v.email };
      if (v.logo !== cur.data!.logo) body.logo = v.logo;
      const r = await api<ClinicInfo>("PUT", "/clinic", body);
      setV(r); cur.reload(); toast(L("Dados da clínica guardados", "Clinic details saved"));
    } catch (e) { setErr(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  return (
    <>
      <p className="sub" style={{ marginBottom: 14 }}>{L("Aparecem no topo de todos os documentos: notas assinadas, consentimentos, cartas de alta, pacotes de reembolso e exportações.", "They appear at the top of every document: signed notes, consents, discharge letters, reimbursement packs and exports.")}</p>
      <div className="group pad">
        <Field label={L("Nome da clínica", "Clinic name")}><input className="input" value={v.name} onChange={set("name")} maxLength={120} aria-invalid={!!bad.name || undefined} /></Field>
        {hint("name")}
        <div className="grid2">
          <div><Field label={L("NIF / NIPC", "Tax number (NIF)")}><input className="input num" value={v.nif} onChange={set("nif")} onBlur={blur("nif")} inputMode="numeric" maxLength={11} placeholder="5XX XXX XXX" aria-invalid={(touched.nif && !!bad.nif) || undefined} /></Field>{hint("nif")}</div>
          <div><Field label={L("Registo ERS", "ERS registration")}><input className="input" value={v.ers_number} onChange={set("ers_number")} maxLength={30} placeholder="E123456" /></Field></div>
        </div>
        <Field label={L("Morada", "Address")}><textarea className="input" rows={2} value={v.address} onChange={set("address")} maxLength={300} placeholder={L("Rua, n.º, código postal, localidade", "Street, number, postcode, town")} /></Field>
        <div className="grid2">
          <div><Field label={L("Telefone", "Phone")}><input className="input" value={v.phone} onChange={set("phone")} onBlur={blur("phone")} inputMode="tel" maxLength={30} placeholder="+351 210 000 000" aria-invalid={(touched.phone && !!bad.phone) || undefined} /></Field>{hint("phone")}</div>
          <div><Field label="Email"><input className="input" type="email" value={v.email} onChange={set("email")} onBlur={blur("email")} maxLength={120} placeholder="geral@clinica.pt" aria-invalid={(touched.email && !!bad.email) || undefined} /></Field>{hint("email")}</div>
        </div>
        <div className="field"><span>{L("Logótipo", "Logo")}</span>
          <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
            {v.logo ? <img src={v.logo} alt={L("Logótipo da clínica", "Clinic logo")} style={{ maxHeight: 48, maxWidth: 160, borderRadius: 6, background: "#fff", boxShadow: "0 0 0 .5px var(--line2)" }} /> : <span className="small muted">{L("Sem logótipo", "No logo")}</span>}
            <button type="button" className="btn sm" onClick={() => file.current?.click()}><ImagePlus />{v.logo ? L("Trocar imagem…", "Replace Image…") : L("Escolher imagem…", "Choose Image…")}</button>
            {v.logo && <button type="button" className="btn sm" onClick={() => setV({ ...v, logo: null })}><Trash2 />{L("Remover", "Remove")}</button>}
            <input ref={file} type="file" accept="image/png,image/jpeg,image/webp" hidden onChange={async (e) => {
              const f = e.target.files?.[0]; e.target.value = "";
              if (!f) return;
              try { setV({ ...v, logo: await readImage(f) }); } catch { setErr(L("Não foi possível ler essa imagem. Use PNG ou JPEG.", "Couldn’t read that picture. Use PNG or JPEG.")); }
            }} />
          </div>
        </div>
        {err && <p className="err" role="alert">{err}</p>}
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 6 }}>
          {changed && <button className="btn" onClick={() => { setV(cur.data!); setErr(""); setTouched({}); }} disabled={busy}>{L("Repor", "Revert")}</button>}
          <button className="btn pri" disabled={!changed || invalid || busy} aria-busy={busy} onClick={save}>{L("Guardar", "Save")}</button>
        </div>
      </div>
    </>
  );
}

type Day = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun";
type Hours = Record<Day, [string, string] | null>;
interface Closure { id: string; starts_on: string; ends_on: string; label: string; days: number }
const DAYS: [Day, [string, string], [string, string]][] = [
  ["mon", ["Segunda-feira", "Monday"], ["Seg", "Mon"]], ["tue", ["Terça-feira", "Tuesday"], ["Ter", "Tue"]], ["wed", ["Quarta-feira", "Wednesday"], ["Qua", "Wed"]],
  ["thu", ["Quinta-feira", "Thursday"], ["Qui", "Thu"]], ["fri", ["Sexta-feira", "Friday"], ["Sex", "Fri"]], ["sat", ["Sábado", "Saturday"], ["Sáb", "Sat"]], ["sun", ["Domingo", "Sunday"], ["Dom", "Sun"]],
];

/** "Seg–Sex 08:00–20:00, Sáb 09:00–13:00": consecutive days with the same hours are joined. */
function hoursSummary(h: Hours, L: (pt: string, en: string) => string) {
  const parts: string[] = [];
  let i = 0;
  while (i < 7) {
    const v = h[DAYS[i][0]];
    let j = i;
    while (j + 1 < 7 && JSON.stringify(h[DAYS[j + 1][0]]) === JSON.stringify(v)) j++;
    if (v) parts.push(`${L(DAYS[i][2][0], DAYS[i][2][1])}${j > i ? `–${L(DAYS[j][2][0], DAYS[j][2][1])}` : ""} ${v[0]}–${v[1]}`);
    i = j + 1;
  }
  return parts.join(", ") || L("Sempre fechado", "Always closed");
}
const dmy = (iso: string) => `${iso.slice(8, 10)}/${iso.slice(5, 7)}/${iso.slice(0, 4)}`;

/** Owner: opening hours, day by day. Closed days have no hours. */
function HoursSheet({ hours }: { hours: Hours }) {
  const { L, toast } = useApp();
  const [h, setH] = useState<Hours>(hours);
  const bad = DAYS.some(([d]) => { const v = h[d]; return !!v && !(v[0] < v[1]); });
  return (
    <Sheet title={L("Horário de funcionamento", "Opening Hours")} sub={L("Os encerramentos usam este horário. Não mexe nas marcações que já existem.", "Closures use these hours. Appointments already booked aren’t changed.")}
      action={L("Guardar", "Save")} disabled={bad}
      onAction={async () => { await api("PUT", "/clinic/hours", { hours: h }); toast(L("Horário guardado", "Opening hours saved")); }}>
      <div className="hours-grid">
        {DAYS.map(([d, name]) => {
          const v = h[d];
          return (
            <Fragment key={d}>
              <label className="check"><input type="checkbox" checked={!!v} onChange={(e) => setH({ ...h, [d]: e.target.checked ? ["09:00", "18:00"] : null })} />{L(name[0], name[1])}</label>
              {v ? <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <TimeInput value={v[0]} step={1800} from={6} to={23} onChange={(e) => setH({ ...h, [d]: [e.target.value, v[1]] })} aria-label={L(`${name[0]}: abre`, `${name[1]}: opens`)} />
                <span className="to">–</span>
                <TimeInput value={v[1]} step={1800} from={6} to={23} onChange={(e) => setH({ ...h, [d]: [v[0], e.target.value] })} aria-label={L(`${name[0]}: fecha`, `${name[1]}: closes`)} />
              </span> : <span className="muted small">{L("Fechado", "Closed")}</span>}
              <span className="small" style={{ color: "var(--bad)" }}>{v && !(v[0] < v[1]) ? L("Fecha antes de abrir", "Closes before it opens") : ""}</span>
            </Fragment>
          );
        })}
      </div>
    </Sheet>
  );
}

/** Owner: close the clinic for one or more days (each open day is blocked in the schedule). */
function CloseDaysSheet() {
  const { L, toast } = useApp();
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [label, setLabel] = useState("");
  const [check, setCheck] = useState<{ open_days: number; booked: number } | null>(null);
  const [err, setErr] = useState("");
  const end = to || from;
  useEffect(() => {
    setCheck(null); setErr("");
    if (!from || !end) return;
    api<{ open_days: number; booked: number }>("GET", `/closures/check?starts_on=${from}&ends_on=${end}`).then(setCheck).catch((e) => setErr(isErr(e) ? e.message : String(e)));
  }, [from, end]);
  return (
    <Sheet title={L("Fechar a clínica", "Close the Clinic")} sub={L("Bloqueia a agenda nesses dias, durante o horário de funcionamento.", "Blocks the schedule on those days, during opening hours.")}
      action={L("Fechar dias", "Close Days")} disabled={!check}
      onAction={async () => {
        const r = await api<{ days: number; booked: number }>("POST", "/closures", { starts_on: from, ends_on: end, label: label.trim() });
        toast(L(`${r.days} dia${r.days === 1 ? "" : "s"} fechado${r.days === 1 ? "" : "s"}`, `${r.days} day${r.days === 1 ? "" : "s"} closed`));
      }}>
      <div className="grid2">
        <Field label={L("Primeiro dia", "First day")}><DateInput value={from} onChange={(e) => { setFrom(e.target.value); if (!to || to < e.target.value) setTo(e.target.value); }} /></Field>
        <Field label={L("Último dia", "Last day")}><DateInput value={to} min={from} onChange={(e) => setTo(e.target.value)} /></Field>
      </div>
      <Field label={L("Motivo (opcional)", "Reason (optional)")}><input className="input" value={label} onChange={(e) => setLabel(e.target.value)} maxLength={80} placeholder={L("Ex.: Natal, formação, obras", "E.g. Christmas, training, works")} /></Field>
      {err && <p className="err" role="alert">{err}</p>}
      {check && <p className="small" role="status">{check.open_days === 0 ? L("A clínica já está fechada nesses dias.", "The clinic is already closed on those days.")
        : L(`${check.open_days} dia${check.open_days === 1 ? "" : "s"} de funcionamento.`, `${check.open_days} opening day${check.open_days === 1 ? "" : "s"}.`)}
        {check.booked > 0 && <> <Badge tone="warn">{L(`${check.booked} marcaç${check.booked === 1 ? "ão" : "ões"}`, `${check.booked} appointment${check.booked === 1 ? "" : "s"}`)}</Badge> {L("continuam marcadas: remarque-as na Agenda.", "stay booked: reschedule them in Schedule.")}</>}</p>}
    </Sheet>
  );
}

function ScheduleSet() {
  const { L, role, openSheet, confirm, toast } = useApp();
  const hc = useApi<{ hours: Hours; closures: Closure[] }>("/clinic/hours");
  const owner = role === "owner";
  const reopen = (c: Closure) => confirm({
    title: L("Reabrir estes dias?", "Reopen these days?"),
    text: `${c.starts_on === c.ends_on ? dmy(c.starts_on) : `${dmy(c.starts_on)} – ${dmy(c.ends_on)}`}${c.label ? ` · ${c.label}` : ""}. ${L("O bloqueio sai da agenda.", "The blocked time leaves the schedule.")}`,
    action: L("Reabrir", "Reopen"), destructive: true, hold: true,
    onConfirm: () => api("DELETE", `/closures/${c.id}`).then(() => toast(L("Dias reabertos", "Days reopened"))).catch((e) => toast(isErr(e) ? e.message : String(e))),
  });
  return (
    <>
      <div className="group">
        <Row title={L("Horário de funcionamento", "Opening hours")} desc={hc.data ? hoursSummary(hc.data.hours, L) : undefined}>
          {owner && hc.data && <button className="btn sm" onClick={() => openSheet(<HoursSheet hours={hc.data!.hours} />)}>{L("Alterar…", "Change…")}</button>}
        </Row>
        {/* Fixed rules, shown as facts (audit C6: a control is either wired or absent). */}
        <Row title={L("Sessões sobrepostas", "Overlapping sessions")} desc={L("A agenda avisa quando um fisioterapeuta já tem alguém à mesma hora e deixa marcar na mesma.", "The schedule warns when a therapist already has someone at that time, and lets you book anyway.")} />
        <Row title={L("Lista de espera", "Waitlist")} desc={L("Em Agenda › Lista de espera. Oferecer vagas canceladas por SMS automaticamente chega mais tarde.", "In Schedule › Waitlist. Offering cancelled slots by SMS automatically comes later.")}><Badge>{L("Automático em breve", "Automatic Soon")}</Badge></Row>
      </div>
      <div className="group-title">{L("Encerramentos", "Closures")}</div>
      <div className="group">
        {hc.data && hc.data.closures.length === 0 && <Row title={L("Sem encerramentos marcados", "No closures planned")} desc={L("Feche dias de férias, formação ou obras; ficam bloqueados na agenda.", "Close days for holidays, training or works; they’re blocked in the schedule.")} />}
        {hc.data?.closures.map((c) => (
          <Row key={c.id} title={c.starts_on === c.ends_on ? dmy(c.starts_on) : `${dmy(c.starts_on)} – ${dmy(c.ends_on)}`} desc={c.label || undefined}>
            {owner && <button className="btn sm" onClick={() => reopen(c)}>{L("Reabrir…", "Reopen…")}</button>}
          </Row>
        ))}
        {owner && <Row title=""><button className="btn sm" onClick={() => openSheet(<CloseDaysSheet />)}>{L("Fechar a clínica…", "Close the Clinic…")}</button></Row>}
      </div>
    </>
  );
}

function Reminders() {
  const { L } = useApp();
  return (
    <>
      <p className="sub" style={{ marginBottom: 14 }}>{L(
        "Por agora os lembretes saem do telemóvel ou computador de quem está na clínica: em Agenda › Lembretes, cada marcação de amanhã tem um botão que abre o WhatsApp, as Mensagens ou o email com a mensagem pronta.",
        "For now reminders go from the phone or computer of whoever is at the clinic: in Schedule › Reminders, each of tomorrow’s appointments has a button that opens WhatsApp, Messages or email with the message ready.")}</p>
      <div className="group">
        <Row icon={<MessageSquare />} title={L("Enviados à mão", "Sent by hand")} desc={L("WhatsApp, SMS (Mensagens) ou email deste computador.", "WhatsApp, SMS (Messages) or email from this computer.")}><Badge tone="ok">{L("Ativo", "On")}</Badge></Row>
        <Row icon={<Bell />} title={L("Envio automático por SMS", "Automatic SMS")} desc={L("Precisa de um fornecedor de SMS.", "Needs an SMS provider.")}><Badge>{L("Brevemente", "Coming Soon")}</Badge></Row>
      </div>
      <ReminderTemplate />
    </>
  );
}

function ConsentsLib() {
  const { L, lang, openSheet } = useApp();
  const { data: texts, error } = useApi<ConsentText[]>("/consent-texts");
  const cur = (k: ConsentKind, l: "pt" | "en") => texts?.find((t) => t.kind === k && t.lang === l && t.current);
  const draftOf = (k: ConsentKind, l: "pt" | "en") => newest(texts ?? [], k, l)?.draft ? newest(texts ?? [], k, l) : undefined;
  const pending = CONSENT_KINDS.filter(([k]) => draftOf(k, "pt") || draftOf(k, "en")).length;
  return (
    <>
      {error && <p className="err">{error}</p>}
      {pending > 0 && (
        <p className="sub" style={{ marginBottom: 14 }}>{L(
          `${pending === 1 ? "Há 1 texto" : `Há ${pending} textos`} por aprovar. Foram escritos com base na lei portuguesa (Norma DGS 015/2013, RGPD). Leia cada um, altere o que quiser e aprove — só depois pode ser assinado.`,
          `${pending === 1 ? "1 text is" : `${pending} texts are`} waiting for your approval. They were written from Portuguese law (DGS standard 015/2013, GDPR). Read each one, change what you like and approve it — only then can it be signed.`)}</p>
      )}
      <div className="group">
        {CONSENT_KINDS.map(([k]) => {
          const pt = cur(k, "pt"), en = cur(k, "en"), any = pt ?? en;
          const waiting = (["pt", "en"] as const).filter((l) => draftOf(k, l));
          return (
            <Row key={k} title={consentLabel(k, lang)}
              desc={any ? `${L("Versão", "Version")} ${pt ? `PT ${pt.version}` : ""}${pt && en ? " · " : ""}${en ? `EN ${en.version}` : ""} — ${any.valid_months ? L(`rever ao fim de ${any.valid_months} meses`, `review after ${any.valid_months} months`) : L("sem validade", "no expiry")}`
                : waiting.length ? L("Rascunho: ainda não pode ser assinado.", "Draft: it can’t be signed yet.")
                : texts ? L("Ainda sem texto: não pode ser assinado.", "No text yet: it can’t be signed.") : "…"}>
              {waiting.length > 0 && <Badge tone="warn">{L(`Por aprovar (${waiting.map((l) => l.toUpperCase()).join(", ")})`, `To approve (${waiting.map((l) => l.toUpperCase()).join(", ")})`)}</Badge>}
              <button className="btn sm" onClick={() => openSheet(<ConsentTextSheet kind={k} texts={texts ?? []} start={waiting[0]} />)}>
                {waiting.length ? L("Rever…", "Review…") : any ? L("Editar…", "Edit…") : L("Escrever…", "Write…")}
              </button>
            </Row>
          );
        })}
      </div>
      <p className="small muted">{L("Alterar um texto cria uma nova versão; os utentes que assinaram a anterior ficam com a sua. Cada assinatura é guardada em PDF na pasta do utente.", "Editing a text creates a new version; patients who signed the old one keep theirs. Every signature is saved as PDF in the patient’s folder.")}</p>
    </>
  );
}

/** The newest version of a text (approved or not). */
const newest = (texts: ConsentText[], kind: ConsentKind, l: "pt" | "en") =>
  texts.filter((t) => t.kind === kind && t.lang === l).sort((a, b) => b.version - a.version)[0];

/** Owner: review a draft (approve it as it is, or change and approve), or write a new version. */
function ConsentTextSheet({ kind, texts, start }: { kind: ConsentKind; texts: ConsentText[]; start?: "pt" | "en" }) {
  const { L, lang, toast } = useApp();
  const [tl, setTl] = useState<"pt" | "en">(start ?? lang);
  const base = (l: "pt" | "en") => newest(texts, kind, l);
  const [title, setTitle] = useState(base(tl)?.title ?? consentLabel(kind, tl));
  const [body, setBody] = useState(base(tl)?.body ?? "");
  const [months, setMonths] = useState(String(base(tl)?.valid_months ?? ""));
  const switchLang = (l: "pt" | "en") => { setTl(l); const c = base(l); setTitle(c?.title ?? consentLabel(kind, l)); setBody(c?.body ?? ""); setMonths(String(c?.valid_months ?? months)); };
  const c = base(tl);
  const changed = !c || c.title !== title.trim() || c.body !== body.trim() || String(c.valid_months ?? "") !== months;
  const isDraft = !!c?.draft;
  const sub = isDraft
    ? L(`Rascunho (versão ${c!.version}), ainda não aprovado. Aprovar torna-o o texto que os utentes assinam.`, `Draft (version ${c!.version}), not approved yet. Approving makes it the text patients sign.`)
    : c ? L(`Versão atual: ${c.version}. Guardar cria a versão ${c.version + 1}.`, `Current version: ${c.version}. Saving creates version ${c.version + 1}.`)
      : L("Primeira versão deste texto.", "First version of this text.");
  const action = isDraft ? (changed ? L("Guardar e aprovar", "Save and Approve") : L("Aprovar", "Approve")) : L("Guardar nova versão", "Save New Version");
  return (
    <Sheet wide title={consentLabel(kind, lang)} sub={sub} action={action} disabled={!title.trim() || !body.trim() || (!isDraft && !changed)}
      onAction={async () => {
        if (isDraft && !changed) { await api("POST", `/consent-texts/${c!.id}/approve`); toast(L("Texto aprovado", "Text approved")); return; }
        await api("POST", "/consent-texts", { kind, lang: tl, title: title.trim(), body: body.trim(), valid_months: months ? +months : null });
        toast(isDraft ? L("Texto guardado e aprovado", "Text saved and approved") : L("Nova versão guardada", "New version saved"));
      }}>
      <div style={{ marginBottom: 12, display: "flex", alignItems: "center", gap: 10 }}>
        <Seg label={L("Idioma do texto", "Text language")} value={tl} onChange={switchLang} options={[["pt", "Português"], ["en", "English"]]} />
        {isDraft && <Badge tone="warn">{L("Rascunho", "Draft")}</Badge>}
      </div>
      <Field label={L("Título", "Title")}><input className="input" value={title} onChange={(e) => setTitle(e.target.value)} /></Field>
      <Field label={L("Texto que o utente lê e assina", "Text the patient reads and signs")}><textarea className="input" style={{ minHeight: 260 }} value={body} onChange={(e) => setBody(e.target.value)} /></Field>
      <Field label={L("Rever ao fim de", "Review after")}>
        <select className="input" value={months} onChange={(e) => setMonths(e.target.value)}>
          <option value="">{L("Não expira", "Doesn’t expire")}</option>{[6, 12, 24, 36].map((m) => <option key={m} value={m}>{L(`${m} meses`, `${m} months`)}</option>)}
        </select>
      </Field>
    </Sheet>
  );
}

/** Owner: the public website (phase 4a). The site is made by the clinic server from these settings, the clinic's
 *  details and the price list; requests from it land in Schedule › Booking Requests. Patient accounts come in 4b. */
function Website() {
  const { L, lang, toast, confirm, openSheet } = useApp();
  const w = useApi<WebsiteInfo>("/website");
  const [busy, setBusy] = useState(false);
  if (w.error) return <p className="err" role="alert">{w.error}</p>;
  if (!w.data) return <div className="skel" style={{ height: 300 }} />;
  const { site, team, products, clinic, site_url } = w.data;
  const put = async (body: Partial<WebsiteSite>, msg: string, undo?: Partial<WebsiteSite>, undoLabel?: string) => {
    setBusy(true);
    try {
      await api("PUT", "/website", body); w.reload();
      toast(msg, undo ? () => { api("PUT", "/website", undo).then(() => w.reload()).catch((e) => toast(isErr(e) ? e.message : String(e))); } : undefined, undoLabel);
    } catch (e) { toast(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  const flip = (k: keyof WebsiteSite, on: string, off: string, label: string) => (v: boolean) => put({ [k]: v }, v ? on : off, { [k]: !v }, label);
  const publish = (v: boolean) => {
    if (!v) return put({ published: false }, L("Site retirado: mostra só “Em preparação”", "Website taken down: it shows “Being prepared” only"), { published: true }, L("Retirar site", "Take Down Website"));
    confirm({
      title: L("Publicar o site?", "Publish the Website?"),
      text: L(`Fica visível para qualquer pessoa${site_url ? ` em ${site_url}` : ""}, com os preços, a equipa marcada e o formulário de pedidos. A política de privacidade e os termos são rascunhos: leia-os primeiro.`,
              `Anyone will be able to see it${site_url ? ` at ${site_url}` : ""}, with the prices, the team you chose and the request form. The privacy policy and terms are drafts: read them first.`),
      action: L("Publicar", "Publish"),
      onConfirm: () => put({ published: true }, L("Site publicado", "Website published"), { published: false }, L("Publicar site", "Publish Website")),
    });
  };
  const missing = !clinic.name.trim() || !clinic.nif.trim() || (!clinic.phone.trim() && !clinic.email.trim());
  const shown = products.filter((p) => p.web_visible).length;
  return (
    <>
      <p className="sub" style={{ marginBottom: 14 }}>{L("O site público da clínica, em português e inglês. Não tem cookies, estatísticas nem conteúdos de outras empresas, e os preços vêm da tabela em Finanças.", "The clinic's public website, in Portuguese and English. It has no cookies, statistics or content from other companies, and the prices come from the price list in Finance.")}</p>
      <p className="list-head">{L("Publicação", "Publishing")}</p>
      <div className="group">
        <Row icon={<Globe />} title={L("Site publicado", "Website published")}
          desc={missing && !site.published ? L("Para publicar, preencha o nome, o NIF e um contacto em Clínica.", "To publish, fill in the name, NIF and a contact in Clinic.")
            : site.published ? L("Qualquer pessoa o pode ver.", "Anyone can see it.") : L("Enquanto não estiver publicado, mostra só “Em preparação” e os contactos.", "Until it's published, it shows only “Being prepared” and the contacts.")}>
          <Switch on={site.published} onChange={publish} label={L("Site publicado", "Website published")} />
        </Row>
        {site_url && (
          <Row icon={<Link2 />} title={L("Endereço", "Address")} desc={site_url}>
            <button className="btn sm" onClick={() => { navigator.clipboard?.writeText(site_url).then(() => toast(L("Endereço copiado", "Address copied"))).catch(() => toast(L("Não foi possível copiar.", "Couldn’t copy."))); }}><Copy />{L("Copiar", "Copy")}</button>
          </Row>
        )}
        <Row icon={<CalendarPlus />} title={L("Pedidos de marcação", "Booking requests")}
          desc={L(`Chegam a Agenda › Pedidos de marcação para aceitar ou recusar. ${w.data.requests_30d} nos últimos 30 dias.`, `They arrive in Schedule › Booking Requests to accept or decline. ${w.data.requests_30d} in the last 30 days.`)}>
          <Switch on={site.requests_open} onChange={flip("requests_open", L("Pedidos pelo site ligados", "Website requests turned on"), L("Pedidos pelo site desligados", "Website requests turned off"), L("Pedidos pelo site", "Website Requests"))} label={L("Pedidos de marcação pelo site", "Booking requests on the website")} />
        </Row>
        <Row icon={<UserRound />} title={L("Área de utente", "Patient area")}
          desc={L("Os utentes com convite entram no site para ver as sessões, pedir horas, cancelar até 24 h antes e descarregar faturas. Nunca veem notas nem testes. Os convites fazem-se na ficha do utente (⋯ › Área de utente).", "Invited patients sign in on the website to see their sessions, ask for times, cancel up to 24 h before and download invoices. They never see notes or tests. Invites are made in the patient's file (⋯ › Patient Area).")}>
          <Switch on={!!site.accounts_open} onChange={flip("accounts_open", L("Área de utente aberta", "Patient area opened"), L("Área de utente fechada", "Patient area closed"), L("Área de utente", "Patient Area"))} label={L("Área de utente", "Patient area")} />
        </Row>
        <Row icon={<House />} title={L("Visitas ao domicílio", "Home visits")} desc={L("Aparecem no site, nos preços e no formulário.", "Shown on the website, the prices and the form.")}>
          <Switch on={site.home_visits} onChange={flip("home_visits", L("Domicílio mostrado no site", "Home visits shown on the website"), L("Domicílio retirado do site", "Home visits removed from the website"), L("Visitas ao domicílio", "Home Visits"))} label={L("Visitas ao domicílio", "Home visits")} />
        </Row>
        <Row icon={<MapPin />} title={L("Abertura em breve", "Opening soon")} desc={L("Mostra “Abertura em breve” em vez da morada da clínica.", "Shows “Opening soon” instead of the clinic's address.")}>
          <Switch on={site.opening_soon} onChange={flip("opening_soon", L("O site diz “Abertura em breve”", "The website says “Opening soon”"), L("O site mostra a morada", "The website shows the address"), L("Abertura em breve", "Opening Soon"))} label={L("Abertura em breve", "Opening soon")} />
        </Row>
        <Row icon={<Euro />} title={L("Mostrar preços", "Show prices")} desc={L(`${shown} de ${products.length} itens da tabela de preços.`, `${shown} of ${products.length} price-list items.`)}>
          <Switch on={site.show_prices} onChange={flip("show_prices", L("Preços mostrados no site", "Prices shown on the website"), L("Preços escondidos no site", "Prices hidden on the website"), L("Mostrar preços", "Show Prices"))} label={L("Mostrar preços", "Show prices")} />
        </Row>
      </div>

      <WebsiteTexts site={site} onSaved={w.reload} />

      <p className="list-head">{L("Equipa no site", "Team on the Website")}</p>
      <div className="group">
        {team.map((p) => (
          <Row key={p.id} icon={<UserRoundCheck />} title={p.name}
            desc={(lang === "pt" ? p.web_title_pt : p.web_title_en) || L("Sem título: aparece como “Fisioterapeuta”", "No title: shown as “Physiotherapist”")}>
            <button className="btn sm" onClick={() => openSheet(<WebsitePersonSheet p={p} onSaved={w.reload} />)}>{L("Editar…", "Edit…")}</button>
            <Switch on={p.web_visible} label={L(`Mostrar ${p.name} no site`, `Show ${p.name} on the website`)} onChange={(v) => {
              api("PUT", `/website/team/${p.id}`, { web_visible: v }).then(() => { w.reload(); toast(v ? L(`${p.name} aparece no site`, `${p.name} shows on the website`) : L(`${p.name} retirado do site`, `${p.name} removed from the website`), () => { api("PUT", `/website/team/${p.id}`, { web_visible: !v }).then(() => w.reload()).catch(() => {}); }, L("Equipa no site", "Team on Website")); })
                .catch((e) => toast(isErr(e) ? e.message : String(e)));
            }} />
          </Row>
        ))}
      </div>
      <p className="small muted" style={{ margin: "-14px 4px 22px" }}>{L("Só nome, título e uma pequena apresentação. Sem fotografias nem emails. A receção nunca aparece.", "Only name, title and a short bio. No photos or emails. Reception never appears.")}</p>

      <p className="list-head">{L("Preços no site", "Prices on the Website")}</p>
      <div className="group">
        {products.map((p) => (
          <Row key={p.id} title={lang === "pt" ? p.name_pt : p.name_en} desc={`${eur(p.price_cents / 100, lang)}${p.kind === "monthly" ? L("/mês", "/month") : ""}`}>
            <Switch on={p.web_visible} label={L(`Mostrar ${p.name_pt} no site`, `Show ${p.name_en} on the website`)} onChange={(v) => {
              api("PUT", `/website/products/${p.id}`, { web_visible: v }).then(() => w.reload()).catch((e) => toast(isErr(e) ? e.message : String(e)));
            }} />
          </Row>
        ))}
      </div>
      <p className="small muted" style={{ margin: "-14px 4px 22px" }}>{L("Os preços mudam em Finanças › Preços e o site acompanha logo.", "Prices are changed in Finance › Prices and the website follows at once.")}</p>

      <p className="list-head">{L("Em breve", "Coming Soon")}</p>
      <div className="group">
        <Row icon={<UserRound />} title={L("Contas de utente", "Patient accounts")} desc={L("Ver marcações, sessões e faturas, pedir marcações e documentos.", "See appointments, sessions and invoices, request appointments and documents.")}><Badge>{L("Em breve", "Coming Soon")}</Badge></Row>
      </div>
      {busy && <span className="sr-only" role="status">{L("A guardar…", "Saving…")}</span>}
    </>
  );
}

/** The site's own texts, in both languages (plain text; a blank line starts a new paragraph). */
function WebsiteTexts({ site, onSaved }: { site: WebsiteSite; onSaved: () => void }) {
  const { L, lang, toast } = useApp();
  const [tab, setTab] = useState<"pt" | "en">(lang);
  const pick = (x: WebsiteSite) => ({ tagline_pt: x.tagline_pt, tagline_en: x.tagline_en, intro_pt: x.intro_pt, intro_en: x.intro_en, home_text_pt: x.home_text_pt, home_text_en: x.home_text_en });
  const [v, setV] = useState(pick(site));
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => { setV(pick(site)); }, [site.updated_at]); // eslint-disable-line react-hooks/exhaustive-deps
  const k = (base: "tagline" | "intro" | "home_text") => `${base}_${tab}` as keyof typeof v;
  const set = (key: keyof typeof v) => (e: { target: { value: string } }) => { setV({ ...v, [key]: e.target.value }); setErr(""); };
  const changed = JSON.stringify(v) !== JSON.stringify(pick(site));
  const bad = Object.values(v).some((x) => /[<>]/.test(x)) ? L("Escreva só texto, sem < nem >.", "Write plain text, without < or >.") : "";
  const save = async () => {
    setBusy(true);
    try { await api("PUT", "/website", v); onSaved(); toast(L("Textos do site guardados", "Website texts saved")); }
    catch (e) { setErr(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  return (
    <>
      <p className="list-head">{L("Textos da página inicial", "Home Page Texts")}</p>
      <div className="group pad">
        <div style={{ marginBottom: 12 }}><Seg label={L("Idioma dos textos", "Language of the texts")} value={tab} onChange={setTab} options={[["pt", "Português"], ["en", "English"]]} /></div>
        <Field label={L("Frase de destaque", "Headline")}><input className="input" value={v[k("tagline")]} onChange={set(k("tagline"))} maxLength={90} lang={tab} /></Field>
        <Field label={L("Apresentação", "Introduction")}><textarea className="input" rows={4} value={v[k("intro")]} onChange={set(k("intro"))} maxLength={700} lang={tab} /></Field>
        <Field label={L("Ao domicílio", "At home")}><textarea className="input" rows={2} value={v[k("home_text")]} onChange={set(k("home_text"))} maxLength={700} lang={tab} /></Field>
        <p className="small muted" style={{ marginTop: -4 }}>{L("Uma linha em branco começa um novo parágrafo.", "A blank line starts a new paragraph.")}</p>
        {(err || bad) && <p className="err" role="alert">{err || bad}</p>}
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 6 }}>
          {changed && <button className="btn" onClick={() => { setV(pick(site)); setErr(""); }} disabled={busy}>{L("Repor", "Revert")}</button>}
          <button className="btn pri" disabled={!changed || !!bad || busy} aria-busy={busy} onClick={save}>{L("Guardar", "Save")}</button>
        </div>
      </div>
    </>
  );
}

/** How one person appears on the team page: title and a short bio, in both languages. */
function WebsitePersonSheet({ p, onSaved }: { p: WebsitePerson; onSaved: () => void }) {
  const { L, toast } = useApp();
  const [f, setF] = useState({ web_title_pt: p.web_title_pt, web_title_en: p.web_title_en, web_bio_pt: p.web_bio_pt, web_bio_en: p.web_bio_en });
  const set = (key: keyof typeof f) => (e: { target: { value: string } }) => setF({ ...f, [key]: e.target.value });
  const bad = Object.values(f).some((x) => /[<>]/.test(x));
  return (
    <Sheet title={L(`${p.name} no site`, `${p.name} on the Website`)} sub={L("Sem fotografia nem email: só o nome, um título e uma pequena apresentação.", "No photo or email: just the name, a title and a short bio.")}
      action={L("Guardar", "Save")} disabled={bad}
      onAction={async () => { await api("PUT", `/website/team/${p.id}`, f); onSaved(); toast(L("Equipa no site atualizada", "Team on the website updated")); }}>
      <div className="grid2">
        <Field label={L("Título (português)", "Title (Portuguese)")}><input className="input" value={f.web_title_pt} onChange={set("web_title_pt")} maxLength={60} placeholder={L("Fisioterapeuta", "Fisioterapeuta")} lang="pt" /></Field>
        <Field label={L("Título (inglês)", "Title (English)")}><input className="input" value={f.web_title_en} onChange={set("web_title_en")} maxLength={60} placeholder="Physiotherapist" lang="en" /></Field>
      </div>
      <Field label={L("Apresentação (português)", "Bio (Portuguese)")}><textarea className="input" rows={3} value={f.web_bio_pt} onChange={set("web_bio_pt")} maxLength={500} lang="pt" /></Field>
      <Field label={L("Apresentação (inglês)", "Bio (English)")}><textarea className="input" rows={3} value={f.web_bio_en} onChange={set("web_bio_en")} maxLength={500} lang="en" /></Field>
      {bad && <p className="err" role="alert">{L("Escreva só texto, sem < nem >.", "Write plain text, without < or >.")}</p>}
    </Sheet>
  );
}

/** Owner: the security rules in force. They're fixed (or set on the server), so they're shown as facts, not switches. */
function Security() {
  const { L, openSheet, idleMinutes } = useApp();
  return (
    <>
      <div className="group">
        <Row icon={<KeyRound />} title={L("Verificação em dois passos", "Two-step verification")} desc={L("Para todos, sempre. O código é pedido uma vez por computador, depois de 30 em 30 dias.", "For everyone, always. The code is asked once per computer, then every 30 days.")}><Badge tone="ok">{L("Sempre ligada", "Always On")}</Badge></Row>
        <Row icon={<MonitorSmartphone />} title={L("Aprovação de dispositivos", "Device approval")} desc={L("Cada computador ou telemóvel novo espera pela sua aprovação, mesmo com a palavra-passe e o código certos.", "Every new computer or phone waits for your approval, even with the right password and code.")}><Badge tone="ok">{L("Sempre ligada", "Always On")}</Badge></Row>
        <Row icon={<LogOut />} title={L("Terminar sessão ao fechar a app", "Sign out when the app closes")} desc={L("Obrigatório para dados de saúde.", "Required for health data.")}><Badge tone="ok">{L("Sempre", "Always")}</Badge></Row>
        <Row icon={<Clock />} title={L("Bloquear após inatividade", "Lock after inactivity")} desc={L("Definido no servidor (IDLE_MINUTES no ficheiro .env).", "Set on the server (IDLE_MINUTES in the .env file).")}><span className="muted num">{idleMinutes} min</span></Row>
      </div>
      <div className="group">
        <Row icon={<ScrollText />} title={L("Registo de acessos", "Access log")} desc={L("Entradas, cada ficha de utente aberta, alterações de contas e de dados. Pode filtrar e exportar. Não pode ser editado nem apagado.", "Sign-ins, every patient file opened, account and data changes. You can filter and export it. It can’t be edited or deleted.")}>
          <button className="btn sm" onClick={() => openSheet(<AccessLogSheet />)}>{L("Ver registo…", "View Log…")}</button>
        </Row>
      </div>
    </>
  );
}

/** "há 5 min", "há 3 h", or the day (Settings › Devices). */
function agoLabel(iso: string | null, L: (pt: string, en: string) => string) {
  if (!iso) return L("nunca", "never");
  const m = Math.round((Date.now() - new Date(iso).getTime()) / 60000);
  if (m < 2) return L("agora", "just now");
  if (m < 60) return L(`há ${m} min`, `${m} min ago`);
  if (m < 60 * 24) return L(`há ${Math.round(m / 60)} h`, `${Math.round(m / 60)} h ago`);
  return whenLabel(iso, L);
}

/** Owner: every computer and phone that asked to use PhysioPulse (PhysioPulse plan, phase 3).
 *  Waiting ones first (approve or decline), then approved ones (rename, remove), then removed ones (approve again). */
function Devices() {
  const { L, toast, confirm, openSheet } = useApp();
  const list = useApi<DeviceRow[]>("/devices");
  // A device can ask while this page is open: look again every 10 seconds.
  useEffect(() => { const t = window.setInterval(() => list.reload(), 10000); return () => window.clearInterval(t); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const rows = list.data ?? [];
  const pending = rows.filter((d) => d.status === "pending");
  const approved = rows.filter((d) => d.status === "approved");
  const removed = rows.filter((d) => d.status === "revoked");
  const roleOf = (r: string | null) => r === "owner" ? L("proprietário", "owner") : r === "therapist" ? L("fisioterapeuta", "therapist") : r === "reception" ? L("receção", "reception") : "";
  const fail = (e: unknown) => toast(isErr(e) ? e.message : String(e));
  const approve = (d: DeviceRow) => api("POST", `/devices/${d.id}/approve`).then(() => { list.reload(); toast(L(`${d.name || d.code} aprovado`, `${d.name || d.code} approved`)); }).catch(fail);
  const remove = (d: DeviceRow) => confirm({
    title: d.status === "pending" ? L(`Recusar ${d.name || d.code}?`, `Decline ${d.name || d.code}?`) : L(`Remover ${d.name || d.code}?`, `Remove ${d.name || d.code}?`),
    text: d.status === "pending"
      ? L("Quem está à espera vê que foi recusado. Pode aprová-lo mais tarde nesta lista.", "Whoever is waiting sees it was declined. You can approve it later from this list.")
      : L("Quem o está a usar sai já, e para voltar a entrar este dispositivo precisa de nova aprovação. Use quando um computador ou telemóvel se perde, é vendido ou deixa a clínica.", "Anyone using it is signed out now, and it needs your approval again to sign in. Use it when a computer or phone is lost, sold or leaves the clinic."),
    action: d.status === "pending" ? L("Recusar", "Decline") : L("Remover", "Remove"), destructive: true,
    onConfirm: () => api<{ sessions_ended: number }>("POST", `/devices/${d.id}/revoke`).then(() => { list.reload(); toast(d.status === "pending" ? L("Pedido recusado", "Request declined") : L("Dispositivo removido", "Device removed")); }).catch(fail),
  });
  const who = (d: DeviceRow) => d.requested_by_name ? `${d.requested_by_name}${d.requested_by_role ? ` (${roleOf(d.requested_by_role)})` : ""}` : "";
  return (
    <>
      <p className="small muted" style={{ margin: "0 2px 14px" }}>{L("Cada computador ou telemóvel novo só entra depois de o aprovar aqui, mesmo com a palavra-passe e o código certos. Um computador partilhado (a receção) é aprovado uma vez para todos. Confirme que o código é o que aparece no ecrã de quem pede.", "A new computer or phone can only sign in after you approve it here, even with the right password and code. A shared computer (reception) is approved once for everyone. Check the code matches the one on the screen of whoever is asking.")}</p>
      {list.loading && !list.data ? <div className="skel" /> : list.error ? <p className="err">{list.error}</p> : <>
        {pending.length > 0 && <>
          <p className="list-head">{L("À espera de aprovação", "Waiting for Approval")}</p>
          <div className="group">
            {pending.map((d) => (
              <div className="setrow dev-row pending" key={d.id}>
                {d.kind === "phone" ? <Smartphone aria-label="iPhone" /> : <MonitorSmartphone />}
                <div className="grow">
                  <div>{d.name || L("Dispositivo sem nome", "Unnamed device")} · <span className="dev-code">{d.code}</span>{d.kind === "phone" && <> <Badge tone="n">iPhone</Badge></>}</div>
                  <div className="d">{L(`Pedido por ${who(d)} ${agoLabel(d.created_at, L)}`, `Asked by ${who(d)} ${agoLabel(d.created_at, L)}`)}{d.last_ip ? ` · ${d.last_ip}` : ""}</div>
                </div>
                <div className="dev-actions">
                  <button className="btn sm" onClick={() => remove(d)}><X />{L("Recusar…", "Decline…")}</button>
                  <button className="btn sm pri" onClick={() => approve(d)}><Check />{L("Aprovar", "Approve")}</button>
                </div>
              </div>
            ))}
          </div>
        </>}
        <p className="list-head">{L("Aprovados", "Approved")}</p>
        <div className="group">
          {approved.length === 0 && <div className="setrow"><div className="grow muted">{L("Ainda nenhum.", "None yet.")}</div></div>}
          {approved.map((d) => (
            <div className="setrow dev-row" key={d.id}>
              {d.kind === "phone" ? <Smartphone aria-label="iPhone" /> : <MonitorSmartphone />}
              <div className="grow">
                <div>{d.name || L("Dispositivo sem nome", "Unnamed device")} {d.kind === "phone" && <Badge tone="n">iPhone</Badge>} {d.this_device && <Badge tone="a">{L("Este dispositivo", "This Device")}</Badge>}</div>
                <div className="d">
                  {d.signed_in.length ? L(`Com sessão: ${d.signed_in.join(", ")}`, `Signed in: ${d.signed_in.join(", ")}`) : L(`Última utilização ${agoLabel(d.last_seen, L)}`, `Last used ${agoLabel(d.last_seen, L)}`)}
                  {d.last_ip ? ` · ${d.last_ip}` : ""} · <span className="dev-code">{d.code}</span>
                  {d.how === "carried_over" ? L(" · já era de confiança antes", " · trusted before") : d.how === "recovery_code" ? L(" · aprovado com código de recuperação", " · approved with a recovery code") : d.how === "console" ? L(" · aprovado no servidor", " · approved on the server") : ""}
                </div>
              </div>
              <div className="dev-actions">
                <button className="btn sm" onClick={() => openSheet(<RenameDevice d={d} done={() => list.reload()} />)} aria-label={L(`Mudar o nome de ${d.name}`, `Rename ${d.name}`)}><Pencil />{L("Mudar nome…", "Rename…")}</button>
                {!d.this_device && <button className="btn sm" onClick={() => remove(d)}><X />{L("Remover…", "Remove…")}</button>}
              </div>
            </div>
          ))}
        </div>
        {removed.length > 0 && <>
          <p className="list-head">{L("Removidos ou recusados (últimos 90 dias)", "Removed or Declined (Last 90 Days)")}</p>
          <div className="group">
            {removed.map((d) => (
              <div className="setrow dev-row" key={d.id}>
                <MonitorSmartphone style={{ opacity: .5 }} />
                <div className="grow"><div className="muted">{d.name || d.code}</div><div className="d">{L(`Removido ${agoLabel(d.revoked_at, L)}`, `Removed ${agoLabel(d.revoked_at, L)}`)} · <span className="dev-code">{d.code}</span></div></div>
                <button className="btn sm" onClick={() => approve(d)}>{L("Aprovar de novo", "Approve Again")}</button>
              </div>
            ))}
          </div>
        </>}
        <p className="small muted" style={{ margin: "14px 2px 0" }}>{L("Sem nenhum dispositivo aprovado à mão? No ecrã de espera, use um dos seus códigos de recuperação. Sem códigos: no servidor, docker compose exec server physiopulse-server approve CÓDIGO.", "No approved device at hand? On the waiting screen, use one of your recovery codes. Without codes: on the server, docker compose exec server physiopulse-server approve CODE.")}</p>
      </>}
    </>
  );
}

function RenameDevice({ d, done }: { d: DeviceRow; done: () => void }) {
  const { L, toast } = useApp();
  const [name, setName] = useState(d.name);
  const clean = name.trim().replace(/\s+/g, " ");
  return (
    <Sheet title={L("Mudar o nome do dispositivo", "Rename Device")} sub={L("Um nome claro ajuda a reconhecê-lo nesta lista e no registo de acessos (ex.: PC da receção).", "A clear name helps you recognise it here and in the access log (e.g. Reception PC).")}
      action={L("Guardar", "Save")} disabled={!clean || clean === d.name || clean.length > 60}
      onAction={async () => { await api("PATCH", `/devices/${d.id}`, { name: clean }); done(); toast(L("Nome alterado", "Name changed")); }}>
      <Field label={L("Nome", "Name")}><input className="input" value={name} onChange={(e) => setName(e.target.value)} maxLength={60} autoFocus /></Field>
    </Sheet>
  );
}

interface BackupState {
  state: "ok" | "old" | "failed" | "running" | "unknown"; requested: boolean; last_ok: string | null; last_error: string | null;
  db_file: string | null; files_file: string | null; bytes: number | null; kept_days: number | null; offsite: "off" | "ok" | "failed"; next_at: string | null;
}
interface SysStatus { version: string; address: string | null; database: string; archive: string; backup: BackupState; stale_hours: number; secrets_encrypted?: boolean }

/** "hoje às 03:00", "ontem às 03:00", "07/11 às 03:00". */
export function whenLabel(iso: string, L: (pt: string, en: string) => string) {
  const d = new Date(iso), now = new Date();
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const t = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  const diff = Math.round((day(now) - day(d)) / 864e5);
  if (diff === 0) return L(`hoje às ${t}`, `today at ${t}`);
  if (diff === 1) return L(`ontem às ${t}`, `yesterday at ${t}`);
  if (diff === -1) return L(`amanhã às ${t}`, `tomorrow at ${t}`);
  const dm = `${String(d.getDate()).padStart(2, "0")}/${String(d.getMonth() + 1).padStart(2, "0")}`;
  return L(`${dm} às ${t}`, `${dm} at ${t}`);
}
const sizeLabel = (b: number) => b >= 1e9 ? `${(b / 1e9).toFixed(1).replace(".", ",")} GB` : b >= 1e6 ? `${Math.round(b / 1e6)} MB` : `${Math.max(1, Math.round(b / 1e3))} KB`;

/** Owner: what the server really knows — its version and address, and the real backup state (audit C1). */
function ServerSet() {
  const { L, toast } = useApp();
  const st = useApi<SysStatus>("/system/status");
  const [asking, setAsking] = useState(false);
  const b = st.data?.backup;
  const busy = !!b && (b.requested || b.state === "running");
  // While a backup is asked for or running, check every 5 seconds.
  useEffect(() => { if (!busy) return; const t = setInterval(st.reload, 5000); return () => clearInterval(t); }, [busy]); // eslint-disable-line react-hooks/exhaustive-deps
  const now = async () => {
    setAsking(true);
    try { await api("POST", "/system/backup"); toast(L("Cópia pedida. Começa dentro de 30 segundos.", "Backup requested. It starts within 30 seconds.")); st.reload(); }
    catch (e) { toast(isErr(e) ? e.message : String(e)); } finally { setAsking(false); }
  };
  if (st.error) return <p className="err" role="alert">{st.error}</p>;
  if (!st.data || !b) return <div className="skel" style={{ height: 240 }} />;
  const badge = b.state === "ok" ? <Badge tone="ok">{L("Em dia", "Up to date")}</Badge>
    : b.state === "running" ? <Badge tone="a">{L("A fazer cópia…", "Backing up…")}</Badge>
    : b.state === "old" ? <Badge tone="warn">{L("Desatualizada", "Out of date")}</Badge>
    : b.state === "failed" ? <Badge tone="bad">{L("Falhou", "Failed")}</Badge>
    : <Badge tone="warn">{L("Sem informação", "No information")}</Badge>;
  const lastDesc = b.state === "unknown"
    ? L("O servidor não recebe notícias do serviço de cópias. Atualize o servidor; se continuar, veja os registos do contentor “backup”.", "The server gets no news from the backup service. Update the server; if it stays like this, check the “backup” container’s logs.")
    : b.last_ok ? `${whenLabel(b.last_ok, L)}${b.bytes ? ` · ${sizeLabel(b.bytes)}` : ""}${b.state === "old" ? L(` — há mais de ${st.data.stale_hours} horas.`, ` — over ${st.data.stale_hours} hours ago.`) : ""}`
    : L("Ainda nenhuma.", "None yet.");
  return (
    <>
      <div className="group">
        <Row icon={<Server />} title={L("Servidor da clínica", "Clinic server")} desc={`${st.data.address ?? L("endereço não indicado", "address not set")} · ${L("versão", "version")} ${st.data.version}`}><Badge tone="ok">{L("Ligado", "Connected")}</Badge></Row>
        <Row icon={<HardDrive />} title={L("Base de dados", "Database")} desc={L(`${st.data.database}. O disco do servidor não está encriptado; as cópias estão.`, `${st.data.database}. The server’s disk isn’t encrypted; the backups are.`)} />
        <Row icon={<KeyRound />} title={L("Segredos dos dois passos", "Two-step secrets")} desc={st.data.secrets_encrypted
          ? L("Encriptados com a chave do servidor (SECRET_KEY), que não vai nas cópias. Guarde-a com a chave das cópias.", "Encrypted with the server key (SECRET_KEY), which isn’t in the backups. Keep it with the backup key.")
          : L("Ainda não encriptados: corra o instalador de novo no servidor para criar a SECRET_KEY.", "Not encrypted yet: run the installer again on the server to create the SECRET_KEY.")}>
          <Badge tone={st.data.secrets_encrypted ? "ok" : "warn"}>{st.data.secrets_encrypted ? L("Encriptados", "Encrypted") : L("Por fazer", "To Do")}</Badge></Row>
      </div>
      <div className="group-title">{L("Cópias de segurança", "Backups")}</div>
      <div className="group">
        <Row title={L("Última cópia", "Last backup")} desc={lastDesc}>{badge}</Row>
        {b.state === "failed" && b.last_error && <Row title={L("Última tentativa falhada", "Last failed attempt")} desc={L(`${whenLabel(b.last_error, L)}. Veja os registos do contentor “backup” no servidor.`, `${whenLabel(b.last_error, L)}. Check the “backup” container’s logs on the server.`)} />}
        {b.next_at && <Row title={L("Próxima cópia automática", "Next automatic backup")} desc={whenLabel(b.next_at, L)} />}
        <Row title={L("O que é guardado", "What’s kept")} desc={L(`A base de dados e as pastas do arquivo, encriptadas (age). Guardadas ${b.kept_days ?? 30} dias. A chave para as abrir não está no servidor.`, `The database and the archive folders, encrypted (age). Kept ${b.kept_days ?? 30} days. The key to open them isn’t on the server.`)} />
        <Row title={L("Cópia fora de casa", "Off-site copy")} desc={b.offsite === "off" ? L("Não configurada (RCLONE_REMOTE no .env).", "Not set up (RCLONE_REMOTE in .env).") : b.offsite === "ok" ? L("Enviada com a última cópia.", "Sent with the last backup.") : L("A última não foi enviada. Verifique o destino.", "The last one wasn’t sent. Check the destination.")}>
          {b.offsite === "ok" ? <Badge tone="ok">{L("Ligada", "On")}</Badge> : b.offsite === "failed" ? <Badge tone="bad">{L("Falhou", "Failed")}</Badge> : <Badge>{L("Desligada", "Off")}</Badge>}
        </Row>
      </div>
      <div style={{ display: "flex", gap: 10, alignItems: "center", marginBottom: 18 }}>
        <button className="btn pri" onClick={now} disabled={asking || busy || b.state === "unknown"} aria-busy={asking || busy}><RefreshCw />{busy ? L("A fazer cópia…", "Backing Up…") : L("Fazer cópia agora", "Back Up Now")}</button>
        {busy && <span className="small muted" role="status">{b.requested ? L("À espera do serviço de cópias (até 30 s)…", "Waiting for the backup service (up to 30 s)…") : L("A copiar a base de dados e o arquivo…", "Copying the database and the archive…")}</span>}
      </div>
      <div className="group-title">{L("Pastas do arquivo", "Archive folders")}</div>
      <div className="group pad tree">
        {["PhysioPulse/Utentes/<n.º>-<nome>/", "PhysioPulse/Financas/<ano>/<mês>/", "PhysioPulse/Agenda/<ano>/<mês>.csv", "PhysioPulse/Relatorios/<ano>/", "PhysioPulse/Sistema/"].map((x) => <div key={x} className="num">{x}</div>)}
      </div>
    </>
  );
}

interface AiCheck { available: boolean; reason?: "offline" | "no_model" | "not_local" | "error"; url: string; model: string; models: string[] }
interface AiConf { url: string; model: string; url_in_app: boolean; model_in_app: boolean; server_url: string; server_model: string;
  speech_url?: string; speech_url_in_app?: boolean; server_speech_url?: string; assistant_model?: string | null }
interface AsstCheck { model: string; available: boolean; reason?: AiCheck["reason"]; own: boolean }

/** Settings › Local AI › Assistant: the assistant can have its own model (13 Nov 2026). It works with tools (looks
 *  things up, prepares changes), which needs a model that supports them — e.g. qwen3:8b on an 8 GB graphics card,
 *  qwen3:14b on 16 GB. Letters, notes and receipts keep the main model. Empty = the main model. */
function AssistantModel({ conf, models, onSaved }: { conf: AiConf; models: string[]; onSaved: () => void }) {
  const { L, toast } = useApp();
  const saved = conf.assistant_model ?? "";
  const [model, setModel] = useState(saved);
  const [check, setCheck] = useState<AsstCheck | null>(null);
  const [busy, setBusy] = useState<"" | "test" | "save">("");
  const [err, setErr] = useState("");
  useEffect(() => { api<{ assistant?: AsstCheck }>("GET", "/ai/status").then((r) => setCheck(r.assistant ?? null)).catch(() => setCheck(null)); }, []);
  const changed = model.trim() !== saved;
  const run = async (kind: "test" | "save") => {
    setBusy(kind); setErr("");
    try {
      if (kind === "test") { const r = await api<AiCheck>("POST", "/ai/test", { assistant_model: model.trim() || conf.model }); setCheck({ model: r.model, available: r.available, reason: r.reason, own: !!model.trim() }); }
      else { const r = await api<{ assistant: AsstCheck }>("PUT", "/ai/settings", { assistant_model: model.trim() }); setCheck(r.assistant); toast(L("Modelo do assistente guardado", "Assistant model saved")); onSaved(); }
    } catch (e) { setCheck(null); setErr(isErr(e) ? e.message : String(e)); }
    finally { setBusy(""); }
  };
  const text = (c: AsstCheck) => c.available ? L(`Ligado — o assistente usa ${c.model}.`, `Connected — the assistant uses ${c.model}.`)
    : c.reason === "no_model" ? L(`O computador da IA não tem ${c.model}. Instale-o com: ollama pull ${c.model}`, `The AI computer doesn’t have ${c.model}. Install it with: ollama pull ${c.model}`)
    : L("Sem resposta do Ollama. Veja o endereço acima.", "No answer from Ollama. Check the address above.");
  return (
    <>
      <div className="group-title">{L("Assistente", "Assistant")}</div>
      <div className="group pad">
        <Field label={L("Modelo do assistente", "Assistant model")}>
          <input className="input" value={model} onChange={(e) => setModel(e.target.value)} placeholder={L(`O mesmo (${conf.model})`, `The same (${conf.model})`)} list="ai-models" spellCheck={false} autoComplete="off" />
        </Field>
        <p className="small muted" style={{ margin: "-4px 0 12px" }}>
          {L("O assistente consulta a app e prepara alterações com ferramentas: precisa de um modelo que as saiba usar. Com uma placa gráfica de 8 GB, qwen3:8b; com 16 GB, qwen3:14b. As cartas, as notas e os talões continuam com o modelo acima. Deixe vazio para usar o mesmo.",
            "The assistant looks things up in the app and prepares changes with tools, so it needs a model that can use them. With an 8 GB graphics card, qwen3:8b; with 16 GB, qwen3:14b. Letters, notes and receipts keep the model above. Leave empty to use the same one.")}
        </p>
        {models.length > 0 && (
          <div className="chips" style={{ display: "flex", flexWrap: "wrap", gap: 6, marginBottom: 12 }} aria-label={L("Modelos instalados", "Installed models")}>
            <span className="small muted" style={{ alignSelf: "center" }}>{L("Instalados:", "Installed:")}</span>
            {models.map((m) => <button key={m} className="btn sm" aria-pressed={(model || conf.model) === m} onClick={() => setModel(m)}>{m}</button>)}
          </div>
        )}
        {check && <p className="small" role="status" style={{ marginBottom: 12, display: "flex", gap: 6, alignItems: "flex-start" }}>
          <Badge tone={check.available ? "ok" : "warn"}>{check.available ? L("Ligado", "Connected") : L("Sem ligação", "Not connected")}</Badge><span>{text(check)}</span></p>}
        {err && <p className="err" role="alert" style={{ marginBottom: 12 }}>{err}</p>}
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <button className="btn" disabled={!!busy} aria-busy={busy === "test"} onClick={() => run("test")}><Plug />{busy === "test" ? L("A testar…", "Testing…") : L("Testar modelo", "Test Model")}</button>
          <button className="btn pri" disabled={!!busy || !changed} aria-busy={busy === "save"} onClick={() => run("save")}>{L("Guardar", "Save")}</button>
        </div>
      </div>
    </>
  );
}
interface SpeechCheck { available: boolean; url: string; reason?: "not_set" | "not_local" | "offline" }

/** Settings › Local AI › Speech to text: the whisper.cpp server on the AI PC that turns session recordings into text. */
function Speech({ conf, onSaved }: { conf: AiConf; onSaved: () => void }) {
  const { L, toast } = useApp();
  const [url, setUrl] = useState(conf.speech_url_in_app ? conf.speech_url ?? "" : "");
  const [check, setCheck] = useState<SpeechCheck | null>(null);
  const [busy, setBusy] = useState<"" | "test" | "save">("");
  const [err, setErr] = useState("");
  useEffect(() => { api<{ speech?: SpeechCheck }>("GET", "/ai/status").then((r) => setCheck(r.speech ?? null)).catch(() => setCheck(null)); }, []);
  const changed = url.trim() !== (conf.speech_url_in_app ? conf.speech_url ?? "" : "");
  const run = async (kind: "test" | "save") => {
    setBusy(kind); setErr("");
    try {
      if (kind === "test") setCheck(await api<SpeechCheck>("POST", "/ai/speech/test", { url: url.trim() }));
      else { const r = await api<{ speech: SpeechCheck }>("PUT", "/ai/settings", { speech_url: url.trim() }); setCheck(r.speech); toast(L("Endereço da transcrição guardado", "Speech-to-text address saved")); onSaved(); }
    } catch (e) { setCheck(null); setErr(isErr(e) ? e.message : String(e)); }
    finally { setBusy(""); }
  };
  const server = (conf.server_speech_url ?? "").replace(/^http:\/\//, "");
  const text = (c: SpeechCheck) => c.available ? L(`Ligada — whisper em ${c.url.replace(/^http:\/\//, "")}`, `Connected — whisper at ${c.url.replace(/^http:\/\//, "")}`)
    : c.reason === "not_set" ? L("Ainda sem endereço. As gravações esperam na fila até haver um.", "No address yet. Recordings wait in line until there is one.")
      : c.reason === "not_local" ? L("Esse endereço não é de um computador da clínica.", "That address isn’t a computer in the clinic.")
        : L(`Sem resposta em ${c.url.replace(/^http:\/\//, "")}. Verifique se o computador da IA está ligado e se o whisper-server está a correr. As gravações esperam na fila.`, `No answer at ${c.url.replace(/^http:\/\//, "")}. Check the AI computer is on and whisper-server is running. Recordings wait in line.`);
  return (
    <>
      <div className="group-title">{L("Transcrição das sessões gravadas", "Speech to text for recorded sessions")}</div>
      <div className="group pad">
        <Field label={L("Endereço do whisper-server", "whisper-server address")}>
          <input className="input" value={url} onChange={(e) => setUrl(e.target.value)} placeholder={server || "192.168.1.66"} spellCheck={false} autoComplete="off" inputMode="url" />
        </Field>
        <p className="small muted" style={{ margin: "-4px 0 12px" }}>
          {L("Ex.: 192.168.1.66 (a porta 8178 é acrescentada). O programa corre no computador da IA, ao lado do Ollama. Deixe vazio para usar o do servidor", "E.g. 192.168.1.66 (port 8178 is added). The program runs on the AI computer, next to Ollama. Leave empty to use the server’s")}{server ? ` (${server})` : ""}.
        </p>
        {check && <p className="small" role="status" style={{ marginBottom: 12, display: "flex", gap: 6, alignItems: "flex-start" }}>
          <Badge tone={check.available ? "ok" : "warn"}>{check.available ? L("Ligada", "Connected") : L("Sem ligação", "Not connected")}</Badge><span>{text(check)}</span></p>}
        {err && <p className="err" role="alert" style={{ marginBottom: 12 }}>{err}</p>}
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <button className="btn" disabled={!!busy} aria-busy={busy === "test"} onClick={() => run("test")}><Plug />{busy === "test" ? L("A testar…", "Testing…") : L("Testar ligação", "Test Connection")}</button>
          <button className="btn pri" disabled={!!busy || !changed} aria-busy={busy === "save"} onClick={() => run("save")}>{L("Guardar", "Save")}</button>
        </div>
      </div>
    </>
  );
}

/** Settings › Local AI: where Ollama is (this server or another clinic computer) and which model it uses.
 *  Owner only (the server checks). Try it before saving; empty fields go back to the server's own settings. */
function AI() {
  const { L, toast } = useApp();
  const conf = useApi<AiConf>("/ai/settings");
  const [url, setUrl] = useState("");
  const [model, setModel] = useState("");
  const [check, setCheck] = useState<AiCheck | null>(null);
  const [busy, setBusy] = useState<"" | "test" | "save">("");
  const [err, setErr] = useState("");
  // Fill the fields when the settings first load and after saving — not on every refresh (a test also
  // counts as a change, and would wipe what's being typed).
  const fill = useRef(true);
  useEffect(() => {
    if (!conf.data || !fill.current) return;
    fill.current = false;
    setUrl(conf.data.url_in_app ? conf.data.url : ""); setModel(conf.data.model_in_app ? conf.data.model : "");
    api<AiCheck>("GET", "/ai/status").then(setCheck).catch(() => setCheck(null));
  }, [conf.data]);
  const changed = !!conf.data && (url.trim() !== (conf.data.url_in_app ? conf.data.url : "") || model.trim() !== (conf.data.model_in_app ? conf.data.model : ""));
  const run = async (kind: "test" | "save") => {
    setBusy(kind); setErr("");
    try {
      const r = await api<AiCheck>(kind === "test" ? "POST" : "PUT", kind === "test" ? "/ai/test" : "/ai/settings", { url: url.trim(), model: model.trim() });
      setCheck(r);
      if (kind === "save") { toast(L("Definições da IA guardadas", "AI settings saved")); fill.current = true; conf.reload(); }
    } catch (e) { setCheck(null); setErr(isErr(e) ? e.message : String(e)); }
    finally { setBusy(""); }
  };
  const statusText = (c: AiCheck) => c.available ? L(`Ligada — ${c.model} em ${c.url}`, `Connected — ${c.model} at ${c.url}`)
    : c.reason === "offline" ? L(`Sem resposta em ${c.url}. Verifique se o computador está ligado, se o Ollama está a correr e se aceita ligações da rede (OLLAMA_HOST=0.0.0.0).`, `No answer at ${c.url}. Check the computer is on, Ollama is running and accepts network connections (OLLAMA_HOST=0.0.0.0).`)
    : c.reason === "no_model" ? L(`O Ollama em ${c.url} não tem o modelo ${c.model}.${c.models.length ? " Escolha um dos que tem." : ` Instale-o com: ollama pull ${c.model}`}`, `Ollama at ${c.url} doesn’t have ${c.model}.${c.models.length ? " Pick one it has." : ` Install it with: ollama pull ${c.model}`}`)
    : c.reason === "not_local" ? L("Esse endereço não é de um computador da clínica.", "That address isn’t a computer in the clinic.")
    : L("O Ollama respondeu com um erro.", "Ollama answered with an error.");
  const models = check?.models ?? [];
  return (
    <>
      <p className="sub" style={{ marginBottom: 14 }}>{L("A IA corre num computador da clínica (Ollama, e o whisper para a transcrição). Nada sai da clínica. O proprietário usa-a sempre; os fisioterapeutas só se lhes ligar a IA em Equipa. Propõe sempre antes de alterar.", "The AI runs on a computer in the clinic (Ollama, and whisper for transcription). Nothing leaves the clinic. The owner always has it; therapists only if you turn their AI on in Team. It always proposes before changing anything.")}</p>
      {conf.error ? <p className="err" role="alert">{conf.error}</p> : !conf.data ? <div className="skel" style={{ height: 160 }} /> : (
        <div className="group pad">
          <div className="grid2">
            <Field label={L("Endereço do Ollama", "Ollama address")}>
              <input className="input" value={url} onChange={(e) => setUrl(e.target.value)} placeholder={conf.data.server_url.replace(/^http:\/\//, "")} spellCheck={false} autoComplete="off" inputMode="url" />
            </Field>
            <Field label={L("Modelo", "Model")}>
              <input className="input" value={model} onChange={(e) => setModel(e.target.value)} placeholder={conf.data.server_model} list="ai-models" spellCheck={false} autoComplete="off" />
              <datalist id="ai-models">{models.map((m) => <option key={m} value={m} />)}</datalist>
            </Field>
          </div>
          <p className="small muted" style={{ margin: "-4px 0 12px" }}>
            {L("Ex.: 192.168.1.66 (a porta 11434 é acrescentada). Só endereços da rede da clínica. Deixe vazio para usar o do servidor", "E.g. 192.168.1.66 (port 11434 is added). Only clinic-network addresses. Leave empty to use the server’s")} ({conf.data.server_url.replace(/^http:\/\//, "")} · {conf.data.server_model}).
          </p>
          {models.length > 0 && (
            <div className="chips" style={{ display: "flex", flexWrap: "wrap", gap: 6, marginBottom: 12 }} aria-label={L("Modelos instalados", "Installed models")}>
              <span className="small muted" style={{ alignSelf: "center" }}>{L("Instalados:", "Installed:")}</span>
              {models.map((m) => <button key={m} className="btn sm" aria-pressed={(model || conf.data!.model) === m} onClick={() => setModel(m)}>{m}</button>)}
            </div>
          )}
          {check && <p className="small" role="status" style={{ marginBottom: 12, display: "flex", gap: 6, alignItems: "flex-start" }}>
            <Badge tone={check.available ? "ok" : "warn"}>{check.available ? L("Ligada", "Connected") : L("Sem ligação", "Not connected")}</Badge><span>{statusText(check)}</span></p>}
          {err && <p className="err" role="alert" style={{ marginBottom: 12 }}>{err}</p>}
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
            <button className="btn" disabled={!!busy} aria-busy={busy === "test"} onClick={() => run("test")}><Plug />{busy === "test" ? L("A testar…", "Testing…") : L("Testar ligação", "Test Connection")}</button>
            <button className="btn pri" disabled={!!busy || !changed} aria-busy={busy === "save"} onClick={() => run("save")}>{L("Guardar", "Save")}</button>
          </div>
        </div>
      )}
      {conf.data && <AssistantModel conf={conf.data} models={models} onSaved={conf.reload} />}
      {conf.data && <Speech conf={conf.data} onSaved={conf.reload} />}
      <div className="group-title">{L("Onde se usa", "Where it’s used")}</div>
      <div className="group">
        {([
          [L("Gravar sessões", "Record sessions"), L("Utentes › Evolução › Gravar sessão", "Patients › Progress › Record Session"), true],
          [L("Cartas de alta", "Discharge letters"), L("Utentes › ⋯ › Carta de alta", "Patients › ⋯ › Discharge Letter"), true],
          [L("Reescrever notas de evolução", "Rewrite progress notes"), L("Utentes › Evolução › Reescrever com IA", "Patients › Progress › Rewrite with AI"), true],
          [L("Ler talões e faturas", "Read receipts and invoices"), L("Finanças › Despesas › Nova despesa", "Finance › Expenses › New Expense"), false],
          [L("Relatórios mensais", "Monthly reports"), L("Finanças › Relatórios IA", "Finance › AI Reports"), false],
          [L("Assistente", "Assistant"), L("Ver › Mostrar assistente (⌥⌘A)", "View › Show Assistant (⌥⌘A)"), true],
        ] as [string, string, boolean][]).map(([x, d, team]) => (
          <Row key={x} title={x} desc={d}><Badge>{team ? L("Proprietário + IA ligada em Equipa", "Owner + AI on in Team") : L("Só proprietário", "Owner only")}</Badge></Row>
        ))}
      </div>
    </>
  );
}

/** Owner decision, 5 Nov 2026: the clinic is a company; invoices come from certified software, recorded by hand for now. */
function Invoicing() {
  const { L } = useApp();
  return (
    <>
      <p className="sub" style={{ marginBottom: 14 }}>{L("A clínica é uma empresa: as faturas são emitidas num programa certificado pela AT. Por agora, emita-a nesse programa e registe o número e o PDF no pagamento (Finanças › Pagamentos).", "The clinic is a company: invoices are issued in AT-certified software. For now, issue it there and record its number and PDF on the payment (Finance › Payments).")}</p>
      <div className="group">
        <Row icon={<Receipt />} title={L("À mão", "By hand")} desc={L("Número e PDF da fatura registados em cada pagamento.", "Invoice number and PDF recorded on each payment.")}><Badge tone="a">{L("Atual", "Current")}</Badge></Row>
        <Row icon={<Plug />} title={L("Ligação a um programa de faturação", "Link to invoicing software")} desc={L("InvoiceXpress, Vendus ou Moloni (ainda por escolher).", "InvoiceXpress, Vendus or Moloni (not chosen yet).")}><Badge>{L("Em breve", "Coming Soon")}</Badge></Row>
      </div>
    </>
  );
}

/** Payments are recorded by hand for now (owner decision, 5 Nov 2026); Stripe and automatic charges come later. */
function PayMethods() {
  const { L, lang } = useApp();
  return (
    <>
      <div className="group">
        <Row icon={<CreditCard />} title={L("Registados à mão", "Recorded by hand")} desc={METHODS.filter(([m]) => m !== "stripe").map(([, n]) => (lang === "pt" ? n[0] : n[1])).join(", ")}><Badge tone="ok">{L("Ativo", "On")}</Badge></Row>
        <Row icon={<CreditCard />} title="Stripe" desc={L("QR no ecrã ou link por SMS: cartão, MB WAY, Multibanco.", "QR on screen or SMS link: card, MB WAY, Multibanco.")}><Badge>{L("Em breve", "Coming Soon")}</Badge></Row>
        <Row icon={<CalendarClock />} title={L("Cobrar planos mensais automaticamente", "Charge monthly plans automatically")} desc={L("Precisa do Stripe.", "Needs Stripe.")}><Badge>{L("Em breve", "Coming Soon")}</Badge></Row>
      </div>
    </>
  );
}

/** The server's append-only access log, newest first. */
/** Kinds of entry for the filter: exact actions, or prefixes ending in a dot (the server's `actions` filter). */
const LOG_KINDS: { id: string; pt: string; en: string; actions: string }[] = [
  { id: "", pt: "Tudo", en: "Everything", actions: "" },
  { id: "files", pt: "Fichas de utentes abertas", en: "Patient files opened", actions: "patient.viewed,document.opened,patient.exported,patient.reimbursement_pack,recording.opened,recording.played" },
  { id: "recordings", pt: "Sessões gravadas", en: "Recorded sessions", actions: "recording." },
  { id: "clinical", pt: "Alterações clínicas", en: "Clinical changes", actions: "patient.created,patient.updated,patient.anonymised,patient.deleted,anamnesis.,note.,test.,prescription.,consent.,document.uploaded,document.saved,document.updated,document.removed,letter." },
  { id: "signin", pt: "Entradas e saídas", en: "Sign-ins and sign-outs", actions: "login.,logout,setup." },
  { id: "devices", pt: "Dispositivos", en: "Devices", actions: "device." },
  { id: "warn", pt: "Só avisos (falhas)", en: "Warnings only (failures)", actions: "login.failed,login.bad_code,login.bad_recovery_code,login.device_refused,device.bad_recovery_code,account.password_change_failed,account.password_reset_failed" },
  { id: "accounts", pt: "Contas da equipa", en: "Staff accounts", actions: "staff.,account.,invite." },
  { id: "money", pt: "Dinheiro", en: "Money", actions: "payment.,plan.,product.,expense.,salary.,cash.,month.,finance." },
  { id: "schedule", pt: "Agenda", en: "Schedule", actions: "appointment.,class.,booking_request.,waitlist.,reminders.,day.,clinic.hours_changed,clinic.closure_added,clinic.closure_removed" },
  { id: "system", pt: "Definições, IA e sistema", en: "Settings, AI and system", actions: "clinic.updated,system.,ai.,assistant.,consent_text.,reminder_text.,complaint.,access_log.,website." },
];
const WARN = ["login.failed", "login.bad_code", "login.bad_recovery_code", "account.password_change_failed", "account.password_reset_failed"];
const PAGE = 100;

/** Owner: the whole access log with filters, "Show more" and a CSV export (audit phase C). */
function AccessLogSheet() {
  const { L, lang, toast } = useApp();
  const [rows, setRows] = useState<LogRow[] | null>(null);
  const [more, setMore] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [f, setF] = useState({ q: "", kind: "", who: "", from: "", to: "" });
  const [dq, setDq] = useState("");
  useEffect(() => { const t = setTimeout(() => setDq(f.q.trim()), 300); return () => clearTimeout(t); }, [f.q]);
  const { data: staff } = useApi<{ id: string; name: string }[]>("/staff");
  const query = (extra?: Record<string, string>) => {
    const p = new URLSearchParams();
    if (f.from) p.set("from", f.from);
    if (f.to) p.set("to", f.to);
    if (f.who) p.set("staff_id", f.who);
    const k = LOG_KINDS.find((x) => x.id === f.kind);
    if (k?.actions) p.set("actions", k.actions);
    if (dq) p.set("q", dq);
    Object.entries(extra ?? {}).forEach(([a, b]) => p.set(a, b));
    return p.toString().replace(/\+/g, "%20");
  };
  const badRange = !!(f.from && f.to && f.from > f.to);
  useEffect(() => {
    if (badRange) return;
    let live = true;
    setRows(null); setErr("");
    api<LogRow[]>("GET", `/access-log?${query({ limit: String(PAGE) })}`)
      .then((r) => { if (live) { setRows(r); setMore(r.length === PAGE); } })
      .catch((e) => { if (live) setErr(isErr(e) ? e.message : String(e)); });
    return () => { live = false; };
  }, [f.from, f.to, f.who, f.kind, dq, badRange]); // eslint-disable-line react-hooks/exhaustive-deps
  const showMore = async () => {
    if (!rows?.length) return;
    setBusy(true);
    try { const r = await api<LogRow[]>("GET", `/access-log?${query({ limit: String(PAGE), before: String(rows[rows.length - 1].id) })}`); setRows([...rows, ...r]); setMore(r.length === PAGE); }
    catch (e) { setErr(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  const exportCsv = async () => {
    setBusy(true);
    try {
      const d = new Date(), day = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
      const where = await downloadAccessLog(query(), `registo-acessos-${day}.csv`);
      toast(L(`Guardado em ${where}`, `Saved to ${where}`));
    } catch (e) { setErr(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  const what: Record<string, [string, string]> = {
    "login.ok": ["Entrou", "Signed in"], "login.failed": ["Palavra-passe errada", "Wrong password"], "login.bad_code": ["Código errado", "Wrong code"],
    "login.two_step_set_up": ["Configurou os dois passos", "Set up two-step"], "login.computer_trusted": ["Computador de confiança (30 dias)", "Trusted this computer (30 days)"], "login.computer_forgotten": ["Esqueceu um computador", "Forgot a computer"],
    "login.device_refused": ["Dispositivo removido tentou entrar", "Removed device tried to sign in"],
    "device.requested": ["Dispositivo novo pediu aprovação", "New device asked for approval"], "device.approved": ["Aprovou um dispositivo", "Approved a device"],
    "device.declined": ["Recusou um dispositivo", "Declined a device"], "device.revoked": ["Removeu um dispositivo", "Removed a device"], "device.renamed": ["Mudou o nome de um dispositivo", "Renamed a device"],
    "device.bad_recovery_code": ["Código de recuperação errado (dispositivo)", "Wrong recovery code (device)"], logout: ["Saiu", "Signed out"], "setup.owner_created": ["Configurou a clínica", "Set up the clinic"],
    "invite.created": ["Criou convite", "Created invite"], "staff.joined": ["Criou conta", "Created account"], "staff.deactivated": ["Desativou conta", "Deactivated account"],
    "staff.reactivated": ["Reativou conta", "Reactivated account"], "staff.role_changed": ["Mudou função", "Changed role"], "staff.signed_out_everywhere": ["Terminou sessões", "Ended sessions"],
    "account.name_changed": ["Mudou o nome", "Changed their name"], "account.password_changed": ["Mudou a palavra-passe", "Changed their password"], "account.password_change_failed": ["Palavra-passe atual errada", "Wrong current password"],
    "account.password_reset": ["Repôs a palavra-passe", "Reset their password"], "account.password_reset_failed": ["Código de reposição errado", "Wrong reset code"], "account.recovery_codes_made": ["Criou códigos de recuperação", "Made recovery codes"],
    "login.recovery_code_used": ["Entrou com código de recuperação", "Signed in with a recovery code"], "login.bad_recovery_code": ["Código de recuperação errado", "Wrong recovery code"],
    "staff.reset_code_made": ["Deu código para repor palavra-passe", "Gave a password reset code"], "staff.two_step_reset": ["Repôs dois passos", "Reset two-step"], "invite.revoked": ["Retirou convite", "Withdrew invite"],
    "clinic.updated": ["Alterou dados da clínica", "Changed clinic details"], "system.backup_requested": ["Pediu cópia de segurança", "Asked for a backup"], ...LOG_LABEL,
    "staff.pay_changed": ["Mudou a remuneração", "Changed pay"],
    "staff.ai_on": ["Ligou a IA local a alguém", "Turned someone’s local AI on"], "staff.ai_off": ["Desligou a IA local a alguém", "Turned someone’s local AI off"],
    "recording.started": ["Começou a gravar uma sessão", "Started recording a session"], "recording.uploaded": ["Enviou uma gravação para a IA local", "Sent a recording to the local AI"],
    "recording.opened": ["Abriu o rascunho de uma gravação", "Opened a recording’s draft"], "recording.played": ["Ouviu uma gravação", "Listened to a recording"],
    "recording.deleted": ["Gravação apagada", "Recording deleted"],
    "appointment.cancelled": ["Desmarcou uma sessão", "Cancelled an appointment"], "appointment.done": ["Marcou como realizada", "Marked as done"], "appointment.no_show": ["Marcou falta", "Marked as no-show"],
    "class.attendance": ["Registou presenças numa aula", "Took class attendance"], "class.attendee_removed": ["Tirou de uma aula", "Removed from a class"], "patients.duplicates_checked": ["Procurou fichas repetidas", "Checked for duplicate files"],
    "payment.recorded": ["Registou um pagamento", "Recorded a payment"], "payment.voided": ["Anulou um pagamento", "Voided a payment"], "payment.invoiced": ["Registou uma fatura", "Recorded an invoice"],
    "plan.sold": ["Vendeu um pacote", "Sold a package"], "plan.updated": ["Alterou um pacote", "Changed a package"], "product.created": ["Criou um preço", "Added a price"], "product.updated": ["Alterou um preço", "Changed a price"],
    "expense.created": ["Registou uma despesa", "Added an expense"], "expense.updated": ["Alterou uma despesa", "Changed an expense"], "expense.deleted": ["Apagou uma despesa", "Deleted an expense"],
    "salary.paid": ["Registou um salário", "Recorded a salary"], "salary.deleted": ["Apagou um salário", "Deleted a salary"], "cash.closed": ["Fechou a caixa", "Closed the till"],
    "month.closed": ["Fechou um mês", "Closed a month"], "month.reopened": ["Reabriu um mês", "Reopened a month"], "finance.exported": ["Exportou para o contabilista", "Exported for the accountant"],
    "finance.file_opened": ["Abriu um ficheiro de contas", "Opened a finance file"], "finance.file_uploaded": ["Juntou um ficheiro de contas", "Added a finance file"],
    "access_log.exported": ["Exportou o registo de acessos", "Exported the access log"], "settings.changed": ["Alterou definições", "Changed settings"],
  };
  const filtered = !!(f.q || f.kind || f.who || f.from || f.to);
  const thisYear = new Date().getFullYear();
  return (
    <Sheet wide title={L("Registo de acessos", "Access Log")} sub={L("Guardado no servidor. Ninguém o pode alterar, nem o proprietário. Inclui cada ficha de utente aberta, por quem e em que computador.", "Kept on the server. Nobody can change it, including the owner. It includes every patient file opened, by whom and on which computer.")}>
      {/* Filters aren't edits: Esc closes the log without asking (data-no-edit). */}
      <div className="log-filters" data-no-edit="">
        <label className="search"><Search /><input value={f.q} onChange={(e) => setF({ ...f, q: e.target.value })} placeholder={L("Pessoa, utente, computador…", "Person, patient, computer…")} aria-label={L("Procurar no registo", "Search the log")} /></label>
        <select className="input" value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value })} aria-label={L("Tipo", "Kind")}>
          {LOG_KINDS.map((k) => <option key={k.id} value={k.id}>{L(k.pt, k.en)}</option>)}
        </select>
        <select className="input" value={f.who} onChange={(e) => setF({ ...f, who: e.target.value })} aria-label={L("Pessoa", "Person")}>
          <option value="">{L("Todas as pessoas", "Everyone")}</option>
          {(staff ?? []).map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
        </select>
        <span className="log-dates">
          <DateInput value={f.from} max={f.to || undefined} onChange={(e) => setF({ ...f, from: e.target.value })} aria-label={L("Desde", "From")} />
          <span className="muted">–</span>
          <DateInput value={f.to} min={f.from || undefined} onChange={(e) => setF({ ...f, to: e.target.value })} aria-label={L("Até", "To")} />
        </span>
      </div>
      <div className="log-bar">
        <span className="small muted">{rows ? (rows.length === 0 ? "" : L(`${rows.length}${more ? "+" : ""} registo${rows.length === 1 ? "" : "s"}`, `${rows.length}${more ? "+" : ""} entr${rows.length === 1 ? "y" : "ies"}`)) : ""}</span>
        <span style={{ display: "flex", gap: 8 }}>
          {filtered && <button className="btn sm" onClick={() => setF({ q: "", kind: "", who: "", from: "", to: "" })}>{L("Limpar filtros", "Clear Filters")}</button>}
          <button className="btn sm" disabled={busy || !rows?.length} onClick={exportCsv}><Download />{L("Exportar CSV…", "Export CSV…")}</button>
        </span>
      </div>
      {badRange ? <p className="err">{L("O primeiro dia tem de ser antes do último.", "The first day must be before the last day.")}</p>
        : err ? <p className="err">{err}</p> : !rows ? <div className="skel" style={{ height: 160 }} /> : rows.length === 0 ? (
          <p className="small muted" style={{ padding: "24px 0", textAlign: "center" }}>{filtered ? L("Nada neste registo com estes filtros.", "Nothing in the log matches these filters.") : L("O registo está vazio.", "The log is empty.")}</p>
        ) : (
        <div className="tbl-wrap" style={{ maxHeight: 380, overflow: "auto" }}><table>
          <thead><tr>{[L("Quando", "When"), L("Quem", "Who"), L("O quê", "What"), L("Computador", "Computer")].map((h) => <th key={h} className="plain">{h}</th>)}</tr></thead>
          <tbody>{rows.map((r) => {
            const d = new Date(r.at);
            return (
              <tr key={r.id}>
                <td className="num" style={{ whiteSpace: "nowrap" }}>{d.toLocaleString(lang === "pt" ? "pt-PT" : "en-GB", { day: "2-digit", month: "2-digit", ...(d.getFullYear() !== thisYear ? { year: "numeric" } : {}), hour: "2-digit", minute: "2-digit" })}</td>
                <td>{r.staff_name ?? <span className="muted">{L("Desconhecido", "Unknown")}</span>}</td>
                <td>{what[r.action] ? L(...what[r.action]) : r.action}{r.action === "patient.viewed" && r.detail === "reduced" ? L(" (vista reduzida)", " (reduced view)") : ""}
                  {r.subject_label && <span className="muted"> · {r.subject_label}</span>}
                  {WARN.includes(r.action) ? <> <Badge tone="warn">!</Badge></> : null}</td>
                <td className="muted" style={{ whiteSpace: "nowrap" }}>{r.device || "—"}</td>
              </tr>
            );
          })}</tbody>
        </table></div>
      )}
      {more && rows && !err && <div style={{ textAlign: "center", marginTop: 10 }}><button className="btn sm" disabled={busy} onClick={showMore}>{L("Mostrar mais", "Show More")}</button></div>}
    </Sheet>
  );
}
