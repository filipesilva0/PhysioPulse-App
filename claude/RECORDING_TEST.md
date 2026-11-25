# Phase 2 gate: judge transcription on an invented two-person session

The plan (decision 11 Nov 2026) puts a check at the start of phase 2: if Portuguese transcription of a
two-person conversation is poor, recording falls back to dictation (your voice only) and nothing else changes.

**Everything below is invented.** No real patient. Two people read it aloud in the treatment room, at a normal
pace, with the computer where it will really be. Record it in PhysioPulse on a test patient (Evolução › Gravar
sessão…), wait for "Rascunho pronto para rever", open the draft and look at the automatic transcript and the note.

What to judge (write the answers in STATUS.md or tell Claude):

1. Transcript: are the numbers right (5/10, 140°, 120°)? Are the clinical words right (rotadores, elástico,
   mobilização)? Roughly how many words are wrong?
2. Who spoke: does the report put the pain and the difficulties on the patient, and the measuring and the plan on
   the therapist?
3. Report: plain paragraphs, no titles or lists, «o utente», nothing invented?
4. Proposed values: pain 5, the techniques ticked?
5. Time: from Terminar to "Rascunho pronto" (the AI PC already on).

If 1 is poor: dictation instead (owner decision). If 2 is poor but 1 is good: real speaker separation (pyannote)
can be added later.

---

**Fisioterapeuta:** Bom dia, senhora Teresa. Então, como tem passado do ombro desde a semana passada?

**Utente:** Bom dia. Melhor, sabe? A dor à noite já quase não aparece. Mas quando levanto o braço acima da
cabeça ainda dói, talvez um cinco em dez.

**Fisioterapeuta:** E no dia a dia? Vestir o casaco, chegar à prateleira de cima da cozinha?

**Utente:** O casaco já consigo sozinha. A prateleira ainda me custa, tenho de pedir ao meu marido.

**Fisioterapeuta:** Fez os exercícios em casa?

**Utente:** Fiz quase todos os dias, só falhei no fim de semana.

**Fisioterapeuta:** Muito bem. Vamos medir. Levante o braço à frente, devagar, até onde conseguir... Cento e
quarenta graus. Na semana passada eram cento e vinte. Agora para o lado... cento e dez.

**Utente:** Aqui no fim é que puxa.

**Fisioterapeuta:** É normal. Hoje vamos fazer mobilização do ombro e depois reforço dos rotadores com o
elástico verde. Diga-me se a dor passar de seis.

*(pausa de um minuto, com o som da sessão)*

**Utente:** Assim está bem, não dói mais do que o costume.

**Fisioterapeuta:** Ótimo. Em casa continue os exercícios duas vezes por dia, com o elástico verde. Na
próxima sessão passamos para o azul e voltamos a medir a elevação.

**Utente:** Está combinado. Até quinta.
