import { useEffect, useRef, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { Building2, Copy, KeyRound, Lock, Mail, Server, ShieldCheck, Ticket, UserRound } from "lucide-react";
import { useApp } from "../lib/ctx";
import { isTauri } from "../lib/platform";
import { api, forgetRemembered, getRemembered, getServer, isErr, setServer, setFirstRun, type Enrol, type Me, type Pending, type Remembered } from "../lib/api";
import { Mark, Seg, CutoutSheet } from "../components/ui";

type Mode = "loading" | "connect" | "welcome" | "signin" | "signup" | "setup" | "enrol" | "code" | "recover" | "reset" | "codes" | "pending" | "pending_recover";
type LoginR = { next: "code" | "enrol" | "session"; challenge?: string; enrol?: Enrol; staff?: Me; idle_minutes?: number };
/** After the code: a session, or (new device, phase 3) a wait for the owner's approval. */
type SessionR = { next?: "session"; staff: Me; idle_minutes: number; recovery_codes?: string[]; codes_left?: number };
/** How often the waiting screen asks whether the owner approved this device. */
const PENDING_POLL_MS = 5000;

/** Sign-in opens first; creating an account is secondary. First run sets up the clinic.
 *  On a computer where someone already signed in, it opens on "Olá de novo, <name>" (neutral: no gendered "Bem-vindo") with the password only;
 *  the 6-digit code is asked only the first time on a computer (then every 30 days; see server `auth::TRUST_DAYS`). */
export default function Login({ onDone, notice }: { onDone: (me: Me, idleMinutes: number) => void; notice?: string }) {
  const { L, lang, setLang, toast } = useApp();
  const [mode, setMode] = useState<Mode>("loading");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState(notice ?? "");
  const [f, setF] = useState({ server: "", clinic: "", name: "", email: "", password: "", password2: "", invite: "", recovery: "", reset: "" });
  /** The owner's recovery codes, shown once after two-step is first set up (audit C4). */
  const [codes, setCodes] = useState<{ list: string[]; me: Me; idle: number } | null>(null);
  const [kept, setKept] = useState(false);
  const [info, setInfo] = useState("");
  const [enrol, setEnrol] = useState<Enrol | null>(null);
  const [known, setKnown] = useState<Remembered | null>(null);
  const [challenge, setChallenge] = useState("");
  /** This device waits for the owner's approval (phase 3): the short code to match in Settings › Devices. */
  const [pending, setPending] = useState<Pending | null>(null);
  const [code, setCode] = useState(["", "", "", "", "", ""]);
  const codeRef = useRef(code); codeRef.current = code;
  const refs = useRef<(HTMLInputElement | null)[]>([]);
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) => { setF({ ...f, [k]: e.target.value }); setErr(""); };

  /** True while the server didn't answer at launch: the error line then offers "Tentar de novo" (audit phase C). */
  const [offline, setOffline] = useState(false);
  // First screen: no server → connect; server not set up yet → clinic setup; otherwise sign in.
  const start = async (retry = false) => {
    const srv = await getServer();
    if (!srv) { setMode("connect"); return; }
    setF((x) => ({ ...x, server: srv }));
    const signin = async () => { const r = await getRemembered(); setKnown(r); setMode((m) => (retry && m !== "loading" ? m : r ? "welcome" : "signin")); };
    try { const st = await api<{ setup_needed: boolean }>("GET", "/status"); setOffline(false); if (retry) setErr(""); if (st.setup_needed) setMode("setup"); else await signin(); }
    catch { await signin(); setOffline(true); if (!notice || retry) setErr(L("Não foi possível contactar o servidor. Verifique a ligação à internet.", "Can’t reach the server. Check the internet connection.")); }
  };
  useEffect(() => { start(); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const run = async (fn: () => Promise<void>) => {
    setBusy(true); setErr(""); setOffline(false);
    try { await fn(); } catch (e) { setErr(isErr(e) ? e.message : String(e)); } finally { setBusy(false); }
  };
  const toCode = (ch: string) => { setInfo(""); setChallenge(ch); setCode(["", "", "", "", "", ""]); setMode("code"); setTimeout(() => refs.current[0]?.focus(), 30); };
  const toEnrol = (en: Enrol) => { setEnrol(en); setChallenge(en.challenge); setCode(["", "", "", "", "", ""]); setMode("enrol"); setTimeout(() => refs.current[0]?.focus(), 30); };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (mode === "connect") return run(async () => { const st = await setServer(f.server); setMode(st.setup_needed ? "setup" : "signin"); });
    if (mode === "signin" || mode === "welcome") return run(async () => {
      const email = mode === "welcome" && known ? known.email : f.email;
      const r = await api<LoginR>("POST", "/auth/login", { email, password: f.password });
      setF((x) => ({ ...x, password: "" }));
      if (r.next === "session" && r.staff) { if (!isTauri) setFirstRun(false); onDone(r.staff, r.idle_minutes ?? 15); }
      else if (r.next === "enrol" && r.enrol) toEnrol(r.enrol); else toCode(r.challenge!);
    });
    if (mode === "setup" || mode === "signup") {
      if (f.password.length < 12) { setErr(L("Escolha uma palavra-passe com pelo menos 12 caracteres.", "Choose a password with at least 12 characters.")); return; }
      if (f.password !== f.password2) { setErr(L("As palavras-passe não coincidem.", "The passwords don’t match.")); return; }
      return run(async () => {
        const en = mode === "setup"
          ? await api<Enrol>("POST", "/setup", { clinic_name: f.clinic, name: f.name, email: f.email, password: f.password })
          : await api<Enrol>("POST", "/auth/signup", { invite: f.invite, name: f.name, email: f.email, password: f.password });
        toEnrol(en);
      });
    }
    if (mode === "code" || mode === "enrol") return verify(code);
    if (mode === "recover") return run(async () => {
      const r = await api<{ enrol: Enrol; codes_left: number }>("POST", "/auth/recover", { challenge, recovery_code: f.recovery });
      setF((x) => ({ ...x, recovery: "" }));
      setInfo(L(`Código de recuperação usado (restam ${r.codes_left}). Configure agora a verificação no telemóvel novo.`, `Recovery code used (${r.codes_left} left). Now set up verification on your new phone.`));
      toEnrol(r.enrol);
    });
    if (mode === "reset") {
      if (f.password.length < 12) { setErr(L("Escolha uma palavra-passe com pelo menos 12 caracteres.", "Choose a password with at least 12 characters.")); return; }
      if (f.password !== f.password2) { setErr(L("As palavras-passe não coincidem.", "The passwords don’t match.")); return; }
      return run(async () => {
        await api("POST", "/auth/reset", { email: f.email, code: f.reset, password: f.password });
        setF((x) => ({ ...x, password: "", password2: "", reset: "" }));
        setInfo(L("Palavra-passe alterada. Entre com a nova.", "Password changed. Sign in with the new one."));
        setMode(known && known.email === f.email.trim().toLowerCase() ? "welcome" : "signin");
      });
    }
    if (mode === "codes" && codes) { onDone(codes.me, codes.idle); }
    if (mode === "pending") return void check(true);
    if (mode === "pending_recover") return run(async () => {
      const r = await api<SessionR>("POST", "/auth/pending/recover", { recovery_code: f.recovery });
      setF((x) => ({ ...x, recovery: "" })); setPending(null);
      if (typeof r.codes_left === "number") toast(L(`Dispositivo aprovado com um código de recuperação (restam ${r.codes_left}).`, `Device approved with a recovery code (${r.codes_left} left).`));
      signedIn(r);
    });
  };

  /** The 6 digits: sent as soon as the sixth is typed (or on Verify). */
  const verify = (digits6: string[]) => {
    if (digits6.some((c) => !c)) { setErr(L("Introduza os 6 dígitos.", "Enter all 6 digits.")); return; }
    if (busy) return;
    return run(async () => {
      const r = await api<SessionR | Pending>("POST", "/auth/verify", { challenge, code: digits6.join("") });
      if (!isTauri) setFirstRun(false);
      if (r.next === "pending") { setPending(r); setInfo(""); setMode("pending"); return; }
      signedIn(r);
    });
  };
  const signedIn = (r: SessionR) => {
    if (r.recovery_codes?.length) { setCodes({ list: r.recovery_codes, me: r.staff, idle: r.idle_minutes }); setKept(false); setMode("codes"); return; }
    onDone(r.staff, r.idle_minutes);
  };

  /** Waiting for approval: ask the server every few seconds; the session arrives by itself once the owner approves. */
  const checking = useRef(false);
  const check = async (byHand = false) => {
    if (checking.current) return;
    checking.current = true;
    if (byHand) { setBusy(true); setErr(""); }
    try {
      const r = await api<SessionR | Pending>("POST", "/auth/pending");
      if (r.next === "pending") { setPending(r); if (byHand) setInfo(L("Ainda não foi aprovado.", "Not approved yet.")); }
      else { setPending(null); signedIn(r); }
    } catch (e) {
      const er = isErr(e) ? e : { status: 0, message: String(e) };
      // Declined, or the 30 minutes ran out: back to sign-in with the reason. A dropped connection just waits.
      if (er.status === 401 || er.status === 403) { setPending(null); setErr(er.message); setMode(known ? "welcome" : "signin"); }
      else if (byHand) setErr(er.message);
    } finally { checking.current = false; if (byHand) setBusy(false); }
  };
  useEffect(() => {
    if (mode !== "pending") return;
    const t = window.setInterval(() => { void check(); }, PENDING_POLL_MS);
    return () => window.clearInterval(t);
  }, [mode]); // eslint-disable-line react-hooks/exhaustive-deps
  const filled = (...k: (keyof typeof f)[]) => k.every((x) => f[x].trim());

  // Rendered through plain functions (not nested components) so inputs keep focus while typing.
  const box = (children: ReactNode) => (
    <div className="login">
      {/* The paper with the pulse-hand line cut out of it; the brand teal shows through (owner, 12 Nov 2026). */}
      <CutoutSheet />
      <form className="box" noValidate onSubmit={submit} aria-busy={busy}>
        <div className="langtop"><Seg label={L("Idioma", "Language")} value={lang} onChange={setLang} options={[["pt", "PT"], ["en", "EN"]]} /></div>
        <Mark className="lotus" />
        {children}
      </form>
    </div>
  );
  const errLine = <>{info && !err && <div className="small" role="status" style={{ textAlign: "center", color: "var(--ok)", marginBottom: 6 }}>{info}</div>}<div className="err" role="alert">{err}</div>
    {offline && err && <div style={{ display: "flex", gap: 8, justifyContent: "center", marginBottom: 8 }}>
      <button type="button" className="btn sm" disabled={busy} onClick={() => run(() => start(true))}>{L("Tentar de novo", "Try Again")}</button>
      {isTauri && <button type="button" className="btn sm" onClick={() => { setOffline(false); setErr(""); setMode("connect"); }}>{L("Mudar de servidor…", "Change Server…")}</button>}
    </div>}</>;
  const primary = (label: string, disabled?: boolean) => (
    <button className="btn pri wide big" type="submit" disabled={busy || disabled}>{busy ? L("Um momento…", "One moment…") : label}</button>
  );
  /** Puts typed, pasted or autofilled digits into the boxes from box i on. Reads the latest code from a ref, so
   *  fast typing (or a password manager) never loses a digit to a stale render (23 Nov 2026 fix). */
  const fill = (i: number, raw: string) => {
    const d = raw.replace(/\D/g, "");
    const n = [...codeRef.current];
    if (!d) { n[i] = ""; codeRef.current = n; setCode(n); return; }
    d.slice(0, 6 - i).split("").forEach((ch, j) => (n[i + j] = ch));
    codeRef.current = n; setCode(n); setErr("");
    const nextEmpty = n.findIndex((x) => !x);
    refs.current[nextEmpty === -1 ? 5 : nextEmpty]?.focus();
    if (n.every((x) => x)) verify(n);
  };
  const digits = (
    <div className="otp">
      {code.map((c, i) => (
        <input key={i} ref={(el) => (refs.current[i] = el)} inputMode="numeric" aria-label={`${L("Dígito", "Digit")} ${i + 1}`} autoComplete={i ? "off" : "one-time-code"} value={c}
          onChange={(e) => { const v = e.target.value; fill(i, v.length > 1 && v.startsWith(c) ? v.slice(c.length) : v); }}
          onKeyDown={(e) => { if (e.key === "Backspace" && !c && i) refs.current[i - 1]?.focus(); }}
          onPaste={(e) => { const d = e.clipboardData.getData("text").replace(/\D/g, ""); if (!d) return; e.preventDefault(); fill(d.length >= 6 ? 0 : i, d); }} />
      ))}
    </div>
  );
  const pw = (k: "password" | "password2", ph: string, auto: string) => (
    <label className="ifield"><Lock /><input type="password" placeholder={ph} autoComplete={auto} value={f[k]} onChange={set(k)} aria-label={ph} /></label>
  );

  if (mode === "loading") return box(<p className="tagline" style={{ marginTop: 20 }}>{L("A ligar ao servidor da clínica…", "Connecting to the clinic server…")}</p>);

  if (mode === "connect") return box(<>
    <h1>{L("Ligar ao servidor", "Connect to the server")}</h1>
    <p className="tagline">{L("O endereço do servidor da clínica, como o responsável lho deu.", "The clinic server’s address, as the owner gave it to you.")}</p>
    <label className="ifield"><Server /><input autoFocus placeholder="clinica.duckdns.org" autoCapitalize="off" autoCorrect="off" spellCheck={false} value={f.server} onChange={set("server")} aria-label={L("Endereço do servidor", "Server address")} /></label>
    {errLine}
    {primary(L("Continuar", "Continue"), !filled("server"))}
  </>);

  if (mode === "setup") return box(<>
    <p className="small muted" style={{ textAlign: "center" }}>{L("Passo 1 de 2", "Step 1 of 2")}</p>
    <h1>{L("Configurar a clínica", "Set up your clinic")}</h1>
    <p className="tagline">{L("Esta será a conta do proprietário, com acesso a tudo.", "This becomes the owner account, with access to everything.")}</p>
    <label className="ifield"><Building2 /><input autoFocus placeholder={L("Nome da clínica", "Clinic name")} value={f.clinic} onChange={set("clinic")} aria-label={L("Nome da clínica", "Clinic name")} /></label>
    <label className="ifield"><UserRound /><input placeholder={L("O seu nome", "Your name")} autoComplete="name" value={f.name} onChange={set("name")} aria-label={L("O seu nome", "Your name")} /></label>
    <label className="ifield"><Mail /><input type="email" placeholder="nome@clinica.pt" autoComplete="email" value={f.email} onChange={set("email")} aria-label="Email" /></label>
    {pw("password", L("Palavra-passe (12+ caracteres)", "Password (12+ characters)"), "new-password")}
    {pw("password2", L("Confirmar palavra-passe", "Confirm password"), "new-password")}
    {errLine}
    {primary(L("Continuar", "Continue"), !filled("clinic", "name", "email", "password", "password2"))}
    {!isTauri && <p className="switch-auth">{L("Pré-visualização.", "Preview.")}<button type="button" onClick={() => { setFirstRun(false); setErr(""); setMode("signin"); }}>{L("Voltar a entrar", "Back to sign in")}</button></p>}
  </>);

  if (mode === "enrol" && enrol) return box(<>
    <p className="small muted" style={{ textAlign: "center" }}>{L("Passo 2 de 2", "Step 2 of 2")}</p>
    <h1>{L("Verificação em dois passos", "Two-step verification")}</h1>
    <p className="tagline">{L("Leia o código com uma app de autenticação (Palavras-passe da Apple, Google Authenticator, 1Password) e escreva os 6 dígitos.", "Scan this with an authenticator app (Apple Passwords, Google Authenticator, 1Password) and enter the 6 digits.")}</p>
    <div style={{ display: "grid", placeItems: "center", marginBottom: 10 }}>
      <div role="img" aria-label={L("Código QR para a app de autenticação", "QR code for your authenticator app")} style={{ width: 180, height: 180, borderRadius: 12, overflow: "hidden", background: "#fff", boxShadow: "0 0 0 .5px var(--line2)" }}
        dangerouslySetInnerHTML={{ __html: enrol.qr_svg.replace(/width="\d+" height="\d+"/, 'width="180" height="180"') }} />
    </div>
    <p className="small" style={{ textAlign: "center", color: "var(--text2)" }}>
      {L("Não consegue ler? Escreva esta chave:", "Can’t scan? Enter this key:")}{" "}
      <button type="button" className="linkbtn num" onClick={() => { navigator.clipboard?.writeText(enrol.secret); toast(L("Chave copiada", "Key copied")); }} title={L("Copiar", "Copy")}>
        {enrol.secret.replace(/(.{4})/g, "$1 ").trim()} <Copy size={11} style={{ verticalAlign: -1 }} />
      </button>
    </p>
    {digits}
    {errLine}
    {primary(L("Concluir", "Finish Setup"), code.some((c) => !c))}
  </>);

  if (mode === "code") return box(<>
    <h1>{L("Verificação em dois passos", "Two-step verification")}</h1>
    <p className="tagline">{L("Introduza o código de 6 dígitos da sua app de autenticação.", "Enter the 6-digit code from your authenticator app.")}</p>
    {digits}
    {errLine}
    {primary(L("Verificar", "Verify"), code.some((c) => !c))}
    <p className="switch-auth">{L("Perdeu o telemóvel?", "Lost your phone?")}<button type="button" onClick={() => { setErr(""); setInfo(""); setMode("recover"); }}>{L("Usar um código de recuperação", "Use a Recovery Code")}</button></p>
    <p className="switch-auth" style={{ marginTop: 6 }}>{L("Não é a sua conta?", "Not your account?")}<button type="button" onClick={() => { setErr(""); setMode(known ? "welcome" : "signin"); }}>{L("Voltar", "Go back")}</button></p>
  </>);

  if (mode === "recover") return box(<>
    <h1>{L("Código de recuperação", "Recovery code")}</h1>
    <p className="tagline">{L("Escreva um dos códigos que guardou quando configurou a clínica. Depois configura a verificação num telemóvel novo.", "Enter one of the codes you saved when you set up the clinic. Then you set up verification on a new phone.")}</p>
    <label className="ifield"><KeyRound /><input autoFocus placeholder="K7QM-2XRP-9TDA" value={f.recovery} onChange={set("recovery")} aria-label={L("Código de recuperação", "Recovery code")} autoCapitalize="characters" autoComplete="off" spellCheck={false} /></label>
    {errLine}
    {primary(L("Continuar", "Continue"), !filled("recovery"))}
    <p className="small muted" style={{ textAlign: "center", marginTop: 12 }}>{L("Sem códigos? Peça a outro proprietário para repor a verificação em Equipa.", "No codes? Ask another owner to reset your verification in Team.")}</p>
    <p className="switch-auth" style={{ marginTop: 6 }}><button type="button" onClick={() => { setErr(""); setMode("code"); setTimeout(() => refs.current[0]?.focus(), 30); }}>{L("Voltar ao código de 6 dígitos", "Back to the 6-Digit Code")}</button></p>
  </>);

  if (mode === "pending" && pending) return box(<>
    <h1>{L("À espera de aprovação", "Waiting for approval")}</h1>
    <p className="tagline">{pending.owner
      ? L("Este computador ainda não foi aprovado. Aprove-o em Definições › Dispositivos noutro computador ou telemóvel seu, ou use um código de recuperação.", "This computer isn’t approved yet. Approve it in Settings › Devices on another computer or phone of yours, or use a recovery code.")
      : L("Por segurança, cada computador novo tem de ser aprovado pelo responsável da clínica. Peça-lhe que o aprove em Definições › Dispositivos.", "For security, the clinic owner approves every new computer. Ask them to approve it in Settings › Devices.")}</p>
    <div className="pending-code" role="group" aria-label={L(`Código deste computador: ${pending.device.code}`, `This computer’s code: ${pending.device.code}`)}>
      <ShieldCheck aria-hidden />
      <span className="num">{pending.device.code}</span>
      <span className="small muted">{pending.device.name || L("Este computador", "This computer")}</span>
    </div>
    <p className="small muted" style={{ textAlign: "center" }} role="status"><span className="spin sm" aria-hidden /> {L("Entra automaticamente assim que for aprovado.", "You’ll be signed in as soon as it’s approved.")}</p>
    {errLine}
    {primary(L("Verificar agora", "Check Now"))}
    {pending.owner && <p className="switch-auth">{L("Sem outro dispositivo aprovado?", "No other approved device?")}<button type="button" onClick={() => { setErr(""); setInfo(""); setMode("pending_recover"); }}>{L("Usar um código de recuperação", "Use a Recovery Code")}</button></p>}
    <p className="switch-auth" style={{ marginTop: 6 }}><button type="button" onClick={() => { setPending(null); setErr(""); setInfo(""); setMode(known ? "welcome" : "signin"); }}>{L("Cancelar", "Cancel")}</button></p>
  </>);

  if (mode === "pending_recover" && pending) return box(<>
    <h1>{L("Aprovar com um código", "Approve with a code")}</h1>
    <p className="tagline">{L("Um dos códigos de recuperação que guardou aprova este computador. Cada código serve uma vez.", "One of the recovery codes you kept approves this computer. Each code works once.")}</p>
    <label className="ifield"><KeyRound /><input autoFocus placeholder="K7QM-2XRP-9TDA" value={f.recovery} onChange={set("recovery")} aria-label={L("Código de recuperação", "Recovery code")} autoCapitalize="characters" autoComplete="off" spellCheck={false} /></label>
    {errLine}
    {primary(L("Aprovar este computador", "Approve This Computer"), !filled("recovery"))}
    <p className="small muted" style={{ textAlign: "center", marginTop: 12 }}>{L("Sem códigos? No servidor: docker compose exec server physiopulse-server approve", "No codes? On the server: docker compose exec server physiopulse-server approve")} {pending.device.code}</p>
    <p className="switch-auth" style={{ marginTop: 6 }}><button type="button" onClick={() => { setErr(""); setMode("pending"); }}>{L("Voltar", "Back")}</button></p>
  </>);

  if (mode === "codes" && codes) return box(<>
    <h1>{L("Códigos de recuperação", "Recovery codes")}</h1>
    <p className="tagline">{L("Se perder o telemóvel ou esquecer a palavra-passe, cada código serve uma vez para voltar a entrar. Imprima-os ou escreva-os e guarde-os longe do computador. Só são mostrados agora.", "If you lose your phone or forget your password, each code gets you back in once. Print or write them down and keep them away from the computer. They’re shown only now.")}</p>
    <div className="codes-grid num" aria-label={L("Códigos de recuperação", "Recovery codes")}>{codes.list.map((c) => <span key={c}>{c}</span>)}</div>
    <button type="button" className="btn wide" style={{ marginBottom: 10 }} onClick={() => { navigator.clipboard?.writeText(codes.list.join("\n")); toast(L("Códigos copiados", "Codes copied")); }}><Copy />{L("Copiar códigos", "Copy Codes")}</button>
    <label className="check" style={{ justifyContent: "center", marginBottom: 12 }}><input type="checkbox" checked={kept} onChange={(e) => setKept(e.target.checked)} />{L("Guardei estes códigos num sítio seguro", "I’ve kept these codes somewhere safe")}</label>
    {primary(L("Continuar", "Continue"), !kept)}
  </>);

  if (mode === "reset") return box(<>
    <h1>{L("Nova palavra-passe", "New password")}</h1>
    <p className="tagline">{L("Precisa do código que o proprietário lhe deu (Equipa › Repor palavra-passe). Se for o proprietário, use um dos seus códigos de recuperação.", "You need the code the owner gave you (Team › Reset Password). If you’re the owner, use one of your recovery codes.")}</p>
    <label className="ifield"><Mail /><input type="email" placeholder="nome@clinica.pt" autoComplete="username" value={f.email} onChange={set("email")} aria-label="Email" autoFocus={!f.email} /></label>
    <label className="ifield"><KeyRound /><input placeholder="K7QM-2XRP-9TDA" value={f.reset} onChange={set("reset")} aria-label={L("Código", "Code")} autoCapitalize="characters" autoComplete="off" spellCheck={false} autoFocus={!!f.email} /></label>
    {pw("password", L("Nova palavra-passe (12+ caracteres)", "New password (12+ characters)"), "new-password")}
    {pw("password2", L("Confirmar palavra-passe", "Confirm password"), "new-password")}
    {errLine}
    {primary(L("Mudar palavra-passe", "Change Password"), !filled("email", "reset", "password", "password2"))}
    <p className="switch-auth" style={{ marginTop: 10 }}><button type="button" onClick={() => { setErr(""); setMode(known ? "welcome" : "signin"); }}>{L("Voltar a entrar", "Back to Sign In")}</button></p>
  </>);

  if (mode === "signup") return box(<>
    <h1>{L("Criar conta", "Create your account")}</h1>
    <p className="tagline">{L("Precisa do código de convite que a clínica lhe deu.", "You need the invite code the clinic gave you.")}</p>
    <label className="ifield"><Ticket /><input autoFocus placeholder="K7QM-2XRP-9TDA" value={f.invite} onChange={set("invite")} aria-label={L("Código de convite", "Invite code")} autoCapitalize="characters" /></label>
    <label className="ifield"><UserRound /><input placeholder={L("Nome", "Name")} autoComplete="name" value={f.name} onChange={set("name")} aria-label={L("Nome", "Name")} /></label>
    <label className="ifield"><Mail /><input type="email" placeholder="nome@clinica.pt" autoComplete="email" value={f.email} onChange={set("email")} aria-label="Email" /></label>
    {pw("password", L("Palavra-passe (12+ caracteres)", "Password (12+ characters)"), "new-password")}
    {pw("password2", L("Confirmar palavra-passe", "Confirm password"), "new-password")}
    {errLine}
    {primary(L("Continuar", "Continue"), !filled("invite", "name", "email", "password", "password2"))}
    <p className="switch-auth">{L("Já tem conta?", "Already have an account?")}<button type="button" onClick={() => { setErr(""); setMode("signin"); }}>{L("Entrar", "Sign in")}</button></p>
  </>);

  if (mode === "welcome" && known) {
    const first = known.name.trim().split(/\s+/)[0] || known.name;
    return box(<>
      <h1>{L(`Olá de novo, ${first}`, `Welcome back, ${first}`)}</h1>
      <p className="tagline">{known.email}</p>
      {/* Hidden username so password managers fill the right account. */}
      <input type="email" autoComplete="username" value={known.email} readOnly hidden />
      <label className="ifield"><Lock /><input type="password" placeholder={L("Palavra-passe", "Password")} autoComplete="current-password" value={f.password} onChange={set("password")} aria-label={L("Palavra-passe", "Password")} autoFocus /></label>
      {errLine}
      {primary(L("Entrar", "Sign In"), !filled("password"))}
      <p className="switch-auth" style={{ marginTop: 10 }}><button type="button" onClick={() => { setErr(""); setInfo(""); setF((x) => ({ ...x, password: "", password2: "", email: known.email })); setMode("reset"); }}>{L("Esqueceu-se da palavra-passe?", "Forgot Your Password?")}</button></p>
      <p className="switch-auth" style={{ marginTop: 14 }}>{L("Outra pessoa?", "Someone else?")}<button type="button" onClick={() => { setErr(""); setF((x) => ({ ...x, email: "", password: "" })); setMode("signin"); }}>{L("Usar outra conta", "Use Another Account")}</button></p>
      <p className="switch-auth" style={{ marginTop: 6 }}><button type="button" style={{ fontWeight: 400, color: "var(--text3)" }} onClick={async () => { await forgetRemembered(known.email); setKnown(null); setErr(""); setF((x) => ({ ...x, email: "", password: "" })); setMode("signin"); toast(L("Este computador já não se lembra de si.", "This computer has forgotten you.")); }}>{L("Esquecer neste computador", "Forget on This Computer")}</button></p>
    </>);
  }

  return box(<>
    <h1>{L("Entrar", "Sign in")}</h1>
    <p className="tagline">{L("Entre na sua conta da clínica.", "Sign in to your clinic account.")}</p>
    <label className="ifield"><Mail /><input type="email" placeholder="nome@clinica.pt" autoComplete="username" value={f.email} onChange={set("email")} aria-label="Email" autoFocus /></label>
    {pw("password", L("Palavra-passe", "Password"), "current-password")}
    {errLine}
    {primary(L("Entrar", "Sign In"), !filled("email", "password"))}
    <p className="switch-auth" style={{ marginTop: 10 }}><button type="button" onClick={() => { setErr(""); setInfo(""); setF((x) => ({ ...x, password: "", password2: "", email: x.email })); setMode("reset"); }}>{L("Esqueceu-se da palavra-passe?", "Forgot Your Password?")}</button></p>
    <p className="switch-auth" style={{ marginTop: 14 }}>{L("Ainda não tem conta?", "Don’t have an account?")}<button type="button" onClick={() => { setErr(""); setMode("signup"); }}>{L("Criar conta", "Create one")}</button></p>
    {isTauri && f.server && <p className="small muted" style={{ textAlign: "center", marginTop: 8 }}>{f.server} — <button type="button" className="linkbtn" onClick={() => setMode("connect")}>{L("Mudar", "Change")}</button></p>}
  </>);
}
