import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { CalendarDays, Check, ChevronLeft, ChevronRight, Contact, Lock, Menu as MenuIcon, Search, Settings as Gear, Sparkles, Sun, Users, Wallet } from "lucide-react";
import { Ctx, useApp, type AppCtx, type ConfirmOpts, type CtxItem, type Lang, type Page, type Role } from "./lib/ctx";
import { call, isMac, isTauri, ks, onNativeMenu } from "./lib/platform";
import { api, SIGNED_OUT, setPreviewRole, type Me } from "./lib/api";
import { version as APP_VERSION } from "../package.json";
import { draftsFor, noteDrafts } from "./lib/drafts";
import { useApi, hm, range, addDays, NOTE_TEMPLATES, type Appointment, type PatientRow, type Therapist } from "./lib/data";
import { isRecording, stopRecording } from "./lib/recorder";
import { RecordSheet, RecordingPill } from "./components/Recording";
import { HoldButton, Mark, Sheet } from "./components/ui";
import { AssistSlot } from "./components/Layout";
import { Assistant } from "./components/Assistant";
import { NewAppointment, NewPatient, RecordPayment, SellPlan } from "./components/Sheets";
import { ComplaintsLog, DeleteDay } from "./components/ClinicSheets";
import { ExportRecord } from "./components/LegalSheets";
import { DischargeLetter, ReimbursementPack } from "./components/LetterSheets";
import { ExpenseSheet } from "./components/MoneySheets";
import { FIN_SECTION } from "./pages/Finance";
import Login from "./pages/Login";
import Today from "./pages/Today";
import Schedule, { openAppointment } from "./pages/Schedule";
import Patients from "./pages/Patients";
import Finance from "./pages/Finance";
import Team from "./pages/Team";
import Settings, { settingsNav, updateErrorText } from "./pages/Settings";
import { checkForUpdate, startUpdateChecks } from "./lib/updates";

const ALL_PAGES: { id: Page; icon: ReactNode; pt: string; en: string; owner?: boolean }[] = [
  { id: "today", icon: <Sun />, pt: "Hoje", en: "Today" },
  { id: "schedule", icon: <CalendarDays />, pt: "Agenda", en: "Schedule" },
  { id: "patients", icon: <Users />, pt: "Utentes", en: "Patients" },
  { id: "finance", icon: <Wallet />, pt: "Finanças", en: "Finance", owner: true },
  { id: "team", icon: <Contact />, pt: "Equipa", en: "Team", owner: true },
];
/** Pages a role can use: Finance and Team are the owner's (audit M3: no dead-end pages for the others; the server enforces it). */
const pagesFor = (role: Role) => ALL_PAGES.filter((p) => !p.owner || role === "owner");
/** Windows: the caption buttons in the app's own title bar (the OS one is off, so there is one bar). Same order,
 *  size and glyphs as Windows 11; Close turns red on hover. Double-click the empty bar to maximise. */
function WinCaption() {
  const { L } = useApp();
  const [max, setMax] = useState(false);
  useEffect(() => {
    const check = () => { call<boolean>("window_maximized").then((m) => setMax(!!m)).catch(() => {}); };
    check(); window.addEventListener("resize", check);
    return () => window.removeEventListener("resize", check);
  }, []);
  const g = { width: 10, height: 10, viewBox: "0 0 10 10", fill: "none", stroke: "currentColor", strokeWidth: 1, "aria-hidden": true } as const;
  return (
    <div className="caption" role="group" aria-label={L("Janela", "Window")}>
      <button className="cap" onClick={() => windowAction("minimize")} title={L("Minimizar", "Minimize")} aria-label={L("Minimizar", "Minimize")}><svg {...g}><path d="M0 5.5h10" /></svg></button>
      <button className="cap" onClick={() => windowAction("toggle_maximize")} title={max ? L("Restaurar", "Restore Down") : L("Maximizar", "Maximize")} aria-label={max ? L("Restaurar", "Restore Down") : L("Maximizar", "Maximize")}>
        {max ? <svg {...g}><path d="M2.5 2.5V.5h7v7h-2M.5 2.5h7v7h-7z" /></svg> : <svg {...g}><rect x=".5" y=".5" width="9" height="9" /></svg>}
      </button>
      <button className="cap close" onClick={() => windowAction("close")} title={L("Fechar", "Close")} aria-label={L("Fechar", "Close")}><svg {...g}><path d="M.5.5l9 9M9.5.5l-9 9" /></svg></button>
    </div>
  );
}

/** The window itself (minimise, maximise, full screen) — the desktop shell does it (`window_action` in src-tauri/src/lib.rs). */
const windowAction = (action: "minimize" | "toggle_maximize" | "toggle_fullscreen" | "close") => { if (isTauri) call("window_action", { action }).catch(() => {}); };
const ALL: Page[] = ["today", "schedule", "patients", "finance", "team", "settings"];
// "settings" is reserved in some hosts' URLs, so the address uses "config" for that page.
const toHash = (p: Page) => (p === "settings" ? "config" : p);
const fromHash = (h: string): Page | null => { const p = (h === "config" ? "settings" : h) as Page; return ALL.includes(p) ? p : null; };

type MenuItem = { id: string; label: string; key?: string; disabled?: boolean; checked?: boolean; danger?: boolean } | "-";

export default function App() {
  const [lang, setLangState] = useState<Lang>(() => { try { return (localStorage.getItem("physiopulse-lang") as Lang) || "pt"; } catch { return "pt"; } });
  const setLang = (l: Lang) => { setLangState(l); try { localStorage.setItem("physiopulse-lang", l); } catch { /* storage unavailable */ } };
  const L = useCallback((pt: string, en: string) => (lang === "pt" ? pt : en), [lang]);
  const [authed, setAuthed] = useState(false);
  const [me, setMe] = useState<Me>({ id: "fs", name: "Filipe Silva", email: "filipe@clinica.pt", role: "owner" });
  const [idleMin, setIdleMin] = useState(15);
  const [lockNotice, setLockNotice] = useState<string | undefined>();
  const lastInput = useRef(Date.now());
  const [role, setRole] = useState<Role>("owner");
  /** The assistant: the owner, and anyone whose AI switch is on (the schedule only; the server enforces it). */
  const canAssist = role === "owner" || !!me.ai_enabled;
  const [page, setPage] = useState<Page>(() => fromHash(location.hash.slice(1)) ?? "today");
  const [patientId, setPatientId] = useState<string>(() => { try { return JSON.parse(localStorage.getItem("physiopulse-ui") || "{}").patientId || ""; } catch { return ""; } });
  const [patientName, setPatientName] = useState("");
  // Restore the window's previous state on launch (HIG: Launching).
  const saved = (() => { try { return JSON.parse(localStorage.getItem("physiopulse-ui") || "{}"); } catch { return {}; } })();
  const [q, setQ] = useState("");
  /** Find (⌘F, Edit › Find…): a floating search field, so the title bar stays empty. */
  const [find, setFind] = useState(false);
  const closeFind = () => { setFind(false); setQ(""); };
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; items: CtxItem[] } | null>(null);
  const kbOpen = useRef(false);
  const [assist, setAssist] = useState(false);
  const [drawer, setDrawer] = useState(false);
  const [menu, setMenu] = useState<string | null>(null);
  /** The open submenu of the one Menu button (23 Nov 2026: File, Edit, View… folded into one button). */
  const [sub, setSub] = useState<string | null>(null);
  const [sheet, setSheetState] = useState<ReactNode>(null);
  // Typing in a sheet marks it edited; Cancel or Esc then asks before throwing it away (audit C2).
  const sheetDirty = useRef(false);
  const [askDiscard, setAskDiscard] = useState(false);
  const setSheet = (n: ReactNode) => { sheetDirty.current = false; setAskDiscard(false); setSheetState(n); };
  const cancelSheet = () => { if (sheetDirty.current) setAskDiscard(true); else setSheet(null); };
  const [alert, setAlert] = useState<ConfirmOpts | null>(null);
  const [toastMsg, setToastMsg] = useState<{ text: string; undo?: () => void; n: number } | null>(null);
  const undoRef = useRef<(() => void) | null>(null);
  const undoLabel = useRef<string>("");

  useEffect(() => {
    const r = document.documentElement;
    r.lang = lang === "pt" ? "pt-PT" : "en";
    r.dataset.platform = isMac ? "mac" : "win";
    r.dataset.shell = isTauri ? "native" : "web";
  }, [lang]);

  // Back/Forward (title bar, Go menu, ⌘[ ⌘]): every move between pages or into a patient's file.
  type Loc = { page: Page; pid: string };
  const hist = useRef<{ back: Loc[]; fwd: Loc[] }>({ back: [], fwd: [] });
  const here = useRef<Loc>({ page, pid: patientId });
  here.current = { page, pid: patientId };
  const apply = (l: Loc) => {
    here.current = l; setPage(l.page); setPatientId(l.pid); setDrawer(false); setMenu(null);
    if (location.hash !== "#" + toHash(l.page)) history.replaceState(null, "", "#" + toHash(l.page));
  };
  const nav = (l: Loc) => {
    const c = here.current;
    if (c.page === l.page && c.pid === l.pid) { setDrawer(false); setMenu(null); return; }
    hist.current.back = [...hist.current.back.slice(-49), c]; hist.current.fwd = [];
    apply(l);
  };
  const navRef = useRef(nav); navRef.current = nav;
  const goBack = () => { const l = hist.current.back.pop(); if (l) { hist.current.fwd.push(here.current); apply(l); } };
  const goForward = () => { const l = hist.current.fwd.pop(); if (l) { hist.current.back.push(here.current); apply(l); } };
  /** Pages open at their start: Patients shows the list, not the last file. */
  const go = (p: Page) => nav({ page: p, pid: p === "patients" ? "" : here.current.pid });
  useEffect(() => {
    // A #page link (old bookmark, link in a message) is a move like any other, so Back returns from it (audit phase C).
    const f = () => { const p = fromHash(location.hash.slice(1)); if (p && p !== here.current.page) navRef.current({ page: p, pid: p === "patients" ? "" : here.current.pid }); };
    window.addEventListener("hashchange", f);
    return () => window.removeEventListener("hashchange", f);
  }, []);
  // Tooltips ([data-tip]): hidden from the click until the pointer leaves the button (see [data-tip-off] in app.css).
  useEffect(() => {
    const down = (e: PointerEvent) => {
      const el = (e.target as Element | null)?.closest?.("[data-tip]");
      if (!el) return;
      el.setAttribute("data-tip-off", "");
      el.addEventListener("pointerleave", () => el.removeAttribute("data-tip-off"), { once: true });
    };
    document.addEventListener("pointerdown", down);
    return () => document.removeEventListener("pointerdown", down);
  }, []);
  useEffect(() => { if (role !== "owner") setAssist(false); if (!isTauri) setPreviewRole(role); }, [role]);
  const PAGES = pagesFor(role);
  // A page this role can't use (an old #finance link, or the role changed): go to Today instead.
  useEffect(() => { if (page !== "settings" && !PAGES.some((p) => p.id === page)) apply({ page: "today", pid: "" }); }, [role, page]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { try { localStorage.setItem("physiopulse-ui", JSON.stringify({ patientId })); } catch { /* storage unavailable */ } }, [patientId]);

  const toast = useCallback((text?: string, undo?: () => void, label?: string) => {
    undoRef.current = undo ?? null;
    undoLabel.current = undo ? (label ?? "") : "";
    setToastMsg({ text: text ?? (lang === "pt" ? "Pré-visualização: esta ação ainda não está ligada." : "Preview: this action isn’t connected yet."), undo, n: Date.now() });
  }, [lang]);
  useEffect(() => {
    if (!toastMsg) return;
    const t = setTimeout(() => { setToastMsg(null); undoRef.current = null; }, toastMsg.undo ? 6000 : 2400);
    return () => clearTimeout(t);
  }, [toastMsg]);

  const ctx: AppCtx = useMemo(() => ({
    lang, setLang, L, role, setRole, me, setMe, idleMinutes: idleMin, page, go, toast,
    openSheet: (n) => { setMenu(null); setCtxMenu(null); setSheet(n); }, closeSheet: () => setSheet(null), cancelSheet, sheetEdited: () => { sheetDirty.current = true; },
    menuAt: (x, y, items) => { setMenu(null); setCtxMenu({ x, y, items }); },
    confirm: (o) => { setSheet(null); setAlert(o); },
    patientId, openPatient: (id) => nav({ page: "patients", pid: id }),
    patientName, setPatientName,
  }), [lang, L, role, me, idleMin, page, patientId, patientName, toast]); // eslint-disable-line react-hooks/exhaustive-deps

  function Shortcuts() {
    const rows: [string, string, string][] = [
      ["h", L("Geral", "General"), ""],
      ["", L("Nova marcação", "New appointment"), ks("⌘N", "Ctrl+N")], ["", L("Novo utente", "New patient"), ks("⇧⌘N", "Ctrl+Shift+N")],
      ["", L("Registar pagamento", "Record payment"), ks("⇧⌘P", "Ctrl+Shift+P")], ["", L("Procurar", "Find"), ks("⌘F", "Ctrl+F")],
      ["", L("Desfazer", "Undo"), ks("⌘Z", "Ctrl+Z")], ["", L("Definições", "Settings"), ks("⌘,", "Ctrl+,")], ["", L("Bloquear", "Lock"), ks("⌃⌘Q", "Ctrl+L")],
      ["h", L("Ver", "View"), ""],
      ["", L("Assistente", "Assistant"), ks("⌥⌘A", "Ctrl+Alt+A")],
      ["h", L("Ir", "Go"), ""],
      ["", L("Anterior", "Back"), ks("⌘[", "Alt+←")], ["", L("Seguinte", "Forward"), ks("⌘]", "Alt+→")],
      ...PAGES.map((p, i) => ["", L(p.pt, p.en), ks(`⌘${i + 1}`, `Ctrl+${i + 1}`)] as [string, string, string]),
      ["h", L("Menus", "Menus"), ""],
      ["", L("Mover entre itens", "Move between items"), "↑ ↓"], ["", L("Mudar de menu", "Switch menu"), "← →"], ["", L("Fechar", "Close"), "Esc"],
    ];
    return (
      <Sheet title={L("Atalhos de teclado", "Keyboard Shortcuts")}>
        <div className="keys">{rows.flatMap(([h, a, k], i) => h ? [<div key={i} className="kh">{a}</div>] : [<span key={i + "a"}>{a}</span>, <kbd key={i + "k"}>{k}</kbd>])}</div>
      </Sheet>
    );
  } // eslint-disable-line react-hooks/exhaustive-deps

  const undo = () => { const u = undoRef.current; if (u) { u(); undoRef.current = null; setToastMsg(null); } };

  /** Sign out on the server and return to the sign-in screen. */
  const signOut = (notice?: string) => {
    if (authed) api("POST", "/auth/logout").catch(() => { /* already signed out */ });
    setSheet(null); setAlert(null); setMenu(null); setCtxMenu(null); setAssist(false);
    setLockNotice(notice); setAuthed(false);
  };
  // The server ended the session (idle timeout, deactivated, signed out elsewhere).
  useEffect(() => {
    const f = () => { setLockNotice(L("A sua sessão terminou. Entre de novo.", "Your session ended. Sign in again.")); setAuthed(false); };
    window.addEventListener(SIGNED_OUT, f);
    return () => window.removeEventListener(SIGNED_OUT, f);
  }, [L]);
  // New versions (GitHub Releases, signed): checked quietly after sign-in and twice a day; only a new one is announced.
  useEffect(() => {
    if (!authed) return;
    return startUpdateChecks((u) => toast(L(`O PhysioPulse ${u.version} está disponível. Atualize em Definições › Visão geral.`, `PhysioPulse ${u.version} is available. Update in Settings › Overview.`)));
  }, [authed]); // eslint-disable-line react-hooks/exhaustive-deps
  // Lock after the idle time set on the server (15 minutes by default).
  useEffect(() => {
    if (!authed) return;
    const touch = () => { lastInput.current = Date.now(); };
    const evs = ["mousemove", "keydown", "mousedown", "wheel", "touchstart"];
    evs.forEach((e) => window.addEventListener(e, touch, { passive: true }));
    lastInput.current = Date.now();
    const t = setInterval(() => {
      // A session being recorded keeps the app awake: locking would sign out and end the recording (phase 2).
      if (isRecording()) { lastInput.current = Date.now(); return; }
      if (Date.now() - lastInput.current > idleMin * 60_000) signOut(L(`Bloqueada após ${idleMin} minutos sem uso.`, `Locked after ${idleMin} minutes without use.`));
    }, 15_000);
    return () => { clearInterval(t); evs.forEach((e) => window.removeEventListener(e, touch)); };
  }, [authed, idleMin]); // eslint-disable-line react-hooks/exhaustive-deps

  /** One command table for in-window menus, native menu bar and keyboard shortcuts. */
  const run = (id: string) => {
    setMenu(null);
    // While a sheet or alert is open, only editing commands work: nothing replaces it or changes the page behind (audit C2).
    if ((sheet || alert || askDiscard) && !["cut", "copy", "paste", "select_all"].includes(id)) return;
    const owner = role === "owner";
    switch (id) {
      case "new_appointment": return setSheet(<NewAppointment />);
      case "new_patient": return setSheet(<NewPatient />);
      case "record_payment": return setSheet(<RecordPayment patientId={page === "patients" ? patientId : undefined} />);
      case "sell_plan": return setSheet(<SellPlan patientId={page === "patients" ? patientId : undefined} />);
      case "new_expense": return owner ? setSheet(<ExpenseSheet />) : undefined;
      case "export_record": return role === "reception" ? undefined : page !== "patients" || !patientName ? (go("patients"), toast(L("Abra a ficha do utente e escolha Exportar de novo.", "Open the patient’s file and choose Export again."))) : setSheet(<ExportRecord pid={patientId} name={patientName} />);
      case "reimbursement_pack": return role === "reception" ? undefined : page !== "patients" || !patientName ? (go("patients"), toast(L("Abra a ficha do utente e escolha de novo.", "Open the patient’s file and choose it again."))) : setSheet(<ReimbursementPack pid={patientId} name={patientName} />);
      case "discharge_letter": return role === "reception" ? undefined : page !== "patients" || !patientName ? (go("patients"), toast(L("Abra a ficha do utente e escolha de novo.", "Open the patient’s file and choose it again."))) : setSheet(<DischargeLetter pid={patientId} name={patientName} />);
      case "accountant_export": if (owner) { go("finance"); setTimeout(() => window.dispatchEvent(new CustomEvent(FIN_SECTION, { detail: "export" })), 0); } return;
      case "complaints_log": return owner ? setSheet(<ComplaintsLog />) : undefined;
      case "delete_day": return owner ? setSheet(<DeleteDay />) : undefined;
      case "undo": return undo();
      case "find": return setFind(true);
      case "back": return goBack();
      case "forward": return goForward();
      case "toggle_assistant": return canAssist ? setAssist((a) => !a) : undefined;
      case "settings": return go("settings");
      case "lock": return isRecording()
        ? setAlert({ title: L("Terminar a gravação e bloquear?", "Stop recording and lock?"), text: L("A gravação é enviada para a IA local antes de bloquear.", "The recording is sent to the local AI before locking."),
            action: L("Terminar e bloquear", "Stop and Lock"), onConfirm: () => { stopRecording().catch(() => undefined).finally(() => signOut()); } })
        : signOut();
      case "record_session": {
        if (role === "reception" || !(owner || me.ai_enabled)) return;
        if (isRecording()) return toast(L("Já está a gravar uma sessão: está na barra de cima.", "A session is already being recorded: it’s in the bar at the top."));
        if (page !== "patients" || !patientName) { go("patients"); return toast(L("Abra a ficha do utente e escolha Gravar sessão de novo.", "Open the patient’s file and choose Record Session again.")); }
        const d = noteDrafts.get(patientId);
        const t = NOTE_TEMPLATES.find((x) => x.id === d?.tpl) ?? NOTE_TEMPLATES[0];
        return setSheet(<RecordSheet pid={patientId} name={patientName} template={t.id} templateName={L(t.name[0], t.name[1])} blocks={d?.blocks ?? t.blocks(lang)} appointmentId={null} />);
      }
      case "about": {
        // The real versions: this app's (package.json) and the clinic server's (/status).
        const show = (srv: string) => setAlert({ title: "PhysioPulse", text: L(`Aplicação ${APP_VERSION} · servidor ${srv}`, `App ${APP_VERSION} · server ${srv}`), action: "OK", onConfirm: () => {} });
        api<{ version?: string }>("GET", "/status").then((r) => show(r.version ?? "—")).catch(() => show(L("sem ligação", "not connected")));
        return;
      }
      case "check_updates": {
        // App menu › Check for Updates… (HIG): say the result in an alert; installing is offered in Settings › Overview,
        // where the download's progress shows.
        checkForUpdate().then((s) => {
          if (s.kind === "available") {
            setAlert({ title: L(`Há uma nova versão: ${s.info.version}`, `A new version is available: ${s.info.version}`),
              text: s.info.notes ?? L(`Tem a versão ${APP_VERSION}.`, `You have version ${APP_VERSION}.`),
              action: L("Ver em Definições", "Show in Settings"), onConfirm: () => { settingsNav.sec = "general"; go("settings"); } });
          } else if (s.kind === "latest") {
            setAlert({ title: L("O PhysioPulse está atualizado", "PhysioPulse is up to date"), text: L(`Tem a versão mais recente (${APP_VERSION}).`, `You have the latest version (${APP_VERSION}).`), action: "OK", onConfirm: () => {} });
          } else if (s.kind === "error") {
            setAlert({ title: L("Não foi possível procurar atualizações", "Couldn’t check for updates"), text: updateErrorText(s.message, L), action: "OK", onConfirm: () => {} });
          }
        });
        return;
      }
      case "cut": case "copy": case "select_all": document.execCommand(id === "select_all" ? "selectAll" : id); return;
      case "paste": {
        const el = document.activeElement as HTMLInputElement | null;
        navigator.clipboard?.readText().then((t) => { if (el && "setRangeText" in el) { el.setRangeText(t, el.selectionStart ?? 0, el.selectionEnd ?? 0, "end"); el.dispatchEvent(new Event("input", { bubbles: true })); } }).catch(() => toast(L("Use ⌘V para colar.", "Use ⌘V to paste.")));
        return;
      }
      case "shortcuts": return setSheet(<Shortcuts />);
      case "minimize": return windowAction("minimize");
      case "zoom": return windowAction("toggle_maximize");
      case "fullscreen": return windowAction("toggle_fullscreen");
      default:
        if (id.startsWith("go_") && PAGES.some((p) => p.id === id.slice(3))) return go(id.slice(3) as Page);
    }
  };

  // Native menu bar (Tauri) sends the same command ids.
  const runRef = useRef(run); runRef.current = run;
  useEffect(() => { let off = () => {}; onNativeMenu((id) => runRef.current(id)).then((u) => (off = u)); return () => off(); }, []);

  // Keyboard shortcuts (⌘ on Mac, Ctrl on Windows).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!authed) return;
      if (e.key === "Escape") { if (ctxMenu) return setCtxMenu(null); if (find) return closeFind(); if (askDiscard) return setAskDiscard(false); if (alert) return setAlert(null); if (sheet) return cancelSheet(); if (menu) return setMenu(null); }
      // Full Keyboard Access in menus: arrows move, Home/End jump, ←/→ switch menu-bar menus.
      if ((menu || ctxMenu) && ["ArrowDown", "ArrowUp", "Home", "End", "ArrowLeft", "ArrowRight"].includes(e.key)) {
        // The Menu button sits at the trailing edge, so its submenus open towards the leading side: ← opens, → goes back.
        if (menu === "bar" && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
          e.preventDefault();
          const k = (document.activeElement as HTMLElement)?.dataset?.sub;
          if (e.key === "ArrowLeft" && k) { kbOpen.current = true; setSub(k); }
          if (e.key === "ArrowRight" && sub) { const s = sub; setSub(null); setTimeout(() => document.querySelector<HTMLButtonElement>(`.menu.root [data-sub="${s}"]`)?.focus(), 0); }
          return;
        }
        const items = [...document.querySelectorAll<HTMLButtonElement>(sub ? ".menu.sub .mi:not(:disabled)" : ".menu .mi:not(:disabled), .ctxmenu .mi:not(:disabled)")];
        if (!items.length) return;
        e.preventDefault();
        const i = items.indexOf(document.activeElement as HTMLButtonElement);
        const n = e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : e.key === "ArrowDown" ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
        items[n]?.focus();
        return;
      }
      if (alert && e.key === "Enter" && !alert.destructive && !alert.hold) { e.preventDefault(); alert.onConfirm(); return setAlert(null); }
      // App shortcuts are off while a sheet or alert is open (they used to replace it or change the page behind).
      if (sheet || alert || askDiscard) return;
      if (isTauri && isMac) return; // the native menu bar handles shortcuts on macOS
      const mod = isMac ? e.metaKey : e.ctrlKey, k = e.key.toLowerCase();
      if (isMac ? e.metaKey && !e.altKey && e.key === "[" : e.altKey && e.key === "ArrowLeft") { e.preventDefault(); return run("back"); }
      if (isMac ? e.metaKey && !e.altKey && e.key === "]" : e.altKey && e.key === "ArrowRight") { e.preventDefault(); return run("forward"); }
      if (isMac ? e.metaKey && e.altKey && e.code === "KeyA" : e.ctrlKey && e.altKey && k === "a") { e.preventDefault(); return run("toggle_assistant"); }
      if (isMac ? e.metaKey && e.ctrlKey && k === "q" : e.ctrlKey && k === "l") { e.preventDefault(); return run("lock"); }
      if (!isMac && e.key === "F11") { e.preventDefault(); return run("fullscreen"); }
      if (!mod || e.altKey) return;
      if (/^[1-9]$/.test(e.key)) { e.preventDefault(); const p = PAGES[+e.key - 1]; return p ? go(p.id) : undefined; }
      if (e.key === ",") { e.preventDefault(); return run("settings"); }
      if (k === "f") { e.preventDefault(); return run("find"); }
      if (k === "n" && e.shiftKey) { e.preventDefault(); return run("new_patient"); }
      if (k === "n") { e.preventDefault(); return run("new_appointment"); }
      if (k === "p" && e.shiftKey) { e.preventDefault(); return run("record_payment"); }
      if (k === "z" && !e.shiftKey && undoRef.current && !/INPUT|TEXTAREA/.test((document.activeElement as HTMLElement)?.tagName)) { e.preventDefault(); return undo(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // Sheets and alerts are modal for the keyboard too (audit M8): everything behind is inert, Tab stays inside,
  // focus starts in the sheet and goes back to what opened it when it closes.
  const opener = useRef<HTMLElement | null>(null);
  const modalOpen = !!(sheet || alert || askDiscard);
  useEffect(() => {
    if (!modalOpen) return;
    if (!opener.current) opener.current = document.activeElement as HTMLElement | null;
    const scrims = [...document.querySelectorAll<HTMLElement>(".app > .scrim")];
    const top = scrims[scrims.length - 1];
    if (!top) return;
    const behind = [...document.querySelectorAll<HTMLElement>(".app > *")].filter((el) => el !== top);
    behind.forEach((el) => { el.inert = true; });
    if (!top.contains(document.activeElement)) {
      const first = top.querySelector<HTMLElement>("[autofocus], input:not([type=hidden]):not(:disabled), select:not(:disabled), textarea:not(:disabled), button:not(:disabled), [tabindex]:not([tabindex='-1'])");
      (first ?? top).focus?.();
    }
    const trap = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const f = [...top.querySelectorAll<HTMLElement>("a[href], button:not(:disabled), input:not(:disabled):not([type=hidden]), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex='-1'])")].filter((el) => el.offsetParent !== null);
      if (!f.length) return;
      const i = f.indexOf(document.activeElement as HTMLElement);
      if (e.shiftKey && (i <= 0)) { e.preventDefault(); f[f.length - 1].focus(); }
      else if (!e.shiftKey && (i === f.length - 1 || i < 0)) { e.preventDefault(); f[0].focus(); }
    };
    document.addEventListener("keydown", trap);
    return () => { document.removeEventListener("keydown", trap); behind.forEach((el) => { el.inert = false; }); };
  }, [modalOpen, sheet, alert, askDiscard]);
  useEffect(() => {
    if (modalOpen) return;
    const el = opener.current; opener.current = null;
    if (el && document.contains(el)) setTimeout(() => el.focus?.(), 0);
  }, [modalOpen]);

  useEffect(() => {
    if ((menu || ctxMenu) && kbOpen.current) { kbOpen.current = false; setTimeout(() => document.querySelector<HTMLButtonElement>(".menu .mi:not(:disabled), .ctxmenu .mi:not(:disabled)")?.focus(), 0); }
  }, [menu, ctxMenu]);
  useEffect(() => { if (menu !== "bar") setSub(null); }, [menu]);
  useEffect(() => {
    if (sub && kbOpen.current) { kbOpen.current = false; setTimeout(() => document.querySelector<HTMLButtonElement>(".menu.sub .mi:not(:disabled)")?.focus(), 0); }
  }, [sub]);
  useEffect(() => {
    if (!ctxMenu) return;
    const f = (e: MouseEvent) => { if (!(e.target as HTMLElement).closest(".ctxmenu")) setCtxMenu(null); };
    const t = setTimeout(() => document.addEventListener("mousedown", f), 0);
    return () => { clearTimeout(t); document.removeEventListener("mousedown", f); };
  }, [ctxMenu]);

  // Close menus on outside click.
  useEffect(() => {
    if (!menu) return;
    const f = (e: MouseEvent) => { if (!(e.target as HTMLElement).closest("[data-menu-root]")) setMenu(null); };
    document.addEventListener("mousedown", f);
    return () => document.removeEventListener("mousedown", f);
  }, [menu]);

  if (!authed) {
    return (
      <Ctx.Provider value={ctx}>
        <div className="wall" aria-hidden="true" />
        {/* No OS frame on Windows, so the sign-in screen needs its own bar to drag, double-click and close the window. */}
        {isTauri && <header className="login-bar" data-tauri-drag-region>{!isMac && <WinCaption />}</header>}
        <Login notice={lockNotice} onDone={(m, idle) => { draftsFor(m.id); setMe(m); setRole(m.role); setIdleMin(idle); setLockNotice(undefined); setAuthed(true); }} />
        <div className={`toast frosted ${toastMsg ? "on" : ""}`} role="status" aria-live="polite">{toastMsg && <span>{toastMsg.text}</span>}</div>
      </Ctx.Provider>
    );
  }

  const owner = role === "owner";
  const meIni = me.name.split(" ").map((w) => w[0]).join("").slice(0, 2).toUpperCase();
  const menus: Record<string, { label: string; items: MenuItem[] }> = {
    app: { label: "PhysioPulse", items: [
      { id: "about", label: L("Acerca do PhysioPulse", "About PhysioPulse") },
      { id: "check_updates", label: L("Procurar atualizações…", "Check for Updates…") }, "-",
      { id: "settings", label: L("Definições…", "Settings…"), key: ks("⌘,", "Ctrl+,") }, "-",
      { id: "lock", label: L("Bloquear", "Lock"), key: ks("⌃⌘Q", "Ctrl+L") },
    ] },
    file: { label: L("Ficheiro", "File"), items: [
      { id: "new_appointment", label: L("Nova marcação…", "New Appointment…"), key: ks("⌘N", "Ctrl+N") },
      { id: "new_patient", label: L("Novo utente…", "New Patient…"), key: ks("⇧⌘N", "Ctrl+Shift+N") },
      { id: "record_payment", label: L("Registar pagamento…", "Record Payment…"), key: ks("⇧⌘P", "Ctrl+Shift+P") },
      { id: "sell_plan", label: L("Vender pacote…", "Sell Package…") },
      ...(owner ? [{ id: "new_expense", label: L("Nova despesa…", "New Expense…") }] : []),
      ...(role !== "reception" ? ["-" as const,
        { id: "export_record", label: L("Exportar processo clínico…", "Export Clinical Record…") },
        { id: "reimbursement_pack", label: L("Pacote de reembolso…", "Reimbursement Pack…") },
        { id: "discharge_letter", label: L("Carta de alta…", "Discharge Letter…") },
        ...(owner || me.ai_enabled ? [{ id: "record_session", label: L("Gravar sessão…", "Record Session…") }] : [])] : []),
      ...(owner ? [{ id: "accountant_export", label: L("Exportar para contabilista…", "Accountant Export…") }, "-" as const,
        { id: "complaints_log", label: L("Livro de reclamações…", "Complaints Log…") },
        { id: "delete_day", label: L("Apagar um dia…", "Delete a Day…") }] : []),
    ] },
    edit: { label: L("Editar", "Edit"), items: [
      { id: "undo", label: undoRef.current && undoLabel.current ? `${L("Desfazer", "Undo")} ${undoLabel.current}` : L("Desfazer", "Undo"), key: ks("⌘Z", "Ctrl+Z"), disabled: !undoRef.current },
      "-",
      { id: "cut", label: L("Cortar", "Cut"), key: ks("⌘X", "Ctrl+X") }, { id: "copy", label: L("Copiar", "Copy"), key: ks("⌘C", "Ctrl+C") },
      { id: "paste", label: L("Colar", "Paste"), key: ks("⌘V", "Ctrl+V") }, { id: "select_all", label: L("Selecionar tudo", "Select All"), key: ks("⌘A", "Ctrl+A") }, "-",
      { id: "find", label: L("Procurar…", "Find…"), key: ks("⌘F", "Ctrl+F") },
    ] },
    view: { label: L("Ver", "View"), items: [
      ...(canAssist ? [{ id: "toggle_assistant", label: assist ? L("Ocultar assistente", "Hide Assistant") : L("Mostrar assistente", "Show Assistant"), key: ks("⌥⌘A", "Ctrl+Alt+A") }] : []),
      ...(isTauri ? ["-" as const, { id: "fullscreen", label: L("Ecrã completo", "Full Screen"), key: ks("⌃⌘F", "F11") }] : []),
    ] },
    go: { label: L("Ir", "Go"), items: [
      { id: "back", label: L("Anterior", "Back"), key: ks("⌘[", "Alt+←"), disabled: !hist.current.back.length },
      { id: "forward", label: L("Seguinte", "Forward"), key: ks("⌘]", "Alt+→"), disabled: !hist.current.fwd.length }, "-",
      ...PAGES.map((p, i) => ({ id: "go_" + p.id, label: L(p.pt, p.en), key: ks(`⌘${i + 1}`, `Ctrl+${i + 1}`), checked: page === p.id })),
    ] },
    // Windows: the window commands are real (the browser preview has no window to change, so it leaves them out).
    ...(isTauri ? { window: { label: L("Janela", "Window"), items: [
      { id: "minimize", label: L("Minimizar", "Minimize") }, { id: "zoom", label: L("Maximizar", "Maximize") },
    ] as MenuItem[] } } : {}),
    help: { label: L("Ajuda", "Help"), items: [
      { id: "shortcuts", label: L("Atalhos de teclado", "Keyboard Shortcuts") },
    ] },
  };
  // Windows has no app-name menu (owner, 11 Nov 2026: the title bar doesn't spell the name). Its items go where
  // Windows apps keep them: Settings and Lock at the end of File, About at the end of Help. Same command ids.
  if (!isMac) {
    const { app, ...rest } = menus;
    const [about, updates, , ...fileEnd] = app.items;
    rest.file = { ...rest.file, items: [...rest.file.items, "-", ...fileEnd] };
    rest.help = { ...rest.help, items: [...rest.help.items, "-", updates, about] };
    for (const k of Object.keys(menus)) delete menus[k];
    Object.assign(menus, rest);
  }

  const MenuList = ({ items, cls = "" }: { items: MenuItem[]; cls?: string }) => (
    <div className={`menu frosted ${cls}`} role="menu">
      {items.map((it, i) => it === "-" ? <div key={i} className="msep" role="separator" /> : (
        <button key={it.id} className={`mi ${it.danger ? "danger" : ""}`} role="menuitem" disabled={it.disabled} onClick={() => run(it.id)}>
          <span className="chk">{it.checked && <Check size={13} />}</span>{it.label}{it.key && <span className="kbd">{it.key}</span>}
        </button>
      ))}
    </div>
  );

  const showMenus = !(isTauri && isMac);
  const winCaption = isTauri && !isMac;
  const cls = drawer ? "drawer" : "";

  return (
    <Ctx.Provider value={ctx}>
      <div className="wall" aria-hidden="true" />
      <div className={`app ${cls}`} data-page={page}>
        {/* One bar (owner, 12–13 Nov 2026): profile · Settings · assistant and Back/Forward on the leading side, the pages as
            pills in the middle, the menus beside the window buttons on the trailing side. No logo, no sidebar toggle. */}
        <header className="titlebar" data-tauri-drag-region>
          <div className="tb-lead" data-tauri-drag-region>
            <div className="avbtn-wrap" style={{ position: "relative" }} data-menu-root>
              <button className={`avbtn ${page === "settings" ? "on" : ""}`} aria-haspopup="menu" aria-expanded={menu === "profile"} aria-label={me.name} title={me.name} onClick={() => setMenu(menu === "profile" ? null : "profile")}>
                <span className="avatar">{meIni}</span>
              </button>
              {menu === "profile" && (
                <div className="menu frosted" role="menu">
                  <div className="mhead" style={{ color: "var(--text)", fontSize: "var(--fs-sub)" }}>{me.name}</div>
                  <div className="mhead" style={{ paddingTop: 0 }}>{me.email}</div>
                  {!isTauri && <>
                    <div className="msep" />
                    <div className="mhead">{L("Ver como (pré-visualização)", "View as (preview)")}</div>
                    {(["owner", "therapist", "reception"] as Role[]).map((r) => (
                      <button key={r} className="mi" role="menuitemradio" aria-checked={role === r} onClick={() => { setRole(r); setMenu(null); }}>
                        <span className="chk">{role === r && <Check size={13} />}</span>{r === "owner" ? L("Proprietário", "Owner") : r === "therapist" ? L("Fisioterapeuta", "Therapist") : L("Receção", "Reception")}
                      </button>
                    ))}
                  </>}
                  <div className="msep" />
                  <div className="mhead">{L("Idioma", "Language")}</div>
                  {([["pt", "Português"], ["en", "English"]] as [Lang, string][]).map(([l, n]) => (
                    <button key={l} className="mi" role="menuitemradio" aria-checked={lang === l} onClick={() => { setLang(l); setMenu(null); }}><span className="chk">{lang === l && <Check size={13} />}</span>{n}</button>
                  ))}
                  <div className="msep" />
                  <button className="mi" role="menuitem" onClick={() => { setMenu(null); run("settings"); }}><span className="chk"><Gear size={13} /></span>{L("Definições…", "Settings…")}<span className="kbd">{ks("⌘,", "Ctrl+,")}</span></button>
                  <button className="mi" role="menuitem" onClick={() => run("lock")}><span className="chk"><Lock size={13} /></span>{L("Bloquear", "Lock")}<span className="kbd">{ks("⌃⌘Q", "Ctrl+L")}</span></button>
                </div>
              )}
            </div>
            {canAssist && <button className="iconbtn round ai" aria-pressed={assist} aria-haspopup="dialog" onClick={() => run("toggle_assistant")} title={L("Assistente", "Assistant")} aria-label={assist ? L("Fechar assistente", "Close Assistant") : L("Abrir assistente", "Open Assistant")}><Sparkles /></button>}
            <span className="tb-sep" aria-hidden="true" />
            <button className="iconbtn" disabled={!hist.current.back.length} onClick={goBack} title={L("Anterior", "Back")} aria-label={L("Anterior", "Back")}><ChevronLeft /></button>
            <button className="iconbtn" disabled={!hist.current.fwd.length} onClick={goForward} title={L("Seguinte", "Forward")} aria-label={L("Seguinte", "Forward")}><ChevronRight /></button>
            <RecordingPill />
          </div>

          <nav className="pills" aria-label={L("Páginas", "Pages")}>
            {PAGES.map((p, i) => (
              <button key={p.id} className="pill" aria-label={L(p.pt, p.en)} title={`${L(p.pt, p.en)} (${ks(`⌘${i + 1}`, `Ctrl+${i + 1}`)})`} aria-current={page === p.id ? "page" : undefined} onClick={() => go(p.id)}>
                <span className="pi" aria-hidden="true">{p.icon}</span><span className="pl">{L(p.pt, p.en)}</span>
              </button>
            ))}
          </nav>

          <div className="tb-trail" data-tauri-drag-region>
            {showMenus && (
              /* One Menu button instead of five words (23 Nov 2026): the words pushed the page names out of the bar on a
                 laptop. Same commands, same shortcuts; submenus open towards the window. */
              <nav className="menus" aria-label="Menu" data-menu-root>
                <button className="iconbtn round" aria-haspopup="menu" aria-expanded={menu === "bar"} aria-label={L("Menu", "Menu")} title={L("Menu", "Menu")} onClick={(e) => { kbOpen.current = e.detail === 0; setMenu(menu === "bar" ? null : "bar"); }}><MenuIcon /></button>
                {menu === "bar" && (
                  <div className="menu frosted right root" role="menu">
                    {Object.entries(menus).map(([k, m]) => (
                      <div key={k} className="mroot" onMouseEnter={() => setSub(k)}>
                        <button className="mi" role="menuitem" aria-haspopup="menu" aria-expanded={sub === k} data-sub={k} onClick={(e) => { kbOpen.current = e.detail === 0; setSub(sub === k ? null : k); }} style={k === "app" ? { fontWeight: 700 } : undefined}>
                          <span className="chk" />{m.label}<ChevronRight size={14} className="kbd" />
                        </button>
                        {sub === k && <MenuList items={m.items} cls="sub" />}
                      </div>
                    ))}
                  </div>
                )}
              </nav>
            )}
            {winCaption && <WinCaption />}
          </div>
        </header>

        <AssistSlot.Provider value={null}>
          {page === "today" && <Today />}
          {page === "schedule" && <Schedule />}
          {page === "patients" && <Patients />}
          {page === "finance" && <Finance />}
          {page === "team" && <Team />}
          {page === "settings" && <Settings />}
        </AssistSlot.Provider>

        {/* The assistant floats in front of everything (owner, 13 Nov 2026: "not a side panel, but a new panel in front"). */}
        {assist && canAssist && (
          <div className="assist-float frosted" role="dialog" aria-label={L("Assistente", "Assistant")} onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); setAssist(false); } }}>
            <Assistant onClose={() => setAssist(false)} />
          </div>
        )}

        {/* Sheets and alerts are modal (HIG): a click outside doesn't close them, so typed details are never lost. Esc or Cancel closes. */}
        {(sheet || alert) && (
          <div className="scrim" onInputCapture={(e) => { if (sheet && !alert && !(e.target as Element).closest?.("[data-no-edit]")) sheetDirty.current = true; }} onChangeCapture={(e) => { if (sheet && !alert && !(e.target as Element).closest?.("[data-no-edit]")) sheetDirty.current = true; }}>
            {alert ? (
              <div className="alert frosted" role="alertdialog" aria-modal="true" aria-labelledby="alert-title">
                <Mark />
                <h2 id="alert-title">{alert.title}</h2>
                {alert.text && <p>{alert.text}</p>}
                <div className="btns">
                  {alert.hold
                    ? <HoldButton label={alert.action} onDone={() => { alert.onConfirm(); setAlert(null); }} />
                    : <button className={`btn wide ${alert.destructive ? "danger" : "pri"}`} onClick={() => { alert.onConfirm(); setAlert(null); }}>{alert.action}</button>}
                  {alert.action !== "OK" && <button className="btn wide" onClick={() => setAlert(null)} autoFocus={alert.destructive || alert.hold}>{L("Cancelar", "Cancel")}</button>}
                </div>
              </div>
            ) : sheet}
          </div>
        )}
        {askDiscard && sheet && (
          <div className="scrim" style={{ zIndex: 60 }}>
            <div className="alert frosted" role="alertdialog" aria-modal="true" aria-labelledby="discard-title" aria-describedby="discard-text">
              <Mark />
              <h2 id="discard-title">{L("Descartar alterações?", "Discard changes?")}</h2>
              <p id="discard-text">{L("O que escreveu nesta janela não fica guardado.", "What you typed in this window won’t be kept.")}</p>
              <div className="btns">
                <button className="btn wide danger" onClick={() => setSheet(null)}>{L("Descartar", "Discard")}</button>
                <button className="btn wide" autoFocus onClick={() => setAskDiscard(false)}>{L("Continuar a editar", "Keep Editing")}</button>
              </div>
            </div>
          </div>
        )}
      </div>
      {ctxMenu && (
        <div className="ctxmenu frosted" role="menu" style={{ left: Math.min(ctxMenu.x, window.innerWidth - 240), top: Math.min(ctxMenu.y, window.innerHeight - 40 - ctxMenu.items.length * 26) }}>
          {ctxMenu.items.map((it, i) => it === "-" ? <div key={i} className="msep" role="separator" /> : (
            <button key={i} className={`mi ${it.danger ? "danger" : ""}`} role="menuitem" disabled={it.disabled} onClick={() => { setCtxMenu(null); it.fn(); }}><span className="ico">{it.icon}</span>{it.label}</button>
          ))}
        </div>
      )}
      {find && (
        <div className="find-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget) closeFind(); }}>
          <div className="find frosted" role="search">
            <label className="find-field"><Search /><input id="global-search" autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder={L("Procurar utentes e sessões", "Search patients and sessions")} aria-label={L("Procurar", "Search")} aria-controls="search-results" autoComplete="off"
              onKeyDown={(e) => { if (e.key === "Enter") document.querySelector<HTMLButtonElement>("#search-results .mi")?.click(); if (e.key === "ArrowDown") { e.preventDefault(); document.querySelector<HTMLButtonElement>("#search-results .mi")?.focus(); } }} /></label>
            {q.trim() && <SearchResults q={q.trim()} close={closeFind} app={ctx} />}
          </div>
        </div>
      )}
      <div className={`toast frosted ${toastMsg ? "on" : ""}`} role="status" aria-live="polite">
        {toastMsg && <><span>{toastMsg.text}</span>{toastMsg.undo && <button className="btn sm" onClick={undo}>{L("Desfazer", "Undo")}</button>}</>}
      </div>
    </Ctx.Provider>
  );
}

/** Find (⌘F): patients from the server and today's sessions. Lives outside App so typing doesn't remount it,
 *  which made the 200 ms wait useless and searched on every keystroke (audit phase C). */
function SearchResults({ q, close, app }: { q: string; close: () => void; app: AppCtx }) {
  const { L } = app;
  const [dq, setDq] = useState(q);
  useEffect(() => { const t = setTimeout(() => setDq(q), 200); return () => clearTimeout(t); }, [q]);
  const day0 = useMemo(() => { const d = new Date(); d.setHours(0, 0, 0, 0); return d; }, []);
  const { data: found } = useApi<PatientRow[]>(`/patients?q=${encodeURIComponent(dq)}&limit=5`);
  const { data: today } = useApi<Appointment[]>(`/appointments?${range(day0, addDays(day0, 1))}`);
  const { data: ths } = useApi<Therapist[]>("/therapists");
  const n = q.toLowerCase();
  const pts = dq === q ? (found ?? []).slice(0, 5) : [];
  const ses = (today ?? []).filter((x) => x.kind !== "block" && x.status !== "cancelled" && ((x.patient_name ?? x.title ?? "").toLowerCase().includes(n) || hm(x.starts_at).startsWith(n))).slice(0, 3);
  const waiting = !found || dq !== q;
  return (
    <div id="search-results" className="results frosted" role="listbox">
      {!waiting && !pts.length && !ses.length && <div className="none">{L(`Nada encontrado para “${q}”.`, `No results for “${q}”.`)}</div>}
      {waiting && !ses.length && <div className="none">{L("A procurar…", "Searching…")}</div>}
      {pts.length > 0 && <div className="rh">{L("Utentes", "Patients")}</div>}
      {pts.map((p) => (
        <button key={p.id} className="mi" role="option" onClick={() => { close(); app.openPatient(p.id); }}>
          <span className="avatar" style={{ width: 22, height: 22, fontSize: 9 }}>{p.name.split(" ").map((w) => w[0]).join("").slice(0, 2)}</span>
          {p.name}<span className="sub2">n.º {p.number}{p.phone ? ` · ${p.phone}` : ""}</span>
        </button>
      ))}
      {ses.length > 0 && <div className="rh">{L("Sessões de hoje", "Today’s sessions")}</div>}
      {ses.map((x) => (
        <button key={x.id} className="mi" role="option" onClick={() => { close(); openAppointment(app, x, ths); }}>
          <span className="num" style={{ width: 40 }}>{hm(x.starts_at)}</span>{x.patient_name ?? x.title}<span className="sub2">{x.therapist_name}</span>
        </button>
      ))}
    </div>
  );
}
