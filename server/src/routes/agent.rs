//! The assistant with tools (owner, 13 Nov 2026: "I want the AI to have all the context it needs … and to be able
//! to do whatever I can in the app, only through me talking to it").
//!
//! How it works:
//! - The local AI gets a short brief (who is asking, today's date and the next two weeks, the therapists) and a
//!   set of **tools**. It looks things up as it needs them — patients, a patient's file and notes, the agenda, free
//!   times, booking requests, the team — instead of being handed everything at once (no local model can hold a
//!   whole clinic, and the old one-step assistant got confused by a long agenda: owner's chat, 13 Nov 2026).
//! - **Same as the app** (owner's answer): every look-up runs through the app's own endpoints, as the person who
//!   asked, so the server's role checks apply unchanged (reception never gets clinical fields) and every patient
//!   file opened is written to the access log like a click in the app.
//! - **Changes always wait for Apply** (owner's answer): a tool that changes something only prepares the
//!   before/after preview (`assistant_changes`). Apply runs it through the app's endpoints as the person who
//!   applies; Undo puts back what was there while nothing else changed it. The AI never signs a note.
//! - Dates are worked out by the server ("amanhã", "sexta", "14 de novembro"), never left to the model.
//! - A model that can't use tools gets the older one-step assistant (`assistant::ask_legacy`).
//! - Health data stays on the clinic's AI PC (`ai.rs` refuses any other address); nothing here is written to the
//!   server log.

use super::agenda::{self, Built, Found, Person, Seen};
use super::assist::{date_of, fold};
use super::assistant::{self, AskReq, Proposal};
use super::schedule::tz;
use super::util::clinical;
use crate::ai::{self, ChatError};
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use time::{Date, Duration as Days, Month, Weekday};
use tower::ServiceExt;
use uuid::Uuid;

pub(super) fn say(en: bool, pt: &str, e: &str) -> String {
    if en { e.to_string() } else { pt.to_string() }
}

/// Text that goes to the model: no control characters, at most `max` characters.
pub(super) fn cut(s: &str, max: usize) -> String {
    let t: String = s.chars().filter(|c| !c.is_control() || *c == '\n').collect();
    let t = t.trim();
    if t.chars().count() <= max { t.to_string() } else { format!("{}…", t.chars().take(max).collect::<String>()) }
}

// ───────── calling the app's own endpoints, as the person ─────────

/// The headers that make a request "this person on this device": the session, the device key and name, the
/// language and the real client address (`main::client_ip` works it out again from the connection info).
pub struct Caller { headers: HeaderMap, ip: Option<std::net::IpAddr> }

impl Caller {
    pub fn from(h: &HeaderMap) -> Caller {
        let mut headers = HeaderMap::new();
        for k in ["authorization", "x-device-key", "x-device", "x-device-kind", "accept-language"] {
            if let Some(v) = h.get(k) {
                headers.insert(k, v.clone());
            }
        }
        Caller { headers, ip: auth::client_ip(h).parse().ok() }
    }
}

/// One request to the app's API, inside the server, as `c`. Answers the status and the JSON body.
pub async fn call(s: &AppState, c: &Caller, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let mut req = Request::builder().method(method).uri(format!("/api{path}"));
    for (k, v) in c.headers.iter() {
        req = req.header(k, v);
    }
    let req = match body {
        Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    };
    let Ok(mut req) = req else { return (500, Value::Null) };
    let ip = c.ip.unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    req.extensions_mut().insert(axum::extract::ConnectInfo(std::net::SocketAddr::new(ip, 0)));
    let res = match crate::app(s.clone()).oneshot(req).await { Ok(r) => r, Err(e) => match e {} };
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), 8 * 1024 * 1024).await.unwrap_or_default();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

pub(super) fn ok(status: u16) -> bool {
    (200..300).contains(&status)
}

/// An answer from the app's API that wasn't a success, passed on as it is (same status, same message).
pub(super) fn passed_on(status: u16, body: Value) -> Response {
    (StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), Json(body)).into_response()
}

// ───────── dates people say ─────────

pub(crate) fn iso(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
}

pub(super) fn parse_iso(s: &str) -> Option<Date> {
    let s = s.get(..10)?;
    Date::from_calendar_date(s[..4].parse().ok()?, Month::try_from(s[5..7].parse::<u8>().ok()?).ok()?, s[8..10].parse().ok()?).ok()
}

const WEEKDAYS: &[(&str, Weekday)] = &[
    ("segunda", Weekday::Monday), ("seg", Weekday::Monday), ("monday", Weekday::Monday), ("mon", Weekday::Monday),
    ("terca", Weekday::Tuesday), ("ter", Weekday::Tuesday), ("tuesday", Weekday::Tuesday), ("tue", Weekday::Tuesday),
    ("quarta", Weekday::Wednesday), ("qua", Weekday::Wednesday), ("wednesday", Weekday::Wednesday), ("wed", Weekday::Wednesday),
    ("quinta", Weekday::Thursday), ("qui", Weekday::Thursday), ("thursday", Weekday::Thursday), ("thu", Weekday::Thursday),
    ("sexta", Weekday::Friday), ("sex", Weekday::Friday), ("friday", Weekday::Friday), ("fri", Weekday::Friday),
    ("sabado", Weekday::Saturday), ("sab", Weekday::Saturday), ("saturday", Weekday::Saturday), ("sat", Weekday::Saturday),
    ("domingo", Weekday::Sunday), ("dom", Weekday::Sunday), ("sunday", Weekday::Sunday), ("sun", Weekday::Sunday),
];

pub(super) const MONTHS: &[(&str, u8)] = &[
    ("janeiro", 1), ("january", 1), ("jan", 1), ("fevereiro", 2), ("february", 2), ("fev", 2), ("feb", 2),
    ("marco", 3), ("march", 3), ("mar", 3), ("abril", 4), ("april", 4), ("abr", 4), ("apr", 4),
    ("maio", 5), ("may", 5), ("mai", 5), ("junho", 6), ("june", 6), ("jun", 6), ("julho", 7), ("july", 7), ("jul", 7),
    ("agosto", 8), ("august", 8), ("ago", 8), ("aug", 8), ("setembro", 9), ("september", 9), ("set", 9), ("sep", 9), ("sept", 9),
    ("outubro", 10), ("october", 10), ("out", 10), ("oct", 10), ("novembro", 11), ("november", 11), ("nov", 11),
    ("dezembro", 12), ("december", 12), ("dez", 12), ("dec", 12),
];

/// A day without a year: this year's, unless that was more than two months ago (then next year's).
fn with_year(m: u8, d: u8, today: Date) -> Option<Date> {
    let this = Date::from_calendar_date(today.year(), Month::try_from(m).ok()?, d).ok()?;
    if this < today - Days::days(60) { Date::from_calendar_date(today.year() + 1, Month::try_from(m).ok()?, d).ok() } else { Some(this) }
}

/// What people (and models) write for a day → the date. "2026-11-14", "14/11/2026", "14/11", "14-11", "hoje",
/// "amanhã", "depois de amanhã", "sexta", "próxima segunda", "14 de novembro", "nov 14", "today", "tomorrow"…
/// A weekday on its own is the next one from today, today included.
pub(crate) fn resolve_date(text: &str, today: Date) -> Option<Date> {
    let raw = text.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(d) = date_of(raw).as_deref().and_then(parse_iso) {
        return Some(d);
    }
    let p: Vec<&str> = raw.split(['/', '-', '.']).map(str::trim).collect();
    if p.len() == 2 {
        if let (Ok(d), Ok(m)) = (p[0].parse::<u8>(), p[1].parse::<u8>()) {
            return with_year(m, d, today);
        }
    }
    let f = fold(raw);
    let words: Vec<&str> = f.split(' ').filter(|w| !matches!(*w, "de" | "do" | "da" | "o" | "a" | "dia" | "the" | "on" | "of" | "feira")).collect();
    let joined = words.join(" ");
    match joined.as_str() {
        "hoje" | "today" => return Some(today),
        "amanha" | "tomorrow" => return Some(today + Days::days(1)),
        "depois amanha" | "day after tomorrow" => return Some(today + Days::days(2)),
        "ontem" | "yesterday" => return Some(today - Days::days(1)),
        _ => {}
    }
    let next = words.iter().any(|w| matches!(*w, "proxima" | "proximo" | "next" | "seguinte"));
    if let Some(wd) = words.iter().find_map(|w| WEEKDAYS.iter().find(|(n, _)| n == w).map(|(_, d)| *d)) {
        let mut d = today;
        while d.weekday() != wd {
            d += Days::days(1);
        }
        if next && d == today {
            d += Days::days(7);
        }
        return Some(d);
    }
    // "14 novembro [2026]", "novembro 14", "nov 14 2026"
    let month = words.iter().find_map(|w| MONTHS.iter().find(|(n, _)| n == w).map(|(_, m)| *m))?;
    let nums: Vec<i32> = words.iter().filter_map(|w| w.parse::<i32>().ok()).collect();
    let day = nums.iter().find(|n| (1..=31).contains(*n)).copied()? as u8;
    match nums.iter().find(|n| **n >= 2000) {
        Some(y) => Date::from_calendar_date(*y, Month::try_from(month).ok()?, day).ok(),
        None => with_year(month, day, today),
    }
}

// ───────── what the assistant knows for one message ─────────

pub struct Ctx<'a> {
    pub(super) s: &'a AppState,
    pub(super) a: &'a Auth,
    pub(super) c: Caller,
    pub(super) en: bool,
    pub(super) today: Date,
    pub(super) seen: Seen,
    pub(super) ths: Vec<Person>,
}

impl Ctx<'_> {
    pub(super) fn date(&self, text: &str) -> Option<Date> {
        resolve_date(text, self.today)
    }
    pub(super) fn day_label(&self, d: Date) -> String {
        agenda::when(&iso(d), d.weekday().number_from_monday() as i32, "", self.en)
    }
}

/// Timestamps → local "YYYY-MM-DD HH:MM" in the clinic's time zone (one query for all).
pub(super) async fn locals(s: &AppState, ts: &[String]) -> ApiResult<Vec<String>> {
    if ts.is_empty() {
        return Ok(vec![]);
    }
    let v: Vec<Option<String>> = sqlx::query_scalar("SELECT to_char(NULLIF(x, '')::timestamptz AT TIME ZONE $1, 'YYYY-MM-DD HH24:MI') FROM unnest($2::text[]) WITH ORDINALITY AS u(x, i) ORDER BY i")
        .bind(tz()).bind(ts).fetch_all(&s.pool).await?;
    Ok(v.into_iter().map(Option::unwrap_or_default).collect())
}

fn role_text(role: &str, en: bool) -> &'static str {
    match (role, en) {
        ("owner", _) => "the clinic's owner (a physiotherapist): may see and do everything",
        ("therapist", _) => "a physiotherapist: sees patient files and the schedule, writes discharge-letter drafts and reimbursement packs, takes payments and sells packages at the desk; not the clinic's finances (totals, expenses, salaries), staff, settings or the complaints log",
        _ => "the receptionist: sees contacts, the schedule and booking requests, takes payments and sells packages, handles scanned prescriptions, consents and reminders; NEVER clinical information (notes, anamnesis, tests) — don't offer it",
    }
}

/// The brief: who is asking, the next two weeks (with opening hours and closures), the therapists, the rules.
async fn brief(x: &Ctx<'_>) -> ApiResult<String> {
    let (hours,): (Value,) = sqlx::query_as("SELECT opening_hours FROM clinic WHERE id = 1").fetch_one(&x.s.pool).await?;
    let closures: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT starts_on::text, ends_on::text, label FROM closures WHERE ends_on >= $1::date AND starts_on <= $1::date + 14")
        .bind(iso(x.today)).fetch_all(&x.s.pool).await.unwrap_or_default();
    let mut cal = String::new();
    for i in 0..14 {
        let d = x.today + Days::days(i);
        let k = super::assist::DAYS[(d.weekday().number_from_monday() - 1) as usize];
        let h = &hours[k];
        let ds = iso(d);
        let shut = closures.iter().find(|(a, b, _)| a.as_str() <= ds.as_str() && ds.as_str() <= b.as_str());
        let open = match (h[0].as_str(), h[1].as_str(), shut) {
            (_, _, Some((_, _, l))) => format!("closed ({})", if l.is_empty() { "closure" } else { l }),
            (Some(o), Some(c), None) => format!("open {o}–{c}"),
            _ => "closed".into(),
        };
        let tag = match i { 0 => " = today/hoje", 1 => " = tomorrow/amanhã", _ => "" };
        cal.push_str(&format!("- {ds} {} ({}){tag}: {open}\n", d.weekday(), ["segunda", "terça", "quarta", "quinta", "sexta", "sábado", "domingo"][(d.weekday().number_from_monday() - 1) as usize]));
    }
    let therapists = x.ths.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ");
    let me = &x.a.staff;
    Ok(format!(
"You are the assistant inside PhysioPulse, the app that runs a physiotherapy clinic in Portugal. You talk with {name}, {role}. \
\"I\", \"me\", \"my\" mean {name}.
Always answer in {lang}, in one to three short plain sentences (no markdown, no lists, no headings).

Calendar (use these dates; times are 24 h, Lisbon):
{cal}
Therapists who can be booked: {therapists}.

How to work:
- Look things up with the tools before answering. Never guess a name, a date, a time, a price or a reference.
- Patients are found by name or by number (nº). If several match, ask which one.
- To change something, call the tool that does it once, with everything you know. The app then shows the change and {name} presses Apply. Never say a change is done; say what you prepared.
- If something you need is missing, ask one short question instead of guessing.
- Use the references (ref) exactly as a tool showed them.
- In a booking, the patient is the person treated and the therapist is the staff member. \"Fisioterapeuta X\" or \"com o X\" is the therapist.
- If a tool says it couldn't do something, fix what it says or ask {name}.
- Only say what the tools showed you. Patient information is confidential: use it only to answer {name}.{money}{docs}",
        money = super::agent_money::brief_rules(), docs = super::agent_docs::brief_rules(), name = me.name, role = role_text(&me.role, x.en), lang = if x.en { "British English" } else { "European Portuguese (português de Portugal)" },
    ))
}

// ───────── the tools ─────────

fn tool(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({ "type": "function", "function": { "name": name, "description": desc,
        "parameters": { "type": "object", "properties": props, "required": required } } })
}

fn s_(d: &str) -> Value { json!({ "type": "string", "description": d }) }
fn i_(d: &str) -> Value { json!({ "type": "integer", "description": d }) }

/// The tools this person's assistant gets: the same reach as their role in the app.
pub fn tools(a: &Auth) -> Value {
    let date = "Day: YYYY-MM-DD from the calendar (or words like amanhã, sexta)";
    let pat = "Patient's name, or nº and number (e.g. \"nº 12\")";
    let mut t = vec![
        tool("search_patients", "Find patients by name, number, phone, e-mail or NIF. Shows nº, name, birth date, phone, therapist, last and next appointment.",
            json!({ "query": s_("What to search for") }), &["query"]),
        tool("open_patient", if clinical(a) { "Open a patient's file: contacts, anamnesis, recent progress notes, tests, prescriptions, consents and appointments." } else { "Open a patient's file (contacts, prescriptions, consents and appointments; no clinical information)." },
            if clinical(a) { json!({ "patient": s_(pat), "notes": i_("How many recent progress notes to show in full (default 3, up to 20)") }) } else { json!({ "patient": s_(pat) }) }, &["patient"]),
        tool("agenda", "The schedule between two days: each appointment with its ref, time, type, patient, therapist and status. Filter by therapist or patient if asked.",
            json!({ "from": s_(date), "to": s_("Last day (default: same as from)"), "therapist": s_("Therapist's name, if asked"), "patient": s_("Patient, if asked") }), &["from"]),
        tool("free_times", "Free times to book, from a day on (looks at 7 days).",
            json!({ "date": s_(date), "part": { "type": "string", "enum": ["morning", "afternoon", "any"] }, "kind": { "type": "string", "enum": ["session", "assessment", "home"] }, "minutes": i_("Length if said"), "therapist": s_("Therapist's name, if asked") }), &["date"]),
        tool("booking_requests", "Booking requests waiting for an answer (website, phone) and the waitlist, with refs.", json!({}), &[]),
        tool("team", "The clinic's staff: names and roles.", json!({}), &[]),
        tool("book", "Prepare a new appointment (or a weekly series). Shown to the user to apply.",
            json!({ "patient": s_(pat), "date": s_(date), "time": s_("HH:MM, 24 h"), "kind": { "type": "string", "enum": ["session", "assessment", "home"] }, "minutes": i_("Length if said"),
                    "therapist": s_("Therapist's name, if said"), "repeat_weekdays": { "type": "array", "items": { "type": "integer" }, "description": "Only for a series: ISO weekdays 1–7" },
                    "repeat_count": i_("Only for a series: how many in total"), "phone": s_("Phone, for someone new"), "address": s_("Address, for a home visit") }), &["patient", "date"]),
        tool("move_appointment", "Prepare a change of day, time or therapist for ONE appointment.",
            json!({ "ref": s_("The appointment's ref from agenda or open_patient"), "date": s_("New day, if it changes"), "time": s_("New time HH:MM, if it changes"), "therapist": s_("New therapist, if it changes") }), &["ref"]),
        tool("cancel_appointments", "Prepare cancelling appointments.", json!({ "refs": { "type": "array", "items": { "type": "string" }, "description": "Their refs" } }), &["refs"]),
        tool("mark_appointments", "Prepare marking appointments as done or missed.",
            json!({ "refs": { "type": "array", "items": { "type": "string" } }, "status": { "type": "string", "enum": ["done", "no_show"] } }), &["refs", "status"]),
        tool("answer_request", "Prepare accepting (booking) or declining a booking request.",
            json!({ "ref": s_("The request's ref from booking_requests"), "accept": { "type": "boolean" }, "date": s_("Day, if the user chose one"), "time": s_("HH:MM, if the user chose one"), "therapist": s_("Therapist, if said") }), &["ref", "accept"]),
        tool("waitlist_add", "Prepare adding a patient to the waitlist for an earlier slot.",
            json!({ "patient": s_(pat), "weekdays": { "type": "array", "items": { "type": "integer" }, "description": "ISO weekdays 1–7 that suit them" }, "part": { "type": "string", "enum": ["morning", "afternoon", "any"] }, "therapist": s_("Therapist, if said") }), &["patient"]),
        tool("new_patient", "Prepare a new patient file.",
            json!({ "name": s_("Full name"), "phone": s_(""), "email": s_(""), "birth_date": s_("YYYY-MM-DD"), "nif": s_(""), "address": s_("") }), &["name"]),
        tool("update_patient", "Prepare changes to a patient's details. Only the fields that change.",
            json!({ "patient": s_(pat), "name": s_(""), "phone": s_(""), "email": s_(""), "address": s_(""), "birth_date": s_("YYYY-MM-DD"), "nif": s_(""), "sns_number": s_(""),
                    "therapist": s_("Their therapist's name"), "admin_note": s_("The note for the front desk (full new text)"), "active": { "type": "boolean", "description": "false = file archived" } }), &["patient"]),
    ];
    if clinical(a) {
        t.push(tool("update_anamnesis", "Prepare changes to a patient's anamnesis. For each field that changes, give its full new text (keep what's there unless asked to remove it).",
            json!({ "patient": s_(pat), "main_complaint": s_(""), "history": s_(""), "medication": s_(""), "medical_history": s_(""), "goals": s_("") }), &["patient"]));
        t.push(tool("write_note", "Prepare a progress note draft (plain flowing text, no titles or lists). It is saved unsigned: the therapist reads and signs it in the app.",
            json!({ "patient": s_(pat), "text": s_("The note"), "appointment": s_("The session's ref, if it's about one") }), &["patient", "text"]));
    }
    if a.staff.role == "owner" {
        t.push(tool("price", "Prepare a change to the price list: a product's price, or taking it off/on sale.",
            json!({ "product": s_("Product name as in the price list"), "price_eur": { "type": "number" }, "on_sale": { "type": "boolean" } }), &["product"]));
        t.push(tool("opening_hours", "Prepare new opening hours for one weekday.",
            json!({ "day": { "type": "string", "enum": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] }, "open": s_("HH:MM"), "close": s_("HH:MM"), "closed": { "type": "boolean" } }), &["day"]));
        t.push(tool("close_clinic", "Prepare closing the clinic on one or more days (holiday, training…).",
            json!({ "from": s_(date), "to": s_("Last day (default: same)"), "reason": s_("Short reason in Portuguese") }), &["from"]));
        t.push(tool("staff_change", "Prepare a change for a staff member: fixed monthly gross pay, or deactivating/reactivating them.",
            json!({ "name": s_("Their name"), "monthly_pay_eur": { "type": "number" }, "active": { "type": "boolean" } }), &["name"]));
        t.push(tool("month_summary", "The figures of one month with a short summary.", json!({ "month": s_("YYYY-MM") }), &["month"]));
    }
    super::agent_money::tools(a, &mut t);
    super::agent_docs::tools(a, &mut t);
    Value::Array(t)
}

/// What a tool gave back.
pub(super) enum Out {
    /// Something looked up, for the model: the text, a line for the person ("Abriu a ficha de …"), maybe a list to show.
    Read { text: String, step: String, list: Option<Vec<Value>> },
    /// It couldn't be done as asked: the model gets the reason and can fix it or ask.
    Retry(String),
    /// A question back to the person, with things to tap (which Maria? which time?).
    Ask(agenda::Said),
    /// A change to show before/after.
    Change(Proposal),
    /// A finished answer (the month summary).
    Done(Json<Value>),
}

pub(super) fn st(v: &Value, k: &str) -> String {
    match &v[k] {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// The patient someone means, or why not.
pub(super) async fn which_patient(x: &Ctx<'_>, text: &str) -> ApiResult<Result<agenda::Patient, Out>> {
    if text.trim().is_empty() {
        return Ok(Err(Out::Retry("Say which patient (name or nº).".into())));
    }
    Ok(match agenda::find_patient(x.s, text).await? {
        Found::One(p) => Ok(p),
        Found::Many(m) => Err(Out::Ask(match agenda::many_patients(&m, x.en) { Built::Said(s) => s, Built::Change(_) => unreachable!() })),
        Found::None => Err(Out::Retry(format!("No patient matches \"{}\". Try search_patients with part of the name, or ask the user.", cut(text, 60)))),
    })
}

/// A tool call → what it gave. `args` come from the model: everything is checked again here and by the app.
async fn run(x: &Ctx<'_>, name: &str, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    Ok(match name {
        "search_patients" => {
            let q = st(args, "query");
            if q.is_empty() {
                return Ok(Out::Retry("Give something to search for.".into()));
            }
            let path = format!("/patients?include_inactive=true&limit=10&q={}", urlencode(&q));
            let (status, v) = call(x.s, &x.c, "GET", &path, None).await;
            if !ok(status) {
                return Ok(Out::Retry(st(&v, "message")));
            }
            let rows = v.as_array().cloned().unwrap_or_default();
            let ts: Vec<String> = rows.iter().flat_map(|p| [st(p, "next_at"), st(p, "last_at")]).collect();
            let loc = locals(x.s, &ts).await?;
            let mut text = String::new();
            for (i, p) in rows.iter().enumerate() {
                text.push_str(&format!("nº {} · {} · born {} · phone {} · therapist {} · last appointment {} · next {}{}\n",
                    st(p, "number"), st(p, "name"), or_dash(&st(p, "birth_date")), or_dash(&st(p, "phone")), or_dash(&st(p, "therapist_name")),
                    or_dash(&loc[i * 2 + 1]), or_dash(&loc[i * 2]), if p["active"] == json!(false) { " · archived" } else { "" }));
            }
            if rows.is_empty() {
                text = format!("No patient matches \"{q}\".");
            }
            Out::Read { text, step: if en { format!("Searched patients: “{}”", cut(&q, 40)) } else { format!("Procurou utentes: «{}»", cut(&q, 40)) }, list: None }
        }
        "open_patient" => {
            let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
            let (status, v) = call(x.s, &x.c, "GET", &format!("/patients/{}", p.id), None).await;
            if !ok(status) {
                return Ok(Out::Retry(st(&v, "message")));
            }
            let notes = args["notes"].as_i64().unwrap_or(3).clamp(1, 20) as usize;
            let text = file_text(x, &p, &v, notes).await?;
            Out::Read { text, step: if en { format!("Opened {}’s file", p.label()) } else { format!("Abriu a ficha de {}", p.label()) }, list: None }
        }
        "agenda" => {
            let Some(from) = x.date(&st(args, "from")) else { return Ok(Out::Retry("Give the first day as YYYY-MM-DD.".into())) };
            let to = x.date(&st(args, "to")).filter(|t| *t >= from).unwrap_or(from).min(from + Days::days(31));
            let th = match st(args, "therapist") {
                t if t.is_empty() => None,
                t => match agenda::find_therapist(&x.ths, &t) { Ok(p) => Some(p.id), Err(_) => return Ok(Out::Retry(format!("No therapist called \"{}\". Therapists: {}.", cut(&t, 40), x.ths.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")))) },
            };
            let pid = match st(args, "patient") {
                t if t.is_empty() => None,
                t => match which_patient(x, &t).await? { Ok(p) => Some(p.id), Err(o) => return Ok(o) },
            };
            let (f, t) = (iso(from), iso(to));
            let picked: Vec<&agenda::Appt> = x.seen.appts.iter()
                .filter(|a| a.day >= f && a.day <= t && th.is_none_or(|id| a.therapist_id == Some(id)) && pid.is_none_or(|id| a.patient_id == Some(id)))
                .take(80).collect();
            let mut text = picked.iter().map(|a| appt_line(a)).collect::<Vec<_>>().join("\n");
            if picked.is_empty() {
                text = "Nothing booked.".into();
            }
            let range = if from == to { x.day_label(from) } else { format!("{} – {}", x.day_label(from), x.day_label(to)) };
            Out::Read { text: format!("Agenda {f} to {t}:\n{text}"), step: if en { format!("Looked at the schedule: {range}") } else { format!("Viu a agenda: {range}") },
                list: (!picked.is_empty()).then(|| picked.iter().take(30).map(|a| a.item(en)).collect()) }
        }
        "free_times" => {
            let Some(day) = x.date(&st(args, "date")) else { return Ok(Out::Retry("Give the day as YYYY-MM-DD.".into())) };
            let it = json!({ "date": iso(day), "part": st(args, "part"), "kind": st(args, "kind"), "minutes": args["minutes"], "therapist": st(args, "therapist") });
            match agenda::build(x.s, x.a, "free_slots", &it, &x.seen, en, None).await? {
                Built::Said(s) => {
                    let lines = s.list.as_ref().map(|l| l.iter().map(|i| format!("{} · {}", st(i, "when"), st(i, "title"))).collect::<Vec<_>>().join("\n")).unwrap_or_default();
                    Out::Read { text: format!("{}\n{lines}", s.text), step: if en { format!("Looked for free times from {}", x.day_label(day)) } else { format!("Procurou horas livres a partir de {}", x.day_label(day)) }, list: s.list }
                }
                Built::Change(_) => Out::Retry(String::new()),
            }
        }
        "booking_requests" => {
            let mut text = String::from("Booking requests waiting:\n");
            for r in &x.seen.reqs {
                text.push_str(&format!("ref {} · {} · wants {} · {} · from {}{}{}\n", agenda::short_ref(r.id), r.name, agenda::service_kind(&r.service), r.asked(true), r.source,
                    if r.patient_id.is_some() { " · has a file" } else { " · no file yet" }, if r.message.trim().is_empty() { String::new() } else { format!(" · says: {}", cut(&r.message, 200)) }));
            }
            if x.seen.reqs.is_empty() {
                text.push_str("(none)\n");
            }
            let (status, w) = call(x.s, &x.c, "GET", "/waitlist", None).await;
            if ok(status) {
                text.push_str("Waitlist:\n");
                let wd = ["", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
                for e in w.as_array().cloned().unwrap_or_default() {
                    let days: Vec<&str> = e["weekdays"].as_array().map(|a| a.iter().filter_map(|d| d.as_u64()).filter(|d| (1..=7).contains(d)).map(|d| wd[d as usize]).collect()).unwrap_or_default();
                    text.push_str(&format!("{} · {} · {}{}{}\n", st(&e, "name"), if days.is_empty() { "any day".into() } else { days.join(", ") },
                        match (st(&e, "from_time"), st(&e, "to_time")) { (a, b) if !a.is_empty() => format!("{}–{}", &a[..a.len().min(5)], &b[..b.len().min(5)]), _ => "any time".into() },
                        if st(&e, "therapist_name").is_empty() { String::new() } else { format!(" · {}", st(&e, "therapist_name")) }, if st(&e, "status") == "offered" { " · offered a slot" } else { "" }));
                }
            }
            Out::Read { text, step: say(en, "Viu os pedidos de marcação e a lista de espera", "Looked at booking requests and the waitlist"), list: None }
        }
        "team" => {
            let rows: Vec<(String, String, bool)> = sqlx::query_as("SELECT name, role, active FROM staff WHERE active OR $1 ORDER BY active DESC, name")
                .bind(x.a.staff.role == "owner").fetch_all(&x.s.pool).await?;
            let text = rows.iter().map(|(n, r, a)| format!("{n} · {r}{}", if *a { "" } else { " · inactive" })).collect::<Vec<_>>().join("\n");
            Out::Read { text, step: say(en, "Viu a equipa", "Looked at the team"), list: None }
        }
        "book" | "move_appointment" | "cancel_appointments" | "mark_appointments" | "answer_request" | "waitlist_add" => schedule_change(x, name, args).await?,
        "new_patient" => new_patient(x, args).await?,
        "update_patient" => update_patient(x, args).await?,
        "update_anamnesis" if clinical(x.a) => update_anamnesis(x, args).await?,
        "write_note" if clinical(x.a) => write_note(x, args).await?,
        "price" | "opening_hours" | "close_clinic" | "staff_change" if x.a.staff.role == "owner" => owner_change(x, name, args).await?,
        "month_summary" if x.a.staff.role == "owner" => Out::Done(assistant::report(x.s, &st(args, "month"), en).await?),
        n if super::agent_money::handles(n, x.a) => super::agent_money::run(x, n, args).await?,
        n if super::agent_docs::handles(n, x.a) => super::agent_docs::run(x, n, args).await?,
        _ => Out::Retry(format!("There's no tool called \"{}\" for this user.", cut(name, 40))),
    })
}

pub(super) fn or_dash(s: &str) -> String {
    if s.trim().is_empty() { "—".into() } else { s.to_string() }
}

pub(super) fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// One appointment for the model: "ref 3f9c2a1b · 2026-11-14 Sat 10:00–10:45 · session · Maria Silva (nº 2) · Ana Rocha · booked".
fn appt_line(a: &agenda::Appt) -> String {
    format!("ref {} · {} {} {}–{} · {} · {} · {} · {}{}", agenda::short_ref(a.id), a.day, agenda::WD_EN[((a.dow - 1).clamp(0, 6)) as usize], a.start, a.end, a.kind,
        a.who(true), if a.therapist.is_empty() { "whole clinic" } else { &a.therapist }, a.status, if a.series { " · part of a series" } else { "" })
}

/// A patient file as short text for the model. Only what the app gave this person (reception: no clinical keys).
async fn file_text(x: &Ctx<'_>, p: &agenda::Patient, v: &Value, notes: usize) -> ApiResult<String> {
    let pt = &v["patient"];
    let mut out = format!("Patient nº {} · {}", p.number, st(pt, "name"));
    for (k, l) in [("birth_date", "born"), ("sex", "sex"), ("phone", "phone"), ("email", "e-mail"), ("address", "address"), ("nif", "NIF"), ("sns_number", "SNS"),
                   ("guardian_name", "guardian"), ("guardian_phone", "guardian's phone"), ("therapist_name", "therapist"), ("lang", "language")] {
        let val = st(pt, k);
        if !val.is_empty() {
            out.push_str(&format!(" · {l} {val}"));
        }
    }
    if pt["active"] == json!(false) {
        out.push_str(" · file archived");
    }
    let note = st(pt, "admin_note");
    if !note.is_empty() {
        out.push_str(&format!("\nFront-desk note: {}", cut(&note, 400)));
    }
    if let Some(an) = v.get("anamnesis").filter(|a| a.is_object()) {
        out.push_str(&format!("\nAnamnesis (updated {} by {}):", st(an, "updated_at").get(..10).unwrap_or(""), or_dash(&st(an, "updated_by_name"))));
        for (k, l) in [("main_complaint", "main complaint"), ("history", "history"), ("medication", "medication"), ("medical_history", "medical history"), ("goals", "goals")] {
            let val = st(an, k);
            if !val.is_empty() {
                out.push_str(&format!("\n- {l}: {}", cut(&val, 800)));
            }
        }
        for r in an["regions"].as_array().cloned().unwrap_or_default() {
            let sym: Vec<String> = r["symptoms"].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default();
            out.push_str(&format!("\n- region {}{}: {}{}", st(&r, "region"), if st(&r, "side").is_empty() { String::new() } else { format!(" ({})", st(&r, "side")) },
                sym.join(", "), if st(&r, "notes").is_empty() { String::new() } else { format!(" — {}", cut(&st(&r, "notes"), 300)) }));
        }
        let flags: Vec<String> = an["red_flags"].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default();
        if !flags.is_empty() {
            out.push_str(&format!("\n- red flags: {}", flags.join(", ")));
        }
    } else if v.get("notes").is_some() {
        out.push_str("\nAnamnesis: not filled in yet.");
    }
    if let Some(ns) = v["notes"].as_array() {
        out.push_str(&format!("\nProgress notes: {} in total, newest first{}", ns.len(), if ns.len() > notes { format!(" (showing {notes})") } else { String::new() }));
        for n in ns.iter().take(notes) {
            let facts = super::assist::note_facts(&n["blocks"], &st(n, "text"));
            out.push_str(&format!("\n- {} · {} · {}{}: {}", st(n, "created_at").get(..10).unwrap_or(""), or_dash(&st(n, "created_by_name")),
                if st(n, "signed_at").is_empty() { "draft (unsigned)".to_string() } else { format!("signed by {}", st(n, "signed_by_name")) },
                if n["ai_assisted"] == json!(true) { " · AI-assisted" } else { "" }, cut(&facts.replace('\n', " "), 1500)));
        }
    }
    if let Some(ts) = v["tests"].as_array().filter(|t| !t.is_empty()) {
        out.push_str("\nTests and questionnaires:");
        for t in ts {
            out.push_str(&format!("\n- {} {}: {}{}{}", st(t, "test_code"), st(t, "taken_on"), st(t, "score"),
                if st(t, "max_score").is_empty() { String::new() } else { format!("/{}", st(t, "max_score")) }, if t["higher_is_worse"] == json!(true) { " (higher is worse)" } else { "" }));
        }
    }
    if let Some(ps) = v["prescriptions"].as_array().filter(|t| !t.is_empty()) {
        out.push_str("\nPrescriptions:");
        for pr in ps {
            out.push_str(&format!("\n- {} · {} {} · {} sessions, {} done, {} booked, {} left{}{}", st(pr, "issued_on"), or_dash(&st(pr, "doctor")), st(pr, "specialty"),
                st(pr, "sessions_total"), st(pr, "used"), st(pr, "booked"), st(pr, "remaining"),
                if st(pr, "valid_until").is_empty() { String::new() } else { format!(" · valid until {}", st(pr, "valid_until")) },
                if st(pr, "payer").is_empty() { String::new() } else { format!(" · {}", st(pr, "payer")) }));
        }
    }
    if let Some(cs) = v["consents"].as_array().filter(|t| !t.is_empty()) {
        out.push_str("\nConsents: ");
        out.push_str(&cs.iter().map(|c| format!("{} ({}, {})", st(c, "title"), st(c, "state"), st(c, "signed_at").get(..10).unwrap_or(""))).collect::<Vec<_>>().join("; "));
    }
    let today = iso(x.today);
    let mine: Vec<&agenda::Appt> = x.seen.appts.iter().filter(|a| a.patient_id == Some(p.id)).collect();
    let next: Vec<String> = mine.iter().filter(|a| a.day >= today && a.status == "booked").take(6).map(|a| appt_line(a)).collect();
    let past: Vec<String> = mine.iter().rev().filter(|a| a.day <= today && a.status != "booked").take(6).map(|a| appt_line(a)).collect();
    out.push_str(&format!("\nNext appointments:\n{}", if next.is_empty() { "(none)".into() } else { next.join("\n") }));
    out.push_str(&format!("\nRecent appointments:\n{}", if past.is_empty() { "(none in the last 60 days)".into() } else { past.join("\n") }));
    if let Some(ds) = v["documents"].as_array().filter(|d| !d.is_empty()) {
        out.push_str(&format!("\nDocuments: {}", ds.iter().take(15).map(|d| format!("{} ({})", st(d, "filename"), st(d, "kind"))).collect::<Vec<_>>().join(", ")));
    }
    Ok(out)
}

// ───────── changes ─────────

/// The schedule jobs, through the checks `agenda.rs` already does (clashes, opening hours, which one…).
async fn schedule_change(x: &Ctx<'_>, name: &str, args: &Value) -> ApiResult<Out> {
    let date = |k: &str| x.date(&st(args, k)).map(iso).unwrap_or_default();
    let (skill, it) = match name {
        "book" => {
            let d = st(args, "date");
            let day = x.date(&d);
            if !d.is_empty() && day.is_none() {
                return Ok(Out::Retry(format!("I can't read the day \"{}\". Use YYYY-MM-DD from the calendar.", cut(&d, 30))));
            }
            if day.is_some_and(|d| d < x.today) {
                return Ok(Out::Retry(format!("{} has passed (today is {}). Ask the user which day.", iso(day.unwrap()), iso(x.today))));
            }
            ("book", json!({ "patient": st(args, "patient"), "date": day.map(iso).unwrap_or_default(), "time": st(args, "time"), "kind": st(args, "kind"),
                "minutes": args["minutes"], "therapist": st(args, "therapist"), "repeat_weekdays": args["repeat_weekdays"], "repeat_count": args["repeat_count"],
                "phone": st(args, "phone"), "address": st(args, "address") }))
        }
        "move_appointment" => {
            if x.seen.appt(&st(args, "ref")).is_none() {
                return Ok(Out::Retry("That ref isn't an appointment. Look it up with agenda or open_patient and use its ref.".into()));
            }
            ("move", json!({ "appointment": st(args, "ref"), "date": date("date"), "time": st(args, "time"), "therapist": st(args, "therapist") }))
        }
        "cancel_appointments" | "mark_appointments" => {
            let refs: Vec<String> = args["refs"].as_array().map(|a| a.iter().filter_map(|r| r.as_str().map(String::from)).collect()).unwrap_or_default();
            if refs.is_empty() || refs.iter().any(|r| x.seen.appt(r).is_none()) {
                return Ok(Out::Retry("Use the refs of the appointments as agenda or open_patient showed them.".into()));
            }
            if name == "cancel_appointments" { ("cancel", json!({ "refs": refs })) } else { ("mark", json!({ "refs": refs, "status": st(args, "status") })) }
        }
        "answer_request" => {
            if x.seen.req(&st(args, "ref")).is_none() {
                return Ok(Out::Retry("That ref isn't a waiting request. Look at booking_requests and use its ref.".into()));
            }
            let accept = args["accept"].as_bool().unwrap_or(true);
            (if accept { "accept_request" } else { "decline_request" }, json!({ "request": st(args, "ref"), "date": date("date"), "time": st(args, "time"), "therapist": st(args, "therapist") }))
        }
        _ => ("waitlist_add", json!({ "patient": st(args, "patient"), "repeat_weekdays": args["weekdays"], "part": st(args, "part"), "therapist": st(args, "therapist") })),
    };
    Ok(match agenda::build(x.s, x.a, skill, &it, &x.seen, x.en, None).await? {
        Built::Change(p) => Out::Change(p),
        Built::Said(s) if s.choices.is_some() || s.list.is_some() => Out::Ask(s),
        Built::Said(s) if s.text.is_empty() => Out::Retry("Something needed is missing. Ask the user.".into()),
        Built::Said(s) => Out::Retry(s.text),
    })
}

/// Prices, hours, closures and staff: the owner's jobs, through `assistant::build`.
async fn owner_change(x: &Ctx<'_>, name: &str, args: &Value) -> ApiResult<Out> {
    let (skill, it) = match name {
        "price" => match args["on_sale"].as_bool() {
            Some(on) if args["price_eur"].as_f64().is_none() => ("product_active", json!({ "product": st(args, "product"), "active": on })),
            _ => ("price", json!({ "product": st(args, "product"), "price_eur": args["price_eur"] })),
        },
        "opening_hours" => ("hours", json!({ "day": st(args, "day"), "open": st(args, "open"), "close": st(args, "close"), "closed": args["closed"] })),
        "close_clinic" => {
            let from = x.date(&st(args, "from")).map(iso).unwrap_or_default();
            let to = x.date(&st(args, "to")).map(iso).unwrap_or(from.clone());
            ("closure", json!({ "from": from, "to": to, "label": st(args, "reason") }))
        }
        _ => match args["active"].as_bool() {
            Some(on) if args["monthly_pay_eur"].as_f64().is_none() => ("staff_active", json!({ "staff": st(args, "name"), "active": on })),
            _ => ("staff_pay", json!({ "staff": st(args, "name"), "amount_eur": args["monthly_pay_eur"] })),
        },
    };
    Ok(match assistant::build(x.s, x.a, skill, &it, x.en).await? {
        Ok(p) => Out::Change(p),
        Err(t) if t.is_empty() => Out::Retry("Something needed is missing. Ask the user.".into()),
        Err(t) => Out::Retry(t),
    })
}

/// The changes `agent.rs` applies itself, through the app's endpoints.
pub const GENERIC_SKILLS: &[&str] = &["patient_create", "patient_update", "anamnesis", "note_draft",
    "payment", "payment_void", "invoice_number", "plan_sell", "plan_status", "expense", "salary", "waive", "month_close", "month_reopen",
    "document_update", "letter_draft", "letter_update", "pack", "consent_revoke", "reminder_mark",
    "complaint_create", "complaint_update", "clinic_details", "reminder_text", "staff_ai", "website"];

const PATIENT_EDIT: &[(&str, &str, &str)] = &[
    ("name", "Nome", "Name"), ("phone", "Telemóvel", "Phone"), ("email", "Email", "Email"), ("address", "Morada", "Address"),
    ("birth_date", "Data de nascimento", "Date of Birth"), ("nif", "NIF", "NIF"), ("sns_number", "Nº SNS", "SNS Number"),
    ("therapist_id", "Fisioterapeuta", "Therapist"), ("admin_note", "Nota da receção", "Front-Desk Note"), ("active", "Ficha ativa", "File Active"),
];

const ANAMNESIS_EDIT: &[(&str, &str, &str)] = &[
    ("main_complaint", "Queixa principal", "Main Complaint"), ("history", "História", "History"), ("medication", "Medicação", "Medication"),
    ("medical_history", "Antecedentes", "Medical History"), ("goals", "Objetivos", "Goals"),
];

fn short(v: &str) -> String {
    if v.trim().is_empty() { "—".into() } else { cut(v, 120) }
}

async fn new_patient(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let name = cut(&st(args, "name"), 120);
    if name.split_whitespace().count() < 2 {
        return Ok(Out::Retry("Ask the user for the person's full name (first and last name).".into()));
    }
    let mut body = Map::new();
    body.insert("name".into(), json!(name));
    for k in ["phone", "email", "nif", "address"] {
        let v = cut(&st(args, k), 200);
        if !v.is_empty() { body.insert(k.into(), json!(v)); }
    }
    if let Some(d) = x.date(&st(args, "birth_date")) { body.insert("birth_date".into(), json!(iso(d))); }
    // Someone who may already have a file (same NIF, phone, or name and birth date).
    let q = format!("/patients/duplicates?name={}&phone={}&nif={}&birth_date={}", urlencode(&name), urlencode(&st(&Value::Object(body.clone()), "phone")),
        urlencode(&st(&Value::Object(body.clone()), "nif")), urlencode(&st(&Value::Object(body.clone()), "birth_date")));
    let (_, dups) = call(x.s, &x.c, "GET", &q, None).await;
    let mut warnings = Vec::new();
    if let Some(d) = dups.as_array().and_then(|a| a.first()) {
        warnings.push(if en { format!("Looks like {} (no. {}), who already has a file.", st(d, "name"), st(d, "number")) } else { format!("Parece ser {} (nº {}), que já tem ficha.", st(d, "name"), st(d, "number")) });
    }
    let after: Vec<String> = body.iter().filter(|(k, _)| *k != "name").map(|(_, v)| v.as_str().unwrap_or("").to_string()).collect();
    Ok(Out::Change(Proposal { skill: "patient_create", target: None, params: json!({ "body": body }), before: Value::Null, after: Value::Null,
        rows: vec![[name, "—".into(), if after.is_empty() { say(en, "Ficha nova", "New file") } else { format!("{} · {}", say(en, "Ficha nova", "New file"), after.join(" · ")) }]], warnings }))
}

async fn update_patient(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
    let (cur,): (Value,) = sqlx::query_as("SELECT to_jsonb(p) FROM patients p WHERE id = $1").bind(p.id).fetch_one(&x.s.pool).await?;
    let mut before = Map::new();
    let mut after = Map::new();
    let mut rows = Vec::new();
    for (k, pt, e) in PATIENT_EDIT {
        let new: Value = match *k {
            "therapist_id" => match st(args, "therapist") {
                t if t.is_empty() => continue,
                t => match agenda::find_therapist(&x.ths, &t) { Ok(th) => json!(th.id.to_string()), Err(_) => return Ok(Out::Retry(format!("No therapist called \"{}\".", cut(&t, 40)))) },
            },
            "active" => match args["active"].as_bool() { Some(b) => json!(b), None => continue },
            "birth_date" => match st(args, "birth_date") { t if t.is_empty() => continue, t => match x.date(&t) { Some(d) => json!(iso(d)), None => return Ok(Out::Retry("Give the birth date as YYYY-MM-DD.".into())) } },
            _ => match args.get(*k) { Some(Value::String(v)) => json!(cut(v, if *k == "admin_note" { 2000 } else { 200 })), _ => continue },
        };
        let old = match &cur[*k] { Value::Null => json!(""), v => v.clone() };
        let old = if *k == "active" { old } else { json!(st(&json!({ "v": old }), "v")) };
        if old == new {
            continue;
        }
        let label = |v: &Value| match *k {
            "therapist_id" => x.ths.iter().find(|t| Some(t.id.to_string()) == v.as_str().map(String::from)).map(|t| t.name.clone()).unwrap_or_else(|| short(v.as_str().unwrap_or(""))),
            "active" => if v.as_bool() == Some(false) { say(en, "Arquivada", "Archived") } else { say(en, "Ativa", "Active") },
            _ => short(v.as_str().unwrap_or("")),
        };
        rows.push([format!("{} — {}", p.label(), if en { *e } else { *pt }), label(&old), label(&new)]);
        before.insert(k.to_string(), old);
        after.insert(k.to_string(), new);
    }
    if after.is_empty() {
        return Ok(Out::Retry("Nothing would change: the file already has those details, or no field to change was given.".into()));
    }
    Ok(Out::Change(Proposal { skill: "patient_update", target: Some(p.id), params: json!({ "patient_id": p.id }), before: Value::Object(before), after: Value::Object(after), rows, warnings: vec![] }))
}

async fn update_anamnesis(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
    let cur: Option<(Value,)> = sqlx::query_as("SELECT to_jsonb(a) FROM anamnesis a WHERE patient_id = $1").bind(p.id).fetch_optional(&x.s.pool).await?;
    let cur = cur.map(|c| c.0).unwrap_or(json!({}));
    let (mut before, mut after, mut rows) = (Map::new(), Map::new(), Vec::new());
    for (k, pt, e) in ANAMNESIS_EDIT {
        let Some(Value::String(new)) = args.get(*k) else { continue };
        let new = cut(new, 4000);
        let old = st(&cur, k);
        if old == new {
            continue;
        }
        rows.push([format!("{} — {}", p.label(), if en { *e } else { *pt }), short(&old), short(&new)]);
        before.insert(k.to_string(), json!(old));
        after.insert(k.to_string(), json!(new));
    }
    if after.is_empty() {
        return Ok(Out::Retry("Nothing would change in the anamnesis.".into()));
    }
    Ok(Out::Change(Proposal { skill: "anamnesis", target: Some(p.id), params: json!({ "patient_id": p.id }), before: Value::Object(before), after: Value::Object(after), rows,
        warnings: vec![say(en, "Leia o texto todo antes de aplicar: substitui o que lá está nesses campos.", "Read the whole text before applying: it replaces what’s in those fields.")] }))
}

async fn write_note(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
    let text = crate::style::tidy(&cut(&st(args, "text"), 6000));
    if text.len() < 10 {
        return Ok(Out::Retry("Write the note's text.".into()));
    }
    let appt = match st(args, "appointment") {
        r if r.is_empty() => None,
        r => match x.seen.appt(&r) {
            Some(a) if a.patient_id == Some(p.id) => Some(a.clone()),
            _ => return Ok(Out::Retry("That ref isn't one of this patient's appointments.".into())),
        },
    };
    let mut warnings = vec![say(en, "Fica como rascunho por assinar: leia-o e assine-o na ficha.", "It stays an unsigned draft: read it and sign it in the patient’s file.")];
    if crate::style::problem(&text).is_some() {
        warnings.push(say(en, "O texto tem títulos ou listas: reveja-o antes de assinar.", "The text has headings or lists: review it before signing."));
    }
    Ok(Out::Change(Proposal { skill: "note_draft", target: Some(p.id),
        params: json!({ "patient_id": p.id, "body": { "text": text, "template": "", "blocks": [], "ai_assisted": true, "appointment_id": appt.as_ref().map(|a| a.id) } }),
        before: Value::Null, after: Value::Null,
        rows: vec![[format!("{} — {}", p.label(), match &appt { Some(a) => a.when(en), None => say(en, "Nota de evolução", "Progress note") }), "—".into(), cut(&text, 300)]], warnings }))
}

// ───────── apply and undo (the changes made here) ─────────

pub(super) type Tx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;

async fn current(s: &AppState, skill: &str, pid: Uuid, keys: &[String]) -> ApiResult<Value> {
    let row: Option<(Value,)> = match skill {
        "patient_update" => sqlx::query_as("SELECT to_jsonb(p) FROM patients p WHERE id = $1").bind(pid).fetch_optional(&s.pool).await?,
        _ => sqlx::query_as("SELECT to_jsonb(a) FROM anamnesis a WHERE patient_id = $1").bind(pid).fetch_optional(&s.pool).await?,
    };
    let row = row.map(|r| r.0).unwrap_or(json!({}));
    let mut out = Map::new();
    for k in keys {
        let v = match &row[k.as_str()] { Value::Null => json!(""), Value::Bool(b) => json!(b), v => json!(st(&json!({ "v": v }), "v")) };
        out.insert(k.clone(), v);
    }
    Ok(Value::Object(out))
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default()
}

pub(super) async fn finish(s: &AppState, a: &Auth, mut tx: Tx<'_>, id: Uuid, status: &str, after: &Value, skill: &str, pid: Option<Uuid>) -> ApiResult<Response> {
    sqlx::query("UPDATE assistant_changes SET status = $2, after = $3, decided_at = now() WHERE id = $1").bind(id).bind(status).bind(after).execute(&mut *tx).await?;
    tx.commit().await?;
    let action = if status == "applied" { "assistant.applied" } else { "assistant.undone" };
    auth::log(&s.pool, Some(a.staff.id), action, pid.map(|p| ("patient", p.to_string())), &a.device, skill).await;
    Ok(Json(json!({ "ok": true, "status": status, "patient_id": pid })).into_response())
}

#[allow(clippy::too_many_arguments)]
pub async fn apply(s: &AppState, a: &Auth, headers: &HeaderMap, tx: Tx<'_>, id: Uuid, skill: &str, target: Option<Uuid>, params: &Value, before: &Value, after: &Value) -> ApiResult<Response> {
    if super::agent_money::SKILLS.contains(&skill) {
        return super::agent_money::apply(s, a, headers, tx, id, skill, target, params, before, after).await;
    }
    if super::agent_docs::SKILLS.contains(&skill) {
        return super::agent_docs::apply(s, a, headers, tx, id, skill, target, params, before, after).await;
    }
    let c = Caller::from(headers);
    let pid: Option<Uuid> = params["patient_id"].as_str().and_then(|p| p.parse().ok());
    match skill {
        "patient_create" => {
            let (status, v) = call(s, &c, "POST", "/patients", Some(&params["body"])).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            let new: Option<Uuid> = v["id"].as_str().and_then(|p| p.parse().ok());
            finish(s, a, tx, id, "applied", &json!({ "patient_id": new, "number": v["number"] }), skill, new).await
        }
        "patient_update" | "anamnesis" => {
            let pid = pid.ok_or(AppError::NotFound)?;
            if current(s, skill, pid, &keys(before)).await? != *before {
                return Err(AppError::Conflict("This was changed since the assistant suggested it. Ask the assistant again."));
            }
            let (m, path) = if skill == "anamnesis" { ("PUT", format!("/patients/{pid}/anamnesis")) } else { ("PATCH", format!("/patients/{pid}")) };
            let (status, v) = call(s, &c, m, &path, Some(after)).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            finish(s, a, tx, id, "applied", after, skill, Some(pid)).await
        }
        "note_draft" => {
            let pid = pid.ok_or(AppError::NotFound)?;
            let (status, v) = call(s, &c, "POST", &format!("/patients/{pid}/notes"), Some(&params["body"])).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            finish(s, a, tx, id, "applied", &json!({ "note_id": v["id"] }), skill, Some(pid)).await
        }
        _ => Err(AppError::NotFound),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn undo(s: &AppState, a: &Auth, headers: &HeaderMap, mut tx: Tx<'_>, id: Uuid, skill: &str, target: Option<Uuid>, params: &Value, before: &Value, after: &Value) -> ApiResult<Response> {
    if super::agent_money::SKILLS.contains(&skill) {
        return super::agent_money::undo(s, a, headers, tx, id, skill, target, params, before, after).await;
    }
    if super::agent_docs::SKILLS.contains(&skill) {
        return super::agent_docs::undo(s, a, headers, tx, id, skill, target, params, before, after).await;
    }
    let c = Caller::from(headers);
    let pid: Option<Uuid> = params["patient_id"].as_str().and_then(|p| p.parse().ok());
    match skill {
        "patient_create" => {
            let new: Uuid = after["patient_id"].as_str().and_then(|p| p.parse().ok()).ok_or(AppError::NotFound)?;
            if !agenda::drop_new_patient(&mut tx, new).await? {
                return Err(AppError::Conflict("This file is already in use (appointments, notes…), so it stays. Archive it in the patient’s file if needed."));
            }
            finish(s, a, tx, id, "undone", after, skill, None).await
        }
        "patient_update" | "anamnesis" => {
            let pid = pid.ok_or(AppError::NotFound)?;
            if current(s, skill, pid, &keys(after)).await? != *after {
                return Err(AppError::Conflict("This was changed again since, so it can’t be undone here. Change it by hand."));
            }
            let (m, path) = if skill == "anamnesis" { ("PUT", format!("/patients/{pid}/anamnesis")) } else { ("PATCH", format!("/patients/{pid}")) };
            let (status, v) = call(s, &c, m, &path, Some(before)).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            finish(s, a, tx, id, "undone", after, skill, Some(pid)).await
        }
        "note_draft" => {
            let note = after["note_id"].as_str().unwrap_or("");
            let (status, v) = call(s, &c, "DELETE", &format!("/notes/{note}"), None).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            finish(s, a, tx, id, "undone", after, skill, pid).await
        }
        _ => Err(AppError::NotFound),
    }
}

// ───────── one message ─────────

const MAX_ROUNDS: usize = 8;

fn memo_of(text: &str) -> String {
    cut(text, 700)
}

/// One message to the assistant: the model looks things up with the tools until it can answer, or prepares a change.
pub async fn ask(s: &AppState, a: &Auth, headers: &HeaderMap, r: AskReq) -> ApiResult<Json<Value>> {
    let en = r.lang.as_deref() == Some("en");
    let msg: String = r.message.chars().take(1000).collect();
    if msg.trim().is_empty() {
        return Err(AppError::BadRequest("Write what you’d like the assistant to do."));
    }
    let today = parse_iso(&super::money::today(s).await?).ok_or(AppError::Internal("today".into()))?;
    let x = Ctx { s, a, c: Caller::from(headers), en, today, seen: agenda::seen_wide(s).await?, ths: agenda::therapists(s).await? };
    let mut messages = vec![json!({ "role": "system", "content": brief(&x).await? })];
    let hist: Vec<&assistant::Turn> = r.history.iter().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect();
    for t in hist {
        let text: String = t.text.chars().take(600).collect();
        if t.who == "me" {
            messages.push(json!({ "role": "user", "content": text }));
        } else {
            messages.push(json!({ "role": "assistant", "content": text }));
            if !t.memo.trim().is_empty() {
                messages.push(json!({ "role": "system", "content": format!("What you looked up for that answer:\n{}", cut(&t.memo, 1500)) }));
            }
        }
    }
    messages.push(json!({ "role": "user", "content": msg }));
    let tools = tools(a);
    let model = ai::assistant_model();
    let (mut steps, mut memo, mut used): (Vec<String>, Vec<String>, Vec<String>) = (vec![], vec![], vec![]);
    let mut list: Option<Vec<Value>> = None;
    let mut fixes = 0;
    let mut last_problem = String::new();
    let done = |reply: String, change: Option<Value>, list: Option<Vec<Value>>, choices: Option<Vec<Value>>, steps: &[String], memo: &[String]| {
        Json(json!({ "reply": reply, "change": change, "rows": null, "list": list, "choices": choices, "model": model, "steps": steps, "memo": cut(&memo.join("\n"), 1500) }))
    };
    for round in 0..MAX_ROUNDS {
        let size: usize = messages.iter().map(|m| m.to_string().len()).sum::<usize>() + tools.to_string().len();
        let ctx = ((size / 3 + 1500) as u32).next_multiple_of(2048);
        let out = match ai::chat(&model, &messages, &tools, ctx).await {
            Ok(o) => o,
            Err(ChatError::NoTools) if round == 0 => {
                auth::log(&s.pool, Some(a.staff.id), "assistant.asked", None, &a.device, "one-step (model without tools)").await;
                return assistant::ask_legacy(s, a, r).await;
            }
            Err(ChatError::NoTools) => return Err(assistant::ai_error(ai::AiError::Failed)),
            Err(ChatError::Ai(e)) => {
                auth::log(&s.pool, Some(a.staff.id), "assistant.asked", None, &a.device, "failed").await;
                return Err(assistant::ai_error(e));
            }
        };
        if out.calls.is_empty() {
            let text = cut(&crate::style::tidy(&out.content), 1500);
            let text = if text.is_empty() { say(en, "Não percebi. Pode dizer de outra forma?", "I didn’t understand. Could you say it another way?") } else { text };
            auth::log(&s.pool, Some(a.staff.id), "assistant.asked", None, &a.device, &format!("answer · {}", used.join(", "))).await;
            return Ok(done(text, None, list, None, &steps, &memo));
        }
        messages.push(json!({ "role": "assistant", "content": out.content,
            "tool_calls": out.calls.iter().map(|(n, a)| json!({ "function": { "name": n, "arguments": a } })).collect::<Vec<_>>() }));
        for (name, args) in out.calls.iter().take(4) {
            used.push(cut(name, 30));
            let res = run(&x, name, args).await?;
            let said = match res {
                Out::Read { text, step, list: l } => {
                    steps.push(step);
                    memo.push(memo_of(&text));
                    if l.is_some() { list = l; }
                    cut(&text, 7000)
                }
                Out::Retry(why) => {
                    fixes += 1;
                    last_problem = why.clone();
                    if fixes > 3 {
                        auth::log(&s.pool, Some(a.staff.id), "assistant.asked", None, &a.device, &format!("gave up · {}", used.join(", "))).await;
                        return Ok(done(if why.is_empty() { say(en, "Não consegui. Pode dar mais detalhes?", "I couldn’t do that. Could you give more details?") } else { why }, None, list, None, &steps, &memo));
                    }
                    format!("Couldn't do that: {}", if why.is_empty() { "something needed is missing".into() } else { why })
                }
                Out::Ask(q) => {
                    auth::log(&s.pool, Some(a.staff.id), "assistant.asked", None, &a.device, &format!("question · {}", used.join(", "))).await;
                    return Ok(done(q.text, None, q.list.or(list), q.choices, &steps, &memo));
                }
                Out::Change(p) => {
                    let change = assistant::store(s, a, &p).await?;
                    auth::log(&s.pool, Some(a.staff.id), "assistant.asked", None, &a.device, &format!("{} · {}", p.skill, used.join(", "))).await;
                    memo.push(format!("Prepared (waiting for Apply): {}", p.rows.iter().map(|r| format!("{}: {} → {}", r[0], r[1], r[2])).collect::<Vec<_>>().join("; ")));
                    let text = cut(&crate::style::tidy(&out.content), 400);
                    let text = if text.is_empty() || text.contains('{') { say(en, "Preparei esta alteração. Veja e carregue em Aplicar.", "Here’s the change. Check it and press Apply.") } else { text };
                    return Ok(done(text, Some(change), None, None, &steps, &memo));
                }
                Out::Done(Json(mut v)) => {
                    auth::log(&s.pool, Some(a.staff.id), "assistant.asked", None, &a.device, "report").await;
                    v["steps"] = json!(steps);
                    return Ok(Json(v));
                }
            };
            messages.push(json!({ "role": "tool", "tool_name": name, "name": name, "content": said }));
        }
    }
    auth::log(&s.pool, Some(a.staff.id), "assistant.asked", None, &a.device, &format!("too long · {}", used.join(", "))).await;
    let text = if last_problem.is_empty() { say(en, "Não consegui terminar. Pode dizer de outra forma ou por partes?", "I couldn’t finish that. Could you say it another way, or in parts?") } else { last_problem };
    Ok(done(text, None, list, None, &steps, &memo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_people_say() {
        let t = Date::from_calendar_date(2026, Month::November, 13).unwrap(); // a Friday
        let r = |s: &str| resolve_date(s, t).map(iso);
        assert_eq!(r("2026-11-14").as_deref(), Some("2026-11-14"));
        assert_eq!(r("14/11/2026").as_deref(), Some("2026-11-14"));
        assert_eq!(r("13-11-2026").as_deref(), Some("2026-11-13"));
        assert_eq!(r("14/11").as_deref(), Some("2026-11-14"));
        assert_eq!(r("hoje").as_deref(), Some("2026-11-13"));
        assert_eq!(r("amanhã").as_deref(), Some("2026-11-14"));
        assert_eq!(r("Amanha").as_deref(), Some("2026-11-14"));
        assert_eq!(r("depois de amanhã").as_deref(), Some("2026-11-15"));
        assert_eq!(r("tomorrow").as_deref(), Some("2026-11-14"));
        assert_eq!(r("sexta").as_deref(), Some("2026-11-13"), "a weekday alone: today counts");
        assert_eq!(r("próxima sexta").as_deref(), Some("2026-11-20"));
        assert_eq!(r("segunda-feira").as_deref(), Some("2026-11-16"));
        assert_eq!(r("Thursday").as_deref(), Some("2026-11-19"));
        assert_eq!(r("14 de novembro").as_deref(), Some("2026-11-14"));
        assert_eq!(r("14 de novembro de 2026").as_deref(), Some("2026-11-14"));
        assert_eq!(r("nov 14").as_deref(), Some("2026-11-14"));
        assert_eq!(r("3 de janeiro").as_deref(), Some("2027-01-03"), "January is next year's");
        assert_eq!(r("1 de novembro").as_deref(), Some("2026-11-01"), "a recent day stays this year (questions about the past)");
        assert_eq!(r("31/02"), None);
        assert_eq!(r(""), None);
        assert_eq!(r("qualquer coisa"), None);
    }

    #[test]
    fn search_text_is_encoded() {
        assert_eq!(urlencode("Maria João & co"), "Maria%20Jo%C3%A3o%20%26%20co");
    }
}
