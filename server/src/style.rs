//! How the local AI writes clinical text (DECISIONS.md, 11 Nov 2026): plain flowing paragraphs, like a
//! physiotherapist's own record — no titles, no bold, no bullets, no SOAP — anywhere.
//!
//! The voice comes from the profession's published standard, not from anyone's real notes: Padrões de
//! Prática da Fisioterapia (APFISIO), standard 14 (records): concise, legible, in a logical sequence,
//! dated and written soon after the contact; abbreviations only from an agreed glossary; each session's
//! review covers the plan and both what the patient reports and what was measured.
//!
//! `tidy` removes markdown the model adds anyway; `problem` finds what can't be tidied (a heading, a
//! list) so the caller can ask once more and then give up rather than show a structured draft.

/// The writing rules every clinical prompt ends with (European Portuguese).
pub const RULES_PT: &str = "Regras de escrita: \
texto corrido em parágrafos curtos, como no registo de um fisioterapeuta experiente; \
sem títulos, sem negrito, sem listas nem marcadores, sem formato SOAP nem as letras S, O, A, P como secções; \
passado para o que aconteceu na sessão e presente para o plano; \
«o utente» ou «a utente», nunca o nome; português europeu (ex.: «utente», «fisioterapeuta», «marcha», «edema»); \
frases curtas, sem rodeios nem frases de enchimento; sem abreviaturas, exceto escalas conhecidas (EVA, EPIC); \
nada de saudações nem comentários teus.";
/// The same rules in English (British English drafts).
pub const RULES_EN: &str = "Writing rules: \
continuous text in short paragraphs, like an experienced physiotherapist's own record; \
no headings, no bold, no lists or bullet points, no SOAP format and no S, O, A, P sections; \
past tense for what happened in the session, present tense for the plan; \
\"the patient\", never their name; British English; \
short sentences, no padding; no abbreviations except well-known scales; \
no greetings or comments of your own.";

/// An invented example (not a real patient) that shows the voice. Labelled as invented in the prompt.
pub const EXAMPLE_PT: &str = "Exemplo inventado, só para mostrar o estilo:\n\
A utente referiu menos dor cervical ao fim do dia, agora 4/10, e já conduz sem limitação na rotação. \
Na sessão fez-se mobilização articular cervical e torácica e reviu-se o exercício de flexores profundos, que executou corretamente.\n\n\
A rotação cervical melhorou para 70 graus à direita e 65 à esquerda. Mantém o plano de exercício em casa duas vezes por dia; \
na próxima sessão progride-se a carga e reavalia-se a dor ao fim do dia de trabalho.";
pub const EXAMPLE_EN: &str = "Invented example, only to show the style:\n\
The patient reported less neck pain at the end of the day, now 4/10, and is driving without restriction on rotation. \
The session included cervical and thoracic joint mobilisation and a review of the deep neck flexor exercise, which was done correctly.\n\n\
Cervical rotation improved to 70 degrees right and 65 left. The home exercise plan continues twice a day; \
next session the load progresses and pain at the end of the working day is reassessed.";

/// Removes what the model adds out of habit: thinking blocks, bold/italic marks, `#` titles, bullet marks.
pub fn tidy(text: &str) -> String {
    let mut s = text.to_string();
    while let (Some(a), Some(b)) = (s.find("<think>"), s.find("</think>")) {
        if b < a { break; }
        s.replace_range(a..b + "</think>".len(), "");
    }
    let s = s.replace("**", "").replace("__", "");
    let lines: Vec<String> = s
        .lines()
        .map(|l| {
            let t = l.trim();
            let t = t.trim_start_matches('#').trim_start();
            let t = ["- ", "* ", "• ", "– "].iter().find_map(|m| t.strip_prefix(m)).unwrap_or(t);
            t.to_string()
        })
        .collect();
    // At most one blank line between paragraphs.
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if l.is_empty() && out.last().is_none_or(|p| p.is_empty()) { continue; }
        out.push(l);
    }
    while out.last().is_some_and(|l| l.is_empty()) { out.pop(); }
    out.join("\n")
}

const SECTION_WORDS: &[&str] = &[
    "subjetivo", "subjectivo", "objetivo", "objectivo", "avaliação", "avaliacao", "plano", "análise", "analise",
    "subjective", "objective", "assessment", "plan", "evolução", "evolucao", "progress", "notas", "notes", "resumo", "summary",
];

/// What makes a tidied draft unusable, if anything: a SOAP section, a heading line or a numbered list.
pub fn problem(text: &str) -> Option<&'static str> {
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let low = line.to_lowercase();
        // "S (subjetivo)", "S:", "O -", "A." at the start of a line.
        let mut ch = low.chars();
        if let (Some(first), Some(second)) = (ch.next(), ch.next()) {
            if "soap".contains(first) && (second == ':' || second == '.' || second == '-' || second == ')'
                || (second == ' ' && matches!(ch.clone().find(|c| *c != ' '), Some('(' | ':' | '-' | '–')))) {
                return Some("soap");
            }
        }
        // "Plano:", "Avaliação (…)", "Assessment -" — a section word used as a title.
        for w in SECTION_WORDS {
            if let Some(rest) = low.strip_prefix(w) {
                let rest = rest.trim_start();
                if rest.is_empty() || rest.starts_with(':') || rest.starts_with('(') || rest.starts_with(" -") || rest.starts_with('-') || rest.starts_with('–') {
                    return Some("heading");
                }
            }
        }
        // A short line that ends with a colon is a title ("Na sessão:").
        if line.ends_with(':') && line.chars().count() <= 40 {
            return Some("heading");
        }
        // "1. …", "2) …"
        let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && digits <= 2 && matches!(line.chars().nth(digits), Some('.' | ')')) && line.chars().nth(digits + 1) == Some(' ') {
            return Some("list");
        }
    }
    None
}

/// Asks the local AI for plain clinical text: tidies the answer and, if it still has a heading or a list,
/// asks once more with a reminder. A second structured answer counts as a failed draft.
pub async fn generate_plain(system: &str, prompt: &str, en: bool) -> Result<String, crate::ai::AiError> {
    generate_plain_ctx(system, prompt, en, None).await
}

/// `generate_plain` with a bigger context for long prompts (a session transcript).
pub async fn generate_plain_ctx(system: &str, prompt: &str, en: bool, ctx: Option<u32>) -> Result<String, crate::ai::AiError> {
    let ask = |p: String| async move {
        match ctx { Some(c) => crate::ai::generate_long(system, &p, c).await, None => crate::ai::generate(system, &p).await }
    };
    let text = tidy(&ask(prompt.to_string()).await?);
    if problem(&text).is_none() {
        return Ok(text);
    }
    let again = if en {
        format!("{prompt}\n\nYour last answer had headings or a list. Write it again as plain paragraphs only, with no headings, no lists and no S, O, A, P sections.")
    } else {
        format!("{prompt}\n\nA tua última resposta tinha títulos ou uma lista. Escreve de novo só em parágrafos corridos, sem títulos, sem listas e sem secções S, O, A, P.")
    };
    let text = tidy(&ask(again).await?);
    if problem(&text).is_none() { Ok(text) } else { Err(crate::ai::AiError::Failed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidies_markdown() {
        assert_eq!(tidy("<think>x</think>**A utente** melhorou.\n\n\n- Mantém o plano."), "A utente melhorou.\n\nMantém o plano.");
        assert_eq!(tidy("## Nota\nTexto"), "Nota\nTexto");
    }

    #[test]
    fn plain_paragraphs_pass() {
        assert_eq!(problem(EXAMPLE_PT), None);
        assert_eq!(problem(EXAMPLE_EN), None);
        assert_eq!(problem("A utente referiu dor 6/10.\n\nO plano mantém-se: exercício em casa."), None);
        assert_eq!(problem("Plano de exercício revisto com a utente e cumprido sem dor."), None);
        assert_eq!(problem("Avaliação da força sem alterações."), None);
        assert_eq!(problem("O utente chegou com menos dor."), None);
        assert_eq!(problem("A rotação melhorou."), None);
        assert_eq!(problem("Em 2026 iniciou tratamento."), None);
    }

    #[test]
    fn structure_is_caught() {
        assert_eq!(problem("S (subjetivo) Refere dor 6/10."), Some("soap"));
        assert_eq!(problem("A utente melhorou.\nO: rotação 70°"), Some("soap"));
        assert_eq!(problem("P - manter exercício"), Some("soap"));
        assert_eq!(problem("Plano: manter."), Some("heading"));
        assert_eq!(problem("Avaliação (sessão 3)\nMelhor."), Some("heading"));
        assert_eq!(problem("Assessment\nBetter."), Some("heading"));
        assert_eq!(problem("Na sessão:\nmobilização."), Some("heading"));
        assert_eq!(problem("1. Mobilização\n2. Exercício"), Some("list"));
    }
}
