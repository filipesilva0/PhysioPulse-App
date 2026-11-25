//! Phase 4: the local AI's jobs. **Owner only** (DECISIONS.md, 3 Nov 2026) — except the note write-up, which since
//! 11 Nov is also for therapists whose AI switch is on (money stays the owner's), all on the clinic's own Ollama
//! (`crate::ai`, private/Tailscale addresses only). Prompts and answers are never logged.
//!
//! - Write up a progress note as plain paragraphs (no titles, no SOAP: `crate::style`) from its blocks and free text. The AI never saves: the app shows the
//!   draft with Use / Retry / Discard and the therapist signs as usual.
//! - Read an expense receipt (photo or PDF) into supplier, NIF, date, amount, VAT, category… for the owner to confirm.
//! - The monthly report: the server works out every figure; the AI only writes the commentary. Kept as a PDF.
//! - The admin assistant: understands a request, the server turns it into a before/after preview, nothing
//!   changes until Apply, every change is logged and can be undone. It never reads patient files.
//! - Opening hours and closures (the assistant's "hours and closures" skill also uses these).

use super::money::{self, category_pt, method_pt, month_pt, parse_month, CATEGORIES, EXPENSE_METHODS};
use super::schedule::tz;
use crate::ai::{self, AiError};
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use crate::export::{self, Line};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

fn owner(a: &Auth) -> ApiResult<()> {
    a.require(&["owner"])
}
fn is_en(l: Option<&str>) -> bool {
    l == Some("en")
}

/// What the app shows when the AI couldn't do it. `failed` is the job-specific sentence.
fn ai_error(e: AiError, failed: &'static str) -> AppError {
    match e {
        AiError::Offline => AppError::ConflictWith("ai_offline", "Can’t reach the local AI. Check the AI computer is on and Ollama is running, and its address in Settings › Local AI.", json!({ "url": ai::url() })),
        AiError::NoModel => AppError::ConflictWith("ai_no_model", "The AI model isn’t installed on the server yet.", json!({ "model": ai::model() })),
        AiError::NoVision => AppError::ConflictWith("ai_no_vision", "The chosen AI model can’t read pictures. Choose one that can (for example gemma3:12b or qwen2.5vl:7b) in Settings › Local AI, or type the details.", json!({ "model": ai::model() })),
        AiError::Failed => AppError::ConflictWith("ai_failed", failed, Value::Null),
    }
}

/// Lower case, no accents, single spaces — for matching names people type.
pub(crate) fn fold(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        out.push(match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a', 'é' | 'è' | 'ê' | 'ë' => 'e', 'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o', 'ú' | 'ù' | 'û' | 'ü' => 'u', 'ç' => 'c', c if c.is_alphanumeric() => c, _ => ' ',
        });
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Money for people: "35,00 €" (pt) or "€35.00" (en).
fn eur(c: i64, en: bool) -> String {
    if en {
        let sign = if c < 0 { "−" } else { "" };
        let c = c.abs();
        let int = (c / 100).to_string();
        let mut g = String::new();
        for (i, ch) in int.chars().enumerate() {
            if i > 0 && (int.len() - i) % 3 == 0 { g.push(','); }
            g.push(ch);
        }
        format!("{sign}€{g}.{:02}", c % 100)
    } else {
        export::euros(c)
    }
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\n').take(max).collect::<String>().trim().to_string()
}

/// "2026-12-24" or "24/12/2026" → a real calendar date as "2026-12-24".
pub(crate) fn date_of(s: &str) -> Option<String> {
    let t = s.trim();
    let (y, m, d): (i32, u8, u8) = if t.len() >= 10 && t.as_bytes()[4] == b'-' {
        (t[..4].parse().ok()?, t[5..7].parse().ok()?, t[8..10].parse().ok()?)
    } else {
        let p: Vec<&str> = t.split(['/', '-', '.']).collect();
        if p.len() != 3 { return None; }
        let y: i32 = p[2].trim().parse().ok()?;
        (if y < 100 { 2000 + y } else { y }, p[1].trim().parse().ok()?, p[0].trim().parse().ok()?)
    };
    let month = time::Month::try_from(m).ok()?;
    let dt = time::Date::from_calendar_date(y, month, d).ok()?;
    Some(format!("{:04}-{:02}-{:02}", dt.year(), dt.month() as u8, dt.day()))
}

fn date_label(iso: &str) -> String {
    if iso.len() >= 10 { format!("{}/{}/{}", &iso[8..10], &iso[5..7], &iso[..4]) } else { iso.to_string() }
}

fn month_name(m: u32, en: bool) -> &'static str {
    if en {
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"][(m.clamp(1, 12) - 1) as usize]
    } else {
        month_pt(m)
    }
}

// ───────── note rewrite ─────────

#[derive(Deserialize)]
pub struct RewriteReq {
    patient_id: Uuid,
    #[serde(default)]
    template: String,
    #[serde(default)]
    blocks: Value,
    #[serde(default)]
    text: String,
    lang: Option<String>,
}

/// The note's content as plain lines ("Dor (0–10): 6/10"). Only what was filled in.
pub(crate) fn note_facts(blocks: &Value, text: &str) -> String {
    let mut out = Vec::new();
    for b in blocks.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let label = clean(b["label"].as_str().unwrap_or(""), 80);
        let label = if label.is_empty() { "—".to_string() } else { label };
        let line = match b["type"].as_str().unwrap_or("") {
            "slider" => b["value"].as_f64().map(|v| format!("{label}: {v}/{}", b["max"].as_f64().unwrap_or(10.0))),
            "checklist" => {
                let items: Vec<String> = b["value"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).map(|x| clean(x, 80)).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
                (!items.is_empty()).then(|| format!("{label}: {}", items.join(", ")))
            }
            _ => b["value"].as_str().map(|v| clean(v, 1000)).filter(|v| !v.is_empty()).map(|v| format!("{label}: {v}")),
        };
        if let Some(l) = line { out.push(format!("- {l}")); }
    }
    let t = clean(text, 4000);
    if !t.is_empty() {
        out.push(String::new());
        out.push(t);
    }
    out.join("\n")
}

const NOTE_PT: &str = "És um fisioterapeuta em Portugal a passar a limpo a nota de evolução de uma sessão, para o processo do utente. \
Usa apenas os factos dados: não inventes valores, testes, diagnósticos, técnicas nem respostas do utente. \
Se faltar algo importante (por exemplo, o plano para a próxima sessão), escreve «[a completar]» nesse ponto da frase. \
Um a três parágrafos curtos.";
const NOTE_EN: &str = "You are a physiotherapist writing up the progress note for one session, for the patient's record. \
Use only the facts given: never invent values, tests, diagnoses, techniques or what the patient said. \
If something important is missing (for example the plan for next session), write \"[to complete]\" at that point in the sentence. \
One to three short paragraphs.";

fn note_system(en: bool) -> String {
    use crate::style;
    if en { format!("{NOTE_EN} {}\n\n{}", style::RULES_EN, style::EXAMPLE_EN) } else { format!("{NOTE_PT} {}\n\n{}", style::RULES_PT, style::EXAMPLE_PT) }
}

/// Owner: a plain-text draft of the note being written (`crate::style`). Nothing is saved.
pub async fn rewrite_note(State(s): State<AppState>, a: Auth, Json(r): Json<RewriteReq>) -> ApiResult<Json<Value>> {
    // Owner, and therapists whose AI switch is on (owner, 11 Nov 2026).
    super::util::require_clinical(&a)?;
    a.require_ai()?;
    let (exists,): (bool,) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM patients WHERE id = $1 AND deleted_at IS NULL AND anonymised_at IS NULL)")
        .bind(r.patient_id).fetch_one(&s.pool).await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    let facts = note_facts(&r.blocks, &r.text);
    if facts.trim().is_empty() {
        return Err(AppError::BadRequest("Fill in the note first: the AI only rewrites what’s there."));
    }
    let en = is_en(r.lang.as_deref());
    let tpl = clean(&r.template, 40);
    let prompt = if en {
        format!("Template: {tpl}\nWhat the therapist recorded in this session:\n{facts}\n\nWrite the note.")
    } else {
        format!("Modelo: {tpl}\nO que o fisioterapeuta registou nesta sessão:\n{facts}\n\nEscreve a nota.")
    };
    let res = crate::style::generate_plain(&note_system(en), &prompt, en).await;
    auth::log(&s.pool, Some(a.staff.id), "ai.note_rewritten", Some(("patient", r.patient_id.to_string())), &a.device, if res.is_ok() { "ok" } else { "failed" }).await;
    let text = res.map_err(|e| ai_error(e, "The AI couldn’t finish the note. Try again, or keep your own text."))?;
    Ok(Json(json!({ "text": text.trim(), "model": ai::model() })))
}

// ───────── receipt reading ─────────

#[derive(Deserialize)]
pub struct ReceiptReq { file_id: Uuid }

fn receipt_schema() -> Value {
    let cats: Vec<&str> = CATEGORIES.iter().map(|c| c.0).collect();
    json!({
        "type": "object",
        "properties": {
            "supplier": { "type": "string" },
            "supplier_nif": { "type": "string" },
            "date": { "type": "string" },
            "total": { "type": "number" },
            "vat_shown": { "type": "boolean" },
            "vat": { "type": "number" },
            "document_number": { "type": "string" },
            "category": { "type": "string", "enum": cats },
            "description": { "type": "string" },
            "payment": { "type": "string", "enum": ["cash", "mbway", "transfer", "card", "debit", "unknown"] }
        },
        "required": ["supplier", "supplier_nif", "date", "total", "vat_shown", "vat", "document_number", "category", "description", "payment"]
    })
}

fn receipt_system() -> String {
    let cats = CATEGORIES.iter().map(|(k, pt)| format!("{k} ({pt})")).collect::<Vec<_>>().join(", ");
    format!("You read Portuguese receipts and invoices (faturas, faturas-recibo, talões) for a physiotherapy clinic's bookkeeping. \
Answer only with the JSON asked for. supplier: the seller's name as printed. supplier_nif: the seller's NIF (9 digits), not the customer's; \
empty if not shown. date: the document date as YYYY-MM-DD. total: the total paid in euros, VAT included, as a number (12.30). \
vat_shown: whether the VAT amount is printed; vat: the total VAT in euros (0 if not shown). document_number: the invoice or receipt number, \
e.g. \"FR 2026/123\"; empty if none. category: the best fit from: {cats}. description: what was bought, in European Portuguese, at most 8 words. \
payment: how it was paid if printed (Numerário = cash, Multibanco/cartão = card, MB WAY = mbway, transferência = transfer, débito direto = debit), \
else unknown. If something can't be read, use an empty string or 0 — never guess.")
}

/// What the model answered → checked fields; anything that doesn't make sense is left empty for the owner.
pub(crate) fn tidy_receipt(v: &Value, today: &str) -> Value {
    let st = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    let nif: String = st("supplier_nif").chars().filter(|c| c.is_ascii_digit()).collect();
    let nif = if nif.len() == 11 && nif.starts_with("351") { nif[2..].to_string() } else { nif };
    let spent_on = date_of(&st("date")).filter(|d| d.as_str() <= today && d.as_str() >= "2000-01-01");
    let cents = |k: &str| v[k].as_f64().filter(|x| x.is_finite() && *x > 0.0 && *x < 1_000_000.0).map(|x| (x * 100.0).round() as i64);
    let amount = cents("total");
    let vat = if v["vat_shown"].as_bool() == Some(true) { cents("vat").or(Some(0)).filter(|c| amount.is_none_or(|a| *c < a)) } else { None };
    let category = Some(st("category")).filter(|c| CATEGORIES.iter().any(|x| x.0 == c)).unwrap_or_else(|| "other".into());
    let method = Some(st("payment")).filter(|m| EXPENSE_METHODS.contains(&m.as_str()));
    let fields = json!({
        "supplier": clean(&st("supplier"), 120),
        "supplier_nif": if nif.len() == 9 { nif } else { String::new() },
        "spent_on": spent_on,
        "amount_cents": amount,
        "vat_cents": vat,
        "category": category,
        "document_ref": clean(&st("document_number"), 60),
        "description": clean(&st("description"), 200),
        "method": method,
    });
    let missing: Vec<&str> = ["supplier", "spent_on", "amount_cents"].into_iter()
        .filter(|k| fields[*k].is_null() || fields[*k].as_str() == Some("")).collect();
    json!({ "fields": fields, "missing": missing })
}

/// Text inside a PDF (invoices made by software have it; scans don't).
fn pdf_text(bytes: &[u8]) -> String {
    let Ok(doc) = lopdf::Document::load_mem(bytes) else { return String::new() };
    let pages: Vec<u32> = doc.get_pages().keys().copied().take(3).collect();
    doc.extract_text(&pages).unwrap_or_default()
}

/// Owner: read an uploaded receipt (`/finance/files`, kind expense) and suggest the expense's details.
pub async fn read_receipt(State(s): State<AppState>, a: Auth, Json(r): Json<ReceiptReq>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let row: Option<(String, String, String)> = sqlx::query_as("SELECT kind, mime, path FROM finance_files WHERE id = $1").bind(r.file_id).fetch_optional(&s.pool).await?;
    let (kind, mime, path) = row.ok_or(AppError::NotFound)?;
    if kind != "expense" || path.contains("..") {
        return Err(AppError::NotFound);
    }
    let bytes = tokio::fs::read(s.archive.join(&path)).await.map_err(|_| AppError::NotFound)?;
    let today = money::today(&s).await?;
    let mut prompt = format!("Today is {today}. Read this receipt and fill in the JSON.");
    let mut images = Vec::new();
    match mime.as_str() {
        "application/pdf" => {
            let text = pdf_text(&bytes);
            if text.split_whitespace().count() < 5 {
                return Err(AppError::ConflictWith("ai_scan_pdf", "This PDF is a scanned picture, so the AI can’t read it. Add a photo of the receipt instead, or type the details.", Value::Null));
            }
            prompt = format!("Today is {today}. This is the text of the receipt:\n\n{}\n\nFill in the JSON.", clean(&text, 6000));
        }
        "image/jpeg" | "image/png" | "image/webp" => images.push(base64::engine::general_purpose::STANDARD.encode(&bytes)),
        _ => return Err(AppError::ConflictWith("ai_heic", "The AI can’t read HEIC photos. Use a JPEG or PDF (on an iPhone: Settings › Camera › Formats › Most Compatible), or type the details.", Value::Null)),
    }
    let res = ai::generate_json(&receipt_system(), &prompt, &images, &receipt_schema()).await;
    auth::log(&s.pool, Some(a.staff.id), "ai.receipt_read", None, &a.device, if res.is_ok() { "ok" } else { "failed" }).await;
    let v = res.map_err(|e| ai_error(e, "The AI couldn’t read this receipt. Try again, or type the details."))?;
    let mut out = tidy_receipt(&v, &today);
    out["model"] = json!(ai::model());
    Ok(Json(out))
}

// ───────── monthly report ─────────

/// Every figure for one month, worked out by the server (the AI never calculates).
pub(crate) async fn month_facts(s: &AppState, first: &str) -> ApiResult<Value> {
    let money_now = money::totals(s, first).await?;
    let (prev_first,): (String,) = sqlx::query_as("SELECT (($1::date - interval '1 month')::date)::text").bind(first).fetch_one(&s.pool).await?;
    let money_prev = money::totals(s, &prev_first).await?;
    let activity = |f: String| {
        let pool = s.pool.clone();
        async move {
            let (v,): (Value,) = sqlx::query_as(
                "WITH b AS (SELECT $1::date AS f, ($1::date + interval '1 month')::date AS t),
                      a AS (SELECT ap.* FROM appointments ap, b WHERE (ap.starts_at AT TIME ZONE $2)::date >= b.f AND (ap.starts_at AT TIME ZONE $2)::date < b.t)
                 SELECT json_build_object(
                   'sessions_done', (SELECT count(*) FROM a WHERE kind = 'session' AND status = 'done'),
                   'assessments_done', (SELECT count(*) FROM a WHERE kind = 'assessment' AND status = 'done'),
                   'home_done', (SELECT count(*) FROM a WHERE kind = 'home' AND status = 'done'),
                   'classes', (SELECT count(*) FROM a WHERE kind = 'class' AND status <> 'cancelled'),
                   'class_attendances', (SELECT count(*) FROM class_attendees ca JOIN a ON a.id = ca.appointment_id WHERE ca.attended),
                   'no_shows', (SELECT count(*) FROM a WHERE kind <> 'block' AND status = 'no_show'),
                   'cancelled', (SELECT count(*) FROM a WHERE kind <> 'block' AND status = 'cancelled'),
                   'patients_seen', (SELECT count(DISTINCT patient_id) FROM a WHERE status = 'done' AND patient_id IS NOT NULL),
                   'new_patients', (SELECT count(*) FROM patients, b WHERE deleted_at IS NULL AND (created_at AT TIME ZONE $2)::date >= b.f AND (created_at AT TIME ZONE $2)::date < b.t),
                   'plans_sold', (SELECT count(*) FROM patient_plans pp, b WHERE pp.status <> 'cancelled' AND pp.starts_on >= b.f AND pp.starts_on < b.t),
                   'plans_sold_cents', (SELECT COALESCE(sum(price_cents), 0) FROM patient_plans pp, b WHERE pp.status <> 'cancelled' AND pp.kind = 'pack' AND pp.starts_on >= b.f AND pp.starts_on < b.t))")
                .bind(f).bind(tz()).fetch_one(&pool).await?;
            Ok::<Value, AppError>(v)
        }
    };
    let act_now = activity(first.to_string()).await?;
    let act_prev = activity(prev_first.clone()).await?;
    let owed = money::owed(s, None).await?;
    let owed_cents: i64 = owed.as_array().map(|l| l.iter().filter_map(|x| x["due_cents"].as_i64()).sum()).unwrap_or(0);
    let (closed,): (bool,) = sqlx::query_as("SELECT month_is_closed($1::date)").bind(first).fetch_one(&s.pool).await?;
    let (in_progress,): (bool,) = sqlx::query_as("SELECT $1::date = date_trunc('month', now() AT TIME ZONE $2)::date").bind(first).bind(tz()).fetch_one(&s.pool).await?;
    Ok(json!({
        "month": &first[..7], "previous_month": &prev_first[..7], "closed": closed, "in_progress": in_progress,
        "money": money_now, "money_previous": money_prev, "activity": act_now, "activity_previous": act_prev, "owed_now_cents": owed_cents,
    }))
}

fn i_(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}

/// The figures as labelled rows ([label, this month, previous month]) — for the app, the PDF and the AI.
pub(crate) fn fact_rows(f: &Value, en: bool) -> Vec<(String, String, String)> {
    let (m, p, a, ap) = (&f["money"], &f["money_previous"], &f["activity"], &f["activity_previous"]);
    let l = |pt: &str, e: &str| if en { e.to_string() } else { pt.to_string() };
    let mut rows = vec![
        (l("Receita", "Revenue"), eur(i_(&m["revenue_cents"]), en), eur(i_(&p["revenue_cents"]), en)),
        (l("Despesas", "Expenses"), eur(i_(&m["expenses_cents"]), en), eur(i_(&p["expenses_cents"]), en)),
        (l("Salários", "Salaries"), eur(i_(&m["salaries_cents"]), en), eur(i_(&p["salaries_cents"]), en)),
        (l("Segurança Social (entidade patronal)", "Employer Social Security"), eur(i_(&m["employer_ss_cents"]), en), eur(i_(&p["employer_ss_cents"]), en)),
        (l("Resultado", "Result"), eur(i_(&m["result_cents"]), en), eur(i_(&p["result_cents"]), en)),
        (l("Pagamentos recebidos", "Payments received"), i_(&m["payments"]).to_string(), i_(&p["payments"]).to_string()),
        (l("Sessões realizadas", "Sessions done"), i_(&a["sessions_done"]).to_string(), i_(&ap["sessions_done"]).to_string()),
        (l("Avaliações iniciais", "First assessments"), i_(&a["assessments_done"]).to_string(), i_(&ap["assessments_done"]).to_string()),
        (l("Domicílios", "Home visits"), i_(&a["home_done"]).to_string(), i_(&ap["home_done"]).to_string()),
        (l("Aulas de grupo (presenças)", "Group classes (attendances)"), format!("{} ({})", i_(&a["classes"]), i_(&a["class_attendances"])), format!("{} ({})", i_(&ap["classes"]), i_(&ap["class_attendances"]))),
        (l("Faltas", "No-shows"), i_(&a["no_shows"]).to_string(), i_(&ap["no_shows"]).to_string()),
        (l("Cancelamentos", "Cancellations"), i_(&a["cancelled"]).to_string(), i_(&ap["cancelled"]).to_string()),
        (l("Utentes atendidos", "Patients seen"), i_(&a["patients_seen"]).to_string(), i_(&ap["patients_seen"]).to_string()),
        (l("Utentes novos", "New patients"), i_(&a["new_patients"]).to_string(), i_(&ap["new_patients"]).to_string()),
        (l("Pacotes e planos vendidos", "Packages and plans sold"), i_(&a["plans_sold"]).to_string(), i_(&ap["plans_sold"]).to_string()),
    ];
    if i_(&m["uninvoiced"]) > 0 {
        rows.push((l("Pagamentos sem fatura registada", "Payments with no invoice recorded"), format!("{} ({})", i_(&m["uninvoiced"]), eur(i_(&m["uninvoiced_cents"]), en)), "—".into()));
    }
    rows
}

fn breakdown(f: &Value, en: bool) -> String {
    let m = &f["money"];
    let mut out = Vec::new();
    if let Some(bm) = m["by_method"].as_object().filter(|o| !o.is_empty()) {
        let parts: Vec<String> = bm.iter().map(|(k, v)| format!("{} {}", method_pt(k), eur(i_(v), en))).collect();
        out.push(format!("{}: {}", if en { "Revenue by method" } else { "Receita por método" }, parts.join("; ")));
    }
    if let Some(bc) = m["expenses_by_category"].as_object().filter(|o| !o.is_empty()) {
        let parts: Vec<String> = bc.iter().map(|(k, v)| format!("{} {}", category_pt(k), eur(i_(v), en))).collect();
        out.push(format!("{}: {}", if en { "Expenses by category" } else { "Despesas por categoria" }, parts.join("; ")));
    }
    out.push(format!("{}: {}", if en { "Owed by patients today" } else { "Em dívida pelos utentes hoje" }, eur(i_(&f["owed_now_cents"]), en)));
    out.join("\n")
}

fn month_label(ym: &str, en: bool) -> String {
    let (y, m) = (&ym[..4], ym[5..7].parse::<u32>().unwrap_or(1));
    if en { format!("{} {y}", month_name(m, true)) } else { format!("{} de {y}", month_name(m, false)) }
}

fn report_prompt(f: &Value, en: bool) -> String {
    let rows = fact_rows(f, en);
    let (this, prev) = (month_label(f["month"].as_str().unwrap_or(""), en), month_label(f["previous_month"].as_str().unwrap_or(""), en));
    let table = rows.iter().map(|(k, a, b)| format!("- {k}: {a} ({} {b})", if en { "previous month" } else { "mês anterior" })).collect::<Vec<_>>().join("\n");
    let status = match (f["in_progress"].as_bool(), f["closed"].as_bool()) {
        (Some(true), _) => if en { "The month is still in progress." } else { "O mês ainda está a decorrer." },
        (_, Some(true)) => if en { "The month is closed." } else { "O mês está fechado." },
        _ => if en { "The month isn’t closed yet, so figures may still change." } else { "O mês ainda não está fechado, por isso os valores podem mudar." },
    };
    if en {
        format!("Month: {this} (compared with {prev}). {status}\n\n{table}\n{}\n\nWrite the report.", breakdown(f, en))
    } else {
        format!("Mês: {this} (comparado com {prev}). {status}\n\n{table}\n{}\n\nEscreve o relatório.", breakdown(f, en))
    }
}

const REPORT_PT: &str = "És o assistente de gestão de uma pequena clínica de fisioterapia em Portugal. Escreve para a dona da clínica \
um relatório mensal curto em português europeu: 3 a 5 parágrafos simples, sem títulos, sem listas, sem markdown. \
Primeiro o essencial (receita, resultado e como compara com o mês anterior), depois custos, depois atividade (sessões, faltas, utentes novos), \
e por fim um ou dois pontos de atenção concretos (por exemplo faltas a subir, pagamentos sem fatura, dívida). \
Usa só os números dados, escritos como estão; não calcules percentagens nem inventes números ou causas. Tom sóbrio e útil.";
const REPORT_EN: &str = "You are the management assistant of a small physiotherapy clinic in Portugal. Write the owner a short monthly report \
in British English: 3 to 5 plain paragraphs, no headings, no lists, no markdown. First the essentials (revenue, result and how they compare \
with the previous month), then costs, then activity (sessions, no-shows, new patients), and finally one or two concrete points to watch \
(for example rising no-shows, payments with no invoice, money owed). Use only the numbers given, written as they are; don't work out \
percentages or invent figures or causes. Sober, useful tone.";

#[derive(Deserialize)]
pub struct ReportReq { month: String, lang: Option<String> }

/// Owner: the figures for a month and the AI's commentary on them. Nothing is saved until Save.
pub async fn draft_report(State(s): State<AppState>, a: Auth, Json(r): Json<ReportReq>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (first, _, _) = parse_month(&r.month)?;
    let en = is_en(r.lang.as_deref());
    let f = month_facts(&s, &first).await?;
    let res = ai::generate(if en { REPORT_EN } else { REPORT_PT }, &report_prompt(&f, en)).await;
    auth::log(&s.pool, Some(a.staff.id), "ai.report_drafted", None, &a.device, &first[..7]).await;
    let text = res.map_err(|e| ai_error(e, "The AI couldn’t finish the report. Try again; the figures are below."))?;
    Ok(Json(json!({ "month": &first[..7], "facts": f, "rows": fact_rows(&f, en), "text": text.replace("**", "").trim(), "model": ai::model() })))
}

/// Owner: the figures only (fast; shown while the AI writes, and when it can't).
pub async fn report_figures(State(s): State<AppState>, a: Auth, Query(r): Query<ReportReq>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (first, _, _) = parse_month(&r.month)?;
    let f = month_facts(&s, &first).await?;
    Ok(Json(json!({ "month": &first[..7], "facts": f, "rows": fact_rows(&f, is_en(r.lang.as_deref())) })))
}

#[derive(Deserialize)]
pub struct SaveReport { month: String, lang: Option<String>, text: String, #[serde(default)] model: String }

/// Owner: keep a report — the text as accepted (maybe corrected), the server's figures, and a PDF in the archive.
pub async fn save_report(State(s): State<AppState>, a: Auth, Json(r): Json<SaveReport>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (first, y, m) = parse_month(&r.month)?;
    let en = is_en(r.lang.as_deref());
    let text = clean(&r.text, 12000);
    if text.is_empty() {
        return Err(AppError::BadRequest("The report is empty."));
    }
    let f = month_facts(&s, &first).await?;
    let (clinic,): (String,) = sqlx::query_as("SELECT name FROM clinic WHERE id = 1").fetch_one(&s.pool).await?;
    let now = money::today(&s).await?;
    let title = if en { format!("{clinic} — report for {}", month_label(&first[..7], true)) } else { format!("{clinic} — relatório de {}", month_label(&first[..7], false)) };
    let info = crate::routes::clinic::read(&s.pool).await.unwrap_or_default();
    let mut l: Vec<Line> = Vec::new();
    // The logo, if any (the clinic's name is already in the title).
    l.extend(crate::export::clinic_header(&serde_json::json!({ "logo": info["logo"].as_str().and_then(|x| x.split_once(",")).map(|x| x.1) }), ""));
    l.extend([Line::Title(title), Line::Text(if en {
        format!("Commentary drafted by the clinic’s local AI ({}) and reviewed by {} on {}.", clean(&r.model, 60), a.staff.name, date_label(&now))
    } else {
        format!("Comentário escrito pela IA local da clínica ({}) e revisto por {} a {}.", clean(&r.model, 60), a.staff.name, date_label(&now))
    }), Line::Gap, Line::Heading(if en { "Figures (previous month in brackets)".into() } else { "Números (mês anterior entre parênteses)".into() })]);
    for (k, a_, b) in fact_rows(&f, en) {
        l.push(Line::Row(k, format!("{a_}   ({b})")));
    }
    l.push(Line::Gap);
    l.push(Line::Heading(if en { "Commentary".into() } else { "Comentário".into() }));
    for par in text.split('\n') {
        if par.trim().is_empty() { l.push(Line::Gap); continue; }
        for w in export::wrap(par.trim(), 10.0, 483.0) { l.push(Line::Text(w)); }
    }
    let pdf = export::pdf(&l);
    let filename = format!("{} {y}-{m:02}.pdf", if en { "Report" } else { "Relatorio" });
    let (file_id, _) = money::save_file(&s, &a, "report", &filename, &first, &pdf, "application/pdf").await?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO ai_reports (id, month, lang, facts, text, model, file_id, created_by) VALUES ($1, $2::date, $3, $4, $5, $6, $7, $8)")
        .bind(id).bind(&first).bind(if en { "en" } else { "pt" }).bind(&f).bind(&text).bind(clean(&r.model, 100)).bind(file_id).bind(a.staff.id)
        .execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "ai.report_saved", None, &a.device, &first[..7]).await;
    Ok(Json(json!({ "id": id, "file_id": file_id })))
}

/// Owner: the reports kept, newest month first.
pub async fn list_reports(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('id', r.id, 'month', to_char(r.month, 'YYYY-MM'), 'lang', r.lang, 'text', r.text, 'model', r.model,
                 'file_id', r.file_id, 'created_at', r.created_at, 'created_by_name', st.name) ORDER BY r.month DESC, r.created_at DESC), '[]')
         FROM ai_reports r LEFT JOIN staff st ON st.id = r.created_by")
        .fetch_one(&s.pool).await?;
    Ok(Json(v))
}

/// Owner: remove a kept report and its PDF.
pub async fn delete_report(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let row: Option<(Option<Uuid>, String)> = sqlx::query_as("SELECT file_id, to_char(month, 'YYYY-MM') FROM ai_reports WHERE id = $1").bind(id).fetch_optional(&s.pool).await?;
    let (file, month) = row.ok_or(AppError::NotFound)?;
    sqlx::query("DELETE FROM ai_reports WHERE id = $1").bind(id).execute(&s.pool).await?;
    if let Some(fid) = file {
        let p: Option<(String,)> = sqlx::query_as("DELETE FROM finance_files WHERE id = $1 AND kind = 'report' RETURNING path").bind(fid).fetch_optional(&s.pool).await?;
        if let Some((path,)) = p.filter(|p| !p.0.contains("..")) {
            let _ = tokio::fs::remove_file(s.archive.join(path)).await;
        }
    }
    auth::log(&s.pool, Some(a.staff.id), "ai.report_deleted", None, &a.device, &month).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── opening hours and closures ─────────

pub(crate) const DAYS: &[&str] = &["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

fn hm_ok(s: &str) -> bool {
    s.len() == 5 && s.as_bytes()[2] == b':' && s[..2].parse::<u8>().is_ok_and(|h| h < 24) && s[3..].parse::<u8>().is_ok_and(|m| m < 60)
}

/// ["08:00","20:00"] (open < close) or null.
fn day_ok(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Array(a) if a.len() == 2 => match (a[0].as_str(), a[1].as_str()) {
            (Some(o), Some(c)) => hm_ok(o) && hm_ok(c) && o < c,
            _ => false,
        },
        _ => false,
    }
}

async fn hours_now(s: &AppState) -> ApiResult<Value> {
    let (v,): (Value,) = sqlx::query_as("SELECT opening_hours FROM clinic WHERE id = 1").fetch_one(&s.pool).await?;
    Ok(v)
}

async fn upcoming_closures(s: &AppState) -> ApiResult<Value> {
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('id', c.id, 'starts_on', c.starts_on, 'ends_on', c.ends_on, 'label', c.label,
                 'days', (SELECT count(*) FROM appointments ap WHERE ap.closure_id = c.id)) ORDER BY c.starts_on), '[]')
         FROM closures c WHERE c.ends_on >= (now() AT TIME ZONE $1)::date")
        .bind(tz()).fetch_one(&s.pool).await?;
    Ok(v)
}

/// Everyone: opening hours and the closures still to come (the schedule and Settings show them).
pub async fn get_hours(State(s): State<AppState>, _a: Auth) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "hours": hours_now(&s).await?, "closures": upcoming_closures(&s).await? })))
}

#[derive(Deserialize)]
pub struct HoursReq { hours: Value }

/// Owner: set the opening hours (all seven days).
pub async fn put_hours(State(s): State<AppState>, a: Auth, Json(r): Json<HoursReq>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let o = r.hours.as_object().ok_or(AppError::BadRequest("Give the hours for every day."))?;
    if o.len() != 7 || !DAYS.iter().all(|d| o.get(*d).is_some_and(day_ok)) {
        return Err(AppError::BadRequest("Each day needs an opening time before its closing time, like 08:00–20:00, or Closed."));
    }
    sqlx::query("UPDATE clinic SET opening_hours = $1 WHERE id = 1").bind(&r.hours).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "clinic.hours_changed", None, &a.device, "").await;
    Ok(Json(json!({ "ok": true })))
}

/// What a closure from..to would do: (open days it blocks, appointments already booked then).
async fn closure_check(s: &AppState, from: &str, to: &str) -> ApiResult<(i64, i64)> {
    let today = money::today(s).await?;
    if from < today.as_str() {
        return Err(AppError::BadRequest("Choose today or a later day."));
    }
    if to < from {
        return Err(AppError::BadRequest("The last day must be on or after the first day."));
    }
    let (span,): (i32,) = sqlx::query_as("SELECT $2::date - $1::date").bind(from).bind(to).fetch_one(&s.pool).await?;
    if span > 92 {
        return Err(AppError::BadRequest("A closure can be at most three months. Add another one for longer."));
    }
    let hours = hours_now(s).await?;
    let (dows,): (Vec<i32>,) = sqlx::query_as("SELECT array_agg(extract(isodow FROM d)::int) FROM generate_series($1::date, $2::date, interval '1 day') d")
        .bind(from).bind(to).fetch_one(&s.pool).await?;
    let open = dows.iter().filter(|d| !hours[DAYS[(**d as usize).clamp(1, 7) - 1]].is_null()).count() as i64;
    let (booked,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM appointments WHERE kind <> 'block' AND status = 'booked'
           AND (starts_at AT TIME ZONE $3)::date BETWEEN $1::date AND $2::date")
        .bind(from).bind(to).bind(tz()).fetch_one(&s.pool).await?;
    Ok((open, booked))
}

/// Creates the closure and one blocked-time slot per open day (opening to closing time, no therapist).
pub(crate) async fn make_closure(s: &AppState, by: Uuid, from: &str, to: &str, label: &str) -> ApiResult<(Uuid, i64)> {
    let hours = hours_now(s).await?;
    let id = Uuid::new_v4();
    let label = if label.trim().is_empty() { "Clínica fechada".to_string() } else { clean(label, 80) };
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO closures (id, starts_on, ends_on, label, created_by) VALUES ($1, $2::date, $3::date, $4, $5)")
        .bind(id).bind(from).bind(to).bind(&label).bind(by).execute(&mut *tx).await?;
    let days: Vec<(String, i32)> = sqlx::query_as("SELECT d::date::text, extract(isodow FROM d)::int FROM generate_series($1::date, $2::date, interval '1 day') d")
        .bind(from).bind(to).fetch_all(&mut *tx).await?;
    let mut n = 0;
    for (day, dow) in days {
        let h = &hours[DAYS[(dow as usize).clamp(1, 7) - 1]];
        let (Some(o), Some(c)) = (h[0].as_str(), h[1].as_str()) else { continue };
        sqlx::query(
            "INSERT INTO appointments (id, kind, starts_at, ends_at, title, color, closure_id, created_by)
             VALUES ($1, 'block', ($2::date + $3::time) AT TIME ZONE $5, ($2::date + $4::time) AT TIME ZONE $5, $6, '#56605D', $7, $8)")
            .bind(Uuid::new_v4()).bind(&day).bind(o).bind(c).bind(tz()).bind(&label).bind(id).bind(by).execute(&mut *tx).await?;
        n += 1;
    }
    tx.commit().await?;
    Ok((id, n))
}

#[derive(Deserialize)]
pub struct ClosureReq { starts_on: String, ends_on: String, #[serde(default)] label: String }

/// Owner: what closing these days would do (before confirming).
pub async fn check_closure(State(s): State<AppState>, a: Auth, Query(r): Query<ClosureReq>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (from, to) = (date_of(&r.starts_on).ok_or(AppError::BadRequest("Choose the first day."))?, date_of(&r.ends_on).ok_or(AppError::BadRequest("Choose the last day."))?);
    let (open, booked) = closure_check(&s, &from, &to).await?;
    Ok(Json(json!({ "open_days": open, "booked": booked })))
}

/// Owner: close the clinic for some days. Appointments already booked stay booked (the app lists them to reschedule).
pub async fn add_closure(State(s): State<AppState>, a: Auth, Json(r): Json<ClosureReq>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (from, to) = (date_of(&r.starts_on).ok_or(AppError::BadRequest("Choose the first day."))?, date_of(&r.ends_on).ok_or(AppError::BadRequest("Choose the last day."))?);
    let (_, booked) = closure_check(&s, &from, &to).await?;
    let (id, days) = make_closure(&s, a.staff.id, &from, &to, &r.label).await?;
    auth::log(&s.pool, Some(a.staff.id), "clinic.closure_added", None, &a.device, &format!("{from}…{to}")).await;
    Ok(Json(json!({ "id": id, "days": days, "booked": booked })))
}

/// Owner: reopen — removes the closure and its blocked time.
pub async fn delete_closure(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let n = sqlx::query("DELETE FROM closures WHERE id = $1").bind(id).execute(&s.pool).await?.rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    auth::log(&s.pool, Some(a.staff.id), "clinic.closure_removed", None, &a.device, "").await;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::{date_of, fold, note_facts, tidy_receipt};
    use serde_json::json;
    #[test]
    fn dates_people_and_receipts_use() {
        assert_eq!(date_of("2026-12-24").as_deref(), Some("2026-12-24"));
        assert_eq!(date_of("24/12/2026").as_deref(), Some("2026-12-24"));
        assert_eq!(date_of("5-1-26").as_deref(), Some("2026-01-05"));
        assert_eq!(date_of("31/02/2026"), None);
        assert_eq!(date_of("ontem"), None);
    }
    #[test]
    fn receipt_answers_are_checked() {
        let v = json!({ "supplier": " Farmácia Central ", "supplier_nif": "PT 501 234 567", "date": "03/11/2026", "total": 12.3, "vat_shown": true, "vat": 2.3,
                        "document_number": "FR 2026/77", "category": "supplies", "description": "Ligaduras", "payment": "card" });
        let t = tidy_receipt(&v, "2026-11-09");
        assert_eq!(t["fields"]["supplier"], "Farmácia Central");
        assert_eq!(t["fields"]["supplier_nif"], "501234567");
        assert_eq!(t["fields"]["spent_on"], "2026-11-03");
        assert_eq!(t["fields"]["amount_cents"], 1230);
        assert_eq!(t["fields"]["vat_cents"], 230);
        assert_eq!(t["fields"]["method"], "card");
        assert_eq!(t["missing"], json!([]));
        // Nonsense is dropped, never guessed: future date, negative total, VAT above the total, unknown category.
        let bad = json!({ "supplier": "", "supplier_nif": "123", "date": "2027-01-01", "total": -5, "vat_shown": true, "vat": 99,
                          "document_number": "", "category": "pizza", "description": "", "payment": "unknown" });
        let t = tidy_receipt(&bad, "2026-11-09");
        assert!(t["fields"]["spent_on"].is_null() && t["fields"]["amount_cents"].is_null() && t["fields"]["method"].is_null());
        assert_eq!(t["fields"]["category"], "other");
        assert_eq!(t["fields"]["supplier_nif"], "");
        assert_eq!(t["missing"], json!(["supplier", "spent_on", "amount_cents"]));
    }
    #[test]
    fn note_facts_keep_only_what_was_filled() {
        let b = json!([{ "type": "slider", "label": "Dor", "value": 6, "max": 10 }, { "type": "text", "label": "Técnica", "value": "" },
                       { "type": "checklist", "label": "Técnicas", "value": ["Mobilização", "Exercício"] }, { "type": "choice", "label": "Resposta", "value": "Melhor" }]);
        assert_eq!(note_facts(&b, " Sem queixas novas. "), "- Dor: 6/10\n- Técnicas: Mobilização, Exercício\n- Resposta: Melhor\n\nSem queixas novas.");
        assert_eq!(note_facts(&json!([]), "  "), "");
    }
    #[test]
    fn names_fold() {
        assert_eq!(fold("  Avaliação  Inicial (60 min)"), "avaliacao inicial 60 min");
    }
}
