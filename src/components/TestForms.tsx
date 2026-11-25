// Record a test: question by question with live scoring for the free questionnaires (NPRS, SPADI, LEFS, PSFS, KOOS),
// or the typed total (ODI, NDI and QuickDASH, which are licensed, or a total already worked out on paper).
// The server scores the answers again and keeps its own result (audit phase C).
import { Fragment, useMemo, useState } from "react";
import { Plus, X } from "lucide-react";
import { useApp } from "../lib/ctx";
import { api } from "../lib/api";
import { TEST_LIBRARY, testName, fmtDate, ymd, type TestResult } from "../lib/data";
import { FORMS, SUB_LABEL, scoreForm, tx, type Answers, type Section } from "../lib/forms";
import { Field, Seg, Sheet, DateInput } from "./ui";

const fmt = (n: number, lang: string) => n.toLocaleString(lang === "pt" ? "pt-PT" : "en-GB", { maximumFractionDigits: 1 });

export function RecordTest({ patientId, code, history = [] }: { patientId: string; code?: string; history?: TestResult[] }) {
  const { L, lang, toast, sheetEdited } = useApp();
  const [c, setC] = useState(code ?? TEST_LIBRARY[0].code);
  const t = TEST_LIBRARY.find((x) => x.code === c)!;
  const form = FORMS[c];
  const [mode, setMode] = useState<"form" | "total">(form ? "form" : "total");
  const useForm = !!form && mode === "form";
  // PSFS: the same activities as last time, so the scores can be compared.
  const lastActs = useMemo(() => {
    const last = history.filter((h) => h.test_code === "psfs").sort((a, b) => a.taken_on.localeCompare(b.taken_on)).pop();
    const acts = (last?.answers as { activities?: string[] } | null)?.activities;
    return Array.isArray(acts) && acts.length ? acts : [""];
  }, [history]);
  const [ans, setAns] = useState<Answers>({ items: {}, activities: lastActs });
  const [score, setScore] = useState("");
  const [day, setDay] = useState(ymd(new Date()));
  const live = useForm ? scoreForm(c, ans) : null;
  const n = Number(score.replace(",", "."));
  const totalOk = score.trim() !== "" && Number.isFinite(n) && n >= 0 && n <= t.max;
  const valid = useForm ? live!.score !== null : totalOk;

  const switchTo = (k: string) => { setC(k); setScore(""); setAns({ items: {}, activities: lastActs }); setMode(FORMS[k] ? "form" : "total"); };
  const setItem = (id: string, v: number | null) => { sheetEdited(); setAns((a) => ({ ...a, items: { ...a.items, [id]: v } })); };

  const submit = async () => {
    const body = useForm
      ? { test_code: c, answers: { items: ans.items, ...(c === "psfs" ? { activities: (ans.activities ?? []).map((x) => x.trim()) } : {}) }, taken_on: day }
      : { test_code: c, score: n, max_score: t.max, higher_is_worse: t.higherIsWorse, taken_on: day };
    const r = await api<{ id: string; score: number; previous: number | null; worse: boolean }>("POST", `/patients/${patientId}/tests`, body);
    toast(r.worse ? L(`Pior do que da última vez (${fmt(r.previous!, "pt")} → ${fmt(r.score, "pt")}). Aparece nos avisos de Hoje.`, `Worse than last time (${fmt(r.previous!, "en")} → ${fmt(r.score, "en")}). It shows in Today’s alerts.`)
      : r.previous != null ? L(`Registado: ${fmt(r.score, "pt")} (antes ${fmt(r.previous, "pt")})`, `Recorded: ${fmt(r.score, "en")} (was ${fmt(r.previous, "en")})`) : L(`Registado: ${fmt(r.score, "pt")}`, `Recorded: ${fmt(r.score, "en")}`),
      () => { api("DELETE", `/tests/${r.id}`).catch(() => {}); }, L("Registar teste", "Record Test"));
  };

  return (
    <Sheet wide={useForm && c !== "nprs"} title={L("Registar teste", "Record Test")}
      sub={useForm ? tx(form!.note, lang) : form ? L("Escreva o total já calculado (por exemplo, num questionário em papel).", "Enter a total already worked out (for example, on a paper questionnaire).")
        : L("Escreva a pontuação total. Este questionário tem licença própria, por isso as perguntas não estão na aplicação.", "Enter the total score. This questionnaire is licensed, so its questions aren’t in the app.")}
      action={L("Registar", "Record")} onAction={submit} disabled={!valid}>
      <div className="grid2">
        <Field label={L("Teste", "Test")}>
          <select className="input" value={c} onChange={(e) => switchTo(e.target.value)}>{TEST_LIBRARY.map((x) => <option key={x.code} value={x.code}>{testName(x.code, lang)}</option>)}</select>
        </Field>
        <Field label={L("Data", "Date")}><DateInput className="input" value={day} max={ymd(new Date())} onChange={(e) => setDay(e.target.value)} /></Field>
      </div>
      {form && <div style={{ marginBottom: 12 }}><Seg label={L("Como registar", "How to record")} value={mode} onChange={setMode}
        options={[["form", L("Pergunta a pergunta", "Question by Question")], ["total", L("Só o total", "Total Only")]]} /></div>}

      {useForm ? (
        <>
          <div className="tf-score" role="status" aria-live="polite">
            <span className="num tf-total">{live!.score === null ? "—" : fmt(live!.score, lang)}<span className="muted">{form!.unit ? " %" : `/${form!.max}`}</span></span>
            <span className="small muted">
              {Object.entries(live!.sub).map(([k, v]) => <Fragment key={k}>{tx(SUB_LABEL[k], lang)} {fmt(v, lang)} · </Fragment>)}
              {live!.problem && (c === "psfs" || live!.blank <= 4) ? tx(live!.problem, lang) : live!.blank ? L(`${live!.blank} por responder`, `${live!.blank} unanswered`) : L("Tudo respondido", "All answered")}
            </span>
          </div>
          {c === "psfs" ? <Psfs ans={ans} setAns={(a) => { sheetEdited(); setAns(a); }} setItem={setItem} sec={form!.sections[0]} /> : form!.sections.map((s, i) => <FormSection key={i} sec={s} ans={ans} setItem={setItem} />)}
        </>
      ) : (
        <>
          <Field label={L(`Pontuação (0–${t.max})`, `Score (0–${t.max})`)} error={score.trim() !== "" && !totalOk ? L(`Escreva um número entre 0 e ${t.max}.`, `Enter a number from 0 to ${t.max}.`) : undefined}>
            <input className="input num" inputMode="decimal" value={score} onChange={(e) => { sheetEdited(); setScore(e.target.value); }} autoFocus style={{ maxWidth: 140 }} /></Field>
          <p className="small muted">{t.higherIsWorse ? L("Neste teste, uma pontuação mais alta é pior.", "On this test, a higher score is worse.") : L("Neste teste, uma pontuação mais alta é melhor.", "On this test, a higher score is better.")}</p>
        </>
      )}
    </Sheet>
  );
}

/** One block of questions sharing the same answers. Native radio buttons, so arrow keys and screen readers work. */
function FormSection({ sec, ans, setItem }: { sec: Section; ans: Answers; setItem: (id: string, v: number | null) => void }) {
  const { lang, L } = useApp();
  const opts = Array.from({ length: sec.scale.max + 1 }, (_, i) => i);
  const wide = sec.scale.max > 4;
  return (
    <div className="tf-sec">
      {tx(sec.title, lang) && <h3>{tx(sec.title, lang)}</h3>}
      {sec.intro && <p className="small muted" style={{ marginBottom: 6 }}>{tx(sec.intro, lang)}</p>}
      {wide && sec.scale.ends && <div className="tf-ends small muted"><span>0 = {tx(sec.scale.ends[0], lang)}</span><span>{sec.scale.max} = {tx(sec.scale.ends[1], lang)}</span></div>}
      {sec.items.map((it) => {
        const cur = ans.items[it.id];
        return (
          <fieldset key={it.id} className={`tf-q ${wide ? "wide" : ""}`}>
            <legend>{tx(it.text, lang)}</legend>
            <div className={`tf-opts ${wide ? "n11" : "n5"}`}>
              {opts.map((o) => (
                <label key={o} className={cur === o ? "on" : ""} title={sec.scale.labels ? `${o} — ${tx(sec.scale.labels[o], lang)}` : String(o)}>
                  <input type="radio" name={it.id} checked={cur === o} onChange={() => setItem(it.id, o)} />
                  <span>{sec.scale.labels ? tx(sec.scale.labels[o], lang) : o}</span>
                </label>
              ))}
              {cur != null && <button type="button" className="linkbtn tf-clear" onClick={() => setItem(it.id, null)} aria-label={L("Limpar resposta", "Clear answer")}>{L("Limpar", "Clear")}</button>}
            </div>
          </fieldset>
        );
      })}
    </div>
  );
}

function Psfs({ ans, setAns, setItem, sec }: { ans: Answers; setAns: (a: Answers) => void; setItem: (id: string, v: number | null) => void; sec: Section }) {
  const { L } = useApp();
  const acts = ans.activities ?? [""];
  const setName = (i: number, v: string) => setAns({ ...ans, activities: acts.map((x, j) => (j === i ? v : x)) });
  const remove = (i: number) => {
    // Scores move up with their activity.
    const items: Record<string, number | null> = {};
    acts.filter((_, j) => j !== i).forEach((_, j) => { const from = j < i ? j : j + 1; items[`a${j + 1}`] = ans.items[`a${from + 1}`] ?? null; });
    setAns({ items, activities: acts.filter((_, j) => j !== i) });
  };
  return (
    <div className="tf-sec">
      <div className="tf-ends small muted"><span>0 = {L("Incapaz", "Unable")}</span><span>10 = {L("Como antes do problema", "As before the problem")}</span></div>
      {acts.map((name, i) => {
        const id = `a${i + 1}`, cur = ans.items[id];
        return (
          <fieldset key={i} className="tf-q wide">
            <legend className="tf-act">
              <input className="input" value={name} onChange={(e) => setName(i, e.target.value)} maxLength={80} aria-label={L(`Atividade ${i + 1}`, `Activity ${i + 1}`)}
                placeholder={L("Atividade difícil, como subir escadas", "A hard activity, like climbing stairs")} autoFocus={i === 0 && !name} />
              {acts.length > 1 && <button type="button" className="btn icon sm" onClick={() => remove(i)} aria-label={L("Tirar esta atividade", "Remove this activity")}><X /></button>}
            </legend>
            <div className="tf-opts n11">
              {Array.from({ length: sec.scale.max + 1 }, (_, o) => (
                <label key={o} className={cur === o ? "on" : ""}><input type="radio" name={id} checked={cur === o} onChange={() => setItem(id, o)} /><span>{o}</span></label>
              ))}
            </div>
          </fieldset>
        );
      })}
      {acts.length < 5 && <button type="button" className="btn sm" onClick={() => setAns({ ...ans, activities: [...acts, ""] })}><Plus />{L("Juntar atividade", "Add Activity")}</button>}
    </div>
  );
}

/** One recorded result: the score, subscales, and each answer if it was recorded question by question. */
export function TestResultSheet({ r }: { r: TestResult }) {
  const { L, lang } = useApp();
  const a = (r.answers ?? {}) as { items?: Record<string, number>; sub?: Record<string, number>; activities?: string[] };
  const form = FORMS[r.test_code];
  const rows: { q: string; v: string }[] = [];
  if (form && a.items) {
    if (r.test_code === "psfs") (a.activities ?? []).forEach((n, i) => rows.push({ q: n, v: String(a.items![`a${i + 1}`] ?? "—") }));
    else for (const s of form.sections) for (const it of s.items) {
      const v = a.items[it.id];
      rows.push({ q: tx(it.text, lang), v: v == null ? "—" : s.scale.labels ? `${v} · ${tx(s.scale.labels[v], lang)}` : String(v) });
    }
  }
  return (
    <Sheet wide={rows.length > 0} title={testName(r.test_code, lang)} sub={`${fmtDate(r.taken_on, lang)}${r.recorded_by_name ? ` · ${r.recorded_by_name}` : ""}`}>
      <p className="num" style={{ fontSize: "var(--fs-title)", fontWeight: 700 }}>{fmt(r.score, lang)}<span className="muted" style={{ fontSize: "var(--fs-sub)" }}>{r.max_score ? `/${fmt(r.max_score, lang)}` : ""}</span></p>
      {a.sub && <p className="small" style={{ marginBottom: 10 }}>{Object.entries(a.sub).map(([k, v]) => `${tx(SUB_LABEL[k] ?? [k, k], lang)} ${fmt(v, lang)}`).join(" · ")}</p>}
      {rows.length > 0 ? (
        <div className="tbl-wrap" style={{ maxHeight: 360, overflow: "auto" }}><table>
          <thead><tr><th className="plain">{L("Pergunta", "Question")}</th><th className="plain">{L("Resposta", "Answer")}</th></tr></thead>
          <tbody>{rows.map((x, i) => <tr key={i}><td>{x.q}</td><td className="num" style={{ whiteSpace: "nowrap" }}>{x.v}</td></tr>)}</tbody>
        </table></div>
      ) : <p className="small muted">{L("Registado só com o total.", "Recorded as a total only.")}</p>}
    </Sheet>
  );
}

/** Every result of one test for this patient, newest first; each opens its answers. */
export function TestHistory({ code, list }: { code: string; list: TestResult[] }) {
  const { L, lang, openSheet } = useApp();
  const rows = [...list].sort((a, b) => b.taken_on.localeCompare(a.taken_on));
  return (
    <Sheet title={testName(code, lang)} sub={L(`${rows.length} resultado${rows.length === 1 ? "" : "s"}`, `${rows.length} result${rows.length === 1 ? "" : "s"}`)}>
      <div className="rows">
        {rows.map((r) => {
          const sub = (r.answers as { sub?: Record<string, number> } | null)?.sub;
          return (
            <button key={r.id} className="row" style={{ width: "100%", textAlign: "left" }} onClick={() => openSheet(<TestResultSheet r={r} />)}>
              <span className="grow"><span className="t" style={{ display: "block" }}>{fmtDate(r.taken_on, lang)}</span>
                <span className="s">{r.recorded_by_name ?? ""}{sub ? ` · ${Object.entries(sub).map(([k, v]) => `${tx(SUB_LABEL[k] ?? [k, k], lang)} ${fmt(v, lang)}`).join(" · ")}` : ""}</span></span>
              <span className="num" style={{ fontWeight: 600 }}>{fmt(r.score, lang)}<span className="muted">{r.max_score ? `/${fmt(r.max_score, lang)}` : ""}</span></span>
            </button>
          );
        })}
      </div>
    </Sheet>
  );
}
