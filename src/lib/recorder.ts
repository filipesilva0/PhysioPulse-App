// Session recording (PhysioPulse plan, phase 2). One recording at a time for the whole app, so it keeps going
// while the person moves between pages. The microphone is read as 16 kHz mono 16-bit PCM — what the
// speech-to-text program wants — and sent to the clinic server in pieces of 30 seconds while recording, so a
// crash or a lost connection loses at most the piece in memory. Nothing is written to this computer's disk.
//
// The audio never goes anywhere but the clinic server (through `api()`, so the token stays in Rust).
import { api, isErr } from "./api";

export const RATE = 16_000;
/** Seconds per piece sent to the server. */
const PIECE = 30;

export type RecState = "idle" | "starting" | "recording" | "paused" | "finishing" | "error";
export interface RecSnapshot {
  state: RecState;
  id: string | null;
  patientId: string | null;
  patientName: string;
  /** Seconds captured so far (pauses not counted). */
  seconds: number;
  /** Seconds already on the server. */
  sent: number;
  /** 0–1, for the level meter. */
  level: number;
  /** A piece couldn't be sent yet (it's kept and retried). */
  offline: boolean;
  error?: string;
}

const idle: RecSnapshot = { state: "idle", id: null, patientId: null, patientName: "", seconds: 0, sent: 0, level: 0, offline: false };
let snap: RecSnapshot = idle;
const subs = new Set<() => void>();
const set = (p: Partial<RecSnapshot>) => { snap = { ...snap, ...p }; subs.forEach((f) => f()); };
export const getRec = () => snap;
export const subscribeRec = (f: () => void) => { subs.add(f); return () => { subs.delete(f); }; };
export const isRecording = () => snap.state === "recording" || snap.state === "paused" || snap.state === "starting" || snap.state === "finishing";

let ctx: AudioContext | null = null;
let stream: MediaStream | null = null;
let node: AudioWorkletNode | ScriptProcessorNode | null = null;
let src: MediaStreamAudioSourceNode | null = null;
/** Samples not yet cut into a piece. */
let pending: Int16Array[] = [];
let pendingLen = 0;
/** Pieces waiting to be sent, in order. */
const queue: { n: number; data: Int16Array }[] = [];
let nextPiece = 0;
let sending = false;
let captured = 0;

const WORKLET = `class Tap extends AudioWorkletProcessor {
  process(inputs) { const c = inputs[0] && inputs[0][0]; if (c) this.port.postMessage(c.slice(0)); return true; }
}
registerProcessor("physiopulse-tap", Tap);`;

/** Float samples at the context's rate → 16 kHz 16-bit (the context is asked for 16 kHz; this covers one that isn't). */
function toPcm(f: Float32Array, rate: number): Int16Array {
  const ratio = rate / RATE;
  const n = Math.floor(f.length / ratio);
  const out = new Int16Array(n);
  for (let i = 0; i < n; i++) {
    const s = Math.max(-1, Math.min(1, f[Math.floor(i * ratio)]));
    out[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
  }
  return out;
}

function onSamples(f: Float32Array) {
  if (snap.state !== "recording" || !ctx) return;
  let sum = 0;
  for (let i = 0; i < f.length; i++) sum += f[i] * f[i];
  const rms = Math.sqrt(sum / Math.max(1, f.length));
  const pcm = toPcm(f, ctx.sampleRate);
  pending.push(pcm); pendingLen += pcm.length; captured += pcm.length;
  if (pendingLen >= PIECE * RATE) cut();
  // Updating the screen ~10 times a second is plenty.
  const secs = Math.floor(captured / RATE);
  const level = Math.min(1, rms * 4);
  if (secs !== snap.seconds || Math.abs(level - snap.level) > 0.05) set({ seconds: secs, level });
}

function cut() {
  if (!pendingLen) return;
  const all = new Int16Array(pendingLen);
  let o = 0;
  for (const p of pending) { all.set(p, o); o += p.length; }
  pending = []; pendingLen = 0;
  queue.push({ n: nextPiece++, data: all });
  void pump();
}

function b64(a: Int16Array): string {
  const bytes = new Uint8Array(a.buffer, a.byteOffset, a.byteLength);
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Sends the queued pieces in order. A failure keeps the piece and tries again (the server may be away a moment). */
async function pump() {
  if (sending || !snap.id) return;
  sending = true;
  try {
    let delay = 2000;
    while (queue.length && snap.id) {
      const p = queue[0];
      try {
        const r = await api<{ chunks: number; seconds: number }>("PUT", `/recordings/${snap.id}/chunks/${p.n}`, { data: b64(p.data) }, { quiet: true });
        queue.shift(); delay = 2000;
        set({ sent: r.seconds, offline: false });
      } catch (e) {
        // The server is missing an earlier piece: nothing to do but report it.
        if (isErr(e) && (e.status === 403 || e.status === 404 || (e.status === 409 && e.code !== "chunk_order"))) { fail(e.message); return; }
        if (isErr(e) && e.code === "chunk_order") { fail(e.message); return; }
        set({ offline: true });
        await wait(delay); delay = Math.min(delay * 2, 30_000);
      }
    }
  } finally { sending = false; }
}

function release() {
  try { node?.disconnect(); src?.disconnect(); } catch { /* already gone */ }
  stream?.getTracks().forEach((t) => t.stop());
  void ctx?.close().catch(() => undefined);
  ctx = null; stream = null; node = null; src = null;
}

function fail(message: string) {
  release();
  set({ state: "error", error: message, level: 0 });
}

/** Asks for the microphone, starts the recording on the server and starts capturing. Throws a readable message. */
export async function startRecording(o: { patientId: string; patientName: string; template: string; blocks: unknown; appointmentId: string | null; lang: "pt" | "en"; L: (pt: string, en: string) => string }) {
  if (isRecording()) throw new Error(o.L("Já está a gravar uma sessão.", "A session is already being recorded."));
  set({ ...idle, state: "starting", patientId: o.patientId, patientName: o.patientName });
  pending = []; pendingLen = 0; queue.length = 0; nextPiece = 0; captured = 0;
  try {
    stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true } });
  } catch {
    set(idle);
    throw new Error(o.L("Não foi possível usar o microfone. Verifique se está ligado e se o PhysioPulse tem autorização para o usar (nas definições de privacidade do computador).",
      "Can’t use the microphone. Check it’s connected and that PhysioPulse is allowed to use it (in the computer’s privacy settings)."));
  }
  try {
    const r = await api<{ id: string }>("POST", `/patients/${o.patientId}/recordings`, { template: o.template, blocks: o.blocks, appointment_id: o.appointmentId, lang: o.lang });
    set({ id: r.id });
    const AC: typeof AudioContext = window.AudioContext ?? (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    try { ctx = new AC({ sampleRate: RATE }); } catch { ctx = new AC(); }
    src = ctx.createMediaStreamSource(stream);
    // The worklet: the app's own file (desktop app, strict CSP), else a blob (one-file browser preview), else the
    // older ScriptProcessor.
    let worklet = false;
    if (ctx.audioWorklet) {
      try { await ctx.audioWorklet.addModule("/rec-worklet.js"); worklet = true; }
      catch {
        try {
          const url = URL.createObjectURL(new Blob([WORKLET], { type: "application/javascript" }));
          await ctx.audioWorklet.addModule(url); URL.revokeObjectURL(url); worklet = true;
        } catch { /* fall back below */ }
      }
    }
    if (worklet) {
      const w = new AudioWorkletNode(ctx, "physiopulse-tap");
      w.port.onmessage = (e) => onSamples(e.data as Float32Array);
      node = w;
    } else {
      const sp = ctx.createScriptProcessor(4096, 1, 1);
      sp.onaudioprocess = (e) => onSamples(e.inputBuffer.getChannelData(0));
      node = sp;
    }
    src.connect(node);
    // A processor must reach the output to run; it outputs silence.
    const mute = ctx.createGain(); mute.gain.value = 0; node.connect(mute); mute.connect(ctx.destination);
    if (ctx.state === "suspended") await ctx.resume();
    set({ state: "recording" });
  } catch (e) {
    const id = snap.id;
    release(); set(idle);
    if (id) api("DELETE", `/recordings/${id}`, undefined, { quiet: true }).catch(() => undefined);
    throw e instanceof Error ? e : new Error(isErr(e) ? e.message : String(e));
  }
}

export function pauseRecording() {
  if (snap.state !== "recording") return;
  void ctx?.suspend(); set({ state: "paused", level: 0 });
}
export function resumeRecording() {
  if (snap.state !== "paused") return;
  void ctx?.resume(); set({ state: "recording" });
}

/** Stops the microphone, sends what's left, and queues the recording for the AI PC. Returns the seconds recorded. */
export async function stopRecording(): Promise<{ seconds: number; patientId: string | null }> {
  if (snap.state !== "recording" && snap.state !== "paused") throw new Error("not recording");
  set({ state: "finishing", level: 0 });
  release();
  cut();
  // Wait for every piece to reach the server (it keeps trying for up to 5 minutes).
  const until = Date.now() + 5 * 60_000;
  // (`now()`: the state changes while we wait, from other calls.)
  const now = (): RecState => snap.state;
  while ((queue.length || sending) && now() === "finishing" && Date.now() < until) { void pump(); await wait(300); }
  const id = snap.id, patientId = snap.patientId;
  if (now() !== "finishing") throw new Error(snap.error ?? "");
  if (queue.length) { set({ state: "error", error: undefined }); throw new Error("offline"); }
  try {
    const r = await api<{ seconds: number }>("POST", `/recordings/${id}/finish`);
    set(idle);
    return { seconds: r.seconds, patientId };
  } catch (e) {
    set(idle);
    throw new Error(isErr(e) ? e.message : String(e));
  }
}

/** Stops and throws everything away (on the server too). */
export async function discardRecording() {
  const id = snap.id;
  release(); queue.length = 0; pending = []; pendingLen = 0;
  set(idle);
  if (id) await api("DELETE", `/recordings/${id}`).catch(() => undefined);
}

/** After an error: forget the local state (what reached the server stays there, listed in the patient's notes). */
export function clearRecording() { release(); queue.length = 0; set(idle); }
