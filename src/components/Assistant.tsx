import { Fragment, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { AlertTriangle, RotateCcw, Search, Send, Sparkles, X } from "lucide-react";
import { useApp } from "../lib/ctx";
import { api, isErr } from "../lib/api";
import { aiOffText, useAiStatus } from "./Ai";
import { DictateButton, LocalChat } from "./LocalChat";
import { localStatus } from "../lib/localAi";
import type { LocalStatus } from "../lib/localAi";
import { isMac } from "../lib/platform";

type Row = [string, string, string];
/** `undoable: false`: a change that stays (voiding a payment — fiscal records; a consent withdrawal — clinical record), shown without Undo. */
interface Change { id: string; skill: string; rows: Row[]; warnings: string[]; status: "pending" | "applied" | "cancelled" | "undone"; undoable?: boolean }
/** An appointment or a free time the server checked: shown as a list under the answer. */
interface Item { id?: string; when: string; title: string; sub: string; status?: string; patient_id?: string | null }
/** Something to tap instead of typing ("Which Maria?"): `send` is what gets sent. */
interface Choice { label: string; send: string }
/** `steps`: what the assistant looked up, in words ("Abriu a ficha de …"); `memo`: the same for the model, sent back
 *  with the next message so a follow-up ("e as notas dela?") still knows who and what (13 Nov 2026). */
interface Reply { reply: string; change: Change | null; rows: Row[] | null; list?: Item[] | null; choices?: Choice[] | null; model: string; steps?: string[]; memo?: string }
interface Msg { id: number; who: "me" | "bot"; text: string; change?: Change; rows?: Row[]; list?: Item[]; choices?: Choice[]; steps?: string[]; memo?: string; error?: boolean; retry?: string }

let seq = 1;

/** One conversation for the whole app, so it stays the same on every page. In memory only (never saved on the
 *  computer or the server): it starts empty each new day, when someone else signs in, and when the app closes. */
type Chat = { key: string; msgs: Msg[]; busy: boolean };
let chat: Chat = { key: "", msgs: [], busy: false };
const subs = new Set<() => void>();
const today = () => new Date().toLocaleDateString("sv-SE");
const setChat = (c: Partial<Chat>) => { chat = { ...chat, ...c }; subs.forEach((f) => f()); };
const subscribe = (f: () => void) => { subs.add(f); return () => { subs.delete(f); }; };
/** Starts a fresh chat when the day or the person changed. Returns the key the chat now belongs to. */
const ensure = (who: string, quiet = false) => {
  const k = `${who}|${today()}`;
  if (chat.key !== k) { if (quiet) chat = { key: k, msgs: [], busy: false }; else setChat({ key: k, msgs: [], busy: false }); }
  return k;
};

/** The assistant: the owner, and anyone whose AI switch is on. It sees and does what the person can in the app (the
 *  server enforces the role): it looks things up with tools (patients, files and notes, the agenda, requests, money,
 *  documents and letters, complaints, settings), and
 *  every change is shown before/after and waits for Apply; every change is logged and can be undone. */
export function Assistant({ onClose }: { onClose: () => void }) {
  const { L, lang, toast, me, role, openPatient } = useApp();
  const owner = role === "owner";
  const ai = useAiStatus();
  ensure(me.id, true); // while rendering: no notifications
  const state = useSyncExternalStore(subscribe, () => chat);
  // Past midnight with the panel open: start the new day's chat.
  useEffect(() => { const t = setInterval(() => ensure(me.id), 60_000); return () => clearInterval(t); }, [me.id]);
  // One line, then the suggestions (23 Nov 2026 overhaul: the paragraph listing every skill went unread).
  const intro: Msg = { id: 0, who: "bot", text: L("Peça o que precisar, por escrito. Mostro sempre a alteração antes de a aplicar.", "Ask for what you need in your own words. I always show the change before applying it.") };
  const msgs = [intro, ...state.msgs];
  const busy = state.busy;
  /** Changes the shared chat, but only if it still belongs to `key` (a reply that arrives after midnight is dropped). */
  const setMsgs = (fn: (m: Msg[]) => Msg[], key = chat.key) => { if (chat.key === key) setChat({ msgs: fn([intro, ...chat.msgs]).slice(1) }); };
  const setBusy = (b: boolean, key = chat.key) => { if (chat.key === key) setChat({ busy: b }); };
  const [input, setInput] = useState("");
  const [acting, setActing] = useState<string | null>(null);
  // On-device AI (the app's own C++ module): a second mode, and dictation (speech to text on this computer) in both.
  const [local, setLocal] = useState<LocalStatus | null>(null);
  const [mode, setMode] = useState<"clinic" | "device">("clinic");
  useEffect(() => { void localStatus().then(setLocal); }, [mode]);
  const log = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => { log.current?.scrollTo({ top: 1e6, behavior: "smooth" }); }, [msgs, busy]);
  useEffect(() => { field.current?.focus(); }, []);

  const suggestions = [
    L("O que temos amanhã?", "What’s on tomorrow?"),
    L("Marca a Maria Silva quinta às 10h", "Book Maria Silva on Thursday at 10:00"),
    owner ? L("Quem me deve dinheiro?", "Who owes me money?") : L("A Maria Silva pagou em MB WAY", "Maria Silva paid by MB WAY"),
    owner ? L("Há reclamações por responder?", "Any complaints waiting for an answer?")
      : role === "therapist" ? L("Prepara a carta de alta da Maria Silva", "Draft Maria Silva’s discharge letter")
      : L("Quem tenho de lembrar amanhã?", "Who do I need to remind tomorrow?"),
  ];
  const off = !!ai.data && !ai.data.available;

  const send = async (q: string) => {
    const text = q.trim();
    const key = ensure(me.id);
    if (!text || chat.busy) return;
    const history = chat.msgs.filter((m) => !m.error).slice(-8).map((m) => ({ who: m.who, text: m.text, memo: m.memo ?? "" }));
    setMsgs((m) => [...m, { id: seq++, who: "me", text }], key);
    setInput("");
    setBusy(true, key);
    try {
      const r = await api<Reply>("POST", "/ai/assistant", { message: text, history, lang });
      setMsgs((m) => [...m, { id: seq++, who: "bot", text: r.reply, change: r.change ?? undefined, rows: r.rows ?? undefined, list: r.list ?? undefined, choices: r.choices ?? undefined,
        steps: r.steps?.length ? r.steps : undefined, memo: r.memo || undefined }], key);
    } catch (e) {
      setMsgs((m) => [...m, { id: seq++, who: "bot", text: isErr(e) ? e.message : String(e), error: true, retry: text }], key);
    } finally { setBusy(false, key); field.current?.focus(); }
  };

  /** Ask again: drop the failed answer and the question before it, then send the question again. */
  const retry = (m: Msg) => {
    setMsgs((x) => { const i = x.findIndex((y) => y.id === m.id); return [...x.slice(0, Math.max(1, i - 1)), ...x.slice(i + 1)]; });
    void send(m.retry!);
  };

  const setStatus = (id: string, status: Change["status"]) =>
    setMsgs((m) => m.map((x) => (x.change?.id === id ? { ...x, change: { ...x.change, status } } : x)));

  const undo = async (c: Change) => {
    try { await api("POST", `/assistant/changes/${c.id}/undo`); setStatus(c.id, "undone"); toast(L("Alteração anulada", "Change undone")); }
    catch (e) { toast(isErr(e) ? e.message : String(e)); }
  };
  const decide = async (c: Change, apply: boolean) => {
    setActing(c.id);
    try {
      await api("POST", `/assistant/changes/${c.id}/${apply ? "apply" : "cancel"}`);
      setStatus(c.id, apply ? "applied" : "cancelled");
      if (apply && c.undoable === false) toast(L("Alteração aplicada e registada", "Change applied and logged"));
      else if (apply) toast(L("Alteração aplicada e registada", "Change applied and logged"), () => { void undo({ ...c, status: "applied" }); }, L("alteração do assistente", "Assistant Change"));
    } catch (e) {
      setMsgs((m) => [...m, { id: seq++, who: "bot", text: isErr(e) ? e.message : String(e), error: true }]);
    } finally { setActing(null); }
  };

  const statusLine = (c: Change) => c.status === "applied" ? L("Aplicada e registada.", "Applied and logged.")
    : c.status === "undone" ? L("Anulada: ficou como estava.", "Undone: back as it was.") : L("Cancelada. Nada mudou.", "Cancelled. Nothing changed.");

  return (
    <div className="chat">
      <div className="chat-head">
        <Sparkles size={16} style={{ color: "var(--ai)" }} aria-hidden />
        <h2>{L("Assistente", "Assistant")}</h2>
        {local?.built && (
          <div className="seg" role="group" aria-label={L("Onde corre a IA", "Where the AI runs")}>
            <button aria-pressed={mode === "clinic"} onClick={() => setMode("clinic")} title={L("Consulta e altera dados da clínica, no computador da IA", "Looks up and changes clinic data, on the AI computer")}>{L("Clínica", "Clinic")}</button>
            <button aria-pressed={mode === "device"} onClick={() => setMode("device")} title={L("Só o que escrever, sem sair deste computador", "Only what you type, without leaving this computer")}>{isMac ? L("Neste Mac", "On This Mac") : L("Neste PC", "On This PC")}</button>
          </div>
        )}

        <button className="iconbtn" onClick={onClose} aria-label={L("Fechar assistente", "Close assistant")}><X /></button>
      </div>
      {mode === "device" && local ? <LocalChat status={local} /> : <>
      <div className="chat-log" ref={log} aria-live="polite">
        {off && <div className="msg bot err-msg" role="status"><AlertTriangle size={13} style={{ verticalAlign: -2 }} /> {aiOffText(ai.data!, L)}</div>}
        {msgs.map((m) => (
          <div key={m.id} className={`msg ${m.who}${m.error ? " err-msg" : ""}`}>
            {m.steps && (
              <ul className="chat-steps" aria-label={L("O que o assistente consultou", "What the assistant looked up")}>
                {m.steps.map((x, i) => <li key={i}><Search size={11} aria-hidden />{x}</li>)}
              </ul>
            )}
            {m.text}
            {m.retry && <div style={{ marginTop: 6 }}><button className="btn sm" disabled={busy || off} onClick={() => retry(m)}>{L("Tentar de novo", "Retry")}</button></div>}
            {m.rows && (
              <div className="ai-card" style={{ marginTop: 8, background: "var(--raise)" }}>
                <div className="diff">{m.rows.map(([k, a, b]) => <Fragment key={k}><span>{k}</span><span className="num"><b>{a}</b> <span className="muted">({b})</span></span></Fragment>)}</div>
                <p className="small muted" style={{ marginTop: 6 }}>{L("Mês anterior entre parênteses. Os números vêm do servidor.", "Previous month in brackets. Figures come from the server.")}</p>
              </div>
            )}
            {m.list && m.list.length > 0 && (
              <div className="chat-list" role="list">
                {m.list.map((it, i) => {
                  const body = <><span className="t">{it.title}</span><span className="s"><span className="num">{it.when}</span> · {it.sub}</span></>;
                  return it.patient_id
                    ? <button key={it.id ?? i} role="listitem" className="chat-item" onClick={() => openPatient(it.patient_id!)} title={L("Abrir ficha", "Open patient file")}>{body}</button>
                    : <div key={it.id ?? i} role="listitem" className="chat-item">{body}</div>;
                })}
              </div>
            )}
            {m.choices && m.choices.length > 0 && (
              <div className="chips" style={{ marginTop: 8 }}>
                {m.choices.map((c) => <button key={c.label} className="chip" disabled={busy || off || m.id !== msgs[msgs.length - 1].id} onClick={() => send(c.send)}>{c.label}</button>)}
              </div>
            )}
            {m.change && (
              <div className="ai-card" style={{ marginTop: 8, background: "var(--raise)" }}>
                <div className="diff">{m.change.rows.map(([k, a, b]) => <Fragment key={k}><span>{k}</span><span>{a === "—" ? <b>{b}</b> : <><span className="old">{a}</span> → <b>{b}</b></>}</span></Fragment>)}</div>
                {m.change.warnings.length > 0 && <ul className="small muted" style={{ margin: "8px 0 0", paddingLeft: 16 }}>{m.change.warnings.map((w) => <li key={w}>{w}</li>)}</ul>}
                {m.change.status === "pending" ? (
                  <div className="ai-actions">
                    <button className="btn pri sm" disabled={acting === m.change.id} aria-busy={acting === m.change.id} onClick={() => decide(m.change!, true)}>{L("Aplicar", "Apply")}</button>
                    <button className="btn sm" disabled={acting === m.change.id} onClick={() => decide(m.change!, false)}>{L("Cancelar", "Cancel")}</button>
                  </div>
                ) : (
                  <p className="small" style={{ marginTop: 8, display: "flex", alignItems: "center", gap: 8, color: m.change.status === "applied" ? "var(--ok)" : "var(--text3)" }}>
                    {statusLine(m.change)}
                    {m.change.status === "applied" && m.change.undoable !== false && <button className="linkbtn" onClick={() => undo(m.change!)}><RotateCcw size={11} style={{ verticalAlign: -1 }} /> {L("Anular", "Undo")}</button>}
                  </p>
                )}
              </div>
            )}
          </div>
        ))}
        {busy && <div className="msg bot small" style={{ color: "var(--text2)" }} role="status"><span className="skel" style={{ display: "inline-block", width: 10, height: 10, borderRadius: 5, marginRight: 6, verticalAlign: -1 }} />{L("A consultar a app e a preparar a resposta… pode demorar um minuto.", "Looking things up in the app and preparing an answer… this can take a minute.")}</div>}
      </div>
      <div className="chat-foot">
        {msgs.length === 1 && <div className="chips">{suggestions.map((s) => <button key={s} className="chip" disabled={busy || off} onClick={() => send(s)}>{s}</button>)}</div>}
        <form onSubmit={(e) => { e.preventDefault(); void send(input); }}>
          <DictateButton status={local} disabled={busy || off} onText={(t) => { setInput((v) => (v.trim() ? `${v.trim()} ${t}` : t).slice(0, 500)); field.current?.focus(); }} />
          <input ref={field} className="input" value={input} onChange={(e) => setInput(e.target.value)} maxLength={500} placeholder={L("Peça ou pergunte o que quiser", "Ask or tell me what to do")} aria-label={L("Mensagem para o assistente", "Message to the assistant")} />
          <button className="btn pri" type="submit" disabled={busy || off || !input.trim()} aria-label={L("Enviar", "Send")}><Send /></button>
        </form>
        <p className="small muted">{L("A IA local pode errar. Veja a alteração antes de aplicar.", "The local AI can make mistakes. Check the change before applying it.")}</p>
      </div>
      </>}
    </div>
  );
}
