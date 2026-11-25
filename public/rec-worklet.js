// Session recording (src/lib/recorder.ts): hands each block of microphone samples to the page. Served from the
// app itself, so the content security policy needn't allow blob: scripts.
class Tap extends AudioWorkletProcessor {
  process(inputs) { const c = inputs[0] && inputs[0][0]; if (c) this.port.postMessage(c.slice(0)); return true; }
}
registerProcessor("physiopulse-tap", Tap);
