// Question-by-question forms for the free outcome questionnaires (audit phase C): NPRS, SPADI, LEFS, PSFS, KOOS.
// The server scores them again (`server/src/forms.rs`, same item ids and rules) and stores its own result;
// this copy only shows the score live while the form is filled in.
//
// The Portuguese wording is the clinic's own translation of the English originals. Where a validated Portuguese
// version is used on paper, check that the wording matches it (see claude/OPEN_QUESTIONS.md).
// ODI, NDI and QuickDASH are licensed instruments, so they keep the typed total.
import type { Lang } from "./ctx";

type T2 = [string, string];
export interface Item { id: string; text: T2 }
export interface Section {
  id: string; title: T2; intro?: T2;
  /** Answer labels from 0 upwards; for 0–10 scales only the two ends are labelled. */
  scale: { max: number; labels?: T2[]; ends?: [T2, T2] };
  items: Item[];
  /** How many items may be left blank in this section (counted per section). */
  maySkip: number;
}
export interface FormDef { code: string; sections: Section[]; higherIsWorse: boolean; max: number; unit?: string; note: T2 }
export interface Answers { items: Record<string, number | null>; activities?: string[] }
export interface FormScore { score: number | null; sub: Record<string, number>; blank: number; problem?: T2 }

const pick = (t: T2, l: Lang) => (l === "pt" ? t[0] : t[1]);
export const tx = pick;

const DIFF5: T2[] = [["Nenhuma", "None"], ["Ligeira", "Mild"], ["Moderada", "Moderate"], ["Grave", "Severe"], ["Extrema", "Extreme"]];
const FREQ5: T2[] = [["Nunca", "Never"], ["Raramente", "Rarely"], ["Às vezes", "Sometimes"], ["Muitas vezes", "Often"], ["Sempre", "Always"]];
const FREQ5_REV: T2[] = [["Sempre", "Always"], ["Muitas vezes", "Often"], ["Às vezes", "Sometimes"], ["Raramente", "Rarely"], ["Nunca", "Never"]];

const ids = (p: string, n: number) => Array.from({ length: n }, (_, i) => `${p}${i + 1}`);
const items = (p: string, list: T2[]): Item[] => list.map((text, i) => ({ id: `${p}${i + 1}`, text }));

export const FORMS: Record<string, FormDef> = {
  nprs: {
    code: "nprs", higherIsWorse: true, max: 10,
    note: ["0 = sem dor, 10 = a pior dor imaginável.", "0 = no pain, 10 = worst pain imaginable."],
    sections: [{ id: "pain", title: ["Dor", "Pain"], maySkip: 0,
      scale: { max: 10, ends: [["Sem dor", "No pain"], ["Pior dor imaginável", "Worst pain imaginable"]] },
      items: [{ id: "now", text: ["Que número descreve a sua dor neste momento?", "Which number describes your pain right now?"] }] }],
  },
  spadi: {
    code: "spadi", higherIsWorse: true, max: 100, unit: "%",
    note: ["Pontuação em %: dor, incapacidade e total. Pode deixar até 2 perguntas em branco (não se aplica).", "Scored in %: pain, disability and total. Up to 2 questions may be left blank (not applicable)."],
    sections: [
      { id: "pain", title: ["Dor", "Pain"], intro: ["Quanta dor teve no ombro?", "How severe was your shoulder pain?"], maySkip: 2,
        scale: { max: 10, ends: [["Sem dor", "No pain"], ["Pior dor imaginável", "Worst pain imaginable"]] },
        items: items("p", [["No pior momento", "At its worst"], ["Deitado sobre o lado afetado", "When lying on the involved side"],
          ["Ao alcançar algo numa prateleira alta", "Reaching for something on a high shelf"], ["Ao tocar na parte de trás do pescoço", "Touching the back of your neck"],
          ["Ao empurrar com o braço afetado", "Pushing with the involved arm"]]) },
      { id: "disability", title: ["Incapacidade", "Disability"], intro: ["Quanta dificuldade teve?", "How much difficulty did you have?"], maySkip: 2,
        scale: { max: 10, ends: [["Sem dificuldade", "No difficulty"], ["Tão difícil que precisou de ajuda", "So difficult it required help"]] },
        items: items("d", [["Lavar o cabelo", "Washing your hair"], ["Lavar as costas", "Washing your back"], ["Vestir uma camisola interior ou camisola", "Putting on an undershirt or jumper"],
          ["Vestir uma camisa com botões à frente", "Putting on a shirt that buttons down the front"], ["Vestir as calças", "Putting on your pants"],
          ["Pôr um objeto numa prateleira alta", "Placing an object on a high shelf"], ["Carregar um objeto pesado de 4,5 kg", "Carrying a heavy object of 10 pounds (4.5 kg)"],
          ["Tirar algo do bolso de trás", "Removing something from your back pocket"]]) },
    ],
  },
  lefs: {
    code: "lefs", higherIsWorse: false, max: 80,
    note: ["0–80: mais alto é melhor. Responda a todas.", "0–80: higher is better. Answer all of them."],
    sections: [{ id: "all", title: ["Hoje, tem ou teria alguma dificuldade em:", "Today, do you or would you have any difficulty at all with:"], maySkip: 0,
      scale: { max: 4, labels: [["Extrema ou incapaz", "Extreme or unable"], ["Bastante", "Quite a bit"], ["Moderada", "Moderate"], ["Um pouco", "A little bit"], ["Nenhuma", "No difficulty"]] },
      items: items("i", [
        ["O seu trabalho habitual, tarefas domésticas ou escola", "Any of your usual work, housework or school activities"],
        ["Os seus passatempos, atividades recreativas ou desportivas", "Your usual hobbies, recreational or sporting activities"],
        ["Entrar ou sair da banheira", "Getting into or out of the bath"], ["Andar de uma divisão para outra", "Walking between rooms"],
        ["Calçar os sapatos ou as meias", "Putting on your shoes or socks"], ["Agachar-se", "Squatting"],
        ["Levantar do chão um objeto, como um saco de compras", "Lifting an object, like a bag of groceries from the floor"],
        ["Fazer tarefas leves em casa", "Performing light activities around your home"], ["Fazer tarefas pesadas em casa", "Performing heavy activities around your home"],
        ["Entrar ou sair de um carro", "Getting into or out of a car"], ["Andar 2 quarteirões", "Walking 2 blocks"], ["Andar cerca de 1,5 km", "Walking a mile"],
        ["Subir ou descer 10 degraus (cerca de um lanço de escadas)", "Going up or down 10 stairs (about 1 flight of stairs)"],
        ["Estar de pé durante 1 hora", "Standing for 1 hour"], ["Estar sentado durante 1 hora", "Sitting for 1 hour"],
        ["Correr em terreno plano", "Running on even ground"], ["Correr em terreno irregular", "Running on uneven ground"],
        ["Mudar de direção bruscamente a correr depressa", "Making sharp turns while running fast"], ["Saltar ao pé-coxinho", "Hopping"], ["Virar-se na cama", "Rolling over in bed"],
      ]) }],
  },
  psfs: {
    code: "psfs", higherIsWorse: false, max: 10,
    note: ["O utente escolhe até 5 atividades difíceis; 0 = incapaz, 10 = como antes do problema. Pontuação = média.", "The patient names up to 5 hard activities; 0 = unable, 10 = as before the problem. Score = average."],
    sections: [{ id: "acts", title: ["Atividades", "Activities"], maySkip: 0,
      scale: { max: 10, ends: [["Incapaz", "Unable"], ["Como antes do problema", "As before the problem"]] }, items: ids("a", 5).map((id) => ({ id, text: ["", ""] })) }],
  },
  koos: {
    code: "koos", higherIsWorse: false, max: 100,
    note: ["Cada subescala vai de 0 (problemas extremos) a 100 (sem problemas). O número do cartão é a média das cinco. Até 2 em branco por subescala.", "Each subscale runs from 0 (extreme problems) to 100 (no problems). The card shows the mean of the five. Up to 2 blank per subscale."],
    sections: [
      { id: "symptoms", title: ["Sintomas", "Symptoms"], intro: ["Na última semana.", "During the last week."], maySkip: 2, scale: { max: 4, labels: FREQ5 },
        items: [
          { id: "s1", text: ["Tem inchaço no joelho?", "Do you have swelling in your knee?"] },
          { id: "s2", text: ["Sente o joelho a ranger, ouve estalidos ou outros ruídos quando o mexe?", "Do you feel grinding, hear clicking or any other type of noise when your knee moves?"] },
          { id: "s3", text: ["O joelho prende ou bloqueia quando o mexe?", "Does your knee catch or hang up when moving?"] },
        ] },
      { id: "symptoms", title: ["", ""], maySkip: 2, scale: { max: 4, labels: FREQ5_REV },
        items: [
          { id: "s4", text: ["Consegue esticar o joelho completamente?", "Can you straighten your knee fully?"] },
          { id: "s5", text: ["Consegue dobrar o joelho completamente?", "Can you bend your knee fully?"] },
        ] },
      { id: "symptoms", title: ["Rigidez", "Stiffness"], maySkip: 2, scale: { max: 4, labels: DIFF5 },
        items: [
          { id: "s6", text: ["Rigidez no joelho logo ao acordar de manhã", "Knee stiffness after first wakening in the morning"] },
          { id: "s7", text: ["Rigidez no joelho depois de estar sentado, deitado ou em repouso, mais tarde no dia", "Knee stiffness after sitting, lying or resting later in the day"] },
        ] },
      { id: "pain", title: ["Dor", "Pain"], maySkip: 2, scale: { max: 4, labels: [["Nunca", "Never"], ["Mensalmente", "Monthly"], ["Semanalmente", "Weekly"], ["Diariamente", "Daily"], ["Sempre", "Always"]] },
        items: [{ id: "p1", text: ["Com que frequência tem dor no joelho?", "How often do you experience knee pain?"] }] },
      { id: "pain", title: ["", ""], intro: ["Quanta dor teve no joelho na última semana ao:", "How much knee pain have you had during the last week when:"], maySkip: 2, scale: { max: 4, labels: DIFF5 },
        items: items("p", [["", ""], ["Rodar ou girar sobre o joelho", "Twisting/pivoting on your knee"], ["Esticar o joelho completamente", "Straightening knee fully"],
          ["Dobrar o joelho completamente", "Bending knee fully"], ["Andar em terreno plano", "Walking on a flat surface"], ["Subir ou descer escadas", "Going up or down stairs"],
          ["À noite, na cama", "At night while in bed"], ["Sentado ou deitado", "Sitting or lying"], ["De pé", "Standing upright"]]).slice(1) },
      { id: "adl", title: ["Atividades do dia a dia", "Daily living"], intro: ["Que dificuldade teve na última semana ao:", "What difficulty have you had during the last week when:"], maySkip: 2, scale: { max: 4, labels: DIFF5 },
        items: items("a", [["Descer escadas", "Descending stairs"], ["Subir escadas", "Ascending stairs"], ["Levantar-se de uma cadeira", "Rising from sitting"], ["Estar de pé", "Standing"],
          ["Baixar-se para apanhar algo do chão", "Bending to the floor or picking up an object"], ["Andar em terreno plano", "Walking on a flat surface"], ["Entrar ou sair do carro", "Getting in or out of a car"],
          ["Ir às compras", "Going shopping"], ["Calçar meias", "Putting on socks or stockings"], ["Levantar-se da cama", "Rising from bed"], ["Descalçar meias", "Taking off socks or stockings"],
          ["Estar deitado na cama (virar-se, manter a posição do joelho)", "Lying in bed (turning over, keeping knee position)"], ["Entrar ou sair da banheira", "Getting in or out of the bath"],
          ["Estar sentado", "Sitting"], ["Sentar-se ou levantar-se da sanita", "Getting on or off the toilet"],
          ["Fazer tarefas domésticas pesadas (mover caixas pesadas, esfregar o chão…)", "Heavy domestic duties (moving heavy boxes, scrubbing floors…)"],
          ["Fazer tarefas domésticas leves (cozinhar, limpar o pó…)", "Light domestic duties (cooking, dusting…)"]]) },
      { id: "sport", title: ["Desporto e lazer", "Sport and recreation"], intro: ["Que dificuldade teve na última semana ao:", "What difficulty have you had during the last week when:"], maySkip: 2, scale: { max: 4, labels: DIFF5 },
        items: items("sp", [["Agachar-se", "Squatting"], ["Correr", "Running"], ["Saltar", "Jumping"], ["Rodar ou girar sobre o joelho afetado", "Twisting/pivoting on your injured knee"], ["Ajoelhar-se", "Kneeling"]]) },
      { id: "qol", title: ["Qualidade de vida", "Quality of life"], maySkip: 2, scale: { max: 4, labels: [["Nunca", "Never"], ["Mensalmente", "Monthly"], ["Semanalmente", "Weekly"], ["Diariamente", "Daily"], ["Constantemente", "Constantly"]] },
        items: [{ id: "q1", text: ["Com que frequência se lembra do problema no joelho?", "How often are you aware of your knee problem?"] }] },
      { id: "qol", title: ["", ""], maySkip: 2, scale: { max: 4, labels: [["Nada", "Not at all"], ["Pouco", "Mildly"], ["Moderadamente", "Moderately"], ["Muito", "Severely"], ["Totalmente", "Totally"]] },
        items: [{ id: "q2", text: ["Mudou o seu modo de vida para evitar atividades que possam prejudicar o joelho?", "Have you modified your lifestyle to avoid activities that could damage your knee?"] }] },
      { id: "qol", title: ["", ""], maySkip: 2, scale: { max: 4, labels: [["Nada", "Not at all"], ["Pouco", "Mildly"], ["Moderadamente", "Moderately"], ["Muito", "Severely"], ["Extremamente", "Extremely"]] },
        items: [{ id: "q3", text: ["Quanto o incomoda a falta de confiança no joelho?", "How much are you troubled by lack of confidence in your knee?"] }] },
      { id: "qol", title: ["", ""], maySkip: 2, scale: { max: 4, labels: DIFF5 },
        items: [{ id: "q4", text: ["Em geral, quanta dificuldade tem com o joelho?", "In general, how much difficulty do you have with your knee?"] }] },
    ],
  },
};

export const SUB_LABEL: Record<string, T2> = {
  pain: ["Dor", "Pain"], disability: ["Incapacidade", "Disability"], symptoms: ["Sintomas", "Symptoms"],
  adl: ["Dia a dia", "Daily living"], sport: ["Desporto e lazer", "Sport and recreation"], qol: ["Qualidade de vida", "Quality of life"],
};

const r1 = (x: number) => Math.round(x * 10) / 10;

/** The same scoring as the server, for the live total on screen. */
export function scoreForm(code: string, a: Answers): FormScore {
  const f = FORMS[code];
  const v = (id: string) => { const x = a.items[id]; return typeof x === "number" ? x : null; };
  // Group the item ids by subscale (KOOS splits a subscale over several sections with different answer labels).
  const subs = new Map<string, string[]>();
  const nActs = code === "psfs" ? (a.activities ?? []).length : 0;
  for (const s of f.sections) subs.set(s.id, [...(subs.get(s.id) ?? []), ...(code === "psfs" ? s.items.slice(0, nActs) : s.items).map((i) => i.id)]);
  const all = [...subs.values()].flat();
  const blank = all.filter((id) => v(id) === null).length;
  const sum = (list: string[]) => list.reduce((t, id) => t + (v(id) ?? 0), 0);
  const answered = (list: string[]) => list.filter((id) => v(id) !== null).length;
  const tooMany: T2 = ["Demasiadas perguntas em branco para calcular.", "Too many questions left blank to work it out."];
  const sub: Record<string, number> = {};
  switch (code) {
    case "nprs": return { score: v("now"), sub, blank };
    case "lefs": return { score: blank ? null : sum(all), sub, blank };
    case "psfs": {
      const names = a.activities ?? [];
      if (!names.length || names.some((n) => !n.trim())) return { score: null, sub, blank, problem: ["Escreva o nome de cada atividade.", "Name each activity."] };
      return { score: blank ? null : r1(sum(all) / all.length), sub, blank };
    }
    case "spadi": {
      let ok = blank <= 2;
      for (const [id, list] of subs) { const n = answered(list); if (!n || list.length - n > 2) ok = false; else sub[id] = r1(sum(list) / (n * 10) * 100); }
      return ok ? { score: r1(sum(all) / (answered(all) * 10) * 100), sub, blank } : { score: null, sub, blank, problem: blank ? tooMany : undefined };
    }
    case "koos": {
      let ok = true, total = 0;
      for (const [id, list] of subs) { const n = answered(list); if (!n || list.length - n > 2) ok = false; else { sub[id] = r1(100 - (sum(list) / n) * 25); total += sub[id]; } }
      return ok ? { score: r1(total / subs.size), sub, blank } : { score: null, sub, blank, problem: tooMany };
    }
  }
  return { score: null, sub, blank };
}
