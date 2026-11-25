//! The assistant, stage 3 (owner, 13 Nov 2026: "build it in stages" — 3: documents, letters, legal tools, settings).
//!
//! Same rules as stages 1 and 2 (`agent.rs`, `agent_money.rs`): look-ups and Apply go **through the app's own
//! endpoints as the person**, so the server's role checks decide, and every change is a before/after preview that
//! waits for Apply and can be undone while nothing else changed it.
//!
//! - **Everyone with the assistant:** a patient's documents and consents (reception: scanned prescriptions and
//!   consent PDFs only, as in the app), the reminders of a day; renaming a document or saying what it is,
//!   recording a consent withdrawal (can't be undone: a withdrawal goes into the clinical record as a PDF), marking
//!   reminders as sent.
//! - **Owner and therapists:** discharge letters (drafts, from the file's template or written by the model; editing
//!   a draft) and the reimbursement pack (ADSE/insurer PDF, saved in the patient's documents).
//! - **Owner only:** the complaints log (log one, add the reply or the ERS date), the legal check of a patient file,
//!   and settings: the clinic's details, the reminder message, the AI switch of a staff member, the website's
//!   switches and texts.
//!
//! What it never does (chosen by Claude, see DECISIONS.md 13 Nov): approve a letter (that's the owner's signature),
//! sign anything, delete or anonymise a patient, delete a day, export a record, remove a document, or write a
//! consent text. Those stay behind their buttons; the assistant says where they are.

use super::agent::{call, cut, finish, iso, ok, or_dash, passed_on, say, st, which_patient, Caller, Ctx, Out, Tx};
use super::agenda::short_ref;
use super::assist::fold;
use super::assistant::Proposal;
use super::util::clinical;
use crate::auth::{AppState, Auth};
use crate::error::{ApiResult, AppError};
use crate::workdays;
use axum::{http::HeaderMap, response::Response};
use serde_json::{json, Map, Value};
use time::Duration as Days;
use uuid::Uuid;

/// The changes this module applies (and undoes) itself.
pub const SKILLS: &[&str] = &["document_update", "letter_draft", "letter_update", "pack", "consent_revoke", "reminder_mark",
    "complaint_create", "complaint_update", "clinic_details", "reminder_text", "staff_ai", "website"];
/// Of those, the owner's only (the endpoints refuse them for anyone else too).
pub const OWNER_ONLY: &[&str] = &["complaint_create", "complaint_update", "clinic_details", "reminder_text", "staff_ai", "website"];
/// Changes that can't be undone (shown without Undo).
pub const NO_UNDO: &[&str] = &["consent_revoke"];

const LOOKUPS_ALL: &[&str] = &["patient_documents", "reminders"];
const CHANGES_ALL: &[&str] = &["rename_document", "withdraw_consent", "mark_reminded"];
const CHANGES_CLINICAL: &[&str] = &["draft_letter", "edit_letter", "reimbursement_pack"];
const LOOKUPS_OWNER: &[&str] = &["complaints", "legal_check", "clinic_settings"];
const CHANGES_OWNER: &[&str] = &["log_complaint", "update_complaint", "clinic_details", "reminder_text", "ai_switch", "website"];

fn is_owner(a: &Auth) -> bool {
    a.staff.role == "owner"
}

/// Whether `name` is one of this module's tools that this person may use.
pub fn handles(name: &str, a: &Auth) -> bool {
    LOOKUPS_ALL.contains(&name) || CHANGES_ALL.contains(&name)
        || (clinical(a) && CHANGES_CLINICAL.contains(&name))
        || (is_owner(a) && (LOOKUPS_OWNER.contains(&name) || CHANGES_OWNER.contains(&name)))
}

// ───────── tools ─────────

fn tool(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({ "type": "function", "function": { "name": name, "description": desc, "parameters": { "type": "object", "properties": props, "required": required } } })
}
fn s_(d: &str) -> Value { json!({ "type": "string", "description": d }) }
fn b_(d: &str) -> Value { json!({ "type": "boolean", "description": d }) }

/// The documents, letters, legal and settings tools this person gets (same reach as their role in the app).
pub fn tools(a: &Auth, t: &mut Vec<Value>) {
    let pat = "Patient's name, or nº and number (e.g. \"nº 12\")";
    t.push(tool("patient_documents", if clinical(a) {
            "A patient's documents (with ref, kind, name, date), consents (with ref and state) and discharge letters (drafts and approved, with ref and text)."
        } else {
            "A patient's scanned prescriptions and consent PDFs (with ref), and their consents (with ref and state)."
        }, json!({ "patient": s_(pat) }), &["patient"]));
    t.push(tool("reminders", "Who to remind of their appointment on one day (default tomorrow): time, patient, phone or e-mail, how they prefer it, and whether a reminder was already sent.",
        json!({ "date": s_("Day: YYYY-MM-DD from the calendar (or words like amanhã)") }), &[]));
    t.push(tool("rename_document", "Prepare renaming a patient document or changing what it is.",
        json!({ "patient": s_(pat), "ref": s_("The document's ref from patient_documents"), "name": s_("New file name, if it changes"),
                "kind": { "type": "string", "enum": super::documents::KINDS, "description": "What the file is, if it changes" } }), &["patient", "ref"]));
    t.push(tool("withdraw_consent", "Prepare recording that the patient withdrew a consent (it stays in the record with a withdrawal PDF; this can't be undone).",
        json!({ "patient": s_(pat), "ref": s_("The consent's ref from patient_documents") }), &["patient", "ref"]));
    t.push(tool("mark_reminded", "Prepare marking that appointment reminders were sent (the assistant never sends messages; the person sends them from Reminders).",
        json!({ "refs": { "type": "array", "items": { "type": "string" }, "description": "Appointment refs from reminders or agenda" },
                "channel": { "type": "string", "enum": ["sms", "whatsapp", "email", "call", "other"] }, "patient": s_("Only for a class: which patient") }), &["refs", "channel"]));
    if clinical(a) {
        t.push(tool("draft_letter", "Prepare a discharge letter draft for a patient. Without text it starts from the file's template (parts to complete marked [a completar]). Only the owner approves letters, in the app.",
            json!({ "patient": s_(pat), "text": s_("The letter's body, plain text, only if the user asked you to write it"), "addressee": s_("To whom (e.g. the family doctor), if said"),
                    "lang": { "type": "string", "enum": ["pt", "en"] } }), &["patient"]));
        t.push(tool("edit_letter", "Prepare changes to a discharge letter draft (not an approved one). Give the full new text of what changes.",
            json!({ "patient": s_(pat), "ref": s_("The letter's ref from patient_documents"), "text": s_("The full new body"), "addressee": s_("New addressee"),
                    "lang": { "type": "string", "enum": ["pt", "en"] } }), &["patient", "ref"]));
        t.push(tool("reimbursement_pack", "Prepare the reimbursement pack (one PDF for ADSE or an insurer: prescription, sessions, invoices and a short clinical report), saved in the patient's documents.",
            json!({ "patient": s_(pat), "prescription": s_("The prescription's date or doctor, if there are several (default: the latest)"),
                    "from": s_("First day (default: the prescription's date)"), "to": s_("Last day (default: today)"),
                    "payer": s_("ADSE, the insurer's name… (default: the prescription's)"), "report": s_("The short clinical report, only if the user dictated it") }), &["patient"]));
    }
    if is_owner(a) {
        t.push(tool("complaints", "The complaints log (Livro de Reclamações): each complaint with ref, day received, who, summary, deadline (10 working days) and whether it's done.", json!({}), &[]));
        t.push(tool("legal_check", "What a patient's file holds before anonymising, deleting or exporting it (notes, documents, payments, last contact, the 20-year keeping period). Those acts are done by hand in the app.",
            json!({ "patient": s_(pat) }), &["patient"]));
        t.push(tool("clinic_settings", "The clinic's details (name, NIF, ERS, address, phone, e-mail), the reminder message, the staff's AI switches and the website's switches and texts.", json!({}), &[]));
        t.push(tool("log_complaint", "Prepare logging a complaint received (paper book, electronic book or other).",
            json!({ "received": s_("Day received (default today)"), "source": { "type": "string", "enum": ["paper", "electronic", "other"] },
                    "complainant": s_("Who complained"), "summary": s_("What it's about, short"), "book_ref": s_("Sheet or reference number, if said"), "patient": s_("The patient, if it's one") }),
            &["source", "summary"]));
        t.push(tool("update_complaint", "Prepare adding the clinic's reply, the day it was answered, or the day it was sent to the ERS, to a complaint.",
            json!({ "ref": s_("The complaint's ref from complaints"), "reply": s_("The reply (full text)"), "replied_on": s_("Day answered"), "sent_to_ers_on": s_("Day sent to the ERS") }), &["ref"]));
        t.push(tool("clinic_details", "Prepare changes to the clinic's details printed on documents. Only the fields that change.",
            json!({ "name": s_(""), "nif": s_("9 digits"), "ers_number": s_(""), "address": s_(""), "phone": s_(""), "email": s_("") }), &[]));
        t.push(tool("reminder_text", "Prepare a new reminder message. Keep the placeholders as they are: {nome}, {dia}, {hora}, {clinica} (Portuguese) and {name}, {day}, {time}, {clinic} (English).",
            json!({ "pt": s_("Full new message in Portuguese, if it changes"), "en": s_("Full new message in English, if it changes") }), &[]));
        t.push(tool("ai_switch", "Prepare turning a staff member's AI switch on or off (the assistant and, for therapists, recording and note write-up).",
            json!({ "name": s_("Their name"), "on": b_("true = on") }), &["name", "on"]));
        t.push(tool("website", "Prepare changes to the public website: published, booking requests open, home visits, opening soon, prices shown, and its short texts. Only what changes.",
            json!({ "published": b_(""), "requests_open": b_("The request-an-appointment form"), "home_visits": b_(""), "opening_soon": b_("The space isn't open yet"), "show_prices": b_(""),
                    "tagline_pt": s_("Up to 90 characters"), "tagline_en": s_("Up to 90 characters"), "intro_pt": s_(""), "intro_en": s_(""), "home_text_pt": s_(""), "home_text_en": s_("") }), &[]));
    }
}

/// A line of the brief about documents and legal tools.
pub fn brief_rules() -> &'static str {
    "\n- Documents and legal: use refs from patient_documents, complaints or reminders. You never approve or sign a letter, send a message, delete or anonymise a patient, export a record or remove a document: say the person does it in the app (letters: Aprovar in the patient's file; legal acts: the patient's file › Legal)."
}

// ───────── small helpers ─────────

#[allow(clippy::result_large_err)]
async fn get(x: &Ctx<'_>, path: &str) -> Result<Value, Out> {
    let (status, v) = call(x.s, &x.c, "GET", path, None).await;
    if ok(status) { Ok(v) } else { Err(Out::Retry(st(&v, "message"))) }
}

fn sref(v: &Value) -> String {
    st(v, "id").parse::<Uuid>().map(short_ref).unwrap_or_default()
}

/// "ref 3f9c2a1b", "3F9C-2A1B" → "3f9c2a1b" (at least 6 hex digits), else "".
fn bare(r: &str) -> String {
    let t = r.trim().to_lowercase();
    let t = t.strip_prefix("ref").unwrap_or(&t);
    let h: String = t.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if h.len() >= 6 { h } else { String::new() }
}

/// The one item of `list` whose id starts with the ref.
fn by_ref<'a>(list: &'a [Value], r: &str) -> Option<&'a Value> {
    let b = bare(r);
    if b.is_empty() {
        return None;
    }
    let m: Vec<&Value> = list.iter().filter(|v| st(v, "id").replace('-', "").starts_with(&b)).collect();
    if m.len() == 1 { Some(m[0]) } else { None }
}

fn date_label(iso: &str) -> String {
    if iso.len() >= 10 { format!("{}/{}/{}", &iso[8..10], &iso[5..7], &iso[..4]) } else { or_dash(iso) }
}

fn short(v: &str) -> String {
    if v.trim().is_empty() { "—".into() } else { cut(v, 160) }
}

fn kind_label(k: &str, en: bool) -> String {
    match (k, en) {
        ("photo", false) => "Fotografia", ("photo", true) => "Photo", ("report", false) => "Relatório", ("report", true) => "Report",
        ("imaging", false) => "Exame de imagem", ("imaging", true) => "Imaging", ("prescription", false) => "Prescrição", ("prescription", true) => "Prescription",
        ("letter", false) => "Carta", ("letter", true) => "Letter", ("note", false) => "Nota assinada", ("note", true) => "Signed Note",
        ("consent", false) => "Consentimento", ("consent", true) => "Consent", (_, false) => "Outro", (_, true) => "Other",
    }.to_string()
}

fn channel_label(c: &str, en: bool) -> String {
    match (c, en) {
        ("sms", _) => "SMS", ("whatsapp", _) => "WhatsApp", ("email", _) => "Email", ("call", false) => "Telefonema", ("call", true) => "Phone Call",
        (_, false) => "Outro", (_, true) => "Other",
    }.to_string()
}

fn yes_no(b: bool, en: bool) -> String {
    match (b, en) { (true, false) => "Sim", (true, true) => "Yes", (false, false) => "Não", (false, true) => "No" }.to_string()
}

fn on_off(b: bool, en: bool) -> String {
    match (b, en) { (true, false) => "Ligado", (true, true) => "On", (false, false) => "Desligado", (false, true) => "Off" }.to_string()
}

fn change(skill: &'static str, target: Option<Uuid>, params: Value, before: Value, after: Value, rows: Vec<[String; 3]>, warnings: Vec<String>) -> Out {
    Out::Change(Proposal { skill, target, params, before, after, rows, warnings })
}

/// A day the person said, or today when nothing was said. Err = couldn't read it.
#[allow(clippy::result_large_err)]
fn day_or_today(x: &Ctx<'_>, text: &str) -> Result<time::Date, Out> {
    if text.trim().is_empty() {
        return Ok(x.today);
    }
    x.date(text).ok_or_else(|| Out::Retry("Give the day as YYYY-MM-DD.".into()))
}

const CLINIC_FIELDS: &[(&str, &str, &str)] = &[
    ("name", "Nome da clínica", "Clinic Name"), ("nif", "NIF", "NIF"), ("ers_number", "Registo ERS", "ERS Registration"),
    ("address", "Morada", "Address"), ("phone", "Telefone", "Phone"), ("email", "Email", "Email"),
];

const SITE_FIELDS: &[(&str, &str, &str, bool)] = &[
    ("published", "Site publicado", "Site Published", true), ("requests_open", "Pedidos de marcação", "Booking Requests", true),
    ("home_visits", "Domicílios", "Home Visits", true), ("opening_soon", "Abertura em breve", "Opening Soon", true), ("show_prices", "Mostrar preços", "Show Prices", true),
    ("tagline_pt", "Frase (PT)", "Tagline (PT)", false), ("tagline_en", "Frase (EN)", "Tagline (EN)", false),
    ("intro_pt", "Apresentação (PT)", "Introduction (PT)", false), ("intro_en", "Apresentação (EN)", "Introduction (EN)", false),
    ("home_text_pt", "Texto dos domicílios (PT)", "Home Visits Text (PT)", false), ("home_text_en", "Texto dos domicílios (EN)", "Home Visits Text (EN)", false),
];

const COMPLAINT_EDIT: &[(&str, &str, &str)] = &[
    ("reply", "Resposta", "Reply"), ("replied_on", "Respondida a", "Replied On"), ("sent_to_ers_on", "Enviada à ERS a", "Sent to ERS On"),
];

// ───────── look-ups ─────────

pub(super) async fn run(x: &Ctx<'_>, name: &str, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    Ok(match name {
        "patient_documents" => {
            let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
            let file = match get(x, &format!("/patients/{}", p.id)).await { Ok(v) => v, Err(o) => return Ok(o) };
            let mut text = format!("Patient nº {} · {}\nDocuments:\n", p.number, p.name);
            let docs = file["documents"].as_array().cloned().unwrap_or_default();
            for d in docs.iter().take(40) {
                let made = if !st(d, "note_id").is_empty() { " · signed note PDF (can't change)" } else if !st(d, "letter_id").is_empty() { " · approved letter PDF (can't change)" }
                    else if !st(d, "consent_id").is_empty() { " · consent PDF (can't change)" } else { "" };
                text.push_str(&format!("ref {} · {} · {} · {} · added by {}{made}\n", sref(d), st(d, "kind"), st(d, "filename"), st(d, "uploaded_at").get(..10).unwrap_or(""), or_dash(&st(d, "uploaded_by_name"))));
            }
            if docs.is_empty() { text.push_str("(none)\n"); }
            text.push_str("Consents:\n");
            let cs = file["consents"].as_array().cloned().unwrap_or_default();
            for c in &cs {
                text.push_str(&format!("ref {} · {} · {} · signed {} by {}{}\n", sref(c), st(c, "title"), st(c, "state"), st(c, "signed_at").get(..10).unwrap_or(""), or_dash(&st(c, "signed_by_name")),
                    if st(c, "expires_on").is_empty() { String::new() } else { format!(" · expires {}", st(c, "expires_on")) }));
            }
            if cs.is_empty() { text.push_str("(none)\n"); }
            if clinical(x.a) {
                let (status, l) = call(x.s, &x.c, "GET", &format!("/patients/{}/letters", p.id), None).await;
                if ok(status) {
                    text.push_str("Discharge letters:\n");
                    let ls = l["letters"].as_array().cloned().unwrap_or_default();
                    for l in &ls {
                        let state = if st(l, "approved_at").is_empty() { format!("draft by {}", or_dash(&st(l, "created_by_name"))) } else { format!("approved {} by {} (can't change)", st(l, "approved_at").get(..10).unwrap_or(""), st(l, "approved_by_name")) };
                        text.push_str(&format!("ref {} · {} · {} · to {}{}\n  text: {}\n", sref(l), st(l, "lang"), state, or_dash(&st(l, "addressee")),
                            if l["ai_drafted"] == json!(true) { " · AI-drafted" } else { "" }, cut(&st(l, "body"), 3000)));
                    }
                    if ls.is_empty() { text.push_str("(none)\n"); }
                }
            }
            Out::Read { text, step: if en { format!("Looked at {}’s documents", p.label()) } else { format!("Viu os documentos de {}", p.label()) }, list: None }
        }
        "reminders" => {
            let day = match st(args, "date") {
                d if d.is_empty() => x.today + Days::days(1),
                d => match x.date(&d) { Some(d) => d, None => return Ok(Out::Retry("Give the day as YYYY-MM-DD.".into())) },
            };
            let v = match get(x, &format!("/reminders?day={}", iso(day))).await { Ok(v) => v, Err(o) => return Ok(o) };
            let items = v["items"].as_array().cloned().unwrap_or_default();
            let ts: Vec<String> = items.iter().map(|i| st(i, "starts_at")).collect();
            let loc = super::agent::locals(x.s, &ts).await?;
            let mut text = format!("Reminders for {}:\n", iso(day));
            for (i, it) in items.iter().enumerate() {
                let contact = match (st(it, "phone").is_empty(), st(it, "email").is_empty()) { (false, _) => "has a phone", (true, false) => "e-mail only", _ => "no phone or e-mail" };
                text.push_str(&format!("ref {} · {} · {} (nº {}) · {} · {} · prefers {} · {}\n", st(it, "appointment_id").parse::<Uuid>().map(short_ref).unwrap_or_default(),
                    loc[i].get(11..).unwrap_or(""), st(it, "name"), st(it, "number"), st(it, "kind"), contact, or_dash(&st(it, "prefers")),
                    if st(it, "sent_at").is_empty() { "not reminded yet".to_string() } else { format!("reminded by {} ({})", or_dash(&st(it, "sent_by_name")), st(it, "sent_channel")) }));
            }
            if items.is_empty() { text.push_str("Nobody booked.\n"); }
            Out::Read { text, step: if en { format!("Looked at the reminders for {}", x.day_label(day)) } else { format!("Viu os lembretes de {}", x.day_label(day)) }, list: None }
        }
        "complaints" => {
            let v = match get(x, "/complaints").await { Ok(v) => v, Err(o) => return Ok(o) };
            let mut text = String::from("Complaints, newest first:\n");
            let all = v.as_array().cloned().unwrap_or_default();
            for c in all.iter().take(30) {
                let done = if st(c, "source") == "paper" { !st(c, "sent_to_ers_on").is_empty() } else { !st(c, "replied_on").is_empty() };
                let left = workdays::parse(&st(c, "due_on")).map(|d| workdays::working_days_between(x.today, d));
                text.push_str(&format!("ref {} · received {} · {} · from {} · {} · deadline {}{} · {}{}\n", sref(c), st(c, "received_on"), st(c, "source"), or_dash(&st(c, "complainant")),
                    cut(&st(c, "summary"), 200), st(c, "due_on"),
                    match (done, left) { (false, Some(n)) if n < 0 => format!(" (overdue by {} working days)", -n), (false, Some(n)) => format!(" ({n} working days left)"), _ => String::new() },
                    if done { "done" } else { "open" },
                    if st(c, "reply").is_empty() { String::new() } else { format!(" · reply: {}", cut(&st(c, "reply"), 300)) }));
            }
            if all.is_empty() { text.push_str("(none)\n"); }
            Out::Read { text, step: say(en, "Viu o livro de reclamações", "Looked at the complaints log"), list: None }
        }
        "legal_check" => {
            let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
            let v = match get(x, &format!("/patients/{}/legal", p.id)).await { Ok(v) => v, Err(o) => return Ok(o) };
            let last = st(&v, "last_contact");
            let keep = if last.len() >= 10 { format!("{}{}", last[..4].parse::<i32>().map(|y| y + 20).unwrap_or(0), &last[4..10]) } else { String::new() };
            let text = format!("Patient nº {} · {}: {} signed notes, {} draft notes, {} documents, {} tests, {} consents, {} visits done, {} upcoming appointments, {} payments, {} active packages, owes {}. Last contact {}.{} \
Anonymise, delete and export are done by hand in the patient's file › Legal (the assistant can't): anonymise keeps the clinical content without the name; delete removes everything (payments stay without a name).",
                p.number, p.name, st(&v, "notes_signed"), st(&v, "notes_draft"), st(&v, "documents"), st(&v, "tests"), st(&v, "consents"), st(&v, "visits"), st(&v, "upcoming"), st(&v, "payments"),
                st(&v, "plans_active"), super::agent_money::eur(v["owed_cents"].as_i64().unwrap_or(0), true), or_dash(&last),
                if keep.is_empty() { String::new() } else { format!(" The clinical record must be kept until {keep} (20 years after the last contact).") });
            Out::Read { text, step: if en { format!("Checked {}’s file for the legal tools", p.label()) } else { format!("Verificou a ficha de {} para as ferramentas legais", p.label()) }, list: None }
        }
        "clinic_settings" => {
            let c = match get(x, "/clinic").await { Ok(v) => v, Err(o) => return Ok(o) };
            let (pt, en_t): (String, String) = sqlx::query_as("SELECT reminder_pt, reminder_en FROM clinic WHERE id = 1").fetch_one(&x.s.pool).await?;
            let mut text = format!("Clinic: name {} · NIF {} · ERS {} · address {} · phone {} · e-mail {} · logo {}\nReminder message (pt): {}\nReminder message (en): {}\nAI switches:\n",
                or_dash(&st(&c, "name")), or_dash(&st(&c, "nif")), or_dash(&st(&c, "ers_number")), or_dash(&st(&c, "address")), or_dash(&st(&c, "phone")), or_dash(&st(&c, "email")),
                if c["logo"].is_null() { "none" } else { "yes" }, pt, en_t);
            let staff: Vec<(String, String, bool, bool)> = sqlx::query_as("SELECT name, role, ai_enabled, active FROM staff WHERE active ORDER BY role <> 'owner', name").fetch_all(&x.s.pool).await?;
            for (n, r, ai, _) in &staff {
                text.push_str(&format!("{n} · {r} · AI {}\n", if *ai { "on" } else { "off" }));
            }
            if let Ok(w) = get(x, "/website").await {
                let site = &w["site"];
                text.push_str("Website:");
                for (k, _, e, b) in SITE_FIELDS {
                    if *b { text.push_str(&format!(" {} {} ·", e.to_lowercase(), if site[*k] == json!(true) { "yes" } else { "no" })); }
                }
                for (k, _, e, b) in SITE_FIELDS {
                    if !*b { text.push_str(&format!("\n{e}: {}", cut(&st(site, k), 400))); }
                }
            }
            Out::Read { text, step: say(en, "Viu as definições da clínica", "Looked at the clinic’s settings"), list: None }
        }
        "rename_document" => rename_document(x, args).await?,
        "withdraw_consent" => withdraw_consent(x, args).await?,
        "mark_reminded" => mark_reminded(x, args).await?,
        "draft_letter" => draft_letter(x, args).await?,
        "edit_letter" => edit_letter(x, args).await?,
        "reimbursement_pack" => pack(x, args).await?,
        "log_complaint" => log_complaint(x, args).await?,
        "update_complaint" => update_complaint(x, args).await?,
        "clinic_details" => clinic_details(x, args).await?,
        "reminder_text" => reminder_text(x, args).await?,
        "ai_switch" => ai_switch(x, args).await?,
        "website" => website(x, args).await?,
        _ => Out::Retry(format!("There's no tool called \"{}\" for this user.", cut(name, 40))),
    })
}

// ───────── changes ─────────

async fn rename_document(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
    let file = match get(x, &format!("/patients/{}", p.id)).await { Ok(v) => v, Err(o) => return Ok(o) };
    let docs = file["documents"].as_array().cloned().unwrap_or_default();
    let Some(d) = by_ref(&docs, &st(args, "ref")) else {
        return Ok(Out::Retry("That ref isn't one of this patient's documents. Look at patient_documents and use its ref.".into()));
    };
    if !st(d, "note_id").is_empty() || !st(d, "letter_id").is_empty() || !st(d, "consent_id").is_empty() {
        return Ok(Out::Retry("That PDF is part of the clinical record and can't be renamed.".into()));
    }
    let id: Uuid = st(d, "id").parse().map_err(|_| AppError::NotFound)?;
    let (mut before, mut after, mut rows) = (Map::new(), Map::new(), Vec::new());
    let name = cut(&st(args, "name"), 150);
    if !name.is_empty() {
        // Keep the file's extension: "relatório" → "relatório.pdf".
        let old = st(d, "filename");
        let ext = old.rsplit_once('.').map(|(_, e)| e.to_string()).filter(|e| e.len() <= 5).unwrap_or_default();
        let new = if !ext.is_empty() && !name.to_lowercase().ends_with(&format!(".{}", ext.to_lowercase())) { format!("{name}.{ext}") } else { name };
        let new = crate::export::safe_name(&new);
        if new != old {
            rows.push([format!("{} — {}", p.label(), say(en, "Nome do ficheiro", "File Name")), old.clone(), new.clone()]);
            before.insert("filename".into(), json!(old));
            after.insert("filename".into(), json!(new));
        }
    }
    let kind = st(args, "kind");
    if !kind.is_empty() && kind != st(d, "kind") {
        if !super::documents::KINDS.contains(&kind.as_str()) {
            return Ok(Out::Retry(format!("Kind must be one of {}.", super::documents::KINDS.join(", "))));
        }
        rows.push([format!("{} — {}", p.label(), say(en, "Tipo", "Kind")), kind_label(&st(d, "kind"), en), kind_label(&kind, en)]);
        before.insert("kind".into(), json!(st(d, "kind")));
        after.insert("kind".into(), json!(kind));
    }
    if after.is_empty() {
        return Ok(Out::Retry("Nothing would change: give a new name or kind.".into()));
    }
    Ok(change("document_update", Some(id), json!({ "patient_id": p.id }), Value::Object(before), Value::Object(after), rows, vec![]))
}

async fn withdraw_consent(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
    let file = match get(x, &format!("/patients/{}", p.id)).await { Ok(v) => v, Err(o) => return Ok(o) };
    let cs = file["consents"].as_array().cloned().unwrap_or_default();
    let Some(c) = by_ref(&cs, &st(args, "ref")) else {
        return Ok(Out::Retry("That ref isn't one of this patient's consents. Look at patient_documents and use its ref.".into()));
    };
    if st(c, "state") == "revoked" {
        return Ok(Out::Retry("That consent was already withdrawn.".into()));
    }
    let id: Uuid = st(c, "id").parse().map_err(|_| AppError::NotFound)?;
    Ok(change("consent_revoke", Some(id), json!({ "patient_id": p.id }), json!({ "revoked": false }), json!({ "revoked": true }),
        vec![[format!("{} — {}", p.label(), st(c, "title")), format!("{} {}", say(en, "Assinado a", "Signed"), date_label(&st(c, "signed_at"))), say(en, "Retirado hoje", "Withdrawn today")]],
        vec![say(en, "Não dá para desfazer: a retirada fica no processo clínico, com o seu PDF. Confirme que foi o utente que a pediu.",
                 "This can’t be undone: the withdrawal goes into the clinical record, with its own PDF. Make sure the patient asked for it.")]))
}

async fn mark_reminded(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let channel = st(args, "channel");
    if !["sms", "whatsapp", "email", "call", "other"].contains(&channel.as_str()) {
        return Ok(Out::Retry("Ask the user how the reminders were sent: SMS, WhatsApp, e-mail or a call.".into()));
    }
    let refs: Vec<String> = args["refs"].as_array().map(|a| a.iter().filter_map(|r| r.as_str().map(String::from)).collect()).unwrap_or_default();
    if refs.is_empty() {
        return Ok(Out::Retry("Give the appointment refs from reminders.".into()));
    }
    let only = match st(args, "patient") { t if t.is_empty() => None, t => match which_patient(x, &t).await? { Ok(p) => Some(p.id), Err(o) => return Ok(o) } };
    let (mut items, mut rows, mut before) = (Vec::new(), Vec::new(), Vec::new());
    let mut lists: std::collections::HashMap<String, Vec<Value>> = Default::default();
    for r in refs.iter().take(40) {
        let Some(a) = x.seen.appt(r) else { return Ok(Out::Retry(format!("\"{}\" isn't an appointment ref. Use the refs reminders showed.", cut(r, 20)))) };
        if !lists.contains_key(&a.day) {
            let v = match get(x, &format!("/reminders?day={}", a.day)).await { Ok(v) => v, Err(o) => return Ok(o) };
            lists.insert(a.day.clone(), v["items"].as_array().cloned().unwrap_or_default());
        }
        let aid = a.id.to_string();
        let people: Vec<&Value> = lists[&a.day].iter().filter(|i| st(i, "appointment_id") == aid && only.is_none_or(|p| st(i, "patient_id") == p.to_string())).collect();
        if people.is_empty() {
            return Ok(Out::Retry(format!("Nobody to remind for ref {} (cancelled, or not that patient).", short_ref(a.id))));
        }
        for i in people {
            let sent = !st(i, "sent_at").is_empty();
            items.push(json!({ "appointment_id": aid, "patient_id": st(i, "patient_id") }));
            before.push(json!(sent));
            rows.push([format!("{} — {}", st(i, "name"), a.when(en)), if sent { format!("{} ({})", say(en, "Lembrado", "Reminded"), channel_label(&st(i, "sent_channel"), en)) } else { say(en, "Por lembrar", "Not reminded") },
                format!("{} ({})", say(en, "Lembrado", "Reminded"), channel_label(&channel, en))]);
        }
    }
    let warnings = vec![say(en, "Só regista que o lembrete foi enviado; a mensagem envia-se em Lembretes.", "This only records that the reminder went out; the message is sent from Reminders.")];
    Ok(change("reminder_mark", None, json!({ "items": items, "channel": channel }), json!({ "sent": before }), json!({ "sent": vec![true; items.len()] }), rows, warnings))
}

async fn draft_letter(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
    let lang = match st(args, "lang").as_str() { "en" => "en", "pt" => "pt", _ => "" };
    let tpl = match get(x, &format!("/patients/{}/letters/template{}", p.id, if lang.is_empty() { String::new() } else { format!("?lang={lang}") })).await { Ok(v) => v, Err(o) => return Ok(o) };
    let text = crate::style::tidy(&cut(&st(args, "text"), 12000));
    let ai = text.len() >= 40;
    let body = if ai { text } else { st(&tpl, "body") };
    let addressee = match cut(&st(args, "addressee"), 300) { a if a.is_empty() => st(&tpl, "addressee"), a => a };
    let lang = st(&tpl, "lang");
    let mut warnings = vec![say(en, "Fica como rascunho na ficha. Só o proprietário aprova cartas, depois de as ler.", "It’s saved as a draft in the file. Only the owner approves letters, after reading them.")];
    if body.contains(super::letters::TODO_PT) || body.contains(super::letters::TODO_EN) {
        warnings.push(say(en, "Tem partes marcadas [a completar] para preencher antes de aprovar.", "It has parts marked [to complete] to fill in before approving."));
    }
    if ai && crate::style::problem(&body).is_some() {
        warnings.push(say(en, "O texto tem títulos ou listas: reveja-o.", "The text has headings or lists: review it."));
    }
    Ok(change("letter_draft", Some(p.id), json!({ "patient_id": p.id, "body": { "lang": lang, "addressee": addressee, "body": body, "ai_drafted": ai } }), Value::Null, Value::Null,
        vec![[format!("{} — {}", p.label(), say(en, "Carta de alta (rascunho)", "Discharge Letter (Draft)")), "—".into(),
              format!("{}{}: {}", if ai { say(en, "Escrita pela IA", "Written by the AI") } else { say(en, "Modelo da ficha", "From the file’s template") },
                      if addressee.is_empty() { String::new() } else { format!(" · {}", cut(&addressee, 60)) }, cut(&body, 300))]],
        warnings))
}

async fn edit_letter(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
    let v = match get(x, &format!("/patients/{}/letters", p.id)).await { Ok(v) => v, Err(o) => return Ok(o) };
    let ls = v["letters"].as_array().cloned().unwrap_or_default();
    let Some(l) = by_ref(&ls, &st(args, "ref")) else {
        return Ok(Out::Retry("That ref isn't one of this patient's letters. Look at patient_documents and use its ref.".into()));
    };
    if !st(l, "approved_at").is_empty() {
        return Ok(Out::Retry("That letter is approved and can't change. Draft a new one instead.".into()));
    }
    let id: Uuid = st(l, "id").parse().map_err(|_| AppError::NotFound)?;
    let (mut before, mut after, mut rows) = (Map::new(), Map::new(), Vec::new());
    let what = say(en, "Carta de alta", "Discharge Letter");
    for (k, arg, pt, e, max) in [("body", "text", "Texto", "Text", 12000), ("addressee", "addressee", "Destinatário", "Addressee", 300), ("lang", "lang", "Língua", "Language", 2)] {
        let Some(Value::String(newv)) = args.get(arg) else { continue };
        let newv = if k == "body" { crate::style::tidy(&cut(newv, max)) } else { cut(newv, max) };
        if newv.is_empty() || (k == "lang" && newv != "pt" && newv != "en") || newv == st(l, k) {
            continue;
        }
        rows.push([format!("{} — {what} · {}", p.label(), if en { e } else { pt }), short(&st(l, k)), short(&newv)]);
        before.insert(k.into(), json!(st(l, k)));
        after.insert(k.into(), json!(newv));
    }
    if after.is_empty() {
        return Ok(Out::Retry("Nothing would change in the letter.".into()));
    }
    if after.contains_key("body") {
        after.insert("ai_drafted".into(), json!(true));
    }
    Ok(change("letter_update", Some(id), json!({ "patient_id": p.id }), Value::Object(before), Value::Object(after), rows,
        vec![say(en, "Substitui o que lá está. Leia o texto todo antes de aplicar.", "It replaces what’s there. Read the whole text before applying.")]))
}

async fn pack(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
    let first = match get(x, &format!("/patients/{}/reimbursement", p.id)).await { Ok(v) => v, Err(o) => return Ok(o) };
    let rxs = first["prescriptions"].as_array().cloned().unwrap_or_default();
    // The prescription: the one the person means (by date or doctor), else the latest.
    let want = fold(&st(args, "prescription"));
    let rx = if want.is_empty() {
        rxs.iter().max_by_key(|r| st(r, "issued_iso")).cloned()
    } else {
        let day = x.date(&st(args, "prescription")).map(iso);
        let m: Vec<&Value> = rxs.iter().filter(|r| day.as_deref() == Some(st(r, "issued_iso").as_str()) || fold(&st(r, "doctor")).contains(&want) || fold(&st(r, "payer")).contains(&want)).collect();
        match m.len() {
            1 => Some(m[0].clone()),
            0 => return Ok(Out::Retry(format!("No prescription matches \"{}\". Prescriptions: {}.", cut(&st(args, "prescription"), 40),
                rxs.iter().map(|r| format!("{} {}", st(r, "issued"), st(r, "doctor"))).collect::<Vec<_>>().join("; ")))),
            _ => return Ok(Out::Retry("Several prescriptions match: ask which (date and doctor).".into())),
        }
    };
    let from = match st(args, "from") { t if t.is_empty() => String::new(), t => match x.date(&t) { Some(d) => iso(d), None => return Ok(Out::Retry("Give the first day as YYYY-MM-DD.".into())) } };
    let to = match st(args, "to") { t if t.is_empty() => String::new(), t => match x.date(&t) { Some(d) => iso(d), None => return Ok(Out::Retry("Give the last day as YYYY-MM-DD.".into())) } };
    let mut q = format!("/patients/{}/reimbursement?from={from}&to={to}", p.id);
    if let Some(r) = &rx { q.push_str(&format!("&prescription_id={}", st(r, "id"))); }
    let v = match get(x, &q).await { Ok(v) => v, Err(o) => return Ok(o) };
    let sessions = v["sessions"].as_array().map(Vec::len).unwrap_or(0);
    let pays = v["payments"].as_array().cloned().unwrap_or_default();
    let report = match crate::style::tidy(&cut(&st(args, "report"), 8000)) { r if r.len() >= 20 => r, _ => st(&v, "report") };
    let payer = match cut(&st(args, "payer"), 120) { t if t.is_empty() => st(&v, "payer"), t => t };
    let body = json!({ "prescription_id": v["prescription_id"], "from": v["from"], "to": v["to"], "payment_ids": pays.iter().map(|p| p["id"].clone()).collect::<Vec<_>>(),
        "payer": payer, "report": report, "professional_id": v["professional_id"] });
    let mut warnings = vec![say(en, "Fica nos documentos do utente como relatório. Leia o relatório clínico antes de o entregar.", "It’s saved in the patient’s documents as a report. Read the clinical report before handing it over.")];
    if sessions == 0 { warnings.push(say(en, "Não há sessões feitas nesse período.", "There are no sessions done in that period.")); }
    if pays.iter().any(|p| st(p, "invoice_number").is_empty()) { warnings.push(say(en, "Há pagamentos sem número de fatura.", "Some payments have no invoice number.")); }
    if st(&v, "professional_id").is_empty() { warnings.push(say(en, "Falta a sua cédula profissional (Definições › Conta).", "Your professional licence number is missing (Settings › Account).")); }
    let rx_label = rx.as_ref().map(|r| format!("{} · {}", st(r, "issued"), or_dash(&st(r, "doctor")))).unwrap_or_else(|| say(en, "sem prescrição", "no prescription"));
    Ok(change("pack", Some(p.id), json!({ "patient_id": p.id, "body": body }), Value::Null, Value::Null,
        vec![[format!("{} — {}", p.label(), say(en, "Pacote de reembolso", "Reimbursement Pack")), "—".into(),
              format!("{} – {} · {rx_label}{} · {} {} · {} {}", date_label(&st(&v, "from")), date_label(&st(&v, "to")), if payer.is_empty() { String::new() } else { format!(" · {payer}") },
                      sessions, say(en, "sessões", "sessions"), pays.len(), say(en, "pagamentos", "payments"))],
             [format!("{} — {}", p.label(), say(en, "Relatório clínico", "Clinical Report")), "—".into(), cut(&report, 300)]],
        warnings))
}

async fn log_complaint(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let day = match day_or_today(x, &st(args, "received")) { Ok(d) => d, Err(o) => return Ok(o) };
    if day > x.today {
        return Ok(Out::Retry(format!("A complaint can't be received in the future (today is {}).", iso(x.today))));
    }
    let source = st(args, "source");
    if !["paper", "electronic", "other"].contains(&source.as_str()) {
        return Ok(Out::Retry("Ask the user where it came from: the paper book, the electronic book or other.".into()));
    }
    let summary = cut(&st(args, "summary"), 2000);
    if summary.is_empty() {
        return Ok(Out::Retry("Ask the user what the complaint is about.".into()));
    }
    let mut body = json!({ "received_on": iso(day), "source": source, "summary": summary, "complainant": cut(&st(args, "complainant"), 200), "book_ref": cut(&st(args, "book_ref"), 60) });
    let mut who = st(args, "complainant");
    if !st(args, "patient").is_empty() {
        let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
        body["patient_id"] = json!(p.id);
        if who.is_empty() { who = p.name.clone(); body["complainant"] = json!(p.name); }
    }
    let due = workdays::add_working_days(day, super::complaints::ERS_WORKING_DAYS);
    let src = match (source.as_str(), en) { ("paper", false) => "Livro em papel", ("paper", true) => "Paper Book", ("electronic", false) => "Livro eletrónico", ("electronic", true) => "Electronic Book", (_, false) => "Outra via", (_, true) => "Other" };
    let mut warnings = vec![if source == "paper" {
        say(en, &format!("Prazo: {} (10 dias úteis) para a registar no site da ERS e responder.", date_label(&workdays::fmt(due))), &format!("Deadline: {} (10 working days) to enter it on the ERS website and reply.", date_label(&workdays::fmt(due))))
    } else {
        say(en, &format!("Prazo para responder: {} (10 dias úteis).", date_label(&workdays::fmt(due))), &format!("Deadline to reply: {} (10 working days).", date_label(&workdays::fmt(due))))
    }];
    if workdays::working_days_between(x.today, due) < 0 {
        warnings.push(say(en, "O prazo já passou.", "The deadline has already passed."));
    }
    Ok(change("complaint_create", None, json!({ "body": body }), Value::Null, Value::Null,
        vec![[format!("{} — {}", say(en, "Reclamação", "Complaint"), date_label(&iso(day))), "—".into(), format!("{src}{} · {}", if who.is_empty() { String::new() } else { format!(" · {}", cut(&who, 60)) }, cut(&summary, 200))]],
        warnings))
}

async fn update_complaint(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let all = match get(x, "/complaints").await { Ok(v) => v.as_array().cloned().unwrap_or_default(), Err(o) => return Ok(o) };
    let Some(c) = by_ref(&all, &st(args, "ref")) else {
        return Ok(Out::Retry("That ref isn't a complaint. Look at complaints and use its ref.".into()));
    };
    let id: Uuid = st(c, "id").parse().map_err(|_| AppError::NotFound)?;
    let (mut before, mut after, mut rows) = (Map::new(), Map::new(), Vec::new());
    for (k, pt, e) in COMPLAINT_EDIT {
        let raw = st(args, k);
        if raw.is_empty() { continue; }
        let newv = if *k == "reply" { cut(&raw, 4000) } else {
            match x.date(&raw) {
                Some(d) if d > x.today => return Ok(Out::Retry(format!("{} is in the future (today is {}).", iso(d), iso(x.today)))),
                Some(d) => iso(d),
                None => return Ok(Out::Retry("Give the day as YYYY-MM-DD.".into())),
            }
        };
        let old = st(c, k);
        if newv == old { continue; }
        let show = |v: &str| if *k == "reply" { short(v) } else { date_label(v) };
        rows.push([format!("{} {} — {}", say(en, "Reclamação de", "Complaint of"), date_label(&st(c, "received_on")), if en { *e } else { *pt }), show(&old), show(&newv)]);
        before.insert(k.to_string(), json!(old));
        after.insert(k.to_string(), json!(newv));
    }
    if after.is_empty() {
        return Ok(Out::Retry("Nothing would change: give the reply, the day it was answered or the day it was sent to the ERS.".into()));
    }
    Ok(change("complaint_update", Some(id), json!({}), Value::Object(before), Value::Object(after), rows, vec![]))
}

async fn clinic_details(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let c = match get(x, "/clinic").await { Ok(v) => v, Err(o) => return Ok(o) };
    let (mut before, mut after, mut rows) = (Map::new(), Map::new(), Vec::new());
    for (k, pt, e) in CLINIC_FIELDS {
        let Some(Value::String(v)) = args.get(*k) else { continue };
        let mut newv = cut(v, if *k == "address" { 300 } else { 120 });
        if *k == "nif" {
            newv.retain(|c| !c.is_whitespace());
            if !newv.is_empty() && !super::clinic::nif_ok(&newv) {
                return Ok(Out::Retry("That NIF isn't valid (9 digits with a check digit). Ask the user to check it.".into()));
            }
        }
        if *k == "email" { newv = newv.to_lowercase(); }
        if (*k == "name" && newv.is_empty()) || newv == st(&c, k) { continue; }
        rows.push([format!("{} — {}", say(en, "Clínica", "Clinic"), if en { *e } else { *pt }), short(&st(&c, k)), short(&newv)]);
        before.insert(k.to_string(), json!(st(&c, k)));
        after.insert(k.to_string(), json!(newv));
    }
    if after.is_empty() {
        return Ok(Out::Retry("Nothing would change in the clinic's details.".into()));
    }
    Ok(change("clinic_details", None, json!({}), Value::Object(before), Value::Object(after), rows,
        vec![say(en, "Aparece no topo de todos os PDFs feitos a partir de agora.", "It appears at the top of every PDF made from now on.")]))
}

async fn reminder_text(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let (pt, en_t): (String, String) = sqlx::query_as("SELECT reminder_pt, reminder_en FROM clinic WHERE id = 1").fetch_one(&x.s.pool).await?;
    let (mut before, mut after, mut rows) = (Map::new(), Map::new(), Vec::new());
    for (k, arg, cur, lbl, keys) in [("reminder_pt", "pt", &pt, say(en, "Português", "Portuguese"), ["{nome}", "{dia}", "{hora}", "{clinica}"]), ("reminder_en", "en", &en_t, say(en, "Inglês", "English"), ["{name}", "{day}", "{time}", "{clinic}"])] {
        let newv = cut(&st(args, arg), 600);
        if newv.is_empty() || newv == *cur { continue; }
        if let Some(missing) = keys.iter().find(|p| cur.contains(**p) && !newv.contains(**p)) {
            return Ok(Out::Retry(format!("The new {arg} message lost the placeholder {missing}; keep it so each patient gets their own name, day and time.")));
        }
        rows.push([format!("{} — {lbl}", say(en, "Mensagem de lembrete", "Reminder Message")), short(cur), short(&newv)]);
        before.insert(k.into(), json!(cur));
        after.insert(k.into(), json!(newv));
    }
    if after.is_empty() {
        return Ok(Out::Retry("Nothing would change in the reminder message.".into()));
    }
    Ok(change("reminder_text", None, json!({}), Value::Object(before), Value::Object(after), rows, vec![]))
}

async fn ai_switch(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let Some(on) = args["on"].as_bool() else { return Ok(Out::Retry("Say whether to turn it on or off.".into())) };
    let all: Vec<(Uuid, String, String, bool, bool)> = sqlx::query_as("SELECT id, name, role, ai_enabled, active FROM staff").fetch_all(&x.s.pool).await?;
    let q = fold(&st(args, "name"));
    let m: Vec<&(Uuid, String, String, bool, bool)> = all.iter().filter(|s| { let n = fold(&s.1); n == q || (!q.is_empty() && q.split(' ').all(|w| n.split(' ').any(|x| x == w))) }).collect();
    let (id, name, role, cur, active) = match m.as_slice() {
        [one] => (*one).clone(),
        [] => return Ok(Out::Retry(format!("No staff member called \"{}\". Staff: {}.", cut(&st(args, "name"), 40), all.iter().map(|s| s.1.as_str()).collect::<Vec<_>>().join(", ")))),
        _ => return Ok(Out::Retry("Several staff members match: ask which one (full name).".into())),
    };
    if role == "owner" {
        return Ok(Out::Retry("The owner's AI switch is always on.".into()));
    }
    if !active {
        return Ok(Out::Retry(format!("{name} is inactive.")));
    }
    if cur == on {
        return Ok(Out::Retry(format!("{name}'s AI switch is already {}.", if on { "on" } else { "off" })));
    }
    let mut warnings = vec![];
    if on && role == "reception" {
        warnings.push(say(en, "Na receção só abre o assistente (agenda, contactos e pagamentos), nunca dados clínicos.", "For reception it only opens the assistant (schedule, contacts and payments), never clinical data."));
    }
    if on && role == "therapist" {
        warnings.push(say(en, "Abre o assistente, a gravação de sessões e a redação de notas, tudo no computador de IA da clínica.", "It opens the assistant, session recording and note write-up, all on the clinic’s AI PC."));
    }
    Ok(change("staff_ai", Some(id), json!({}), json!({ "ai_enabled": cur }), json!({ "ai_enabled": on }),
        vec![[format!("{name} — {}", say(en, "Interruptor de IA", "AI Switch")), on_off(cur, en), on_off(on, en)]], warnings))
}

async fn website(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let w = match get(x, "/website").await { Ok(v) => v, Err(o) => return Ok(o) };
    let site = &w["site"];
    let (mut before, mut after, mut rows) = (Map::new(), Map::new(), Vec::new());
    for (k, pt, e, boolean) in SITE_FIELDS {
        let (old, new) = if *boolean {
            let Some(b) = args[*k].as_bool() else { continue };
            (site[*k].clone(), json!(b))
        } else {
            let Some(Value::String(v)) = args.get(*k) else { continue };
            let v = cut(v, if k.starts_with("tagline") { 90 } else { 700 });
            if v.is_empty() || v.contains('<') || v.contains('>') { continue; }
            (json!(st(site, k)), json!(v))
        };
        if old == new { continue; }
        let show = |v: &Value| if *boolean { yes_no(v == &json!(true), en) } else { short(v.as_str().unwrap_or("")) };
        rows.push([format!("{} — {}", say(en, "Site", "Website"), if en { *e } else { *pt }), show(&old), show(&new)]);
        before.insert(k.to_string(), old);
        after.insert(k.to_string(), new);
    }
    if after.is_empty() {
        return Ok(Out::Retry("Nothing would change on the website.".into()));
    }
    let mut warnings = vec![say(en, "O site público muda logo que aplicar.", "The public site changes as soon as you apply.")];
    if after.get("published") == Some(&json!(true)) {
        warnings.push(say(en, "Publicar precisa do nome, NIF e um contacto da clínica.", "Publishing needs the clinic’s name, NIF and a contact."));
    }
    Ok(change("website", None, json!({}), Value::Object(before), Value::Object(after), rows, warnings))
}

// ───────── apply and undo ─────────

const CHANGED: &str = "This was changed since the assistant suggested it. Ask the assistant again.";
const CHANGED_AGAIN: &str = "This was changed again since, so it can’t be undone here. Change it by hand.";

fn id(v: &Value, k: &str) -> Option<Uuid> {
    v[k].as_str().and_then(|x| x.parse().ok())
}

/// A row's fields as the previews hold them: text (dates as YYYY-MM-DD, null as ""), booleans as booleans.
fn pick(row: &Value, keys: &[String]) -> Value {
    let mut out = Map::new();
    for k in keys {
        let v = match &row[k.as_str()] { Value::Null => json!(""), Value::Bool(b) => json!(b), v => json!(st(&json!({ "v": v }), "v")) };
        out.insert(k.clone(), v);
    }
    Value::Object(out)
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default()
}

/// The current values of the fields a change touches.
async fn cur(s: &AppState, skill: &str, target: Option<Uuid>, ks: &[String]) -> ApiResult<Value> {
    let sql = match skill {
        "document_update" => "SELECT to_jsonb(x) FROM documents x WHERE id = $1",
        "letter_update" => "SELECT to_jsonb(x) FROM discharge_letters x WHERE id = $1 AND approved_at IS NULL",
        "complaint_update" => "SELECT to_jsonb(x) FROM complaints x WHERE id = $1",
        "clinic_details" | "reminder_text" => "SELECT to_jsonb(x) - 'logo' FROM clinic x WHERE id = 1 AND ($1::uuid IS NULL OR TRUE)",
        "staff_ai" => "SELECT to_jsonb(x) - 'password_hash' FROM staff x WHERE id = $1",
        "website" => "SELECT to_jsonb(x) FROM website x WHERE id = 1 AND ($1::uuid IS NULL OR TRUE)",
        "consent_revoke" => "SELECT jsonb_build_object('revoked', revoked_at IS NOT NULL) FROM consents WHERE id = $1",
        _ => return Ok(Value::Null),
    };
    let row: Option<(Value,)> = sqlx::query_as(sql).bind(target).fetch_optional(&s.pool).await?;
    // A dated column comes back as "2026-11-13"; a timestamp isn't among the fields compared here.
    Ok(row.map(|r| pick(&r.0, ks)).unwrap_or(Value::Null))
}

/// A fingerprint of a row the assistant created, so Undo only removes it while it holds what it held then
/// (`updated_at` aside: a change that was itself undone doesn't block it).
async fn fingerprint(s: &AppState, table: &str, rid: &str) -> ApiResult<String> {
    let v: Option<(String,)> = sqlx::query_as(&format!("SELECT md5((to_jsonb(x) - 'updated_at')::text) FROM {table} x WHERE id::text = $1")).bind(rid).fetch_optional(&s.pool).await?;
    Ok(v.map(|x| x.0).unwrap_or_default())
}

/// The endpoint and body that set the state `v` (Apply with `after`, Undo with `before`).
async fn setter(s: &AppState, skill: &str, target: Option<Uuid>, v: &Value) -> ApiResult<(&'static str, String, Value)> {
    let t = target.map(|t| t.to_string()).unwrap_or_default();
    Ok(match skill {
        "document_update" => ("PATCH", format!("/documents/{t}"), v.clone()),
        "letter_update" => ("PATCH", format!("/letters/{t}"), v.clone()),
        "complaint_update" => ("PATCH", format!("/complaints/{t}"), v.clone()),
        "staff_ai" => ("PATCH", format!("/staff/{t}"), v.clone()),
        "website" => ("PUT", "/website".into(), v.clone()),
        "clinic_details" => {
            // The app's form sends every field (the logo is left alone when absent).
            let mut body = cur(s, skill, None, &CLINIC_FIELDS.iter().map(|f| f.0.to_string()).collect::<Vec<_>>()).await?;
            for (k, x) in v.as_object().cloned().unwrap_or_default() { body[k] = x; }
            ("PUT", "/clinic".into(), body)
        }
        "reminder_text" => {
            let c = cur(s, skill, None, &["reminder_pt".into(), "reminder_en".into()]).await?;
            let pick = |k: &str| v.get(k).cloned().unwrap_or_else(|| c[k].clone());
            ("PUT", "/reminders/template".into(), json!({ "pt": pick("reminder_pt"), "en": pick("reminder_en") }))
        }
        _ => return Err(AppError::NotFound),
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn apply(s: &AppState, a: &Auth, headers: &HeaderMap, tx: Tx<'_>, cid: Uuid, skill: &str, target: Option<Uuid>, params: &Value, before: &Value, after: &Value) -> ApiResult<Response> {
    let c = Caller::from(headers);
    let pid = id(params, "patient_id");
    match skill {
        "letter_draft" => {
            let pid = pid.ok_or(AppError::NotFound)?;
            let (status, v) = call(s, &c, "POST", &format!("/patients/{pid}/letters"), Some(&params["body"])).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            let lid = st(&v, "id");
            let fp = fingerprint(s, "discharge_letters", &lid).await?;
            finish(s, a, tx, cid, "applied", &json!({ "letter_id": lid, "fp": fp }), skill, Some(pid)).await
        }
        "pack" => {
            let pid = pid.ok_or(AppError::NotFound)?;
            let (status, v) = call(s, &c, "POST", &format!("/patients/{pid}/reimbursement"), Some(&params["body"])).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            let doc = st(&v, "document_id");
            let fp = fingerprint(s, "documents", &doc).await?;
            finish(s, a, tx, cid, "applied", &json!({ "document_id": doc, "filename": v["filename"], "fp": fp }), skill, Some(pid)).await
        }
        "complaint_create" => {
            let (status, v) = call(s, &c, "POST", "/complaints", Some(&params["body"])).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            let cid2 = st(&v, "id");
            let fp = fingerprint(s, "complaints", &cid2).await?;
            finish(s, a, tx, cid, "applied", &json!({ "complaint_id": cid2, "due_on": v["due_on"], "fp": fp }), skill, None).await
        }
        "consent_revoke" => {
            if cur(s, skill, target, &keys(before)).await? != *before { return Err(AppError::Conflict(CHANGED)); }
            let (status, v) = call(s, &c, "POST", &format!("/consents/{}/revoke", target.map(|t| t.to_string()).unwrap_or_default()), Some(&json!({}))).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            finish(s, a, tx, cid, "applied", after, skill, pid).await
        }
        "reminder_mark" => {
            let channel = params["channel"].clone();
            for it in params["items"].as_array().cloned().unwrap_or_default() {
                let body = json!({ "appointment_id": it["appointment_id"], "patient_id": it["patient_id"], "channel": channel, "sent": true });
                let (status, v) = call(s, &c, "POST", "/reminders/sent", Some(&body)).await;
                if !ok(status) { return Ok(passed_on(status, v)); }
            }
            finish(s, a, tx, cid, "applied", after, skill, None).await
        }
        _ => {
            if cur(s, skill, target, &keys(before)).await? != *before { return Err(AppError::Conflict(CHANGED)); }
            let (m, path, body) = setter(s, skill, target, after).await?;
            let (status, v) = call(s, &c, m, &path, Some(&body)).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            finish(s, a, tx, cid, "applied", after, skill, pid).await
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn undo(s: &AppState, a: &Auth, headers: &HeaderMap, tx: Tx<'_>, cid: Uuid, skill: &str, target: Option<Uuid>, params: &Value, before: &Value, after: &Value) -> ApiResult<Response> {
    let c = Caller::from(headers);
    let pid = id(params, "patient_id");
    let pass = |status: u16, v: Value| -> Option<Response> { (!ok(status)).then(|| passed_on(status, v)) };
    // Something the assistant created: removed while nobody changed it since.
    let cr = &c;
    let created = |table: &'static str, key: &'static str, path: &'static str| async move {
        let rid = st(after, key);
        let fp = fingerprint(s, table, &rid).await?;
        if fp.is_empty() { return Err(AppError::NotFound); }
        if fp != st(after, "fp") { return Err(AppError::Conflict(CHANGED_AGAIN)); }
        Ok::<_, AppError>(call(s, cr, "DELETE", &format!("{path}/{rid}"), None).await)
    };
    match skill {
        "consent_revoke" => Err(AppError::Conflict("A withdrawn consent stays withdrawn. Ask the patient to sign it again if needed.")),
        "letter_draft" => {
            let (status, v) = created("discharge_letters", "letter_id", "/letters").await?;
            if let Some(r) = pass(status, v) { return Ok(r); }
            finish(s, a, tx, cid, "undone", after, skill, pid).await
        }
        "pack" => {
            let (status, v) = created("documents", "document_id", "/documents").await?;
            if let Some(r) = pass(status, v) { return Ok(r); }
            finish(s, a, tx, cid, "undone", after, skill, pid).await
        }
        "complaint_create" => {
            let (status, v) = created("complaints", "complaint_id", "/complaints").await?;
            if let Some(r) = pass(status, v) { return Ok(r); }
            finish(s, a, tx, cid, "undone", after, skill, None).await
        }
        "reminder_mark" => {
            // Only the ones that weren't marked before go back to "not reminded".
            let was = before["sent"].as_array().cloned().unwrap_or_default();
            for (i, it) in params["items"].as_array().cloned().unwrap_or_default().into_iter().enumerate() {
                if was.get(i) == Some(&json!(true)) { continue; }
                let body = json!({ "appointment_id": it["appointment_id"], "patient_id": it["patient_id"], "sent": false });
                let (status, v) = call(s, &c, "POST", "/reminders/sent", Some(&body)).await;
                if let Some(r) = pass(status, v) { return Ok(r); }
            }
            finish(s, a, tx, cid, "undone", after, skill, None).await
        }
        _ => {
            // A letter change's "ai_drafted" mark isn't put back (the app keeps it once set).
            let mut aft = after.clone();
            if let Some(o) = aft.as_object_mut() { o.remove("ai_drafted"); }
            if cur(s, skill, target, &keys(&aft)).await? != aft { return Err(AppError::Conflict(CHANGED_AGAIN)); }
            let (m, path, body) = setter(s, skill, target, before).await?;
            let (status, v) = call(s, &c, m, &path, Some(&body)).await;
            if let Some(r) = pass(status, v) { return Ok(r); }
            finish(s, a, tx, cid, "undone", after, skill, pid).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_people_type() {
        assert_eq!(bare("ref 3f9c2a1b"), "3f9c2a1b");
        assert_eq!(bare("3F9C-2A1B"), "3f9c2a1b");
        assert_eq!(bare("12"), "");
        let list = vec![json!({ "id": "3f9c2a1b-0000-4000-8000-000000000000" }), json!({ "id": "3f9d0000-0000-4000-8000-000000000000" })];
        assert!(by_ref(&list, "3f9c2a").is_some());
        assert!(by_ref(&list, "3f9c").is_none(), "too short");
        assert!(by_ref(&list, "ffffff").is_none());
    }

    #[test]
    fn fields_as_previews_hold_them() {
        let row = json!({ "replied_on": null, "sent_to_ers_on": "2026-11-12", "published": true, "reply": "Ok" });
        let k = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick(&row, &k(&["replied_on", "sent_to_ers_on", "published", "reply"])), json!({ "replied_on": "", "sent_to_ers_on": "2026-11-12", "published": true, "reply": "Ok" }));
    }
}
