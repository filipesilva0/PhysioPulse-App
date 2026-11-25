// On-device AI (desktop app): chat and dictation that run on this computer, through the app's own C++ module
// (src-tauri/native, src-tauri/src/local_ai.rs). Nothing is sent to the server or anywhere else; nothing is saved.
import { isTauri } from "./platform";
import type { Lang } from "./ctx";

export interface LocalStatus {
  /** The C++ module is in this build of the app. */
  built: boolean;
  models_dir: string;
  /** The chat model file found (null = not downloaded yet). */
  chat_model: string | null;
  chat_loaded: boolean;
  /** The speech-to-text model file found (null = not downloaded yet). */
  stt_model: string | null;
}
export interface LocalReply { text: string; tokens: number; model: string; seconds: number; stopped: boolean }
export interface LocalMsg { role: "system" | "user" | "assistant"; content: string }

async function core() { return import("@tauri-apps/api/core"); }

// Browser preview (no Rust): a stand-in so the UI can be clicked through. Nothing runs a model.
const PREVIEW: LocalStatus = { built: true, models_dir: "~/Library/Application Support/app.physiopulse/models",
  chat_model: "gemma-4-E4B-it-Q4_K_M.gguf", chat_loaded: false, stt_model: "ggml-large-v3.bin" };
let previewStop = false;

export async function localStatus(): Promise<LocalStatus | null> {
  if (!isTauri) return PREVIEW;
  try { return await (await core()).invoke<LocalStatus>("local_ai_status"); } catch { return null; }
}

/** Answers on this computer; `onPiece` gets the answer as it's written. Rejects with {code, message}. */
export async function localChat(messages: LocalMsg[], onPiece: (s: string) => void): Promise<LocalReply> {
  if (!isTauri) {
    const pt = /portugu/i.test(messages[0]?.content ?? "");
    const text = pt ? "(Pré-visualização) Aqui responderia o Gemma 4, a correr neste computador. A resposta aparece à medida que é escrita."
      : "(Preview) Gemma 4 would answer here, running on this computer. The answer appears as it is written.";
    previewStop = false;
    let out = "";
    for (const w of text.split(/(?<= )/)) { if (previewStop) break; await new Promise((r) => setTimeout(r, 60)); out += w; onPiece(w); }
    return { text: out, tokens: out.split(" ").length, model: "preview", seconds: 1, stopped: previewStop };
  }
  const { invoke, Channel } = await core();
  const ch = new Channel<string>();
  ch.onmessage = onPiece;
  return invoke<LocalReply>("local_chat", { messages, onPiece: ch });
}

export async function localChatStop() { if (isTauri) await (await core()).invoke("local_chat_stop"); else previewStop = true; }
export async function openModelsFolder() { if (isTauri) await (await core()).invoke("local_ai_open_folder"); }

// ───────── dictation: speech to text on this computer (whisper.cpp, Whisper large-v3) ─────────
// The microphone is read as 16 kHz mono float samples in memory, then given to the C++ module. Nothing is saved,
// nothing is sent anywhere.

const RATE = 16_000;
let mic: { ctx: AudioContext; stream: MediaStream; node: ScriptProcessorNode; chunks: Float32Array[] } | null = null;

export const dictating = () => mic !== null;

/** Starts listening. Throws if the microphone can't be used. */
export async function startDictation(): Promise<void> {
  if (mic) return;
  const stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true } });
  const AC: typeof AudioContext = window.AudioContext ?? (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
  let ctx: AudioContext;
  try { ctx = new AC({ sampleRate: RATE }); } catch { ctx = new AC(); }
  const src = ctx.createMediaStreamSource(stream);
  const node = ctx.createScriptProcessor(4096, 1, 1);
  const chunks: Float32Array[] = [];
  node.onaudioprocess = (e) => { chunks.push(new Float32Array(e.inputBuffer.getChannelData(0))); };
  src.connect(node);
  const mute = ctx.createGain(); mute.gain.value = 0; node.connect(mute); mute.connect(ctx.destination);
  if (ctx.state === "suspended") await ctx.resume();
  mic = { ctx, stream, node, chunks };
}

/** Stops listening and throws the audio away. */
export function cancelDictation() {
  if (!mic) return;
  mic.node.disconnect(); mic.stream.getTracks().forEach((t) => t.stop()); void mic.ctx.close();
  mic = null;
}

/** Stops listening and transcribes what was said. `onProgress` gets 0–100. Rejects with {code, message}. */
export async function finishDictation(lang: Lang, onProgress: (pct: number) => void): Promise<string> {
  if (!mic) return "";
  const { chunks, ctx } = mic;
  const rate = ctx.sampleRate;
  cancelDictation();
  const n = chunks.reduce((a, c) => a + c.length, 0);
  let pcm = new Float32Array(n);
  let at = 0;
  for (const c of chunks) { pcm.set(c, at); at += c.length; }
  if (rate !== RATE) pcm = resample(pcm, rate, RATE);
  if (!isTauri) { // browser preview: a stand-in
    for (const p of [20, 60, 100]) { await new Promise((r) => setTimeout(r, 250)); onProgress(p); }
    return lang === "pt" ? "(Pré-visualização) O texto ditado aparece aqui." : "(Preview) The dictated text appears here.";
  }
  const { invoke } = await core();
  const { listen } = await import("@tauri-apps/api/event");
  const off = await listen<number>("local-stt-progress", (e) => onProgress(e.payload));
  try {
    return await invoke<string>("local_transcribe", new Uint8Array(pcm.buffer), { headers: { "x-lang": lang } });
  } finally { off(); }
}

export async function stopTranscribing() { if (isTauri) await (await core()).invoke("local_transcribe_stop"); }

/** Linear resampling (the microphone may refuse 16 kHz). Good enough for speech. */
function resample(x: Float32Array, from: number, to: number): Float32Array {
  const out = new Float32Array(Math.floor((x.length * to) / from));
  const step = from / to;
  for (let i = 0; i < out.length; i++) {
    const p = i * step, j = Math.floor(p), f = p - j;
    out[i] = (x[j] ?? 0) * (1 - f) + (x[j + 1] ?? x[j] ?? 0) * f;
  }
  return out;
}
