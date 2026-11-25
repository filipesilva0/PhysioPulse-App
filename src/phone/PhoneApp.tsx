// The staff iPhone app (PhysioPulse plan, phase 5). A small companion to the desktop app: what you need on the move
// (today, the schedule, read-only patient files) and session recording. Writing notes, assessments and tests,
// finance, team, settings, legal tools and the assistant stay on the computer (owner, 11 Nov 2026).
//
// Built from the same code as the desktop app (Tauri 2 for iOS): the same server calls through `api()`, the same
// rule that the session token never reaches this layer. iOS conventions (Apple HIG): a floating tab bar at the bottom,
// large titles that shrink into the bar on scroll, grouped lists, sheets from the bottom, 17 pt text, 44 pt targets.
//
// Sign-in: password + two-step code + the owner's approval of the phone (phase 3), once every 30 days. In between
// the app asks for Face ID every time it comes back to the screen (Rust core: src-tauri/src/phone.rs).
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import type { ReactNode } from "react";
import { CalendarDays, CircleUser, House, Mic, ScanFace, Users } from "lucide-react";
import { Ctx, type AppCtx, type ConfirmOpts, type Lang, type Role } from "../lib/ctx";
import { api, SIGNED_OUT, isErr, type Me } from "../lib/api";
import { draftsFor } from "../lib/drafts";
import { getRec, isRecording, subscribeRec } from "../lib/recorder";
import { isCancel, lock, phoneUser, previewKeep, signOutPhone, unlock, type PhoneUser } from "../lib/phone";
import { CutoutSheet, HoldButton } from "../components/ui";
import Login from "../pages/Login";
import { TodayScreen, ScheduleScreen } from "./schedule";
import { PatientsScreen } from "./patients";
import { RecordScreen } from "./record";
import { AccountScreen } from "./account";
import "./phone.css";

export type Tab = "today" | "schedule" | "patients" | "record" | "account";
/** Recording is for the owner and therapists whose AI switch is on; the server checks it again. */
export const canRecord = (me: Me) => me.role === "owner" || (me.role === "therapist" && !!me.ai_enabled);

type Gate = "boot" | "signin" | "locked" | "unlocking" | "open";

/** Lets screens move between tabs and open a patient file from anywhere (Today → patient). */
export interface PhoneNav { tab: Tab; setTab: (t: Tab) => void; patient: string; openPatient: (id: string) => void; closePatient: () => void;
  /** Start a recording for this appointment (from the appointment sheet). */
  recordFor: (x: { patientId: string; name: string; appointmentId: string | null }) => void;
  recordTarget: { patientId: string; name: string; appointmentId: string | null } | null; clearRecordTarget: () => void }

export default function PhoneApp() {
  const [lang, setLangState] = useState<Lang>(() => { try { return (localStorage.getItem("physiopulse-lang") as Lang) || "pt"; } catch { return "pt"; } });
  const setLang = (l: Lang) => { setLangState(l); try { localStorage.setItem("physiopulse-lang", l); } catch { /* storage unavailable */ } };
  const L = useCallback((pt: string, en: string) => (lang === "pt" ? pt : en), [lang]);
  useEffect(() => { document.documentElement.lang = lang === "pt" ? "pt-PT" : "en"; document.documentElement.dataset.form = "phone"; }, [lang]);

  const [gate, setGate] = useState<Gate>("boot");
  const [user, setUser] = useState<PhoneUser | null>(null);
  const [notice, setNotice] = useState<string | undefined>();
  const [me, setMe] = useState<Me>({ id: "", name: "", email: "", role: "therapist" });
  const [tab, setTabState] = useState<Tab>("today");
  const [patient, setPatient] = useState("");
  const [recordTarget, setRecordTarget] = useState<PhoneNav["recordTarget"]>(null);
  const [sheet, setSheet] = useState<ReactNode>(null);
  const [alert, setAlert] = useState<ConfirmOpts | null>(null);
  const [toastMsg, setToastMsg] = useState<{ text: string; undo?: () => void; n: number } | null>(null);
  const rec = useSyncExternalStore(subscribeRec, getRec);

  const toast = useCallback((text?: string, undo?: () => void) => setToastMsg({ text: text ?? "", undo, n: Date.now() }), []);
  useEffect(() => {
    if (!toastMsg) return;
    const t = setTimeout(() => setToastMsg(null), toastMsg.undo ? 6000 : 2600);
    return () => clearTimeout(t);
  }, [toastMsg]);

  /** Face ID, then who is signed in. A cancelled Face ID stays on the lock screen with a button to try again. */
  const doUnlock = useCallback(async () => {
    setGate("unlocking"); setNotice(undefined);
    try {
      await unlock(L("Desbloquear o PhysioPulse", "Unlock PhysioPulse"));
      const m = await api<Me>("GET", "/me");
      draftsFor(m.id); setMe(m); setGate("open");
    } catch (e) {
      if (isCancel(e)) { setGate("locked"); return; }
      // No session any more (30 days, signed out from Team, phone removed): sign in again.
      if (isErr(e) && (e.code === "signed_out" || e.status === 401)) {
        await signOutPhone(); setUser(null);
        setNotice(L("A sessão neste iPhone terminou. Entre de novo com a palavra-passe e o código.", "The session on this iPhone ended. Sign in again with your password and code."));
        setGate("signin"); return;
      }
      setNotice(isErr(e) ? e.message : String(e)); setGate("locked");
    }
  }, [L]);

  // Start: someone signed in on this phone → the lock screen asks for Face ID straight away; otherwise sign in.
  useEffect(() => { phoneUser().then((u) => { setUser(u); if (u) void doUnlock(); else setGate("signin"); }); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Leaving the screen locks the app (the token leaves memory); coming back asks for Face ID again.
  // Not while a session is being recorded: locking would stop it.
  const gateRef = useRef(gate); gateRef.current = gate;
  useEffect(() => {
    const f = () => {
      if (document.visibilityState === "hidden") {
        if (gateRef.current === "open" && !isRecording()) { void lock(); setSheet(null); setAlert(null); setGate("locked"); }
      } else if (gateRef.current === "locked") void doUnlock();
    };
    document.addEventListener("visibilitychange", f);
    return () => document.removeEventListener("visibilitychange", f);
  }, [doUnlock]);

  // The server ended the session (owner signed this phone out, 30 days, phone removed).
  useEffect(() => {
    const f = () => {
      setSheet(null); setAlert(null); setUser(null);
      setNotice(L("A sessão neste iPhone terminou. Entre de novo.", "The session on this iPhone ended. Sign in again."));
      setGate("signin");
    };
    window.addEventListener(SIGNED_OUT, f);
    return () => window.removeEventListener(SIGNED_OUT, f);
  }, [L]);

  const signOut = async () => {
    await signOutPhone();
    setSheet(null); setAlert(null); setUser(null); setTabState("today"); setPatient("");
    setNotice(undefined); setGate("signin");
  };

  const setTab = (t: Tab) => { setTabState(t); if (t !== "patients") setPatient(""); };
  const nav: PhoneNav = {
    tab, setTab, patient,
    openPatient: (id) => { setSheet(null); setPatient(id); setTabState("patients"); },
    closePatient: () => setPatient(""),
    recordFor: (x) => { setSheet(null); setRecordTarget(x); setTabState("record"); },
    recordTarget, clearRecordTarget: () => setRecordTarget(null),
  };

  const role: Role = me.role;
  const ctx: AppCtx = useMemo(() => ({
    lang, setLang, L, role, setRole: () => {}, me, setMe, idleMinutes: 0, page: "today", go: () => {}, toast,
    menuAt: () => {}, openSheet: (n) => setSheet(n), closeSheet: () => setSheet(null), cancelSheet: () => setSheet(null), sheetEdited: () => {},
    confirm: (o) => { setSheet(null); setAlert(o); }, patientId: patient, openPatient: nav.openPatient, patientName: "", setPatientName: () => {},
  }), [lang, L, role, me, toast, patient]); // eslint-disable-line react-hooks/exhaustive-deps

  if (gate === "boot") return <Ctx.Provider value={ctx}><div className="phone p-boot" /></Ctx.Provider>;

  if (gate === "signin") {
    return (
      <Ctx.Provider value={ctx}>
        <div className="phone p-signin">
          <Login notice={notice} onDone={(m) => {
            draftsFor(m.id); setMe(m);
            const u = { name: m.name, email: m.email, server: "" }; previewKeep(u); setUser(u);
            setNotice(undefined); setTabState("today"); setGate("open");
          }} />
        </div>
        <Toast msg={toastMsg} L={L} clear={() => setToastMsg(null)} />
      </Ctx.Provider>
    );
  }

  if (gate === "locked" || gate === "unlocking") {
    return (
      <Ctx.Provider value={ctx}>
        <main className="phone p-lock">
          <CutoutSheet id="pp-cut-lock" band={[8, 22]} />
          <div className="p-lock-in">
            <h1>PhysioPulse</h1>
            {user && <p className="p-lock-who">{user.name}<span>{user.email}</span></p>}
            {notice && <p className="p-lock-err" role="alert">{notice}</p>}
          </div>
          <div className="p-lock-actions">
            <button className="p-btn pri wide" onClick={() => void doUnlock()} disabled={gate === "unlocking"}>
              <ScanFace aria-hidden />{gate === "unlocking" ? L("A desbloquear…", "Unlocking…") : L("Desbloquear com Face ID", "Unlock with Face ID")}
            </button>
            <button className="p-link" onClick={() => setAlert({
              title: L("Entrar com outra conta?", "Sign in with another account?"),
              text: L("Termina a sessão de quem está neste iPhone. Para voltar a entrar é preciso a palavra-passe e o código de 6 dígitos.", "This signs out whoever is on this iPhone. Signing back in needs the password and the 6-digit code."),
              action: L("Terminar sessão", "Sign Out"), destructive: true, onConfirm: () => void signOut(),
            })}>{L("Entrar com outra conta", "Use Another Account")}</button>
          </div>
        </main>
        <ActionAlert alert={alert} close={() => setAlert(null)} L={L} />
      </Ctx.Provider>
    );
  }

  // Tab bar as in the owner's reference: a small floating capsule of icons (the current one in a dark circle) and,
  // beside it, one round dark button — here Record, for those who may record. Labels are read out by VoiceOver.
  const tabs: { id: Tab; icon: ReactNode; pt: string; en: string }[] = [
    { id: "today", icon: <House />, pt: "Hoje", en: "Today" },
    { id: "schedule", icon: <CalendarDays />, pt: "Agenda", en: "Schedule" },
    { id: "patients", icon: <Users />, pt: "Utentes", en: "Patients" },
    { id: "account", icon: <CircleUser />, pt: "Conta", en: "Account" },
  ];
  const live = rec.state !== "idle" && rec.state !== "error";
  return (
    <Ctx.Provider value={ctx}>
      <div className="phone" data-tab={tab}>
        {tab === "today" && <TodayScreen nav={nav} />}
        {tab === "schedule" && <ScheduleScreen nav={nav} />}
        {tab === "patients" && <PatientsScreen nav={nav} />}
        {tab === "record" && <RecordScreen nav={nav} />}
        {tab === "account" && <AccountScreen signOut={() => setAlert({
          title: L("Terminar sessão neste iPhone?", "Sign out on this iPhone?"),
          text: L("Para voltar a entrar precisa da palavra-passe e do código de 6 dígitos.", "To sign back in you’ll need your password and the 6-digit code."),
          action: L("Terminar sessão", "Sign Out"), destructive: true, onConfirm: () => void signOut(),
        })} />}
        <div className="p-dock">
          <nav className="p-tabbar" aria-label={L("Secções", "Sections")}>
            {tabs.map((t) => (
              <button key={t.id} className={`p-tab ${tab === t.id ? "on" : ""}`} aria-current={tab === t.id ? "page" : undefined} aria-label={L(t.pt, t.en)} title={L(t.pt, t.en)} onClick={() => setTab(t.id)}>
                {t.icon}
              </button>
            ))}
          </nav>
          {canRecord(me) && (
            <button className={`p-fab ${tab === "record" ? "on" : ""} ${live ? "live" : ""}`} onClick={() => setTab("record")} aria-current={tab === "record" ? "page" : undefined}
              aria-label={live ? L("Gravação em curso", "Recording in progress") : L("Gravar sessão", "Record Session")} title={L("Gravar sessão", "Record Session")}>
              <Mic />
            </button>
          )}
        </div>
        {sheet && <div className="p-scrim" onClick={() => setSheet(null)} aria-hidden />}
        {sheet && <div className="p-sheet" role="dialog" aria-modal="true">{sheet}</div>}
        <ActionAlert alert={alert} close={() => setAlert(null)} L={L} />
        <Toast msg={toastMsg} L={L} clear={() => setToastMsg(null)} />
      </div>
    </Ctx.Provider>
  );
}

/** Confirmations as an iOS action sheet: the action (red when destructive, held for 5 s when it deletes), then Cancel. */
function ActionAlert({ alert, close, L }: { alert: ConfirmOpts | null; close: () => void; L: (pt: string, en: string) => string }) {
  if (!alert) return null;
  const go = () => { close(); alert.onConfirm(); };
  return (
    <>
      <div className="p-scrim on" onClick={close} aria-hidden />
      <div className="p-action" role="alertdialog" aria-labelledby="p-action-t">
        <div className="p-action-card">
          <div className="p-action-head"><b id="p-action-t">{alert.title}</b>{alert.text && <span>{alert.text}</span>}</div>
          {alert.hold ? <div className="p-action-hold"><HoldButton label={alert.action} onDone={go} /></div>
            : <button className={`p-action-btn ${alert.destructive ? "bad" : ""}`} onClick={go}>{alert.action}</button>}
        </div>
        <button className="p-action-card p-action-btn cancel" onClick={close} autoFocus>{L("Cancelar", "Cancel")}</button>
      </div>
    </>
  );
}

function Toast({ msg, L, clear }: { msg: { text: string; undo?: () => void } | null; L: (pt: string, en: string) => string; clear: () => void }) {
  return (
    <div className={`p-toast frosted ${msg ? "on" : ""}`} role="status" aria-live="polite">
      {msg && <><span>{msg.text}</span>{msg.undo && <button onClick={() => { msg.undo?.(); clear(); }}>{L("Desfazer", "Undo")}</button>}</>}
    </div>
  );
}
