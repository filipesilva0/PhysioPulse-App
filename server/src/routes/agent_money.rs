//! The assistant, stage 2 (owner, 13 Nov 2026: "build it in stages"): money and staff.
//!
//! Same rules as stage 1 (`agent.rs`): look-ups and Apply go through the app's own endpoints as the person, so
//! the server's role checks decide (everyone at the desk: a patient's packages, payments and what they owe, the
//! price list, recording a payment, selling or pausing a package, the invoice number, voiding their own payment on
//! the same day; the owner only: all payments, the owed list, expenses, salaries, waiving a charge, closing a
//! month). Every change is a before/after preview that waits for Apply and can be undone while nothing else changed
//! it — except a void: payments are fiscal records, a voided one stays voided.

use super::agent::{call, cut, finish, ok, or_dash, passed_on, say, st, which_patient, Caller, Ctx, Out, Tx, MONTHS};
use super::agenda::short_ref;
use super::assist::fold;
use super::assistant::Proposal;
use crate::auth::{AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{http::HeaderMap, response::Response};
use serde_json::{json, Map, Value};
use time::Date;
use uuid::Uuid;

/// The changes this module applies (and undoes) itself.
pub const SKILLS: &[&str] = &["payment", "payment_void", "invoice_number", "plan_sell", "plan_status", "expense", "salary", "waive", "month_close", "month_reopen"];
/// Of those, the owner's only (also refused by the endpoints for anyone else).
pub const OWNER_ONLY: &[&str] = &["expense", "salary", "waive", "month_close", "month_reopen"];
/// Changes that can't be undone (shown without Undo).
pub const NO_UNDO: &[&str] = &["payment_void"];

const LOOKUPS_ALL: &[&str] = &["patient_money", "price_list"];
const LOOKUPS_OWNER: &[&str] = &["owed_list", "payments", "expenses", "salaries", "finance_overview"];
const CHANGES_ALL: &[&str] = &["record_payment", "sell_package", "package_status", "invoice_number", "void_payment"];
const CHANGES_OWNER: &[&str] = &["add_expense", "pay_salary", "waive_charge", "close_month"];

fn is_owner(a: &Auth) -> bool {
    a.staff.role == "owner"
}

/// Whether `name` is one of this module's tools that this person may use.
pub fn handles(name: &str, a: &Auth) -> bool {
    LOOKUPS_ALL.contains(&name) || CHANGES_ALL.contains(&name) || (is_owner(a) && (LOOKUPS_OWNER.contains(&name) || CHANGES_OWNER.contains(&name)))
}

// ───────── amounts, months, words ─────────

/// Cents as the person reads them: "35,50 €" / "€35.50".
pub(super) fn eur(c: i64, en: bool) -> String {
    let (sign, c) = if c < 0 { ("-", -c) } else { ("", c) };
    if en { format!("{sign}€{}.{:02}", c / 100, c % 100) } else { format!("{sign}{},{:02} €", c / 100, c % 100) }
}
/// For the model: always "€35.50".
fn e(c: i64) -> String {
    eur(c, true)
}

/// An amount in euros from the model (35, 35.5, "35,50", "35 €") → cents.
fn cents(v: &Value) -> Option<i64> {
    let x = match v {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.replace(['€', ' '], "").replace(',', ".").parse::<f64>().ok()?,
        _ => return None,
    };
    (x.is_finite() && x > 0.0 && x <= 100_000.0).then(|| (x * 100.0).round() as i64)
}

/// "2026-11", "11/2026", "novembro", "novembro de 2026", "este mês", "mês passado", "2026-11-05" → "2026-11".
pub(super) fn month_of(text: &str, today: Date) -> Option<String> {
    let f = fold(text);
    let this = (today.year(), today.month() as u8);
    let prev = if this.1 == 1 { (this.0 - 1, 12) } else { (this.0, this.1 - 1) };
    let fmt = |(y, m): (i32, u8)| format!("{y:04}-{m:02}");
    if f.is_empty() || matches!(f.as_str(), "este mes" | "this month" | "mes atual" | "current month") {
        return Some(fmt(this));
    }
    if matches!(f.as_str(), "mes passado" | "last month" | "mes anterior" | "previous month") {
        return Some(fmt(prev));
    }
    let nums: Vec<i32> = f.split(' ').filter_map(|w| w.parse().ok()).collect();
    if let Some(m) = f.split(' ').find_map(|w| MONTHS.iter().find(|(n, _)| *n == w).map(|(_, m)| *m)) {
        let y = nums.iter().find(|n| **n >= 2000).copied().unwrap_or(if m > this.1 { this.0 - 1 } else { this.0 });
        return Some(fmt((y, m)));
    }
    let t = text.trim();
    let p: Vec<&str> = t.split(['-', '/', '.']).collect();
    match p.as_slice() {
        [y, m] | [y, m, _] if y.len() == 4 => Some(fmt((y.parse().ok()?, m.parse::<u8>().ok().filter(|m| (1..=12).contains(m))?))),
        [m, y] if y.len() == 4 => Some(fmt((y.parse().ok()?, m.parse::<u8>().ok().filter(|m| (1..=12).contains(m))?))),
        _ => None,
    }
}

fn month_label(m: &str, en: bool) -> String {
    let n: usize = m.get(5..7).and_then(|x| x.parse().ok()).unwrap_or(1).clamp(1, 12);
    let names = if en { ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"] }
                else { ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"] };
    if en { format!("{} {}", names[n - 1], &m[..4]) } else { format!("{} de {}", names[n - 1], &m[..4]) }
}

fn day_label(iso: &str) -> String {
    if iso.len() >= 10 { format!("{}/{}/{}", &iso[8..10], &iso[5..7], &iso[..4]) } else { iso.to_string() }
}

/// How it was paid, in the words people use → the app's code.
fn method(text: &str, expense: bool) -> Option<&'static str> {
    let f = fold(text);
    let m = match f.as_str() {
        "" => return None,
        x if x.contains("mb way") || x.contains("mbway") => "mbway",
        x if x.contains("transfer") || x.split(' ').any(|w| w == "iban") => "transfer",
        x if x.contains("debit") || x.contains("debito direto") => if expense { "debit" } else { "card" },
        x if x.contains("cart") || x.contains("card") || x.contains("multibanco") || x.contains("tpa") || x.contains("atm") => "card",
        x if x.contains("cash") || x.contains("numerario") || x.contains("dinheiro") || x.contains("notas") => "cash",
        _ => return None,
    };
    Some(m)
}

fn method_label(m: &str, en: bool) -> String {
    match (m, en) {
        ("cash", false) => "Numerário", ("cash", true) => "Cash", ("mbway", _) => "MB WAY", ("transfer", false) => "Transferência", ("transfer", true) => "Transfer",
        ("card", false) => "Cartão (TPA)", ("card", true) => "Card", ("debit", false) => "Débito direto", ("debit", true) => "Direct Debit",
        ("stripe", _) => "Stripe", (_, false) => "Outro", (_, true) => "Other",
    }.to_string()
}

const CATEGORIES: &[(&str, &str, &str, &[&str])] = &[
    ("rent", "Renda", "Rent", &["renda", "rent", "aluguer"]),
    ("utilities", "Eletricidade, água e gás", "Utilities", &["eletricidade", "electricidade", "luz", "agua", "gas", "edp", "utilities", "electricity", "water"]),
    ("telecom", "Telecomunicações", "Telecoms", &["telecom", "internet", "telefone", "telemovel", "meo", "nos", "vodafone", "phone"]),
    ("supplies", "Consumíveis clínicos", "Clinical Supplies", &["consumiveis", "supplies", "material clinico", "ligaduras", "creme", "gel", "kinesio"]),
    ("equipment", "Equipamento", "Equipment", &["equipamento", "equipment", "marquesa", "aparelho"]),
    ("software", "Software e subscrições", "Software and Subscriptions", &["software", "subscricao", "subscription", "licenca"]),
    ("insurance", "Seguros", "Insurance", &["seguro", "insurance"]),
    ("training", "Formação", "Training", &["formacao", "curso", "training", "course", "congresso"]),
    ("marketing", "Marketing", "Marketing", &["marketing", "publicidade", "anuncio", "advertising"]),
    ("accounting", "Contabilidade e serviços", "Accounting and Services", &["contabilidade", "contabilista", "accounting", "accountant", "servicos"]),
    ("office", "Material de escritório", "Office Supplies", &["escritorio", "office", "papel", "toner"]),
    ("bank", "Comissões bancárias", "Bank Fees", &["banco", "bancaria", "comissao", "bank", "fee"]),
    ("taxes", "Impostos e taxas", "Taxes and Fees", &["imposto", "taxa", "tax", "iuc", "imi"]),
    ("other", "Outras", "Other", &["outra", "outro", "other"]),
];

fn category(text: &str) -> Option<&'static str> {
    let f = fold(text);
    if f.is_empty() {
        return None;
    }
    CATEGORIES.iter().find(|c| c.0 == f || fold(c.1) == f || fold(c.2) == f).map(|c| c.0)
        .or_else(|| CATEGORIES.iter().find(|c| c.3.iter().any(|w| f.split(' ').any(|x| x == *w) || f.contains(w) && w.len() > 4)).map(|c| c.0))
}

fn category_label(c: &str, en: bool) -> String {
    CATEGORIES.iter().find(|x| x.0 == c).map(|x| if en { x.2 } else { x.1 }).unwrap_or("—").to_string()
}

fn kind_label(k: &str, en: bool) -> String {
    match k {
        "session" => say(en, "Sessão", "Session"), "assessment" => say(en, "Avaliação", "Assessment"), "home" => say(en, "Domicílio", "Home Visit"),
        "class" => say(en, "Aula", "Class"), "pack" => say(en, "Pacote", "Package"), "monthly" => say(en, "Plano mensal", "Monthly Plan"),
        _ => k.to_string(),
    }
}

fn name_of(v: &Value, en: bool) -> String {
    let n = if en { st(v, "name_en") } else { st(v, "name_pt") };
    if n.is_empty() { st(v, "name_pt") } else { n }
}

/// "ap:3f9c2a1b", "#3f9c2a1b", "3F9C2A1B" → "3f9c2a1b".
fn bare_ref(r: &str) -> String {
    let r = r.trim().to_ascii_lowercase();
    let r = r.rsplit([':', '#', ' ']).next().unwrap_or("").to_string();
    r.split('/').next().unwrap_or("").to_string()
}

fn id_matches(id: &str, r: &str) -> bool {
    let r = bare_ref(r);
    r.len() >= 6 && id.replace('-', "").starts_with(&r)
}

fn number_in(text: &str) -> Option<i64> {
    let digits: String = text.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

// ───────── the tools ─────────

fn tool(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({ "type": "function", "function": { "name": name, "description": desc, "parameters": { "type": "object", "properties": props, "required": required } } })
}
fn s_(d: &str) -> Value { json!({ "type": "string", "description": d }) }
fn n_(d: &str) -> Value { json!({ "type": "number", "description": d }) }

/// The money and staff tools this person gets (same reach as their role in the app).
pub fn tools(a: &Auth, t: &mut Vec<Value>) {
    let pat = "Patient's name, or nº and number (e.g. \"nº 12\")";
    let methods = json!({ "type": "string", "enum": ["cash", "mbway", "transfer", "card"], "description": "How it was paid" });
    t.push(tool("patient_money", "A patient's money: packages and monthly plans (sessions left, expiry, paid), what they owe (each item with its ref) and their payments (with the payment nº and invoice number).",
        json!({ "patient": s_(pat) }), &["patient"]));
    t.push(tool("price_list", "The clinic's price list: sessions, assessments, home visits, classes, packages and monthly plans.", json!({}), &[]));
    t.push(tool("record_payment", "Prepare recording money received from a patient. By default it pays everything they owe; or one item (ref from patient_money), a package, a month of a monthly plan, or something else described in words. Look at patient_money first.",
        json!({ "patient": s_(pat), "method": methods, "pay_for": { "type": "string", "enum": ["all_owed", "item", "package", "month", "other"], "description": "What is being paid (default all_owed)" },
                "ref": s_("For an item, package or month: its ref from patient_money"), "package": s_("Package or plan name, if no ref"), "month": s_("For a monthly plan: YYYY-MM"),
                "amount_eur": n_("Amount in euros, only if the user said it (default: what's owed)"), "description": s_("Only for pay_for=other: what it's for"),
                "date": s_("Day it was received, if not today"), "invoice_number": s_("Invoice number from the invoicing software, if said") }), &["patient", "method"]));
    t.push(tool("sell_package", "Prepare selling a package or monthly plan from the price list to a patient (the payment is recorded separately).",
        json!({ "patient": s_(pat), "product": s_("Name as in the price list"), "start_date": s_("Start day, if not today"), "share_with": s_("Another patient who shares it, if said") }), &["patient", "product"]));
    t.push(tool("package_status", "Prepare pausing or resuming a monthly plan, or ending a package or plan.",
        json!({ "patient": s_(pat), "package": s_("Its name or ref from patient_money"), "status": { "type": "string", "enum": ["paused", "active", "ended"] } }), &["patient", "status"]));
    t.push(tool("invoice_number", "Prepare adding (or correcting) the invoice number of a payment, from the certified invoicing software.",
        json!({ "payment": s_("The payment nº from patient_money or payments"), "invoice_number": s_("The invoice number, e.g. FT 2026/123") }), &["payment", "invoice_number"]));
    t.push(tool("void_payment", "Prepare voiding a payment recorded by mistake (it stays in the records, marked void; this can't be undone).",
        json!({ "payment": s_("The payment nº"), "reason": s_("Why, in Portuguese") }), &["payment", "reason"]));
    if is_owner(a) {
        let month = "Month: YYYY-MM (default this month)";
        t.push(tool("owed_list", "Everyone who owes money, with how much, oldest first.", json!({}), &[]));
        t.push(tool("payments", "Payments received between two days (default: this month), with nº, patient, amount, method, invoice.",
            json!({ "from": s_("First day"), "to": s_("Last day"), "method": methods, "uninvoiced": { "type": "boolean", "description": "Only those without an invoice number" } }), &[]));
        t.push(tool("expenses", "Expenses between two days (default: this month).", json!({ "from": s_("First day"), "to": s_("Last day") }), &[]));
        t.push(tool("salaries", "Staff pay settings and the salaries recorded for a month.", json!({ "month": s_(month) }), &[]));
        t.push(tool("finance_overview", "A month's figures: revenue, expenses, salaries, result, by method, uninvoiced, owed; with the previous month.", json!({ "month": s_(month) }), &[]));
        t.push(tool("add_expense", "Prepare recording an expense (a bill, a purchase).",
            json!({ "amount_eur": n_("Total in euros"), "date": s_("Day it was paid (default today)"),
                    "category": { "type": "string", "enum": CATEGORIES.iter().map(|c| c.0).collect::<Vec<_>>() }, "supplier": s_("Who was paid"), "description": s_("What it was"),
                    "method": { "type": "string", "enum": ["cash", "mbway", "transfer", "card", "debit"] }, "vat_eur": n_("VAT in euros, if said"), "document_ref": s_("Invoice/receipt number, if said") }),
            &["amount_eur", "category"]));
        t.push(tool("pay_salary", "Prepare recording a salary paid to a staff member for a month. Default amount: their fixed monthly pay.",
            json!({ "name": s_("Their name"), "month": s_(month), "amount_eur": n_("Gross amount, if different from their fixed pay"), "date": s_("Day it was paid (default today)"),
                    "method": { "type": "string", "enum": ["transfer", "cash"] } }), &["name"]));
        t.push(tool("waive_charge", "Prepare waiving (or charging again) a done appointment's charge, so the patient doesn't owe it.",
            json!({ "ref": s_("The appointment's ref (from patient_money, agenda or open_patient)"), "waive": { "type": "boolean", "description": "true = don't charge (default), false = charge again" } }), &["ref"]));
        t.push(tool("close_month", "Prepare closing a finished month (its payments, expenses and salaries are then locked), or reopening a closed one.",
            json!({ "month": s_("YYYY-MM"), "reopen": { "type": "boolean" } }), &["month"]));
    }
}

/// A line of the brief about money (only for people who have these tools).
pub fn brief_rules() -> &'static str {
    "\n- Money: amounts are euros. Before recording a payment, look at patient_money (what they owe, with refs). Never invent an amount, a method or a payment nº; ask how it was paid if not said."
}

// ───────── look-ups ─────────

async fn get(x: &Ctx<'_>, path: &str) -> Result<Value, Out> {
    let (status, v) = call(x.s, &x.c, "GET", path, None).await;
    if ok(status) { Ok(v) } else { Err(Out::Retry(st(&v, "message"))) }
}

fn plan_line(p: &Value, en: bool) -> String {
    let mut l = format!("package ref {} · {} · {} · {}", short_ref_s(&st(p, "id")), name_of(p, en), st(p, "kind"), st(p, "status"));
    if st(p, "kind") == "pack" {
        l.push_str(&format!(" · {} of {} sessions left", st(p, "remaining"), st(p, "sessions")));
    } else {
        l.push_str(&format!(" · {} used this month", st(p, "used_month")));
    }
    if !st(p, "expires_on").is_empty() {
        l.push_str(&format!(" · expires {}", st(p, "expires_on")));
    }
    l.push_str(&format!(" · price {} · paid {}", e(p["price_cents"].as_i64().unwrap_or(0)), e(p["paid_cents"].as_i64().unwrap_or(0))));
    if !st(p, "shared_with_name").is_empty() {
        l.push_str(&format!(" · shared with {}", st(p, "shared_with_name")));
    }
    l
}

fn short_ref_s(id: &str) -> String {
    id.parse::<Uuid>().map(short_ref).unwrap_or_default()
}

/// An owed item's ref: the appointment's (like in the agenda), the package's, or "package/YYYY-MM" for a month.
fn owed_ref(o: &Value) -> String {
    match st(o, "type").as_str() {
        "appointment" => short_ref_s(&st(o, "appointment_id")),
        "month" => format!("{}/{}", short_ref_s(&st(o, "plan_id")), st(o, "month").get(..7).unwrap_or("")),
        _ => short_ref_s(&st(o, "plan_id")),
    }
}

fn owed_what(o: &Value, en: bool) -> String {
    match st(o, "type").as_str() {
        "appointment" => format!("{} {}", kind_label(&st(o, "what"), en), day_label(&st(o, "day"))),
        "month" => format!("{} — {}", name_of(o, en), month_label(st(o, "month").get(..7).unwrap_or(""), en)),
        _ => name_of(o, en),
    }
}

/// An owed item for the model: the kind in English (like the agenda), package names as the person reads them.
fn for_model(o: &Value, en: bool) -> String {
    if st(o, "type") == "appointment" { owed_what(o, true) } else { owed_what(o, en) }
}

fn payment_line(p: &Value) -> String {
    format!("payment nº {} · {} · {} · {} · {}{}{}", st(p, "number"), st(p, "received_on"), e(p["amount_cents"].as_i64().unwrap_or(0)), st(p, "method"), st(p, "description"),
        if st(p, "invoice_number").is_empty() { " · no invoice number yet".to_string() } else { format!(" · invoice {}", st(p, "invoice_number")) },
        if st(p, "voided_at").is_empty() { String::new() } else { format!(" · VOID ({})", cut(&st(p, "void_reason"), 80)) })
}

pub(super) async fn run(x: &Ctx<'_>, name: &str, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let range = |k: &str| x.date(&st(args, k)).map(super::agent::iso).unwrap_or_default();
    Ok(match name {
        "patient_money" => {
            let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
            let v = match get(x, &format!("/patients/{}/money", p.id)).await { Ok(v) => v, Err(o) => return Ok(o) };
            let mut text = format!("Money of {} · owes {} in total", p.label(), e(v["due_cents"].as_i64().unwrap_or(0)));
            let owed = v["owed"].as_array().cloned().unwrap_or_default();
            text.push_str("\nOwed:");
            for o in &owed {
                text.push_str(&format!("\n- ref {} · {} · {} due{}", owed_ref(o), for_model(o, en), e(o["due_cents"].as_i64().unwrap_or(0)),
                    if o["due_cents"] != o["price_cents"] { format!(" (of {})", e(o["price_cents"].as_i64().unwrap_or(0))) } else { String::new() }));
            }
            if owed.is_empty() { text.push_str(" nothing"); }
            let plans = v["plans"].as_array().cloned().unwrap_or_default();
            text.push_str("\nPackages and plans:");
            for pl in plans.iter().filter(|p| st(p, "status") != "cancelled").take(10) {
                text.push_str(&format!("\n- {}", plan_line(pl, en)));
            }
            if plans.is_empty() { text.push_str(" none"); }
            let pays = v["payments"].as_array().cloned().unwrap_or_default();
            text.push_str("\nPayments (newest first):");
            for py in pays.iter().take(12) {
                text.push_str(&format!("\n- {}", payment_line(py)));
            }
            if pays.is_empty() { text.push_str(" none"); }
            Out::Read { text, step: if en { format!("Looked at {}’s payments", p.label()) } else { format!("Viu os pagamentos de {}", p.label()) }, list: None }
        }
        "price_list" => {
            let v = match get(x, "/products").await { Ok(v) => v, Err(o) => return Ok(o) };
            let text = v.as_array().cloned().unwrap_or_default().iter().map(|p| {
                let mut l = format!("{} · {} · {}", name_of(p, en), st(p, "kind"), e(p["price_cents"].as_i64().unwrap_or(0)));
                if !st(p, "sessions").is_empty() { l.push_str(&format!(" · {} sessions", st(p, "sessions"))); }
                if !st(p, "valid_months").is_empty() { l.push_str(&format!(" · valid {} months", st(p, "valid_months"))); }
                if p["shareable"] == json!(true) { l.push_str(" · can be shared"); }
                l
            }).collect::<Vec<_>>().join("\n");
            Out::Read { text: format!("Price list (on sale):\n{text}"), step: say(en, "Viu a tabela de preços", "Looked at the price list"), list: None }
        }
        "owed_list" => {
            let v = match get(x, "/finance/owed").await { Ok(v) => v, Err(o) => return Ok(o) };
            let mut by: Vec<(String, String, i64, usize)> = vec![];
            for o in v.as_array().cloned().unwrap_or_default() {
                let k = format!("{} (nº {})", st(&o, "patient_name"), st(&o, "patient_number"));
                let d = o["due_cents"].as_i64().unwrap_or(0);
                match by.iter_mut().find(|b| b.0 == k) { Some(b) => { b.2 += d; b.3 += 1; } None => by.push((k, st(&o, "day"), d, 1)) }
            }
            let total: i64 = by.iter().map(|b| b.2).sum();
            let text = by.iter().take(40).map(|b| format!("{} · {} · {} item(s) · oldest {}", b.0, e(b.2), b.3, b.1)).collect::<Vec<_>>().join("\n");
            Out::Read { text: format!("Owed in total: {} by {} patient(s)\n{text}", e(total), by.len()), step: say(en, "Viu quem deve dinheiro", "Looked at who owes money"), list: None }
        }
        "payments" => {
            let mut q = format!("/payments?from={}&to={}", range("from"), range("to"));
            if let Some(m) = method(&st(args, "method"), false) { q.push_str(&format!("&method={m}")); }
            if args["uninvoiced"].as_bool() == Some(true) { q.push_str("&uninvoiced=true"); }
            let v = match get(x, &q).await { Ok(v) => v, Err(o) => return Ok(o) };
            let rows = v.as_array().cloned().unwrap_or_default();
            let total: i64 = rows.iter().filter(|p| st(p, "voided_at").is_empty()).filter_map(|p| p["amount_cents"].as_i64()).sum();
            let text = rows.iter().take(60).map(|p| format!("{} · {}", payment_line(p), or_dash(&st(p, "patient_name")))).collect::<Vec<_>>().join("\n");
            Out::Read { text: format!("{} payment(s), {} received (voids excluded){}\n{text}", rows.len(), e(total), if rows.len() > 60 { ", showing 60" } else { "" }),
                step: say(en, "Viu os pagamentos recebidos", "Looked at payments received"), list: None }
        }
        "expenses" => {
            let v = match get(x, &format!("/expenses?from={}&to={}", range("from"), range("to"))).await { Ok(v) => v, Err(o) => return Ok(o) };
            let rows = v.as_array().cloned().unwrap_or_default();
            let total: i64 = rows.iter().filter_map(|p| p["amount_cents"].as_i64()).sum();
            let text = rows.iter().take(60).map(|r| format!("{} · {} · {} · {} · {}{}", st(r, "spent_on"), e(r["amount_cents"].as_i64().unwrap_or(0)), category_label(&st(r, "category"), true),
                or_dash(&st(r, "supplier")), or_dash(&st(r, "description")), if st(r, "method").is_empty() { String::new() } else { format!(" · {}", st(r, "method")) })).collect::<Vec<_>>().join("\n");
            Out::Read { text: format!("{} expense(s), {} in total\n{text}", rows.len(), e(total)), step: say(en, "Viu as despesas", "Looked at expenses"), list: None }
        }
        "salaries" => {
            let Some(m) = month_of(&st(args, "month"), x.today) else { return Ok(Out::Retry("Give the month as YYYY-MM.".into())) };
            let v = match get(x, &format!("/finance/salaries?month={m}")).await { Ok(v) => v, Err(o) => return Ok(o) };
            let mut text = format!("Salaries {m}{} · employer Social Security {}%:", if v["closed"] == json!(true) { " (month closed)" } else { "" }, v["employer_ss_bp"].as_f64().unwrap_or(0.0) / 100.0);
            for s in v["staff"].as_array().cloned().unwrap_or_default() {
                let paid: Vec<String> = s["paid"].as_array().cloned().unwrap_or_default().iter()
                    .map(|p| format!("{} gross + {} SS on {}", e(p["gross_cents"].as_i64().unwrap_or(0)), e(p["employer_ss_cents"].as_i64().unwrap_or(0)), st(p, "paid_on"))).collect();
                text.push_str(&format!("\n- {} · {} · fixed pay {} · {}", st(&s, "name"), st(&s, "role"), e(s["fixed_cents"].as_i64().unwrap_or(0)),
                    if paid.is_empty() { "not paid yet".into() } else { format!("paid: {}", paid.join("; ")) }));
            }
            Out::Read { text, step: if en { format!("Looked at salaries for {}", month_label(&m, en)) } else { format!("Viu os salários de {}", month_label(&m, en)) }, list: None }
        }
        "finance_overview" => {
            let Some(m) = month_of(&st(args, "month"), x.today) else { return Ok(Out::Retry("Give the month as YYYY-MM.".into())) };
            let v = match get(x, &format!("/finance/overview?month={m}")).await { Ok(v) => v, Err(o) => return Ok(o) };
            let c = |v: &Value, k: &str| e(v[k].as_i64().unwrap_or(0));
            let p = &v["previous"];
            let by: Vec<String> = v["by_method"].as_object().map(|o| o.iter().map(|(k, n)| format!("{k} {}", e(n.as_i64().unwrap_or(0)))).collect()).unwrap_or_default();
            let text = format!("Month {m}{}: revenue {} ({} payments; previous month {}), expenses {} (prev {}), salaries {} + employer SS {} (prev {} + {}), result {} (prev {}). By method: {}. Without invoice number: {} payments, {}. Owed now: {} by {} patient(s).",
                if v["closed"].is_null() { "" } else { " (closed)" }, c(&v, "revenue_cents"), st(&v, "payments"), c(p, "revenue_cents"), c(&v, "expenses_cents"), c(p, "expenses_cents"),
                c(&v, "salaries_cents"), c(&v, "employer_ss_cents"), c(p, "salaries_cents"), c(p, "employer_ss_cents"), c(&v, "result_cents"), c(p, "result_cents"),
                if by.is_empty() { "—".into() } else { by.join(", ") }, st(&v, "uninvoiced"), c(&v, "uninvoiced_cents"), c(&v, "owed_cents"), st(&v, "owed_patients"));
            Out::Read { text, step: if en { format!("Looked at the figures for {}", month_label(&m, en)) } else { format!("Viu os números de {}", month_label(&m, en)) }, list: None }
        }
        "record_payment" => record_payment(x, args).await?,
        "sell_package" => sell_package(x, args).await?,
        "package_status" => package_status(x, args).await?,
        "invoice_number" => invoice_number(x, args).await?,
        "void_payment" => void_payment(x, args).await?,
        "add_expense" => add_expense(x, args).await?,
        "pay_salary" => pay_salary(x, args).await?,
        "waive_charge" => waive_charge(x, args).await?,
        "close_month" => close_month(x, args).await?,
        _ => Out::Retry(format!("There's no tool called \"{}\" for this user.", cut(name, 40))),
    })
}

// ───────── changes ─────────

fn change(skill: &'static str, target: Option<Uuid>, params: Value, before: Value, after: Value, rows: Vec<[String; 3]>, warnings: Vec<String>) -> Out {
    Out::Change(Proposal { skill, target, params, before, after, rows, warnings })
}

async fn record_payment(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let Some(m) = method(&st(args, "method"), false) else {
        return Ok(Out::Retry("Ask the user how it was paid: cash, MB WAY, transfer or card.".into()));
    };
    let day = match st(args, "date") {
        d if d.is_empty() => x.today,
        d => match x.date(&d) { Some(d) => d, None => return Ok(Out::Retry("Give the day it was received as YYYY-MM-DD.".into())) },
    };
    if day > x.today {
        return Ok(Out::Retry(format!("A payment can't be dated in the future (today is {}).", super::agent::iso(x.today))));
    }
    let amount = cents(&args["amount_eur"]);
    let pay_for = match st(args, "pay_for") { s if s.is_empty() => if st(args, "ref").is_empty() && st(args, "package").is_empty() && st(args, "month").is_empty() { "all_owed".to_string() } else { "item".to_string() }, s => s };
    let patient_text = st(args, "patient");
    let p = if pay_for == "other" && patient_text.is_empty() { None } else {
        match which_patient(x, &patient_text).await? { Ok(p) => Some(p), Err(o) => return Ok(o) }
    };
    let owed: Vec<Value> = match &p {
        Some(p) => match get(x, &format!("/patients/{}/money", p.id)).await { Ok(v) => v["owed"].as_array().cloned().unwrap_or_default(), Err(o) => return Ok(o) },
        None => vec![],
    };
    let due = |o: &Value| o["due_cents"].as_i64().unwrap_or(0);
    let item = |o: &Value, amount: i64| json!({ "amount_cents": amount, "description": "", "appointment_id": o["appointment_id"], "plan_id": o["plan_id"],
        "covers_month": if st(o, "type") == "month" { o["month"].clone() } else { Value::Null } });
    let mut items: Vec<(Value, String)> = vec![]; // (item, what it is, for the preview)
    let mut warnings = vec![];
    match pay_for.as_str() {
        "other" => {
            let d = cut(&st(args, "description"), 200);
            let Some(a) = amount else { return Ok(Out::Retry("Ask the user how much was paid.".into())) };
            if d.is_empty() { return Ok(Out::Retry("Say what the payment is for (description).".into())); }
            items.push((json!({ "amount_cents": a, "description": d }), d));
        }
        "all_owed" => {
            if owed.is_empty() {
                return Ok(Out::Retry(format!("{} owes nothing. Ask the user what the payment is for (a package, or something else).", p.as_ref().map(|p| p.label()).unwrap_or_default())));
            }
            let total: i64 = owed.iter().map(due).sum();
            match amount {
                Some(a) if a != total && owed.len() > 1 => return Ok(Out::Retry(format!("They owe {} over {} items, not {}. Ask which item(s) this pays (refs from patient_money), or record pay_for=item with a ref.", e(total), owed.len(), e(a)))),
                Some(a) if a != total => {
                    items.push((item(&owed[0], a), owed_what(&owed[0], en)));
                    if a < total { warnings.push(say(en, &format!("Fica em dívida {}.", eur(total - a, false)), &format!("{} stays owed.", eur(total - a, true)))); }
                }
                _ => for o in &owed { items.push((item(o, due(o)), owed_what(o, en))); },
            }
        }
        _ => {
            let r = st(args, "ref");
            let pk = fold(&st(args, "package"));
            let mo = month_of(&st(args, "month"), x.today).filter(|_| !st(args, "month").is_empty());
            let picked: Vec<&Value> = owed.iter().filter(|o| {
                if !r.is_empty() {
                    let ty = st(o, "type");
                    let id = if ty == "appointment" { st(o, "appointment_id") } else { st(o, "plan_id") };
                    let month_ok = match r.split_once('/') { Some((_, m)) => st(o, "month").starts_with(m.trim()), None => ty != "month" || mo.as_ref().is_none_or(|m| st(o, "month").starts_with(m.as_str())) };
                    return id_matches(&id, &r) && month_ok;
                }
                let name_ok = pk.is_empty() || fold(&st(o, "name_pt")).contains(&pk) || fold(&st(o, "name_en")).contains(&pk);
                let month_ok = mo.as_ref().is_none_or(|m| st(o, "month").starts_with(m.as_str()));
                let kind_ok = match pay_for.as_str() { "package" => st(o, "type") == "plan", "month" => st(o, "type") == "month", _ => true };
                name_ok && month_ok && kind_ok && (!pk.is_empty() || mo.is_some() || pay_for != "item")
            }).collect();
            let o = match picked.as_slice() {
                [] => {
                    // Paying a package or month that isn't owed (e.g. paid ahead): not something to guess.
                    return Ok(Out::Retry(if owed.is_empty() { "They owe nothing. Ask what the payment is for.".into() }
                        else { format!("That isn't one of the items they owe. Owed items: {}.", owed.iter().map(|o| format!("ref {} {}", owed_ref(o), owed_what(o, true))).collect::<Vec<_>>().join("; ")) }));
                }
                // A monthly plan with several months owed and no month said: the oldest first.
                [first, ..] if pay_for == "month" || picked.iter().all(|o| st(o, "type") == "month" && st(o, "plan_id") == st(first, "plan_id")) => *first,
                [one] => *one,
                _ => return Ok(Out::Retry(format!("Several items match: {}. Ask which one.", picked.iter().map(|o| format!("ref {} {}", owed_ref(o), owed_what(o, true))).collect::<Vec<_>>().join("; ")))),
            };
            let a = amount.unwrap_or(due(o));
            if a > due(o) {
                warnings.push(say(en, &format!("É mais do que está em dívida ({}).", eur(due(o), false)), &format!("That’s more than is owed ({}).", eur(due(o), true))));
            } else if a < due(o) {
                warnings.push(say(en, &format!("Pagamento parcial: fica em dívida {}.", eur(due(o) - a, false)), &format!("Part payment: {} stays owed.", eur(due(o) - a, true))));
            }
            items.push((item(o, a), owed_what(o, en)));
        }
    }
    let inv = cut(&st(args, "invoice_number"), 60);
    if inv.is_empty() {
        warnings.push(say(en, "Depois de emitir a fatura no programa certificado, junte o número ao pagamento.", "After issuing the invoice in the certified software, add its number to the payment."));
    }
    warnings.push(say(en, "É um registo fiscal: depois de aplicado, só se anula (fica no histórico).", "It’s a fiscal record: once applied it can only be voided (it stays in the records)."));
    let who = p.as_ref().map(|p| p.label()).unwrap_or_else(|| say(en, "Sem utente", "No patient"));
    let when = if day == x.today { String::new() } else { format!(" · {}", day_label(&super::agent::iso(day))) };
    let rows: Vec<[String; 3]> = items.iter().map(|(it, what)| [format!("{who} — {what}"), "—".into(),
        format!("{} · {}{when}{}", eur(it["amount_cents"].as_i64().unwrap_or(0), en), method_label(m, en), if inv.is_empty() { String::new() } else { format!(" · {inv}") })]).collect();
    let body = json!({ "patient_id": p.as_ref().map(|p| p.id), "received_on": super::agent::iso(day), "method": m, "invoice_number": inv,
        "items": items.iter().map(|(i, _)| i.clone()).collect::<Vec<_>>() });
    Ok(change("payment", p.as_ref().map(|p| p.id), json!({ "patient_id": p.as_ref().map(|p| p.id), "body": body }), Value::Null, Value::Null, rows, warnings))
}

/// A product from the price list by name (Portuguese or English) or code; packages and plans only.
async fn find_product(x: &Ctx<'_>, text: &str) -> Result<Value, Out> {
    let all = get(x, "/products").await?;
    let q = fold(text);
    let sellable: Vec<&Value> = all.as_array().map(|a| a.iter().filter(|p| st(p, "kind") != "visit").collect()).unwrap_or_default();
    let list = || sellable.iter().map(|p| st(p, "name_pt")).collect::<Vec<_>>().join(", ");
    if q.is_empty() {
        return Err(Out::Retry(format!("Say which package. On sale: {}.", list())));
    }
    if let Some(p) = sellable.iter().find(|p| st(p, "code") == text.trim() || fold(&st(p, "name_pt")) == q || fold(&st(p, "name_en")) == q) {
        return Ok((*p).clone());
    }
    let m: Vec<&&Value> = sellable.iter().filter(|p| { let n = format!("{} {}", fold(&st(p, "name_pt")), fold(&st(p, "name_en"))); q.split(' ').all(|w| n.contains(w)) }).collect();
    match m.as_slice() {
        [one] => Ok((**one).clone()),
        [] => Err(Out::Retry(format!("No package or plan called \"{}\". On sale: {}.", cut(text, 40), list()))),
        _ => Err(Out::Retry(format!("Several match: {}. Ask which one.", m.iter().map(|p| st(p, "name_pt")).collect::<Vec<_>>().join(", ")))),
    }
}

async fn sell_package(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
    let pr = match find_product(x, &st(args, "product")).await { Ok(v) => v, Err(o) => return Ok(o) };
    let start = match st(args, "start_date") {
        d if d.is_empty() => x.today,
        d => match x.date(&d) { Some(d) => d, None => return Ok(Out::Retry("Give the start day as YYYY-MM-DD.".into())) },
    };
    let share = match st(args, "share_with") {
        t if t.is_empty() => None,
        t => {
            if pr["shareable"] != json!(true) {
                return Ok(Out::Retry(format!("{} can't be shared. Ask the user.", st(&pr, "name_pt"))));
            }
            match which_patient(x, &t).await? { Ok(s) if s.id != p.id => Some(s), Ok(_) => return Ok(Out::Retry("Share it with someone else, not the same patient.".into())), Err(o) => return Ok(o) }
        }
    };
    let mut desc = format!("{} · {}", name_of(&pr, en), eur(pr["price_cents"].as_i64().unwrap_or(0), en));
    if !st(&pr, "sessions").is_empty() { desc.push_str(&format!(" · {} {}", st(&pr, "sessions"), say(en, "sessões", "sessions"))); }
    desc.push_str(&format!(" · {} {}", say(en, "início", "starts"), day_label(&super::agent::iso(start))));
    if let Some(s) = &share { desc.push_str(&format!(" · {} {}", say(en, "partilhado com", "shared with"), s.label())); }
    let mut warnings = vec![say(en, "Fica por pagar até registar o pagamento.", "It stays unpaid until a payment is recorded.")];
    if start < x.today {
        warnings.push(say(en, "Data no passado: as sessões já feitas por pagar desde essa data passam a usar o pacote.", "A past start: unpaid sessions done since then will use the package."));
    }
    Ok(change("plan_sell", Some(p.id), json!({ "patient_id": p.id, "body": { "product_id": pr["id"], "starts_on": super::agent::iso(start), "shared_with": share.as_ref().map(|s| s.id) } }),
        Value::Null, Value::Null, vec![[format!("{} — {}", p.label(), kind_label(&st(&pr, "kind"), en)), "—".into(), desc]], warnings))
}

fn status_label(s: &str, en: bool) -> String {
    match s { "active" => say(en, "Ativo", "Active"), "paused" => say(en, "Em pausa", "Paused"), "ended" => say(en, "Terminado", "Ended"), "cancelled" => say(en, "Cancelado", "Cancelled"), _ => s.to_string() }
}

async fn package_status(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let want = st(args, "status");
    if !["paused", "active", "ended"].contains(&want.as_str()) {
        return Ok(Out::Retry("status must be paused, active or ended.".into()));
    }
    let p = match which_patient(x, &st(args, "patient")).await? { Ok(p) => p, Err(o) => return Ok(o) };
    let v = match get(x, &format!("/patients/{}/money", p.id)).await { Ok(v) => v, Err(o) => return Ok(o) };
    let pk = st(args, "package");
    let plans: Vec<Value> = v["plans"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|pl| st(pl, "patient_id") == p.id.to_string() && st(pl, "status") != "cancelled" && st(pl, "status") != want).collect();
    let picked: Vec<&Value> = plans.iter().filter(|pl| pk.is_empty() || id_matches(&st(pl, "id"), &pk) || fold(&name_of(pl, false)).contains(&fold(&pk)) || fold(&name_of(pl, true)).contains(&fold(&pk))).collect();
    let pl = match picked.as_slice() {
        [one] => *one,
        [] => return Ok(Out::Retry(format!("{} has no package or plan that can become {want}{}.", p.label(), if pk.is_empty() { String::new() } else { format!(" matching \"{}\"", cut(&pk, 40)) }))),
        _ => return Ok(Out::Retry(format!("Several match: {}. Ask which one.", picked.iter().map(|pl| plan_line(pl, true)).collect::<Vec<_>>().join("; ")))),
    };
    if want == "paused" && st(pl, "kind") != "monthly" {
        return Ok(Out::Retry("Only monthly plans can be paused. A package can be ended instead.".into()));
    }
    let id: Uuid = st(pl, "id").parse().map_err(|_| AppError::NotFound)?;
    let old = st(pl, "status");
    let warnings = match want.as_str() {
        "paused" => vec![say(en, "Este mês deixa de ser cobrado (se ainda não foi pago).", "This month is no longer charged (unless already paid).")],
        "ended" if st(pl, "kind") == "pack" => vec![say(en, &format!("Sessões por usar: {}. Deixam de contar.", st(pl, "remaining")), &format!("Sessions left: {}. They stop counting.", st(pl, "remaining")))],
        "ended" => vec![say(en, "Os meses seguintes deixam de ser cobrados.", "Following months are no longer charged.")],
        _ => vec![],
    };
    Ok(change("plan_status", Some(id), json!({ "patient_id": p.id, "plan_id": id }), json!({ "status": old }), json!({ "status": want }),
        vec![[format!("{} — {}", p.label(), name_of(pl, en)), status_label(&old, en), status_label(&want, en)]], warnings))
}

/// A payment by its nº (as the app shows it).
async fn payment_by_number(s: &AppState, text: &str) -> ApiResult<Option<(Uuid, Option<Uuid>, String, i64, String, String, bool, i32)>> {
    let Some(n) = number_in(text) else { return Ok(None) };
    if n > i32::MAX as i64 { return Ok(None); }
    let row: Option<(Uuid, Option<Uuid>, String, i32, String, String, bool, i32)> = sqlx::query_as(
        "SELECT py.id, py.patient_id, COALESCE(p.name, ''), py.amount_cents, py.description, py.invoice_number, py.voided_at IS NOT NULL, py.number
         FROM payments py LEFT JOIN patients p ON p.id = py.patient_id WHERE py.number = $1").bind(n as i32).fetch_optional(&s.pool).await?;
    Ok(row.map(|r| (r.0, r.1, r.2, r.3 as i64, r.4, r.5, r.6, r.7)))
}

async fn invoice_number(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let Some((id, pid, name, amount, desc, old, voided, n)) = payment_by_number(x.s, &st(args, "payment")).await? else {
        return Ok(Out::Retry("No payment with that nº. Look it up with patient_money (or payments) and use its nº.".into()));
    };
    if voided { return Ok(Out::Retry(format!("Payment nº {n} was voided."))); }
    let new = cut(&st(args, "invoice_number"), 60);
    if new.is_empty() { return Ok(Out::Retry("Ask the user for the invoice number.".into())); }
    if new == old { return Ok(Out::Retry(format!("Payment nº {n} already has invoice {old}."))); }
    let mut warnings = vec![];
    if !old.is_empty() { warnings.push(say(en, &format!("Substitui o número {old}."), &format!("Replaces number {old}."))); }
    Ok(change("invoice_number", Some(id), json!({ "patient_id": pid, "payment_id": id }), json!({ "invoice_number": old }), json!({ "invoice_number": new }),
        vec![[format!("{} {n} — {} · {} · {}", say(en, "Pagamento nº", "Payment no."), or_dash(&name), cut(&desc, 60), eur(amount, en)), or_dash(&old), new]], warnings))
}

async fn void_payment(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let Some((id, pid, name, amount, desc, _, voided, n)) = payment_by_number(x.s, &st(args, "payment")).await? else {
        return Ok(Out::Retry("No payment with that nº. Look it up with patient_money (or payments) and use its nº.".into()));
    };
    if voided { return Ok(Out::Retry(format!("Payment nº {n} was already voided."))); }
    let reason = cut(&st(args, "reason"), 200);
    if reason.is_empty() { return Ok(Out::Retry("Ask the user why it's being voided.".into())); }
    Ok(change("payment_void", Some(id), json!({ "patient_id": pid, "payment_id": id, "reason": reason }), json!({ "voided": false }), json!({ "voided": true }),
        vec![[format!("{} {n} — {} · {}", say(en, "Pagamento nº", "Payment no."), or_dash(&name), cut(&desc, 60)), eur(amount, en), format!("{} — {reason}", say(en, "Anulado", "Voided"))]],
        vec![say(en, "Não dá para desfazer: o pagamento anulado fica no histórico. Só o proprietário ou quem o registou, no próprio dia, pode anular.",
                 "This can’t be undone: the voided payment stays in the records. Only the owner, or whoever recorded it on the same day, can void it.")]))
}

async fn add_expense(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let Some(amount) = cents(&args["amount_eur"]) else { return Ok(Out::Retry("Ask the user for the amount.".into())) };
    let Some(cat) = category(&st(args, "category")).or_else(|| category(&format!("{} {}", st(args, "supplier"), st(args, "description")))) else {
        return Ok(Out::Retry(format!("Say the category: {}.", CATEGORIES.iter().map(|c| c.0).collect::<Vec<_>>().join(", "))));
    };
    let day = match st(args, "date") {
        d if d.is_empty() => x.today,
        d => match x.date(&d) { Some(d) => d, None => return Ok(Out::Retry("Give the day as YYYY-MM-DD.".into())) },
    };
    if day > x.today { return Ok(Out::Retry("An expense is recorded once it's paid: the day can't be in the future.".into())); }
    let m = match st(args, "method") { t if t.is_empty() => None, t => match method(&t, true) { Some(m) => Some(m), None => return Ok(Out::Retry("method must be cash, mbway, transfer, card or debit.".into())) } };
    let vat = match &args["vat_eur"] { Value::Null => None, v => match cents(v) { Some(c) if c < amount => Some(c), _ => return Ok(Out::Retry("The VAT must be less than the total.".into())) } };
    let (supplier, desc, docref) = (cut(&st(args, "supplier"), 120), cut(&st(args, "description"), 300), cut(&st(args, "document_ref"), 60));
    let mut body = Map::new();
    body.insert("spent_on".into(), json!(super::agent::iso(day)));
    body.insert("amount_cents".into(), json!(amount));
    body.insert("category".into(), json!(cat));
    body.insert("method".into(), json!(m.unwrap_or("transfer")));
    body.insert("supplier".into(), json!(supplier));
    body.insert("description".into(), json!(desc));
    body.insert("document_ref".into(), json!(docref));
    if let Some(v) = vat { body.insert("vat_cents".into(), json!(v)); }
    let mut after = format!("{} · {}", eur(amount, en), method_label(m.unwrap_or("transfer"), en));
    if let Some(v) = vat { after.push_str(&format!(" · {} {}", say(en, "IVA", "VAT"), eur(v, en))); }
    for t in [&supplier, &desc, &docref] { if !t.is_empty() { after.push_str(&format!(" · {t}")); } }
    let mut warnings = vec![say(en, "Pode juntar a fatura ou o recibo em Finanças › Despesas.", "You can attach the invoice or receipt in Finance › Expenses.")];
    if m.is_none() { warnings.push(say(en, "Não disse como foi pago: fica como transferência.", "How it was paid wasn’t said: recorded as a transfer.")); }
    Ok(change("expense", None, json!({ "body": body }), Value::Null, Value::Null,
        vec![[format!("{} — {} {}", say(en, "Despesa", "Expense"), day_label(&super::agent::iso(day)), category_label(cat, en)), "—".into(), after]], warnings))
}

async fn find_staff(s: &AppState, name: &str) -> ApiResult<Result<(Uuid, String, String, bool), usize>> {
    let all: Vec<(Uuid, String, String, bool)> = sqlx::query_as("SELECT id, name, role, active FROM staff").fetch_all(&s.pool).await?;
    let q = fold(name);
    if q.is_empty() { return Ok(Err(0)); }
    if let Some(x) = all.iter().find(|x| fold(&x.1) == q) { return Ok(Ok(x.clone())); }
    let m: Vec<_> = all.iter().filter(|x| { let n = fold(&x.1); let w: Vec<&str> = n.split(' ').collect(); q.split(' ').all(|qw| w.contains(&qw)) }).collect();
    Ok(if m.len() == 1 { Ok(m[0].clone()) } else { Err(m.len()) })
}

async fn pay_salary(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let (sid, name) = match find_staff(x.s, &st(args, "name")).await? {
        Ok((id, n, _, _)) => (id, n),
        Err(n) => return Ok(Out::Retry(if n > 1 { "More than one person has that name. Ask for the full name.".into() } else { "No one in the team has that name. Look at team.".into() })),
    };
    let Some(month) = month_of(&st(args, "month"), x.today) else { return Ok(Out::Retry("Give the month as YYYY-MM.".into())) };
    let row: Option<(i32, Option<i32>)> = sqlx::query_as("SELECT fixed_cents, employer_ss_bp FROM staff_pay WHERE staff_id = $1").bind(sid).fetch_optional(&x.s.pool).await?;
    let (rate,): (i32,) = sqlx::query_as("SELECT COALESCE((SELECT employer_ss_bp FROM clinic WHERE id = 1), 2375)").fetch_one(&x.s.pool).await?;
    let fixed = row.map(|r| r.0 as i64).unwrap_or(0);
    let bp = row.and_then(|r| r.1).unwrap_or(rate) as i64;
    let gross = match cents(&args["amount_eur"]) {
        Some(c) => c,
        None if fixed > 0 => fixed,
        None => return Ok(Out::Retry(format!("{name} has no fixed pay set. Ask the user the gross amount."))),
    };
    let day = match st(args, "date") {
        d if d.is_empty() => x.today,
        d => match x.date(&d) { Some(d) => d, None => return Ok(Out::Retry("Give the day it was paid as YYYY-MM-DD.".into())) },
    };
    let m = match st(args, "method") { t if t.is_empty() || fold(&t).contains("transf") => "transfer", t if method(&t, false) == Some("cash") => "cash", _ => return Ok(Out::Retry("method must be transfer or cash.".into())) };
    let ss = super::money::employer_ss(gross, bp);
    let (already,): (i64,) = sqlx::query_as("SELECT count(*) FROM salary_payments WHERE staff_id = $1 AND month = ($2 || '-01')::date").bind(sid).bind(&month).fetch_one(&x.s.pool).await?;
    let mut warnings = vec![say(en, &format!("Custo para a clínica: {} (bruto + Segurança Social {}%).", eur(gross + ss, false), bp as f64 / 100.0),
                                &format!("Cost to the clinic: {} (gross + employer Social Security {}%).", eur(gross + ss, true), bp as f64 / 100.0))];
    if already > 0 { warnings.push(say(en, &format!("Já há {already} salário(s) registado(s) para {name} em {}.", month_label(&month, false)), &format!("{name} already has {already} salary payment(s) for {}.", month_label(&month, true)))); }
    if fixed > 0 && gross != fixed { warnings.push(say(en, &format!("Diferente do ordenado fixo ({}).", eur(fixed, false)), &format!("Different from the fixed pay ({}).", eur(fixed, true)))); }
    Ok(change("salary", Some(sid), json!({ "body": { "staff_id": sid, "month": month, "gross_cents": gross, "paid_on": super::agent::iso(day), "method": m } }), Value::Null, Value::Null,
        vec![[format!("{name} — {} {}", say(en, "salário de", "salary for"), month_label(&month, en)), "—".into(),
              format!("{} {} · {} {} · {} · {}", eur(gross, en), say(en, "bruto", "gross"), say(en, "SS", "SS"), eur(ss, en), method_label(m, en), day_label(&super::agent::iso(day)))]], warnings))
}

async fn waive_charge(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let r = bare_ref(&st(args, "ref"));
    if r.len() < 6 { return Ok(Out::Retry("Use the appointment's ref (8 letters/digits) from patient_money, agenda or open_patient.".into())); }
    let rows: Vec<(Uuid, Option<Uuid>, String, String, Option<i32>, bool, String, String)> = sqlx::query_as(
        "SELECT ap.id, ap.patient_id, COALESCE(p.name, ''), COALESCE(p.number::text, ''), ap.charge_cents, ap.charge_waived, ap.status, to_char(ap.starts_at AT TIME ZONE $2, 'YYYY-MM-DD')
         FROM appointments ap LEFT JOIN patients p ON p.id = ap.patient_id WHERE replace(ap.id::text, '-', '') LIKE $1 || '%' LIMIT 2")
        .bind(&r).bind(super::schedule::tz()).fetch_all(&x.s.pool).await?;
    let [(id, pid, name, number, charge, waived, status, day)] = rows.as_slice() else {
        return Ok(Out::Retry("That ref isn't one appointment. Look it up and use its ref.".into()));
    };
    let want = args["waive"].as_bool().unwrap_or(true);
    if status != "done" { return Ok(Out::Retry("Only a done appointment has a charge.".into())); }
    if charge.unwrap_or(0) <= 0 { return Ok(Out::Retry("That appointment has no charge (it used a package or is free).".into())); }
    if *waived == want { return Ok(Out::Retry(if want { "That charge is already waived.".into() } else { "That charge isn't waived.".into() })); }
    let (paid,): (i64,) = sqlx::query_as("SELECT COALESCE(sum(amount_cents), 0)::bigint FROM payments WHERE appointment_id = $1 AND voided_at IS NULL").bind(id).fetch_one(&x.s.pool).await?;
    let mut warnings = vec![];
    if paid > 0 && want { warnings.push(say(en, &format!("Já foram pagos {}: anule esse pagamento se for para devolver.", eur(paid, false)), &format!("{} was already paid: void that payment if it’s being refunded.", eur(paid, true)))); }
    let lbl = |w: bool| if w { say(en, "Não cobrar", "Not charged") } else { eur(charge.unwrap_or(0) as i64, en) };
    Ok(change("waive", Some(*id), json!({ "patient_id": pid, "appointment_id": id }), json!({ "charge_waived": waived }), json!({ "charge_waived": want }),
        vec![[format!("{} (nº {}) — {} {}", or_dash(name), number, say(en, "sessão de", "session on"), day_label(day)), lbl(*waived), lbl(want)]], warnings))
}

async fn close_month(x: &Ctx<'_>, args: &Value) -> ApiResult<Out> {
    let en = x.en;
    let Some(m) = month_of(&st(args, "month"), x.today).filter(|_| !st(args, "month").is_empty()) else { return Ok(Out::Retry("Say which month, as YYYY-MM.".into())) };
    let reopen = args["reopen"].as_bool() == Some(true);
    let (closed, ended): (bool, bool) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM month_closes WHERE month = ($1 || '-01')::date), ($1 || '-01')::date < date_trunc('month', $2::date)::date")
        .bind(&m).bind(super::agent::iso(x.today)).fetch_one(&x.s.pool).await?;
    if reopen && !closed { return Ok(Out::Retry(format!("{m} isn't closed."))); }
    if !reopen && closed { return Ok(Out::Retry(format!("{m} is already closed."))); }
    if !reopen && !ended { return Ok(Out::Retry(format!("{m} hasn't ended yet: a month can be closed once it's over."))); }
    let lbl = |c: bool| if c { say(en, "Fechado", "Closed") } else { say(en, "Aberto", "Open") };
    let mut warnings = vec![];
    if !reopen {
        let (uninv,): (i64,) = sqlx::query_as("SELECT count(*) FROM payments WHERE voided_at IS NULL AND invoice_number = '' AND to_char(received_on, 'YYYY-MM') = $1").bind(&m).fetch_one(&x.s.pool).await?;
        warnings.push(say(en, "Pagamentos, despesas e salários desse mês ficam bloqueados.", "That month’s payments, expenses and salaries are locked."));
        if uninv > 0 { warnings.push(say(en, &format!("{uninv} pagamento(s) ainda sem número de fatura."), &format!("{uninv} payment(s) still have no invoice number."))); }
    } else {
        warnings.push(say(en, "Fica registado que o mês foi reaberto.", "The reopening is logged."));
    }
    Ok(change(if reopen { "month_reopen" } else { "month_close" }, None, json!({ "month": m }), json!({ "closed": closed }), json!({ "closed": !reopen }),
        vec![[format!("{} — {}", say(en, "Mês", "Month"), month_label(&m, en)), lbl(closed), lbl(!reopen)]], warnings))
}

// ───────── apply and undo ─────────

async fn cur(s: &AppState, skill: &str, params: &Value, target: Option<Uuid>) -> ApiResult<Value> {
    Ok(match skill {
        "plan_status" => { let v: Option<(String,)> = sqlx::query_as("SELECT status FROM patient_plans WHERE id = $1").bind(target).fetch_optional(&s.pool).await?; json!({ "status": v.map(|x| x.0) }) }
        "invoice_number" => { let v: Option<(String,)> = sqlx::query_as("SELECT invoice_number FROM payments WHERE id = $1").bind(target).fetch_optional(&s.pool).await?; json!({ "invoice_number": v.map(|x| x.0) }) }
        "payment_void" => { let v: Option<(bool,)> = sqlx::query_as("SELECT voided_at IS NOT NULL FROM payments WHERE id = $1").bind(target).fetch_optional(&s.pool).await?; json!({ "voided": v.map(|x| x.0) }) }
        "waive" => { let v: Option<(bool,)> = sqlx::query_as("SELECT charge_waived FROM appointments WHERE id = $1").bind(target).fetch_optional(&s.pool).await?; json!({ "charge_waived": v.map(|x| x.0) }) }
        "month_close" | "month_reopen" => { let (v,): (bool,) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM month_closes WHERE month = ($1 || '-01')::date)").bind(params["month"].as_str().unwrap_or("")).fetch_one(&s.pool).await?; json!({ "closed": v }) }
        _ => Value::Null,
    })
}

const CHANGED: &str = "This was changed since the assistant suggested it. Ask the assistant again.";
const CHANGED_AGAIN: &str = "This was changed again since, so it can’t be undone here. Change it by hand.";

fn id(v: &Value, k: &str) -> Option<Uuid> {
    v[k].as_str().and_then(|x| x.parse().ok())
}

/// The endpoint and body that set a change's state `v` (used for Apply with `after` and Undo with `before`).
fn setter(skill: &str, params: &Value, target: Option<Uuid>, v: &Value) -> Option<(&'static str, String, Value)> {
    let t = target.map(|t| t.to_string()).unwrap_or_default();
    Some(match skill {
        "plan_status" => ("PATCH", format!("/plans/{t}"), json!({ "status": v["status"] })),
        "invoice_number" => ("PATCH", format!("/payments/{t}"), json!({ "invoice_number": v["invoice_number"] })),
        "waive" => ("PATCH", format!("/appointments/{t}"), json!({ "charge_waived": v["charge_waived"] })),
        "month_close" | "month_reopen" => ("POST", format!("/finance/months/{}/{}", params["month"].as_str().unwrap_or(""), if v["closed"] == json!(true) { "close" } else { "reopen" }), json!({})),
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn apply(s: &AppState, a: &Auth, headers: &HeaderMap, tx: Tx<'_>, cid: Uuid, skill: &str, target: Option<Uuid>, params: &Value, before: &Value, after: &Value) -> ApiResult<Response> {
    let c = Caller::from(headers);
    let pid = id(params, "patient_id");
    let post = |path: String, body: Value| { let c = &c; async move { call(s, c, "POST", &path, Some(&body)).await } };
    match skill {
        "payment" => {
            let (status, v) = post("/payments".into(), params["body"].clone()).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            finish(s, a, tx, cid, "applied", &json!({ "ids": v["ids"], "numbers": v["numbers"] }), skill, pid).await
        }
        "plan_sell" => {
            let pid = pid.ok_or(AppError::NotFound)?;
            let (status, v) = post(format!("/patients/{pid}/plans"), params["body"].clone()).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            finish(s, a, tx, cid, "applied", &json!({ "plan_id": v["id"], "attached": v["attached"] }), skill, Some(pid)).await
        }
        "expense" => {
            let (status, v) = post("/expenses".into(), params["body"].clone()).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            let body = &params["body"];
            finish(s, a, tx, cid, "applied", &json!({ "expense_id": v["id"], "spent_on": body["spent_on"], "amount_cents": body["amount_cents"] }), skill, None).await
        }
        "salary" => {
            let (status, v) = post("/finance/salaries".into(), params["body"].clone()).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            finish(s, a, tx, cid, "applied", &json!({ "salary_id": v["id"], "employer_ss_cents": v["employer_ss_cents"] }), skill, None).await
        }
        "payment_void" => {
            if cur(s, skill, params, target).await? != *before { return Err(AppError::Conflict(CHANGED)); }
            let (status, v) = post(format!("/payments/{}/void", target.map(|t| t.to_string()).unwrap_or_default()), json!({ "reason": params["reason"] })).await;
            if !ok(status) { return Ok(passed_on(status, v)); }
            finish(s, a, tx, cid, "applied", after, skill, pid).await
        }
        _ => {
            if cur(s, skill, params, target).await? != *before { return Err(AppError::Conflict(CHANGED)); }
            let (m, path, body) = setter(skill, params, target, after).ok_or(AppError::NotFound)?;
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
    match skill {
        "payment_void" => Err(AppError::Conflict("A voided payment stays voided. Record the payment again if needed.")),
        "payment" => {
            // Payments are never deleted: Undo voids what was recorded (allowed to whoever recorded it, on the same day).
            for pay in after["ids"].as_array().cloned().unwrap_or_default() {
                let p = pay.as_str().unwrap_or("");
                let (voided,): (bool,) = sqlx::query_as("SELECT voided_at IS NOT NULL FROM payments WHERE id = $1::uuid").bind(p).fetch_one(&s.pool).await?;
                if voided { continue; }
                let (status, v) = call(s, &c, "POST", &format!("/payments/{p}/void"), Some(&json!({ "reason": "Desfeito no assistente" }))).await;
                if let Some(r) = pass(status, v) { return Ok(r); }
            }
            finish(s, a, tx, cid, "undone", after, skill, pid).await
        }
        "plan_sell" => {
            let plan = after["plan_id"].as_str().unwrap_or("");
            let (status, v) = call(s, &c, "PATCH", &format!("/plans/{plan}"), Some(&json!({ "status": "cancelled" }))).await;
            if let Some(r) = pass(status, v) { return Ok(r); }
            finish(s, a, tx, cid, "undone", after, skill, pid).await
        }
        "expense" => {
            let e_id = after["expense_id"].as_str().unwrap_or("");
            let row: Option<(String, i32)> = sqlx::query_as("SELECT spent_on::text, amount_cents FROM expenses WHERE id::text = $1").bind(e_id).fetch_optional(&s.pool).await?;
            match row {
                Some((d, amt)) if json!(d) == after["spent_on"] && json!(amt) == after["amount_cents"] => {}
                Some(_) => return Err(AppError::Conflict(CHANGED_AGAIN)),
                None => return Err(AppError::NotFound),
            }
            let (status, v) = call(s, &c, "DELETE", &format!("/expenses/{e_id}"), None).await;
            if let Some(r) = pass(status, v) { return Ok(r); }
            finish(s, a, tx, cid, "undone", after, skill, None).await
        }
        "salary" => {
            let (status, v) = call(s, &c, "DELETE", &format!("/finance/salaries/{}", after["salary_id"].as_str().unwrap_or("")), None).await;
            if let Some(r) = pass(status, v) { return Ok(r); }
            finish(s, a, tx, cid, "undone", after, skill, None).await
        }
        _ => {
            if cur(s, skill, params, target).await? != *after { return Err(AppError::Conflict(CHANGED_AGAIN)); }
            let (m, path, body) = setter(skill, params, target, before).ok_or(AppError::NotFound)?;
            let (status, v) = call(s, &c, m, &path, Some(&body)).await;
            if let Some(r) = pass(status, v) { return Ok(r); }
            finish(s, a, tx, cid, "undone", after, skill, pid).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    #[test]
    fn months_people_say() {
        let t = Date::from_calendar_date(2026, Month::November, 13).unwrap();
        let m = |s: &str| month_of(s, t);
        assert_eq!(m("2026-10").as_deref(), Some("2026-10"));
        assert_eq!(m("10/2026").as_deref(), Some("2026-10"));
        assert_eq!(m("2026-10-05").as_deref(), Some("2026-10"));
        assert_eq!(m("outubro").as_deref(), Some("2026-10"));
        assert_eq!(m("dezembro").as_deref(), Some("2025-12"), "a month still to come this year means last year's");
        assert_eq!(m("novembro de 2025").as_deref(), Some("2025-11"));
        assert_eq!(m("este mês").as_deref(), Some("2026-11"));
        assert_eq!(m("").as_deref(), Some("2026-11"));
        assert_eq!(m("mês passado").as_deref(), Some("2026-10"));
        assert_eq!(m("2026-13"), None);
        assert_eq!(m("qualquer"), None);
    }

    #[test]
    fn money_words() {
        assert_eq!(cents(&json!(35)), Some(3500));
        assert_eq!(cents(&json!("35,50 €")), Some(3550));
        assert_eq!(cents(&json!(0)), None);
        assert_eq!(cents(&json!(-5)), None);
        assert_eq!(eur(3550, false), "35,50 €");
        assert_eq!(eur(3550, true), "€35.50");
        assert_eq!(method("MB Way", false), Some("mbway"));
        assert_eq!(method("numerário", false), Some("cash"));
        assert_eq!(method("multibanco", false), Some("card"));
        assert_eq!(method("débito direto", true), Some("debit"));
        assert_eq!(method("transferência", false), Some("transfer"));
        assert_eq!(method("bitcoin", false), None);
        assert_eq!(category("Renda"), Some("rent"));
        assert_eq!(category("conta da EDP"), Some("utilities"));
        assert_eq!(category("software"), Some("software"));
        assert_eq!(category("rent"), Some("rent"));
        assert_eq!(bare_ref("ap:3F9C2A1B"), "3f9c2a1b");
        assert_eq!(bare_ref("3f9c2a1b/2026-11"), "3f9c2a1b");
        assert!(id_matches("3f9c2a1b-0000-4000-8000-000000000000", "3f9c2a1b"));
        assert!(!id_matches("3f9c2a1b-0000-4000-8000-000000000000", "3f9"));
        assert_eq!(number_in("nº 12"), Some(12));
    }
}
