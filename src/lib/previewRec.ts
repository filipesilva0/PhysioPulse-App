// Browser preview: a stand-in for session recordings (server: routes/recordings.rs) so the screens can be tried.
// A finished recording goes queued → transcribing → drafting → ready over ~15 seconds. Invented content only.
import type { Role } from "./ctx";
type M = (pt: string, en: string) => string;
const fail = (status: number, message: string): never => { throw { status, message }; };

interface Rec { id: string; patient_id: string; template: string; lang: "pt" | "en"; seconds: number; finished: number | null; failed?: string; created_at: string; created_by: string }
const recs: Rec[] = [];
let n = 0;
const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();
// One ready draft and one waiting for the AI computer, on the first patient.
recs.push({ id: "rec-a", patient_id: "p1", template: "neck", lang: "pt", seconds: 2310, finished: Date.now() - 3600_000, created_at: ago(95), created_by: "fs" });
recs.push({ id: "rec-b", patient_id: "p1", template: "neck", lang: "pt", seconds: 1520, finished: null, failed: "waiting", created_at: ago(20), created_by: "fs" });

const status = (r: Rec) => {
  if (r.failed === "waiting") return "queued";
  if (r.failed) return "failed";
  if (r.finished == null) return "recording";
  const s = (Date.now() - r.finished) / 1000;
  return s < 4 ? "queued" : s < 9 ? "transcribing" : s < 15 ? "drafting" : "ready";
};
const DRAFT_PT = {
  report: "A utente referiu menos dor cervical ao fim do dia, agora 4/10, e já conduz sem dificuldade na rotação para a esquerda. Mantém algum desconforto ao trabalhar ao computador mais de duas horas seguidas.\n\nNa sessão fez-se mobilização articular cervical e torácica e reviu-se o exercício de flexores profundos, que executou corretamente após correção da posição do queixo. A rotação cervical melhorou para 70 graus à direita e 65 à esquerda.",
  plan: "Na próxima sessão progride-se a carga do exercício de flexores profundos e reavalia-se a dor ao fim do dia de trabalho.",
  values: { pain: 4, rotation: 70, techniques: ["Mobilização articular", "Exercício terapêutico"] },
  transcript: "Então, como tem passado desde a semana passada?\nMelhor, a dor ao fim do dia já é menos, talvez um quatro.\nE a conduzir, a olhar para a esquerda?\nJá consigo, sem aquela pontada.\nÓtimo. Vamos ver a rotação… setenta para a direita, sessenta e cinco para a esquerda.\nAo computador ainda me custa se passar das duas horas.\nVamos rever o exercício do pescoço. Queixo um pouco mais para dentro… isso.",
};
const DRAFT_EN = {
  report: "The patient reported less neck pain at the end of the day, now 4/10, and is driving without difficulty when turning to the left. Some discomfort remains when working at the computer for more than two hours at a time.\n\nThe session included cervical and thoracic joint mobilisation and a review of the deep neck flexor exercise, done correctly once the chin position was corrected. Cervical rotation improved to 70 degrees right and 65 left.",
  plan: "Next session the deep neck flexor exercise progresses in load and pain at the end of the working day is reassessed.",
  values: { pain: 4, rotation: 70, techniques: ["Joint mobilisation", "Therapeutic exercise"] },
  transcript: "So, how have you been since last week?\nBetter, the pain at the end of the day is less, maybe a four.\nAnd driving, looking to the left?\nI can now, without that twinge.\nGreat. Let's check the rotation… seventy to the right, sixty-five to the left.\nAt the computer it still bothers me past two hours.\nLet's go over the neck exercise. Chin a little further in… that's it.",
};

export function previewRec(method: string, path: string, body: unknown, m: M, role: Role, audit: (action: string, pid: string | null, detail: string) => void): unknown {
  const seg = path.split("?")[0].split("/").filter(Boolean);
  const b = (body ?? {}) as Record<string, unknown>;
  const clinical = () => { if (role === "reception") fail(403, m("A sua função não permite fazer isto.", "Your role can’t do this.")); };
  if (seg[0] === "patients" && seg[2] === "recordings") {
    clinical();
    if (method === "GET") {
      const rows = recs.filter((r) => r.patient_id === seg[1]).map((r) => ({
        id: r.id, status: status(r), seconds: r.seconds, template: r.template, lang: r.lang, error: r.failed && r.failed !== "waiting" ? r.failed : "", note_id: null,
        created_by: r.created_by, created_by_name: "Filipe Silva", created_at: r.created_at, ready_at: null, delete_after: new Date(Date.parse(r.created_at) + 30 * 864e5).toISOString(),
      })).reverse();
      return { recordings: rows, speech: { available: false, url: "http://192.168.1.66:8178", reason: "offline" } };
    }
    if (method === "POST") {
      const id = `rec-${++n}`;
      recs.push({ id, patient_id: seg[1], template: String(b.template ?? "blank"), lang: b.lang === "en" ? "en" : "pt", seconds: 0, finished: null, created_at: new Date().toISOString(), created_by: "fs" });
      audit("recording.started", seg[1], id);
      return { id };
    }
  }
  if (seg[0] === "recordings") {
    clinical();
    const r = recs.find((x) => x.id === seg[1]);
    if (!r) return fail(404, m("Não encontrado.", "Not found."));
    if (seg[2] === "chunks" && method === "PUT") { r.seconds += Math.floor((String(b.data ?? "").length * 3) / 4 / 32000); return { chunks: Number(seg[3]) + 1, seconds: r.seconds }; }
    if (seg[2] === "finish") {
      if (r.seconds < 10) fail(400, m("A gravação é demasiado curta. Grave a sessão, ou apague esta gravação.", "The recording is too short. Record the session, or delete this recording."));
      r.finished = Date.now(); audit("recording.uploaded", r.patient_id, `${r.id} · ${r.seconds} s`); return { status: "queued", seconds: r.seconds };
    }
    if (seg[2] === "retry") { r.failed = undefined; r.finished = Date.now(); return { status: "queued" }; }
    if (method === "DELETE") { recs.splice(recs.indexOf(r), 1); audit("recording.deleted", r.patient_id, `${r.id} · by_hand`); return { ok: true }; }
    if (method === "GET" && seg.length === 2) {
      if (status(r) !== "ready") fail(409, m("O rascunho ainda não está pronto.", "The draft isn’t ready yet."));
      audit("recording.opened", r.patient_id, r.id);
      return { id: r.id, template: r.template, lang: r.lang, model: "gemma4:12b", ...(r.lang === "en" ? DRAFT_EN : DRAFT_PT) };
    }
  }
  return undefined;
}
