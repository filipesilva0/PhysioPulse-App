//! Phase 4: the admin assistant (owner only, ⌥⌘A in the app).
//!
//! The local AI only *understands* the request: it answers a fixed JSON shape (skill + details). The server then
//! checks every detail against the database and builds the change as a before/after preview, stored as
//! `pending`. Nothing changes until the owner presses Apply; applying checks nothing moved in between, runs
//! in one transaction, is logged, and can be undone while the thing is still as the change left it.
//!
//! Skills (SPEC.md › AI): prices and packages (price, on sale or not), hours and closures, staff (fixed pay,
//! active or not), reports (a month's figures with a short AI summary) — the owner's — and, since 11 Nov 2026, the
//! schedule (`agenda.rs`: book, move, cancel, mark done/missed, booking requests, waitlist, free times, questions),
//! open to everyone whose AI switch is on. The AI then sees the agenda (names, times, therapists); it never reads
//! clinical notes, contacts or money.

use super::assist::{fact_rows, fold, month_facts, DAYS};
use super::money::{self, parse_month};
use crate::ai::{self, AiError};
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

fn owner(a: &Auth) -> ApiResult<()> {
    a.require(&["owner"])
}

/// The owner's jobs (money, hours, staff). Everything else is the schedule.
pub(crate) const OWNER_SKILLS: &[&str] = &["price", "product_active", "hours", "closure", "staff_pay", "staff_active", "report"];
pub(crate) const SCHEDULE_SKILLS: &[&str] = &["book", "move", "cancel", "mark", "accept_request", "decline_request", "waitlist_add", "free_slots", "agenda"];

/// The assistant: the owner, and whoever has the AI switch on (therapists, reception) for the schedule.
fn may_use(a: &Auth) -> ApiResult<()> {
    if auth::assistant_allowed(&a.staff) { Ok(()) } else { Err(AppError::Forbidden) }
}

fn eur(c: i64, en: bool) -> String {
    if en { format!("€{}.{:02}", c / 100, c % 100) } else { format!("{},{:02} €", c / 100, c % 100) }
}
fn date_label(iso: &str) -> String {
    if iso.len() >= 10 { format!("{}/{}/{}", &iso[8..10], &iso[5..7], &iso[..4]) } else { iso.to_string() }
}

pub(crate) fn ai_error(e: AiError) -> AppError {
    match e {
        AiError::Offline => AppError::ConflictWith("ai_offline", "Can’t reach the local AI. Check the AI computer is on and Ollama is running, and its address in Settings › Local AI.", json!({ "url": ai::url() })),
        AiError::NoModel => AppError::ConflictWith("ai_no_model", "The AI model isn’t installed on the server yet.", json!({ "model": ai::model() })),
        _ => AppError::ConflictWith("ai_failed", "The assistant didn’t understand that. Try saying it another way.", Value::Null),
    }
}

fn schema(owner: bool) -> Value {
    let mut skills: Vec<&str> = if owner { OWNER_SKILLS.to_vec() } else { vec![] };
    skills.extend(SCHEDULE_SKILLS);
    skills.push("answer");
    json!({
        "type": "object",
        "properties": {
            "skill": { "type": "string", "enum": skills },
            "product": { "type": "string" },
            "price_eur": { "type": "number" },
            "active": { "type": "boolean" },
            "day": { "type": "string", "enum": ["mon", "tue", "wed", "thu", "fri", "sat", "sun", ""] },
            "open": { "type": "string" },
            "close": { "type": "string" },
            "closed": { "type": "boolean" },
            "from": { "type": "string" },
            "to": { "type": "string" },
            "label": { "type": "string" },
            "staff": { "type": "string" },
            "amount_eur": { "type": "number" },
            "month": { "type": "string" },
            "patient": { "type": "string" },
            "appointment": { "type": "string" },
            "refs": { "type": "array", "items": { "type": "string" } },
            "request": { "type": "string" },
            "date": { "type": "string" },
            "time": { "type": "string" },
            "minutes": { "type": "integer" },
            "kind": { "type": "string", "enum": ["session", "assessment", "home", ""] },
            "therapist": { "type": "string" },
            "status": { "type": "string", "enum": ["done", "no_show", ""] },
            "part": { "type": "string", "enum": ["morning", "afternoon", "any", ""] },
            "repeat_weekdays": { "type": "array", "items": { "type": "integer" } },
            "repeat_count": { "type": "integer" },
            "phone": { "type": "string" },
            "address": { "type": "string" },
            "reply": { "type": "string" }
        },
        "required": ["skill", "reply"]
    })
}

const OWNER_HELP: &str = "\
- price: change the price of a product. product = its code from the list; price_eur = the new price in euros.\n\
- product_active: stop selling a product (active=false) or sell it again (active=true). product = its code.\n\
- hours: change the opening hours of one weekday. day = mon…sun; open and close as HH:MM (24 h); or closed=true for closed all day.\n\
- closure: close the clinic on one or more days. from and to as YYYY-MM-DD (the same day for one day); label = a short reason in Portuguese, e.g. \"Natal\". \
Work out dates from today's date; a date that has already passed this year means next year.\n\
- staff_pay: set someone's fixed monthly gross pay. staff = their name from the list; amount_eur = euros.\n\
- staff_active: deactivate someone (active=false) or reactivate them (active=true). staff = their name from the list.\n\
- report: summarise how a month went. month = YYYY-MM (\"last month\" = the month before today's).\n";

fn system(owner: bool) -> String {
    format!("You are the admin assistant inside a physiotherapy clinic's management app in Portugal. The user writes in Portuguese or English. \
Work out which ONE of these the user wants and answer only with the JSON asked for:\n{}{}\n\
- answer: anything else, or if something needed is missing or unclear. Then reply = one short sentence in the user's language that asks for what's missing \
or says what you can do ({}). Never claim to have changed anything.\n\
For the other skills, reply = one short sentence in the user's language saying what you'll prepare (e.g. \"Here's the appointment for Maria on Thursday.\"). \
Nothing changes until the user presses Apply. You can't see clinical notes, contacts or payments and must not try.",
        if owner { OWNER_HELP } else { "" }, super::agenda::SKILLS_HELP,
        if owner { "appointments and the schedule, prices and packages, opening hours and closures, staff pay, monthly summaries" } else { "booking, moving and cancelling appointments, booking requests, the waitlist and free times" })
}

#[derive(Deserialize)]
pub struct Turn {
    pub(crate) who: String,
    pub(crate) text: String,
    /// What the assistant looked up for that answer (13 Nov 2026), so a follow-up ("and her notes?") still knows.
    #[serde(default)]
    pub(crate) memo: String,
}
#[derive(Deserialize)]
pub struct AskReq { pub(crate) message: String, #[serde(default)] pub(crate) history: Vec<Turn>, pub(crate) lang: Option<String> }

async fn context(s: &AppState, owner: bool) -> ApiResult<(String, super::agenda::Seen)> {
    let today = money::today(s).await?;
    let (dow,): (String,) = sqlx::query_as("SELECT trim(to_char($1::date, 'Day'))").bind(&today).fetch_one(&s.pool).await?;
    let (hours,): (Value,) = sqlx::query_as("SELECT opening_hours FROM clinic WHERE id = 1").fetch_one(&s.pool).await?;
    let (agenda, seen) = super::agenda::context(s).await?;
    if !owner {
        return Ok((format!("Today is {dow}, {today}.\nOpening hours: {hours}\n{agenda}"), seen));
    }
    let products: Vec<(String, String, String, String, i32, bool)> = sqlx::query_as(
        "SELECT code, name_pt, name_en, kind, price_cents, active FROM products ORDER BY active DESC, sort, name_pt").fetch_all(&s.pool).await?;
    let staff: Vec<(String, String, bool)> = sqlx::query_as("SELECT name, role, active FROM staff ORDER BY name").fetch_all(&s.pool).await?;
    let mut out = format!("Today is {dow}, {today}.\n\nProducts (code | Portuguese name | English name | kind | price | on sale):\n");
    for (code, pt, en, kind, cents, active) in products {
        out.push_str(&format!("{code} | {pt} | {en} | {kind} | €{}.{:02} | {}\n", cents / 100, cents % 100, if active { "yes" } else { "no" }));
    }
    out.push_str("\nStaff (name | role | active):\n");
    for (name, role, active) in staff {
        out.push_str(&format!("{name} | {role} | {}\n", if active { "yes" } else { "no" }));
    }
    out.push_str(&format!("\nOpening hours: {hours}\n"));
    out.push_str(&agenda);
    Ok((out, seen))
}

/// A pending change as the app shows it.
pub(crate) struct Proposal {
    pub(crate) skill: &'static str, pub(crate) target: Option<Uuid>, pub(crate) params: Value, pub(crate) before: Value, pub(crate) after: Value,
    pub(crate) rows: Vec<[String; 3]>, pub(crate) warnings: Vec<String>,
}

/// What the assistant answers: a sentence, maybe a change to approve, maybe figures.
pub(crate) fn answer(reply: String, change: Option<Value>, rows: Option<Value>) -> Json<Value> {
    Json(json!({ "reply": reply, "change": change, "rows": rows, "list": null, "choices": null, "model": ai::model() }))
}

fn say(en: bool, pt: &str, e: &str) -> String {
    if en { e.to_string() } else { pt.to_string() }
}

/// The owner, or anyone with the AI switch on: one message to the assistant. Since 13 Nov 2026 the assistant
/// works with tools (`agent.rs`); a model that can't use tools gets the older one-step assistant below.
pub async fn ask(State(s): State<AppState>, a: Auth, headers: axum::http::HeaderMap, Json(r): Json<AskReq>) -> ApiResult<Json<Value>> {
    may_use(&a)?;
    super::agent::ask(&s, &a, &headers, r).await
}

/// The one-step assistant (11 Nov 2026): the AI answers one JSON intent, the server does the rest. Used when the
/// model can't use tools.
pub(crate) async fn ask_legacy(s: &AppState, a: &Auth, r: AskReq) -> ApiResult<Json<Value>> {
    let s = s.clone();
    may_use(a)?;
    let is_owner = a.staff.role == "owner";
    let en = r.lang.as_deref() == Some("en");
    let msg: String = r.message.chars().take(500).collect();
    if msg.trim().is_empty() {
        return Err(AppError::BadRequest("Write what you’d like the assistant to do."));
    }
    let (mut prompt, seen) = context(&s, is_owner).await?;
    let hist: Vec<&Turn> = r.history.iter().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
    if !hist.is_empty() {
        prompt.push_str("\nConversation so far:\n");
        for t in hist {
            prompt.push_str(&format!("{}: {}\n", if t.who == "me" { "User" } else { "Assistant" }, t.text.chars().take(300).collect::<String>()));
        }
    }
    prompt.push_str(&format!("\nUser: {msg}\n"));
    // The agenda can be long: a bigger context (about 3 characters a token, plus the instructions and the answer).
    let ctx = ((prompt.len() / 3 + 3000) as u32).next_multiple_of(1024);
    let (sys, sch) = (system(is_owner), schema(is_owner));
    let res = if ctx > 8192 { ai::generate_json_long(&sys, &prompt, &sch, ctx).await } else { ai::generate_json(&sys, &prompt, &[], &sch).await };
    let intent = match res {
        Ok(v) => v,
        Err(e) => {
            auth::log(&s.pool, Some(a.staff.id), "assistant.asked", None, &a.device, "failed").await;
            return Err(ai_error(e));
        }
    };
    let mut skill = intent["skill"].as_str().unwrap_or("answer").to_string();
    if !is_owner && OWNER_SKILLS.contains(&skill.as_str()) {
        skill = "answer".into(); // whatever the model answers, the server decides who does what
    }
    auth::log(&s.pool, Some(a.staff.id), "assistant.asked", None, &a.device, &skill).await;
    let reply = intent["reply"].as_str().map(|r| r.chars().take(400).collect::<String>()).filter(|r| !r.trim().is_empty());
    let help = if is_owner {
        say(en, "Posso marcar, mudar e cancelar marcações, tratar dos pedidos de marcação, mudar preços e pacotes, o horário e os encerramentos, o ordenado fixo e quem está ativo na equipa, ou resumir um mês. O que quer fazer?",
            "I can book, move and cancel appointments, handle booking requests, change prices and packages, opening hours and closures, staff fixed pay and who’s active, or summarise a month. What would you like to do?")
    } else {
        say(en, "Posso marcar, mudar e cancelar marcações, tratar dos pedidos de marcação, pôr alguém na lista de espera ou procurar horas livres. O que quer fazer?",
            "I can book, move and cancel appointments, handle booking requests, put someone on the waitlist or find free times. What would you like to do?")
    };

    if skill == "report" {
        return report(&s, intent["month"].as_str().unwrap_or(""), en).await;
    }
    ask_rest(&s, a, &skill, &intent, &seen, en, reply, help).await
}

/// Owner: a month's figures (from the server) and a short AI summary of them.
pub(crate) async fn report(s: &AppState, month: &str, en: bool) -> ApiResult<Json<Value>> {
    {
        let first = match parse_month(month) {
            Ok((f, _, _)) => f,
            Err(_) => return Ok(answer(say(en, "De que mês quer o resumo? Por exemplo: outubro de 2026.", "Which month would you like summarised? For example: October 2026."), None, None)),
        };
        let f = month_facts(s, &first).await?;
        let rows: Vec<[String; 3]> = fact_rows(&f, en).into_iter().take(6).map(|(a, b, c)| [a, b, c]).collect();
        let lines = fact_rows(&f, en).iter().map(|(k, a_, b)| format!("- {k}: {a_} ({b})")).collect::<Vec<_>>().join("\n");
        let sys = if en { "Summarise these clinic figures for the owner in 2–3 plain sentences of British English. Use only the numbers given (previous month in brackets); no percentages, no causes, no markdown." }
                  else { "Resume estes números da clínica para a dona em 2 a 3 frases simples de português europeu. Usa só os números dados (mês anterior entre parênteses); sem percentagens, sem causas, sem markdown." };
        let text = ai::generate(sys, &format!("{}: {}\n{lines}", if en { "Month" } else { "Mês" }, &first[..7])).await.map_err(ai_error)?;
        Ok(answer(text.replace("**", ""), None, Some(json!(rows))))
    }
}

#[allow(clippy::too_many_arguments)]
async fn ask_rest(s: &AppState, a: &Auth, skill: &str, intent: &Value, seen: &super::agenda::Seen, en: bool, reply: Option<String>, help: String) -> ApiResult<Json<Value>> {
    let (s, a, skill) = (s.clone(), a, skill.to_string());
    let p = if SCHEDULE_SKILLS.contains(&skill.as_str()) {
        match super::agenda::build(&s, a, &skill, intent, seen, en, reply.clone()).await? {
            super::agenda::Built::Change(p) => p,
            super::agenda::Built::Said(x) => {
                let text = if x.text.is_empty() { reply.unwrap_or(help) } else { x.text };
                return Ok(Json(json!({ "reply": text, "change": null, "rows": null, "list": x.list, "choices": x.choices, "model": ai::model() })));
            }
        }
    } else {
        match build(&s, a, &skill, intent, en).await? {
            Ok(p) => p,
            Err(text) => return Ok(answer(if text.is_empty() { reply.unwrap_or(help) } else { text }, None, None)),
        }
    };
    let change = store(&s, a, &p).await?;
    Ok(answer(reply.unwrap_or_else(|| say(en, "Esta é a alteração que proponho.", "Here’s the change I’d make.")), Some(change), None))
}

/// Keeps a proposed change until the person applies or cancels it; answers it as the app shows it.
pub(crate) async fn store(s: &AppState, a: &Auth, p: &Proposal) -> ApiResult<Value> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO assistant_changes (id, skill, target, params, before, after, rows, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(id).bind(p.skill).bind(p.target).bind(&p.params).bind(&p.before).bind(&p.after).bind(json!(p.rows)).bind(a.staff.id)
        .execute(&s.pool).await?;
    Ok(json!({ "id": id, "skill": p.skill, "rows": p.rows, "warnings": p.warnings, "status": "pending", "undoable": !super::agent_money::NO_UNDO.contains(&p.skill) && !super::agent_docs::NO_UNDO.contains(&p.skill) }))
}

async fn find_product(s: &AppState, code: &str) -> ApiResult<Option<(Uuid, String, String, i32, bool, String)>> {
    let all: Vec<(Uuid, String, String, String, i32, bool, String)> = sqlx::query_as("SELECT id, code, name_pt, name_en, price_cents, active, kind FROM products").fetch_all(&s.pool).await?;
    let q = fold(code);
    if q.is_empty() {
        return Ok(None);
    }
    let hit = all.iter().find(|p| p.1 == code.trim())
        .or_else(|| all.iter().find(|p| fold(&p.2) == q || fold(&p.3) == q))
        .or_else(|| { let m: Vec<_> = all.iter().filter(|p| fold(&p.2).contains(&q) || fold(&p.3).contains(&q)).collect(); if m.len() == 1 { Some(m[0]) } else { None } });
    Ok(hit.map(|p| (p.0, p.2.clone(), p.3.clone(), p.4, p.5, p.6.clone())))
}

async fn find_staff(s: &AppState, name: &str) -> ApiResult<Result<(Uuid, String, String, bool), usize>> {
    let all: Vec<(Uuid, String, String, bool)> = sqlx::query_as("SELECT id, name, role, active FROM staff").fetch_all(&s.pool).await?;
    let q = fold(name);
    if q.is_empty() {
        return Ok(Err(0));
    }
    if let Some(x) = all.iter().find(|x| fold(&x.1) == q) {
        return Ok(Ok(x.clone()));
    }
    // "Ana" → the only person whose name has the word "ana".
    let m: Vec<_> = all.iter().filter(|x| { let n = fold(&x.1); let words: Vec<&str> = n.split(' ').collect(); q.split(' ').all(|qw| words.contains(&qw)) }).collect();
    Ok(if m.len() == 1 { Ok(m[0].clone()) } else { Err(m.len()) })
}

fn hm_ok(s: &str) -> bool {
    s.len() == 5 && s.as_bytes()[2] == b':' && s[..2].parse::<u8>().is_ok_and(|h| h < 24) && s[3..].parse::<u8>().is_ok_and(|m| m < 60)
}
/// "9:00" / "9h" / "09h30" → "09:00" / "09:30".
pub(crate) fn hm(s: &str) -> Option<String> {
    let t = s.trim().to_lowercase().replace('h', ":");
    let t = t.trim_end_matches(':');
    let (h, m) = t.split_once(':').unwrap_or((t, "00"));
    let m = if m.is_empty() { "00" } else { m };
    let out = format!("{:02}:{:02}", h.parse::<u8>().ok()?, m.parse::<u8>().ok()?);
    hm_ok(&out).then_some(out)
}

/// Turns what the AI understood into a checked change. `Err(text)`: something's missing — say it ("" = the AI's own reply).
pub(crate) async fn build(s: &AppState, a: &Auth, skill: &str, it: &Value, en: bool) -> ApiResult<Result<Proposal, String>> {
    let st = |k: &str| it[k].as_str().unwrap_or("").trim().to_string();
    Ok(match skill {
        "price" | "product_active" => {
            let Some((id, pt, en_name, cents, active, kind)) = find_product(s, &st("product")).await? else {
                return Ok(Err(say(en, "Não encontrei esse produto na tabela de preços. Diga o nome como aparece em Finanças › Preços e pacotes.", "I couldn’t find that product in the price list. Use its name as shown in Finance › Prices and Packages.")));
            };
            let name = if en { en_name } else { pt };
            if skill == "price" {
                let Some(new) = it["price_eur"].as_f64().filter(|x| x.is_finite() && *x >= 0.0 && *x <= 5000.0).map(|x| (x * 100.0).round() as i64) else {
                    return Ok(Err(say(en, "Qual é o novo preço?", "What’s the new price?")));
                };
                if new == cents as i64 {
                    return Ok(Err(say(en, &format!("{name} já custa {}.", eur(new, false)), &format!("{name} already costs {}.", eur(new, true)))));
                }
                let mut warnings = vec![say(en, "Quem já comprou um pacote mantém o preço que pagou.", "People who already bought a package keep the price they paid.")];
                if kind == "visit" { warnings.push(say(en, "Aplica-se às marcações marcadas como realizadas a partir de agora.", "Applies to appointments marked done from now on.")); }
                Ok(Proposal { skill: "price", target: Some(id), params: json!({}), before: json!({ "price_cents": cents }), after: json!({ "price_cents": new }),
                    rows: vec![[name, eur(cents as i64, en), eur(new, en)]], warnings })
            } else {
                let Some(want) = it["active"].as_bool() else { return Ok(Err(String::new())) };
                if want == active {
                    return Ok(Err(if want { say(en, &format!("{name} já está à venda."), &format!("{name} is already on sale.")) } else { say(en, &format!("{name} já não está à venda."), &format!("{name} is already off sale.")) }));
                }
                let lbl = |on: bool| if on { say(en, "À venda", "On sale") } else { say(en, "Retirado", "Off sale") };
                Ok(Proposal { skill: "product_active", target: Some(id), params: json!({}), before: json!({ "active": active }), after: json!({ "active": want }),
                    rows: vec![[name, lbl(active), lbl(want)]], warnings: if want { vec![] } else { vec![say(en, "Quem já o comprou continua a usá-lo.", "People who already bought it keep using it.")] } })
            }
        }
        "hours" => {
            let day = st("day");
            if !DAYS.contains(&day.as_str()) {
                return Ok(Err(say(en, "Para que dia da semana?", "For which day of the week?")));
            }
            let (cur,): (Value,) = sqlx::query_as("SELECT opening_hours -> $1 FROM clinic WHERE id = 1").bind(&day).fetch_one(&s.pool).await?;
            let new = if it["closed"].as_bool() == Some(true) {
                Value::Null
            } else {
                match (hm(&st("open")), hm(&st("close"))) {
                    (Some(o), Some(c)) if o < c => json!([o, c]),
                    _ => return Ok(Err(say(en, "A que horas abre e fecha? Por exemplo: das 9:00 às 13:00.", "What time does it open and close? For example: 9:00 to 13:00."))),
                }
            };
            if new == cur {
                return Ok(Err(say(en, "O horário desse dia já é esse.", "That day already has those hours.")));
            }
            let lbl = |v: &Value| match v.as_array() { Some(x) if x.len() == 2 => format!("{}–{}", x[0].as_str().unwrap_or(""), x[1].as_str().unwrap_or("")), _ => say(en, "Fechado", "Closed") };
            let dn = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"].iter().position(|d| *d == day).unwrap_or(0);
            let name = if en { ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][dn] } else { ["Segunda-feira", "Terça-feira", "Quarta-feira", "Quinta-feira", "Sexta-feira", "Sábado", "Domingo"][dn] };
            Ok(Proposal { skill: "hours", target: None, params: json!({ "day": day }), before: json!({ "hours": cur }), after: json!({ "hours": new }),
                rows: vec![[name.to_string(), lbl(&cur), lbl(&new)]],
                warnings: vec![say(en, "Não mexe nas marcações que já existem.", "Appointments already booked aren’t changed.")] })
        }
        "closure" => {
            let today = money::today(s).await?;
            let to_s = if st("to").is_empty() { st("from") } else { st("to") };
            let (Some(from), Some(to)) = (super::assist::date_of(&st("from")), super::assist::date_of(&to_s)) else {
                return Ok(Err(say(en, "Que dias quer fechar?", "Which days would you like to close?")));
            };
            if from < today || to < from {
                return Ok(Err(say(en, "Escolha dias a partir de hoje, com o último dia igual ou depois do primeiro.", "Choose days from today on, with the last day on or after the first.")));
            }
            let (span,): (i32,) = sqlx::query_as("SELECT $2::date - $1::date").bind(&from).bind(&to).fetch_one(&s.pool).await?;
            if span > 92 {
                return Ok(Err(say(en, "Um encerramento pode ter no máximo três meses.", "A closure can be at most three months.")));
            }
            let (booked,): (i64,) = sqlx::query_as("SELECT count(*) FROM appointments WHERE kind <> 'block' AND status = 'booked' AND (starts_at AT TIME ZONE $3)::date BETWEEN $1::date AND $2::date")
                .bind(&from).bind(&to).bind(super::schedule::tz()).fetch_one(&s.pool).await?;
            let label: String = st("label").chars().take(80).collect();
            let range = if from == to { date_label(&from) } else { format!("{} – {}", date_label(&from), date_label(&to)) };
            let mut warnings = vec![say(en, "Fica bloqueado na agenda durante o horário de funcionamento.", "It’s blocked in the schedule during opening hours.")];
            if booked > 0 {
                warnings.push(if en { format!("{booked} appointment{} already booked then stay booked: reschedule them in Schedule.", if booked == 1 { " is" } else { "s are" }) }
                              else { format!("{booked} marcaç{} já marcada{} nesses dias continua{}: remarque-a{} na Agenda.", if booked == 1 { "ão" } else { "ões" }, if booked == 1 { "" } else { "s" }, if booked == 1 { "" } else { "m" }, if booked == 1 { "" } else { "s" }) });
            }
            Ok(Proposal { skill: "closure", target: None, params: json!({ "from": from, "to": to, "label": label }), before: Value::Null, after: Value::Null,
                rows: vec![[range, say(en, "Aberta", "Open"), if label.is_empty() { say(en, "Fechada", "Closed") } else { format!("{} — {label}", say(en, "Fechada", "Closed")) }]], warnings })
        }
        "staff_pay" | "staff_active" => {
            let who = match find_staff(s, &st("staff")).await? {
                Ok(x) => x,
                Err(n) => return Ok(Err(if n > 1 { say(en, "Há mais do que uma pessoa com esse nome. Diga o nome completo.", "More than one person has that name. Use their full name.") }
                                        else { say(en, "Não encontrei essa pessoa na equipa.", "I couldn’t find that person in the team.") })),
            };
            let (id, name, role, active) = who;
            if skill == "staff_pay" {
                let Some(new) = it["amount_eur"].as_f64().filter(|x| x.is_finite() && *x >= 0.0 && *x <= 100_000.0).map(|x| (x * 100.0).round() as i64) else {
                    return Ok(Err(say(en, "Qual é o novo valor mensal bruto?", "What’s the new gross monthly amount?")));
                };
                let cur: Option<(i32,)> = sqlx::query_as("SELECT fixed_cents FROM staff_pay WHERE staff_id = $1").bind(id).fetch_optional(&s.pool).await?;
                let cur = cur.map(|c| c.0 as i64).unwrap_or(0);
                if cur == new {
                    return Ok(Err(say(en, "O valor já é esse.", "That’s already the amount.")));
                }
                Ok(Proposal { skill: "staff_pay", target: Some(id), params: json!({}), before: json!({ "fixed_cents": cur }), after: json!({ "fixed_cents": new }),
                    rows: vec![[format!("{name} — {}", say(en, "ordenado fixo (bruto)", "fixed pay (gross)")), eur(cur, en), eur(new, en)]],
                    warnings: vec![say(en, "Conta a partir do próximo salário registado em Finanças › Salários.", "Counts from the next salary recorded in Finance › Salaries.")] })
            } else {
                let Some(want) = it["active"].as_bool() else { return Ok(Err(String::new())) };
                if id == a.staff.id || role == "owner" {
                    return Ok(Err(say(en, "Não posso desativar a conta da dona da clínica.", "I can’t deactivate the owner’s account.")));
                }
                if want == active {
                    return Ok(Err(if want { say(en, &format!("{name} já está ativa."), &format!("{name} is already active.")) } else { say(en, &format!("{name} já está desativada."), &format!("{name} is already deactivated.")) }));
                }
                let lbl = |on: bool| if on { say(en, "Ativa", "Active") } else { say(en, "Desativada", "Deactivated") };
                Ok(Proposal { skill: "staff_active", target: Some(id), params: json!({}), before: json!({ "active": active }), after: json!({ "active": want }),
                    rows: vec![[name, lbl(active), lbl(want)]],
                    warnings: if want { vec![] } else { vec![say(en, "Termina já a sessão em todos os computadores. Os dados e o histórico ficam.", "Signs them out of every computer now. Their data and history stay.")] } })
            }
        }
        _ => Err(String::new()),
    })
}

// ───────── apply / cancel / undo ─────────

/// The current value of what a change touches, in the same shape as its `before`/`after`.
async fn current(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, skill: &str, target: Option<Uuid>, params: &Value) -> ApiResult<Value> {
    Ok(match skill {
        "price" => { let (c,): (i32,) = sqlx::query_as("SELECT price_cents FROM products WHERE id = $1").bind(target).fetch_one(&mut **tx).await?; json!({ "price_cents": c }) }
        "product_active" => { let (b,): (bool,) = sqlx::query_as("SELECT active FROM products WHERE id = $1").bind(target).fetch_one(&mut **tx).await?; json!({ "active": b }) }
        "hours" => { let (v,): (Value,) = sqlx::query_as("SELECT opening_hours -> $1 FROM clinic WHERE id = 1").bind(params["day"].as_str().unwrap_or("")).fetch_one(&mut **tx).await?; json!({ "hours": v }) }
        "staff_pay" => { let c: Option<(i32,)> = sqlx::query_as("SELECT fixed_cents FROM staff_pay WHERE staff_id = $1").bind(target).fetch_optional(&mut **tx).await?; json!({ "fixed_cents": c.map(|x| x.0).unwrap_or(0) }) }
        "staff_active" => { let (b,): (bool,) = sqlx::query_as("SELECT active FROM staff WHERE id = $1").bind(target).fetch_one(&mut **tx).await?; json!({ "active": b }) }
        _ => Value::Null,
    })
}

async fn set(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, skill: &str, target: Option<Uuid>, params: &Value, v: &Value, by: Uuid) -> ApiResult<()> {
    match skill {
        "price" => { sqlx::query("UPDATE products SET price_cents = $2, updated_at = now(), updated_by = $3 WHERE id = $1").bind(target).bind(v["price_cents"].as_i64().unwrap_or(0) as i32).bind(by).execute(&mut **tx).await?; }
        "product_active" => { sqlx::query("UPDATE products SET active = $2, updated_at = now(), updated_by = $3 WHERE id = $1").bind(target).bind(v["active"].as_bool().unwrap_or(true)).bind(by).execute(&mut **tx).await?; }
        "hours" => { sqlx::query("UPDATE clinic SET opening_hours = jsonb_set(opening_hours, ARRAY[$1], $2) WHERE id = 1").bind(params["day"].as_str().unwrap_or("")).bind(&v["hours"]).execute(&mut **tx).await?; }
        "staff_pay" => {
            sqlx::query("INSERT INTO staff_pay (staff_id, model, fixed_cents) VALUES ($1, 'fixed', $2) ON CONFLICT (staff_id) DO UPDATE SET model = 'fixed', fixed_cents = $2, updated_at = now()")
                .bind(target).bind(v["fixed_cents"].as_i64().unwrap_or(0) as i32).execute(&mut **tx).await?;
        }
        "staff_active" => {
            let on = v["active"].as_bool().unwrap_or(true);
            sqlx::query("UPDATE staff SET active = $2 WHERE id = $1").bind(target).bind(on).execute(&mut **tx).await?;
            if !on { sqlx::query("DELETE FROM sessions WHERE staff_id = $1").bind(target).execute(&mut **tx).await?; }
        }
        _ => {}
    }
    Ok(())
}

type ChangeRow = (String, Option<Uuid>, Value, Value, Value, Value, String, bool);

async fn load(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid, a: &Auth) -> ApiResult<ChangeRow> {
    let row: Option<(String, Option<Uuid>, Value, Value, Value, Value, String, bool, Option<Uuid>)> = sqlx::query_as(
        "SELECT skill, target, params, before, after, rows, status, created_at < now() - interval '1 day', created_by FROM assistant_changes WHERE id = $1 FOR UPDATE")
        .bind(id).fetch_optional(&mut **tx).await?;
    let r = row.ok_or(AppError::NotFound)?;
    // The owner's jobs stay the owner's; a schedule change is decided by whoever asked for it, or the owner.
    if OWNER_SKILLS.contains(&r.0.as_str()) || super::agent_money::OWNER_ONLY.contains(&r.0.as_str()) || super::agent_docs::OWNER_ONLY.contains(&r.0.as_str()) {
        owner(a)?;
    } else if a.staff.role != "owner" && r.8 != Some(a.staff.id) {
        return Err(AppError::Forbidden);
    }
    Ok((r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7))
}

fn is_schedule(skill: &str) -> bool {
    SCHEDULE_SKILLS.contains(&skill)
}


fn detail(skill: &str, rows: &Value) -> String {
    let r = &rows[0];
    format!("{skill}: {} · {} → {}", r[0].as_str().unwrap_or(""), r[1].as_str().unwrap_or(""), r[2].as_str().unwrap_or("")).chars().take(200).collect()
}

/// Do what the change says. Refused if it was already decided, is more than a day old, or the thing changed meanwhile.
pub async fn apply(State(s): State<AppState>, a: Auth, headers: axum::http::HeaderMap, Path(id): Path<Uuid>) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    may_use(&a)?;
    let mut tx = s.pool.begin().await?;
    let (skill, target, params, before, after, rows, status, old) = load(&mut tx, id, &a).await?;
    if status != "pending" {
        return Err(AppError::Conflict("This change was already applied or cancelled."));
    }
    if old {
        return Err(AppError::Conflict("This proposal is more than a day old. Ask the assistant again."));
    }
    // Patient files and notes (13 Nov 2026): done through the app's own endpoints, as the person who applies it.
    if super::agent::GENERIC_SKILLS.contains(&skill.as_str()) {
        return super::agent::apply(&s, &a, &headers, tx, id, &skill, target, &params, &before, &after).await;
    }
    apply_rest(&s, &a, tx, id, skill, target, params, before, after, rows).await.map(IntoResponse::into_response)
}

#[allow(clippy::too_many_arguments)]
async fn apply_rest(s: &AppState, a: &Auth, mut tx: sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid, skill: String, target: Option<Uuid>, params: Value, before: Value, after: Value, rows: Value) -> ApiResult<Json<Value>> {
    let (s, a) = (s.clone(), a);
    if is_schedule(&skill) {
        let (keep, patient) = super::agenda::apply(&mut tx, a, &skill, target, &params, &before, &after).await?;
        let after = keep.unwrap_or(after);
        sqlx::query("UPDATE assistant_changes SET status = 'applied', after = $2, decided_at = now() WHERE id = $1").bind(id).bind(&after).execute(&mut *tx).await?;
        tx.commit().await?;
        if after["created_patient"].as_bool() == Some(true) {
            auth::log(&s.pool, Some(a.staff.id), "patient.created", patient.map(|p| ("patient", p.to_string())), &a.device, "assistant").await;
        }
        auth::log(&s.pool, Some(a.staff.id), "assistant.applied", patient.map(|p| ("patient", p.to_string())), &a.device, &super::agenda::detail(&skill, &params, &after)).await;
        return Ok(Json(json!({ "ok": true, "status": "applied", "patient_id": patient })));
    }
    let mut target = target;
    if skill == "closure" {
        tx.commit().await?;
        let (from, to, label) = (params["from"].as_str().unwrap_or(""), params["to"].as_str().unwrap_or(""), params["label"].as_str().unwrap_or(""));
        let today = money::today(&s).await?;
        if from < today.as_str() {
            return Err(AppError::Conflict("Those days have passed. Ask the assistant again."));
        }
        let (cid, _) = super::assist::make_closure(&s, a.staff.id, from, to, label).await?;
        target = Some(cid);
        sqlx::query("UPDATE assistant_changes SET status = 'applied', target = $2, decided_at = now() WHERE id = $1").bind(id).bind(target).execute(&s.pool).await?;
    } else {
        if current(&mut tx, &skill, target, &params).await? != before {
            return Err(AppError::Conflict("This was changed since the assistant suggested it. Ask the assistant again."));
        }
        set(&mut tx, &skill, target, &params, &after, a.staff.id).await?;
        sqlx::query("UPDATE assistant_changes SET status = 'applied', decided_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    auth::log(&s.pool, Some(a.staff.id), "assistant.applied", target.map(|t| (if skill.starts_with("staff") { "staff" } else { "assistant" }, t.to_string())), &a.device, &detail(&skill, &rows)).await;
    Ok(Json(json!({ "ok": true, "status": "applied" })))
}

/// Don't do it.
pub async fn cancel(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    may_use(&a)?;
    let mut tx = s.pool.begin().await?;
    let (_, _, _, _, _, _, status, _) = load(&mut tx, id, &a).await?;
    if status != "pending" {
        return Err(AppError::Conflict("This change was already applied or cancelled."));
    }
    sqlx::query("UPDATE assistant_changes SET status = 'cancelled', decided_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true, "status": "cancelled" })))
}

/// Put back what an applied change replaced — only if nothing else changed it since.
pub async fn undo(State(s): State<AppState>, a: Auth, headers: axum::http::HeaderMap, Path(id): Path<Uuid>) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    may_use(&a)?;
    let mut tx = s.pool.begin().await?;
    let (skill, target, params, before, after, rows, status, _) = load(&mut tx, id, &a).await?;
    if status != "applied" {
        return Err(AppError::Conflict("Only an applied change can be undone."));
    }
    if super::agent::GENERIC_SKILLS.contains(&skill.as_str()) {
        return super::agent::undo(&s, &a, &headers, tx, id, &skill, target, &params, &before, &after).await;
    }
    undo_rest(&s, &a, tx, id, skill, target, params, before, after, rows).await.map(IntoResponse::into_response)
}

#[allow(clippy::too_many_arguments)]
async fn undo_rest(s: &AppState, a: &Auth, mut tx: sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid, skill: String, target: Option<Uuid>, params: Value, before: Value, after: Value, rows: Value) -> ApiResult<Json<Value>> {
    if is_schedule(&skill) {
        let patient = super::agenda::undo(&mut tx, &skill, target, &params, &before, &after).await?;
        sqlx::query("UPDATE assistant_changes SET status = 'undone', decided_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        auth::log(&s.pool, Some(a.staff.id), "assistant.undone", patient.map(|p| ("patient", p.to_string())), &a.device, &super::agenda::detail(&skill, &params, &after)).await;
        return Ok(Json(json!({ "ok": true, "status": "undone" })));
    }
    if skill == "closure" {
        sqlx::query("DELETE FROM closures WHERE id = $1").bind(target).execute(&mut *tx).await?;
    } else {
        if current(&mut tx, &skill, target, &params).await? != after {
            return Err(AppError::Conflict("This was changed again since, so it can’t be undone here. Change it by hand."));
        }
        set(&mut tx, &skill, target, &params, &before, a.staff.id).await?;
    }
    sqlx::query("UPDATE assistant_changes SET status = 'undone', decided_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "assistant.undone", target.map(|t| (if skill.starts_with("staff") { "staff" } else { "assistant" }, t.to_string())), &a.device, &detail(&skill, &rows)).await;
    Ok(Json(json!({ "ok": true, "status": "undone" })))
}

#[cfg(test)]
mod tests {
    use super::hm;
    #[test]
    fn times_people_type() {
        assert_eq!(hm("9:00").as_deref(), Some("09:00"));
        assert_eq!(hm("9h").as_deref(), Some("09:00"));
        assert_eq!(hm("13h30").as_deref(), Some("13:30"));
        assert_eq!(hm("25:00"), None);
        assert_eq!(hm("x"), None);
    }
}
