// The assistant's "On This Mac" mode: a conversation with the model that runs on this computer (the app's own C++
// module). It has no tools and sees nothing from the clinic's server: only what the person types. In memory only,
// like the clinic assistant's chat (gone when the app closes, the day changes or someone else signs in).
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { FolderOpen, Mic, Send, Square } from "lucide-react";
import { useApp } from "../lib/ctx";
import { isErr } from "../lib/api";
import { isMac } from "../lib/platform";
import { cancelDictation, finishDictation, localChat, localChatStop, openModelsFolder, startDictation, stopTranscribing } from "../lib/localAi";
import type { LocalMsg, LocalStatus } from "../lib/localAi";

interface Msg { id: number; who: "me" | "bot"; text: string; error?: boolean }
let seq = 1;
type Chat = { key: string; msgs: Msg[]; busy: boolean };
let chat: Chat = { key: "", msgs: [], busy: false };
const subs = new Set<() => void>();
const setChat = (c: Partial<Chat>) => { chat = { ...chat, ...c }; subs.forEach((f) => f()); };
const subscribe = (f: () => void) => { subs.add(f); return () => { subs.delete(f); }; };
const ensure = (who: string) => { const k = `${who}|${new Date().toLocaleDateString("sv-SE")}`; if (chat.key !== k) chat = { key: k, msgs: [], busy: false }; return k; };

/** Dictation into a text field, on this computer (Whisper large-v3): press to talk, press again to stop; the text is
 *  added to the field. Hidden when the app has no speech model. */
export function DictateButton({ status, onText, disabled }: { status: LocalStatus | null; onText: (t: string) => void; disabled?: boolean }) {
  const { L, lang, toast } = useApp();
  const [state, setState] = useState<"idle" | "listening" | "working">("idle");
  const [pct, setPct] = useState(0);
  const [secs, setSecs] = useState(0);
  useEffect(() => {
    if (state !== "listening") return;
    const t0 = Date.now();
    const t = setInterval(() => setSecs(Math.round((Date.now() - t0) / 1000)), 500);
    return () => clearInterval(t);
  }, [state]);
  useEffect(() => () => cancelDictation(), []);
  if (!status?.built || !status.stt_model) return null;

  const click = async () => {
    if (state === "working") { void stopTranscribing(); return; }
    if (state === "idle") {
      try { await startDictation(); setSecs(0); setState("listening"); }
      catch { toast(L("Não foi possível usar o microfone. Verifique se o PhysioPulse tem autorização (definições de privacidade do computador).", "Can’t use the microphone. Check PhysioPulse is allowed to use it (the computer’s privacy settings).")); }
      return;
    }
    setState("working"); setPct(0);
    try {
      const text = await finishDictation(lang, setPct);
      if (text) onText(text); else toast(L("Não se ouviu nada.", "Nothing was heard."));
    } catch (e) {
      const code = isErr(e) ? e.code : undefined;
      if (code !== "stopped") toast(code === "too_short" ? L("Gravação demasiado curta.", "Recording too short.")
        : code === "load_failed" ? L("Não foi possível carregar o modelo de voz. Feche outras apps e tente de novo.", "Couldn’t load the speech model. Close other apps and try again.")
        : L("Não foi possível passar a voz a texto. Tente de novo.", "Couldn’t turn the speech into text. Try again."));
    } finally { setState("idle"); }
  };
  const label = state === "listening" ? L(`A ouvir… ${secs} s. Carregue para terminar`, `Listening… ${secs} s. Press to finish`)
    : state === "working" ? L(`A passar a texto… ${pct}%. Carregue para parar`, `Transcribing… ${pct}%. Press to stop`)
    : L("Ditar", "Dictate");
  return (
    <button type="button" className={`btn dictate${state !== "idle" ? ` ${state}` : ""}`} onClick={() => void click()} disabled={disabled && state === "idle"}
      aria-pressed={state === "listening"} aria-busy={state === "working"} aria-label={label} title={label}>
      {state === "working" ? <span className="num small">{pct}%</span> : state === "listening" ? <Square size={13} /> : <Mic size={15} />}
    </button>
  );
}

export function LocalChat({ status }: { status: LocalStatus }) {
  const { L, lang, me } = useApp();
  ensure(me.id);
  const state = useSyncExternalStore(subscribe, () => chat);
  const [input, setInput] = useState("");
  const log = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => { log.current?.scrollTo({ top: 1e6 }); }, [state.msgs]);
  useEffect(() => { field.current?.focus(); }, []);
  const here = isMac ? L("neste Mac", "on this Mac") : L("neste computador", "on this computer");

  const system = L(
    "És o assistente da clínica de fisioterapia PhysioPulse e corres apenas neste computador, sem internet. Não tens acesso à agenda, às fichas nem a outros dados da clínica: usa só o que a pessoa escreve. Responde em português de Portugal, de forma breve e clara. Se não souberes, diz que não sabes; não inventes dados clínicos.",
    "You are the assistant of the PhysioPulse physiotherapy clinic and you run only on this computer, offline. You can’t see the schedule, patient files or any other clinic data: use only what the person writes. Answer in British English, briefly and clearly. If you don’t know, say so; never make up clinical facts.");

  const send = async (q: string) => {
    const text = q.trim();
    const key = ensure(me.id);
    if (!text || chat.busy) return;
    const history: LocalMsg[] = chat.msgs.filter((m) => !m.error).slice(-12).map((m) => ({ role: m.who === "me" ? "user" : "assistant", content: m.text }));
    const mine: Msg = { id: seq++, who: "me", text };
    const bot: Msg = { id: seq++, who: "bot", text: "" };
    setChat({ msgs: [...chat.msgs, mine, bot], busy: true });
    setInput("");
    // The answer appears as it's written.
    const put = (fn: (m: Msg) => Msg) => { if (chat.key === key) setChat({ msgs: chat.msgs.map((m) => (m.id === bot.id ? fn(m) : m)) }); };
    try {
      const r = await localChat([{ role: "system", content: system }, ...history, { role: "user", content: text }], (p) => put((m) => ({ ...m, text: m.text + p })));
      put((m) => ({ ...m, text: r.text.trim() || m.text }));
    } catch (e) {
      const code = isErr(e) ? e.code : undefined;
      const msg = code === "no_model" ? L("Ainda não há modelo de conversa neste computador. Veja abaixo como o instalar.", "There’s no chat model on this computer yet. See below how to install it.")
        : code === "too_long" ? L("A conversa ficou longa demais. Comece uma nova.", "The conversation got too long. Start a new one.")
        : code === "load_failed" ? L("Não foi possível carregar o modelo. Feche outras apps e tente de novo.", "Couldn’t load the model. Close other apps and try again.")
        : L("A IA deste computador não conseguiu responder. Tente de novo.", "The on-device AI couldn’t answer. Try again.");
      put((m) => ({ ...m, text: msg, error: true }));
    } finally {
      if (chat.key === key) setChat({ busy: false });
      field.current?.focus();
    }
  };

  const missing = !status.chat_model;
  return (
    <>
      <div className="chat-log" ref={log} aria-live="polite">
        <div className="msg bot">{L(`Esta conversa corre ${here}: nada sai do computador e não vê dados da clínica.`, `This conversation runs ${here}: nothing leaves the computer and it can’t see clinic data.`)}</div>
        {missing && (
          <div className="msg bot err-msg" role="status">
            {L("Falta o modelo. Corra tools/local-ai/setup.sh uma vez (descarrega o Gemma 4).", "The model is missing. Run tools/local-ai/setup.sh once (it downloads Gemma 4).")}
            <div style={{ marginTop: 6 }}><button className="btn sm" onClick={() => void openModelsFolder()}><FolderOpen size={13} /> {L("Abrir pasta dos modelos", "Open Models Folder")}</button></div>
          </div>
        )}
        {state.msgs.map((m) => (
          <div key={m.id} className={`msg ${m.who}${m.error ? " err-msg" : ""}`}>
            {m.text || (state.busy && m.who === "bot" ? <span className="skel" style={{ display: "inline-block", width: 10, height: 10, borderRadius: 5 }} aria-label={L("A escrever…", "Writing…")} /> : null)}
          </div>
        ))}
      </div>
      <div className="chat-foot">
        <form onSubmit={(e) => { e.preventDefault(); void send(input); }}>
          <DictateButton status={status} disabled={missing || state.busy} onText={(t) => { setInput((v) => (v.trim() ? `${v.trim()} ${t}` : t)); field.current?.focus(); }} />
          <input ref={field} className="input" value={input} onChange={(e) => setInput(e.target.value)} maxLength={4000} disabled={missing}
            placeholder={L("Escreva uma pergunta", "Type a question")} aria-label={L("Mensagem para a IA deste computador", "Message to the on-device AI")} />
          {state.busy
            ? <button className="btn" type="button" onClick={() => void localChatStop()} aria-label={L("Parar", "Stop")}><Square size={14} /></button>
            : <button className="btn pri" type="submit" disabled={missing || !input.trim()} aria-label={L("Enviar", "Send")}><Send /></button>}
        </form>
        <p className="small muted">{L(`Corre ${here}. Pode errar: confirme antes de usar.`, `Runs ${here}. It can make mistakes: check before using.`)}</p>
      </div>
    </>
  );
}
