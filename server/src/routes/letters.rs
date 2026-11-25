//! Reimbursement pack and discharge letters (owner decisions, 8 Nov 2026).
//!
//! **Reimbursement pack** (owner and therapists; it holds a short clinical report, so never reception):
//! one PDF for ADSE or an insurer — identification, prescription, sessions done, invoices and a short
//! clinical report the therapist can edit, then the prescription scan and each invoice file as pages.
//! Saved in the patient's `Documentos` folder as a `report` document, so it can be opened again and saved.
//!
//! **Discharge letters**: the local AI (Ollama on this server, see `ai.rs`) drafts the text for the owner;
//! the AI never saves. Therapists may start a draft by hand (pre-filled from the file). Only the owner
//! approves; an approved letter is saved as PDF in `Utentes/<n.º>-<nome>/Cartas/`, can't be changed, and
//! the owner can mark the patient as discharged (inactive) at the same time.

use super::documents::write;
use super::schedule::tz;
use super::util::require_clinical;
use crate::ai::{self, AiError};
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use crate::export::{self, Line};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn subject(id: Uuid) -> Option<(&'static str, String)> {
    Some(("patient", id.to_string()))
}

fn st(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string().replace('.', ","),
        _ => String::new(),
    }
}

/// Marker for the parts of a pre-filled text a person must complete. An approved letter can't contain it.
pub const TODO_PT: &str = "[a completar]";
pub const TODO_EN: &str = "[to complete]";

// ───────── what the file says (shared by the pack, the template and the AI) ─────────

/// Facts about one patient's treatment, in the clinic's time zone. `from`/`to` limit the sessions
/// (whole history when absent).
const FACTS_SQL: &str = "
    SELECT json_build_object(
      'p', (SELECT json_build_object('number', p.number, 'name', p.name, 'birth', to_char(p.birth_date, 'DD/MM/YYYY'),
                'age', date_part('year', age(p.birth_date))::int, 'sex', p.sex, 'nif', p.nif, 'sns', p.sns_number, 'lang', p.lang,
                'active', p.active)
            FROM patients p WHERE p.id = $1 AND p.deleted_at IS NULL AND p.anonymised_at IS NULL),
      'clinic', (SELECT name FROM clinic LIMIT 1), 'clinic_info', (SELECT json_build_object('name', c.name, 'nif', c.nif, 'ers', c.ers_number, 'address', c.address, 'phone', c.phone, 'email', c.email, 'logo', encode(c.logo, 'base64')) FROM clinic c WHERE c.id = 1),
      'today', to_char(now() AT TIME ZONE $2, 'DD/MM/YYYY'),
      'day', to_char(now() AT TIME ZONE $2, 'YYYY-MM-DD'),
      'an', (SELECT json_build_object('main_complaint', main_complaint, 'history', history, 'goals', goals,
                'medical_history', medical_history, 'regions', regions) FROM anamnesis WHERE patient_id = $1),
      'visits', (SELECT COALESCE(json_agg(json_build_object('id', ap.id, 'on', to_char(ap.starts_at AT TIME ZONE $2, 'DD/MM/YYYY'),
                'time', to_char(ap.starts_at AT TIME ZONE $2, 'HH24:MI'), 'kind', ap.kind, 'title', ap.title, 'therapist', st.name,
                'minutes', (extract(epoch FROM ap.ends_at - ap.starts_at) / 60)::int, 'rx', ap.prescription_id) ORDER BY ap.starts_at), '[]')
             FROM appointments ap LEFT JOIN staff st ON st.id = ap.therapist_id
             WHERE ((ap.patient_id = $1 AND ap.status = 'done')
                    OR ap.id IN (SELECT appointment_id FROM class_attendees WHERE patient_id = $1 AND attended IS TRUE))
               AND ap.kind <> 'block'
               AND ($3::date IS NULL OR (ap.starts_at AT TIME ZONE $2)::date >= $3::date)
               AND ($4::date IS NULL OR (ap.starts_at AT TIME ZONE $2)::date <= $4::date)),
      'tests', (SELECT COALESCE(json_agg(json_build_object('code', t.test_code, 'score', t.score, 'max', t.max_score,
                'worse_up', t.higher_is_worse, 'on', to_char(t.taken_on, 'DD/MM/YYYY')) ORDER BY t.test_code, t.taken_on, t.created_at), '[]')
             FROM test_results t WHERE t.patient_id = $1),
      'notes', (SELECT COALESCE(json_agg(json_build_object('blocks', n.blocks, 'text', n.text,
                'on', to_char(n.signed_at AT TIME ZONE $2, 'DD/MM/YYYY')) ORDER BY n.signed_at), '[]')
             FROM notes n WHERE n.patient_id = $1 AND n.signed_at IS NOT NULL),
      'rx', (SELECT COALESCE(json_agg(json_build_object('id', pr.id, 'doctor', pr.doctor, 'specialty', pr.specialty, 'payer', pr.payer,
                'issued', to_char(pr.issued_on, 'DD/MM/YYYY'), 'issued_iso', pr.issued_on, 'valid', to_char(pr.valid_until, 'DD/MM/YYYY'),
                'total', pr.sessions_total, 'document_id', pr.document_id,
                'used', (SELECT count(*) FROM appointments WHERE prescription_id = pr.id AND status = 'done'))
                ORDER BY pr.issued_on DESC), '[]')
             FROM prescriptions pr WHERE pr.patient_id = $1))";

async fn facts(s: &AppState, pid: Uuid, from: Option<&str>, to: Option<&str>) -> ApiResult<Value> {
    let (v,): (Value,) = sqlx::query_as(FACTS_SQL).bind(pid).bind(tz()).bind(from).bind(to).fetch_one(&s.pool).await?;
    if v["p"].is_null() {
        return Err(AppError::NotFound);
    }
    Ok(v)
}

fn region(k: &str, en: bool) -> String {
    let (pt, e) = match k {
        "cervical" => ("coluna cervical", "cervical spine"), "thoracic" => ("coluna dorsal", "thoracic spine"),
        "lumbar" => ("coluna lombar", "lumbar spine"), "shoulder_r" => ("ombro direito", "right shoulder"),
        "shoulder_l" => ("ombro esquerdo", "left shoulder"), "elbow_r" => ("cotovelo direito", "right elbow"),
        "elbow_l" => ("cotovelo esquerdo", "left elbow"), "wrist_hand" => ("punho e mão", "wrist and hand"),
        "hip" => ("anca", "hip"), "knee_r" => ("joelho direito", "right knee"), "knee_l" => ("joelho esquerdo", "left knee"),
        "ankle_foot" => ("tornozelo e pé", "ankle and foot"), o => (o, o),
    };
    (if en { e } else { pt }).to_string()
}
fn symptom(k: &str, en: bool) -> String {
    let (pt, e) = match k {
        "pain" => ("dor", "pain"), "stiffness" => ("rigidez", "stiffness"), "weakness" => ("fraqueza", "weakness"),
        "tingling" => ("formigueiro", "tingling"), "numbness" => ("dormência", "numbness"), "swelling" => ("edema", "swelling"),
        "instability" => ("instabilidade", "instability"), "cramps" => ("cãibras", "cramps"), o => (o, o),
    };
    (if en { e } else { pt }).to_string()
}
fn test_name(k: &str, en: bool) -> String {
    match k {
        "nprs" => if en { "Pain (NPRS, 0–10)".into() } else { "Dor (END, 0–10)".into() },
        "ndi" => "NDI".into(), "odi" => "ODI (%)".into(), "quickdash" => "QuickDASH".into(), "spadi" => "SPADI (%)".into(),
        "lefs" => "LEFS".into(), "koos" => "KOOS".into(), "psfs" => "PSFS".into(), o => o.to_uppercase(),
    }
}
fn kind_pt(k: &str) -> &str {
    match k { "session" => "Sessão", "assessment" => "Avaliação", "class" => "Aula", "home" => "Domicílio", o => o }
}

/// "a, b e c" / "a, b and c".
fn and_list(items: &[String], en: bool) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        n => format!("{} {} {}", items[..n - 1].join(", "), if en { "and" } else { "e" }, items[n - 1]),
    }
}

/// Regions with their symptoms: "dor e rigidez na coluna cervical; dor no ombro direito".
fn complaints(v: &Value, en: bool) -> String {
    let empty = vec![];
    v["an"]["regions"].as_array().unwrap_or(&empty).iter().map(|r| {
        let syms: Vec<String> = r["symptoms"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(|x| symptom(x, en)).collect()).unwrap_or_default();
        let place = region(&st(r, "region"), en);
        if syms.is_empty() { place } else if en { format!("{} in the {place}", and_list(&syms, en)) } else { format!("{} — {place}", and_list(&syms, en)) }
    }).collect::<Vec<_>>().join("; ")
}

/// Techniques ticked in signed notes, most used first (at most eight).
fn techniques(v: &Value) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    let empty = vec![];
    for n in v["notes"].as_array().unwrap_or(&empty) {
        for b in n["blocks"].as_array().unwrap_or(&empty) {
            if b["type"] == "checklist" {
                for t in b["value"].as_array().unwrap_or(&empty).iter().filter_map(Value::as_str) {
                    match counts.iter_mut().find(|(x, _)| x.eq_ignore_ascii_case(t)) {
                        Some(c) => c.1 += 1,
                        None => counts.push((t.to_string(), 1)),
                    }
                }
            }
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    counts.into_iter().take(8).map(|c| c.0.to_lowercase()).collect()
}

/// First and last result of each test, one per line: "NDI: de 32/50 para 12/50 (04/09/2026 – 30/10/2026)".
/// No arrows: the PDF font has none.
fn test_changes(v: &Value, en: bool) -> Vec<String> {
    let empty = vec![];
    let tests = v["tests"].as_array().unwrap_or(&empty);
    let mut codes: Vec<String> = tests.iter().map(|t| st(t, "code")).collect();
    codes.dedup();
    let score = |t: &Value| if t["max"].is_null() { st(t, "score") } else { format!("{}/{}", st(t, "score"), st(t, "max")) };
    codes.iter().filter_map(|c| {
        let rs: Vec<&Value> = tests.iter().filter(|t| &st(t, "code") == c).collect();
        let (f, l) = (rs.first()?, rs.last()?);
        Some(match (rs.len() == 1, en) {
            (true, _) => format!("{}: {} ({})", test_name(c, en), score(f), st(f, "on")),
            (false, false) => format!("{}: de {} para {} ({} – {})", test_name(c, en), score(f), score(l), st(f, "on"), st(l, "on")),
            (false, true) => format!("{}: from {} to {} ({} – {})", test_name(c, en), score(f), score(l), st(f, "on"), st(l, "on")),
        })
    }).collect()
}

fn first_last(v: &Value) -> Option<(String, String, usize)> {
    let visits = v["visits"].as_array()?;
    Some((st(visits.first()?, "on"), st(visits.last()?, "on"), visits.len()))
}

// ───────── reimbursement pack ─────────

#[derive(Deserialize)]
pub struct PackQuery { prescription_id: Option<Uuid>, from: Option<String>, to: Option<String> }

/// The short clinical report a pack starts with (the therapist can change it before making the pack).
fn pack_report(v: &Value, rx: Option<&Value>) -> String {
    let p = &v["p"];
    let mut out = String::new();
    match first_last(v) {
        Some((f, l, n)) => out.push_str(&format!("{} realizou {n} {} de fisioterapia nesta clínica entre {f} e {l}", st(p, "name"), if n == 1 { "sessão" } else { "sessões" })),
        None => out.push_str(&format!("{} está em acompanhamento de fisioterapia nesta clínica", st(p, "name"))),
    }
    if let Some(r) = rx {
        let who = [st(r, "doctor"), st(r, "specialty")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
        if !who.is_empty() {
            out.push_str(&format!(", por prescrição de {who} de {}", st(r, "issued")));
        }
    }
    out.push('.');
    let reason = st(&v["an"], "main_complaint");
    if !reason.trim().is_empty() {
        out.push_str(&format!("\n\nMotivo: {}.", reason.trim().trim_end_matches('.')));
    }
    let c = complaints(v, false);
    if !c.is_empty() {
        out.push_str(&format!(" Na avaliação inicial: {c}."));
    }
    let t = techniques(v);
    if !t.is_empty() {
        out.push_str(&format!("\n\nTratamento: {}.", and_list(&t, false)));
    }
    let changes = test_changes(v, false);
    if !changes.is_empty() {
        out.push_str(&format!("\n\nEvolução:\n• {}", changes.join("\n• ")));
    }
    out
}

/// Owner/therapist: what a pack would hold, to choose from before making it. Logs `patient.viewed` (reimbursement).
pub async fn pack_preview(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Query(q): Query<PackQuery>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let mut v = facts(&s, pid, None, None).await?;
    let rx = q.prescription_id.and_then(|id| v["rx"].as_array().and_then(|a| a.iter().find(|r| st(r, "id") == id.to_string()).cloned()));
    if q.prescription_id.is_some() && rx.is_none() {
        return Err(AppError::NotFound);
    }
    // Period: as asked; else from the prescription's date (or three months back) to today.
    let from = q.from.clone().filter(|x| !x.is_empty()).or_else(|| rx.as_ref().map(|r| st(r, "issued_iso")));
    let (from, to): (String, String) = sqlx::query_as(&format!(
        "SELECT COALESCE($1::date, ((now() AT TIME ZONE '{0}')::date - interval '3 months')::date)::text,
                COALESCE($2::date, (now() AT TIME ZONE '{0}')::date)::text", tz()))
        .bind(from).bind(q.to.clone().filter(|x| !x.is_empty())).fetch_one(&s.pool).await?;
    let ranged = facts(&s, pid, Some(&from), Some(&to)).await?;
    v["visits"] = ranged["visits"].clone();
    let (payments,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('id', p.id, 'number', p.number, 'received_on', p.received_on,
                'amount_cents', p.amount_cents, 'description', p.description, 'invoice_number', p.invoice_number,
                'has_file', p.invoice_file_id IS NOT NULL, 'file_mime', f.mime) ORDER BY p.received_on, p.number), '[]')
         FROM payments p LEFT JOIN finance_files f ON f.id = p.invoice_file_id
         WHERE p.patient_id = $1 AND p.voided_at IS NULL AND p.received_on BETWEEN $2::date AND $3::date")
        .bind(pid).bind(&from).bind(&to).fetch_one(&s.pool).await?;
    let (pro,): (String,) = sqlx::query_as("SELECT professional_id FROM staff WHERE id = $1").bind(a.staff.id).fetch_one(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "patient.viewed", subject(pid), &a.device, "reimbursement").await;
    Ok(Json(json!({
        "from": from, "to": to,
        "prescriptions": v["rx"], "prescription_id": rx.as_ref().map(|r| r["id"].clone()),
        "payer": rx.as_ref().map(|r| st(r, "payer")).unwrap_or_default(),
        "sessions": v["visits"], "payments": payments,
        "report": pack_report(&v, rx.as_ref()),
        "signer": a.staff.name, "professional_id": pro,
    })))
}

#[derive(Deserialize)]
pub struct PackReq {
    prescription_id: Option<Uuid>,
    from: String,
    to: String,
    #[serde(default)]
    payment_ids: Vec<Uuid>,
    #[serde(default)]
    payer: String,
    report: String,
    #[serde(default)]
    professional_id: String,
}

/// A scanned file turned into PDF page(s): a PDF as is, a JPEG or PNG on its own page with a heading.
/// None when it can't be joined (HEIC/WebP, protected or damaged PDF).
fn attachment(title: &str, mime: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    match mime {
        "application/pdf" => export::joinable(bytes).then(|| bytes.to_vec()),
        "image/jpeg" | "image/png" => {
            let pic = if mime == "image/png" { export::picture_from_png(bytes) } else { export::picture_from_jpeg(bytes) }?;
            // Fit inside the page under the heading (483 × 680 points).
            let w = 483f32.min(680.0 * pic.aspect());
            Some(export::pdf(&[Line::Heading(title.to_string()), Line::Image(pic, w)]))
        }
        _ => None,
    }
}

/// Owner/therapist: make the pack, save it in the patient's documents and return it.
pub async fn make_pack(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Json(r): Json<PackReq>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let v = facts(&s, pid, Some(&r.from), Some(&r.to)).await?;
    let p = &v["p"];
    let empty = vec![];
    let rx = match r.prescription_id {
        Some(id) => Some(v["rx"].as_array().unwrap_or(&empty).iter().find(|x| st(x, "id") == id.to_string()).cloned().ok_or(AppError::NotFound)?),
        None => None,
    };
    if r.report.trim().is_empty() {
        return Err(AppError::BadRequest("Write the short clinical report."));
    }
    if r.report.len() > 8000 || r.payer.len() > 120 || r.professional_id.len() > 40 {
        return Err(AppError::BadRequest("Some details are too long."));
    }
    let pro = r.professional_id.trim().to_string();
    sqlx::query("UPDATE staff SET professional_id = $2 WHERE id = $1").bind(a.staff.id).bind(&pro).execute(&s.pool).await?;

    // Only this patient's own, valid payments.
    let (pays,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('number', p.number, 'on', to_char(p.received_on, 'DD/MM/YYYY'),
                'amount', p.amount_cents, 'description', p.description, 'invoice', p.invoice_number,
                'path', f.path, 'mime', f.mime) ORDER BY p.received_on, p.number), '[]')
         FROM payments p LEFT JOIN finance_files f ON f.id = p.invoice_file_id
         WHERE p.patient_id = $1 AND p.voided_at IS NULL AND p.id = ANY($2)")
        .bind(pid).bind(&r.payment_ids).fetch_one(&s.pool).await?;
    let pays = pays.as_array().cloned().unwrap_or_default();
    if pays.len() != r.payment_ids.len() {
        return Err(AppError::BadRequest("Some payments aren’t this patient’s or were voided. Refresh and try again."));
    }

    // Attachments, in order: the prescription scan, then each invoice.
    let mut attached: Vec<Vec<u8>> = Vec::new();
    let mut listed: Vec<String> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    let read = |path: &str| {
        let ok = !path.is_empty() && !path.contains("..");
        let full = s.archive.join(path);
        async move { if ok { tokio::fs::read(full).await.ok() } else { None } }
    };
    if let Some(rxv) = &rx {
        if let Some(doc) = rxv["document_id"].as_str() {
            let row: Option<(String, String)> = sqlx::query_as("SELECT path, mime FROM documents WHERE id = $1::uuid AND patient_id = $2")
                .bind(doc).bind(pid).fetch_optional(&s.pool).await?;
            let label = "Prescrição médica".to_string();
            match row {
                Some((path, mime)) => match read(&path).await.and_then(|b| attachment(&label, &mime, &b)) {
                    Some(pdf) => { attached.push(pdf); listed.push(label); }
                    None => skipped.push(json!({ "what": label, "reason": "unreadable" })),
                },
                None => skipped.push(json!({ "what": label, "reason": "missing" })),
            }
        }
    }
    for pay in &pays {
        let label = if st(pay, "invoice").is_empty() { format!("Recibo interno n.º {} de {}", pay["number"], st(pay, "on")) } else { format!("Fatura {} de {}", st(pay, "invoice"), st(pay, "on")) };
        let path = st(pay, "path");
        if path.is_empty() {
            skipped.push(json!({ "what": label, "reason": "no_file" }));
            continue;
        }
        match read(&path).await.and_then(|b| attachment(&label, &st(pay, "mime"), &b)) {
            Some(pdf) => { attached.push(pdf); listed.push(label); }
            None => skipped.push(json!({ "what": label, "reason": "unreadable" })),
        }
    }

    // The first pages.
    let mut l = vec![Line::Title("Pedido de reembolso — fisioterapia".into())];
    l.extend(crate::export::clinic_header(&v["clinic_info"], &st(&v, "clinic")));
    l.push(Line::Text(format!("Emitido a {}", st(&v, "today"))));
    l.push(Line::Heading("Utente".into()));
    l.push(Line::Row("Nome".into(), st(p, "name")));
    l.push(Line::Row("N.º de utente na clínica".into(), p["number"].to_string()));
    for (label, key) in [("Data de nascimento", "birth"), ("NIF", "nif"), ("N.º SNS", "sns")] {
        if !st(p, key).is_empty() {
            l.push(Line::Row(label.into(), st(p, key)));
        }
    }
    let payer = r.payer.trim();
    if !payer.is_empty() {
        l.push(Line::Row("Entidade".into(), payer.to_string()));
    }
    if let Some(x) = &rx {
        l.push(Line::Heading("Prescrição médica".into()));
        for (label, val) in [("Médico(a)", st(x, "doctor")), ("Especialidade", st(x, "specialty")), ("Data", st(x, "issued")),
                             ("Sessões prescritas", st(x, "total")), ("Válida até", st(x, "valid"))] {
            if !val.is_empty() {
                l.push(Line::Row(label.into(), val));
            }
        }
    }
    let visits = v["visits"].as_array().unwrap_or(&empty);
    l.push(Line::Heading(format!("Sessões realizadas de {} a {} ({})", date_pt(&r.from), date_pt(&r.to), visits.len())));
    if visits.is_empty() {
        l.push(Line::Text("Sem sessões realizadas neste período.".into()));
    }
    for x in visits {
        let what = if st(x, "title").is_empty() { kind_pt(&st(x, "kind")).to_string() } else { format!("{} — {}", kind_pt(&st(x, "kind")), st(x, "title")) };
        let who = if st(x, "therapist").is_empty() { String::new() } else { format!(" · {}", st(x, "therapist")) };
        l.push(Line::Row(format!("{} {} — {what}{who}", st(x, "on"), st(x, "time")), format!("{} min", st(x, "minutes"))));
    }
    if !pays.is_empty() {
        l.push(Line::Heading(format!("Faturas ({})", pays.len())));
        let mut total = 0i64;
        for pay in &pays {
            let amount = pay["amount"].as_i64().unwrap_or(0);
            total += amount;
            let doc = if st(pay, "invoice").is_empty() { format!("Recibo interno n.º {}", pay["number"]) } else { st(pay, "invoice") };
            let desc = if st(pay, "description").is_empty() { String::new() } else { format!(" — {}", st(pay, "description")) };
            l.push(Line::Row(format!("{} · {doc}{desc}", st(pay, "on")), export::euros(amount)));
        }
        l.push(Line::Row("Total".into(), export::euros(total)));
    }
    l.push(Line::Heading("Relatório clínico".into()));
    for para in r.report.lines() {
        if para.trim().is_empty() {
            l.push(Line::Gap);
        } else {
            l.extend(export::wrap(para, 10.0, 483.0).into_iter().map(Line::Text));
        }
    }
    l.push(Line::Gap);
    l.push(Line::Gap);
    l.push(Line::Text(a.staff.name.clone()));
    l.push(Line::Text(if pro.is_empty() { "Fisioterapeuta".into() } else { format!("Fisioterapeuta — cédula profissional n.º {pro}") }));
    l.push(Line::Text(st(&v, "today")));
    if !listed.is_empty() || !skipped.is_empty() {
        l.push(Line::Heading("Anexos".into()));
        for x in &listed {
            l.push(Line::Text(format!("•  {x}")));
        }
        for x in &skipped {
            let why = match x["reason"].as_str() {
                Some("no_file") => "sem ficheiro registado na clínica; junte-a à parte",
                Some("missing") => "o ficheiro falta no arquivo da clínica; junte-o à parte",
                _ => "o ficheiro não pôde ser junto (formato não suportado ou PDF protegido); junte-o à parte",
            };
            l.push(Line::Text(format!("•  {}: {why}", st(x, "what"))));
        }
    }
    let mut parts = vec![export::pdf(&l)];
    parts.extend(attached);
    let bytes = if parts.len() == 1 { parts.remove(0) } else { export::join(&parts).ok_or(AppError::Internal("joining the pack's PDFs".into()))? };

    // Saved with the patient's documents.
    let id = Uuid::new_v4();
    let payer_part = if payer.is_empty() { String::new() } else { format!(" {}", export::safe_name(payer)) };
    let filename = format!("{} Pacote de reembolso{payer_part}.pdf", st(&v, "day"));
    let rel = write(&s, pid, "Documentos", &format!("{} {}-{}", st(&v, "day"), &id.simple().to_string()[..8], filename.split_once(' ').map(|x| x.1).unwrap_or(&filename)), &bytes).await?;
    let sha = hex::encode(Sha256::digest(&bytes));
    let res = sqlx::query("INSERT INTO documents (id, patient_id, kind, filename, mime, bytes, path, sha256, uploaded_by) VALUES ($1,$2,'report',$3,'application/pdf',$4,$5,$6,$7)")
        .bind(id).bind(pid).bind(&filename).bind(bytes.len() as i64).bind(&rel).bind(sha).bind(a.staff.id)
        .execute(&s.pool).await;
    if let Err(e) = res {
        let _ = tokio::fs::remove_file(s.archive.join(&rel)).await;
        return Err(e.into());
    }
    let pages = lopdf::Document::load_mem(&bytes).map(|d| d.get_pages().len()).unwrap_or(0);
    auth::log(&s.pool, Some(a.staff.id), "patient.reimbursement_pack", subject(pid), &a.device,
              &format!("{} invoices, {} attachments, {} not attached", pays.len(), listed.len(), skipped.len())).await;
    Ok(Json(json!({ "document_id": id, "filename": filename, "pages": pages, "attached": listed, "skipped": skipped })))
}

/// "2026-11-08" → "08/11/2026".
fn date_pt(iso: &str) -> String {
    match (iso.get(0..4), iso.get(5..7), iso.get(8..10)) {
        (Some(y), Some(m), Some(d)) => format!("{d}/{m}/{y}"),
        _ => iso.to_string(),
    }
}

// ───────── discharge letters ─────────

fn default_addressee(v: &Value, en: bool) -> String {
    let doctor = v["rx"].as_array().and_then(|a| a.first()).map(|r| st(r, "doctor")).unwrap_or_default();
    let doctor = doctor.trim();
    // "Dr. Lopes", "Dra. Sá", "Dr.ª Sá" already carry the title.
    let titled = ["Dr", "Prof"].iter().any(|t| doctor.starts_with(t));
    match (doctor.is_empty(), en) {
        (false, false) if titled => format!("Ao(À) Exmo.(a) Sr.(a) {doctor}"),
        (false, false) => format!("Ao(À) Exmo.(a) Sr.(a) Dr.(a) {doctor}"),
        (false, true) if titled => format!("To {doctor}"),
        (false, true) => format!("To Dr {doctor}"),
        (true, false) => "Ao(À) médico(a) assistente".into(),
        (true, true) => "To the attending physician".into(),
    }
}

/// A letter pre-filled from the file, with "[a completar]" where a person must write.
fn letter_template(v: &Value, en: bool) -> String {
    let p = &v["p"];
    let todo = if en { TODO_EN } else { TODO_PT };
    let age = p["age"].as_i64().map(|a| if en { format!(", aged {a},") } else { format!(", de {a} anos,") }).unwrap_or_default();
    let followed = match p["sex"].as_str() { Some("f") => "acompanhada", Some("m") => "acompanhado", _ => "acompanhado(a)" };
    let mut out = match (first_last(v), en) {
        (Some((f, l, n)), false) => format!("Venho por este meio informar que {}{age} foi {followed} em fisioterapia nesta clínica entre {f} e {l}, num total de {n} {}.", st(p, "name"), if n == 1 { "sessão" } else { "sessões" }),
        (Some((f, l, n)), true) => format!("I am writing to let you know that {}{age} attended physiotherapy at this clinic from {f} to {l}, {n} {} in total.", st(p, "name"), if n == 1 { "session" } else { "sessions" }),
        (None, false) => format!("Venho por este meio informar que {}{age} foi {followed} em fisioterapia nesta clínica.", st(p, "name")),
        (None, true) => format!("I am writing to let you know that {}{age} attended physiotherapy at this clinic.", st(p, "name")),
    };
    let reason = st(&v["an"], "main_complaint");
    let c = complaints(v, en);
    out.push_str("\n\n");
    out.push_str(&match (reason.trim().is_empty(), en) {
        (false, false) => format!("Motivo da referenciação: {}.", reason.trim().trim_end_matches('.')),
        (false, true) => format!("Reason for referral: {}.", reason.trim().trim_end_matches('.')),
        (true, false) => format!("Motivo da referenciação: {todo}."),
        (true, true) => format!("Reason for referral: {todo}."),
    });
    if !c.is_empty() {
        out.push_str(&if en { format!(" At the first assessment: {c}.") } else { format!(" Na avaliação inicial: {c}.") });
    }
    let t = techniques(v);
    out.push_str("\n\n");
    out.push_str(&match (t.is_empty(), en) {
        (false, false) => format!("O tratamento incluiu {}.", and_list(&t, false)),
        (false, true) => format!("Treatment included {}.", and_list(&t, true)),
        (true, false) => format!("O tratamento incluiu {todo}."),
        (true, true) => format!("Treatment included {todo}."),
    });
    let changes = test_changes(v, en);
    if !changes.is_empty() {
        out.push_str(&if en { format!("\n\nOutcome measures:\n• {}", changes.join("\n• ")) } else { format!("\n\nEvolução nos testes:\n• {}", changes.join("\n• ")) });
    }
    out.push_str(&if en {
        format!("\n\nAt discharge: {todo}.\n\nRecommendations: {todo}.")
    } else {
        format!("\n\nÀ data da alta: {todo}.\n\nRecomendações: {todo}.")
    });
    out
}

/// What the AI is told about the patient: only facts from the file, in plain lines.
fn ai_facts(v: &Value, en: bool) -> String {
    let p = &v["p"];
    let an = &v["an"];
    let mut f = vec![format!("Nome: {}", st(p, "name"))];
    if let Some(age) = p["age"].as_i64() {
        f.push(format!("Idade: {age} anos"));
    }
    if let Some(sex) = p["sex"].as_str() {
        f.push(format!("Sexo: {}", match sex { "f" => "feminino", "m" => "masculino", _ => "outro" }));
    }
    let clip = |s: String, n: usize| if s.chars().count() > n { format!("{}…", s.chars().take(n).collect::<String>()) } else { s };
    for (label, key) in [("Queixa principal", "main_complaint"), ("História atual", "history"), ("Antecedentes", "medical_history"), ("Objetivos", "goals")] {
        let x = st(an, key);
        if !x.trim().is_empty() {
            f.push(format!("{label}: {}", clip(x.replace('\n', " "), 600)));
        }
    }
    let c = complaints(v, false);
    if !c.is_empty() {
        f.push(format!("Regiões e sintomas na avaliação inicial: {c}"));
    }
    if let Some((first, last, n)) = first_last(v) {
        f.push(format!("Sessões realizadas: {n}, de {first} a {last}"));
    }
    let t = techniques(v);
    if !t.is_empty() {
        f.push(format!("Técnicas registadas nas notas: {}", t.join(", ")));
    }
    for x in test_changes(v, false) {
        f.push(format!("Teste: {x}"));
    }
    if let Some(r) = v["rx"].as_array().and_then(|a| a.first()) {
        let who = [st(r, "doctor"), st(r, "specialty")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
        if !who.is_empty() {
            f.push(format!("Prescrição de: {who} ({})", st(r, "issued")));
        }
    }
    // The last three signed notes, shortened: how the patient was doing at the end.
    let empty = vec![];
    let notes = v["notes"].as_array().unwrap_or(&empty);
    for n in notes.iter().rev().take(3).rev() {
        let mut parts = Vec::new();
        for b in n["blocks"].as_array().unwrap_or(&empty) {
            let val = match b["type"].as_str() {
                Some("checklist") => b["value"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
                _ => st(b, "value"),
            };
            if !val.trim().is_empty() {
                parts.push(format!("{}: {val}", st(b, "label")));
            }
        }
        if !st(n, "text").trim().is_empty() {
            parts.push(st(n, "text").replace('\n', " "));
        }
        if !parts.is_empty() {
            f.push(format!("Nota de {}: {}", st(n, "on"), clip(parts.join("; "), 700)));
        }
    }
    let lang = if en { "inglês (britânico)" } else { "português europeu" };
    format!("Escreve a carta em {lang}.\n\nFactos do processo clínico:\n- {}", f.join("\n- "))
}

const AI_SYSTEM: &str = "És fisioterapeuta numa clínica em Portugal e redige cartas de alta para o médico assistente. \
Escreves apenas o corpo da carta: sem cabeçalho, sem destinatário, sem saudação inicial, sem despedida e sem assinatura (a aplicação acrescenta-os). \
Usa só os factos fornecidos; não inventes diagnósticos, resultados, datas nem recomendações que não decorram deles. \
Quando faltar uma informação necessária (por exemplo, o estado à data da alta ou as recomendações), escreve [a completar] nesse lugar, ou [to complete] se a carta for em inglês. \
Tom profissional, frases claras, 120 a 250 palavras, 3 a 5 parágrafos curtos, sem títulos, listas nem markdown, sem formato SOAP.";

#[derive(Deserialize)]
pub struct LangQuery { lang: Option<String> }

fn is_en(l: Option<&str>) -> bool {
    l == Some("en")
}

/// Owner/therapist: a letter pre-filled from the file (no AI): `{addressee, body, lang}`.
pub async fn letter_template_get(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Query(q): Query<LangQuery>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let v = facts(&s, pid, None, None).await?;
    let en = match q.lang.as_deref() { Some(l) => is_en(Some(l)), None => st(&v["p"], "lang") == "en" };
    auth::log(&s.pool, Some(a.staff.id), "patient.viewed", subject(pid), &a.device, "letter").await;
    Ok(Json(json!({ "lang": if en { "en" } else { "pt" }, "addressee": default_addressee(&v, en), "body": letter_template(&v, en) })))
}

#[derive(Deserialize)]
pub struct AiReq { lang: Option<String> }

/// Owner: the local AI drafts the body of a discharge letter. Nothing is saved; the owner uses, retries or discards it.
pub async fn ai_draft(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Json(r): Json<AiReq>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    a.require_ai()?;
    let v = facts(&s, pid, None, None).await?;
    let en = is_en(r.lang.as_deref());
    let res = crate::style::generate_plain(AI_SYSTEM, &ai_facts(&v, en), en).await;
    // The access log says that the AI was used, never what it wrote.
    auth::log(&s.pool, Some(a.staff.id), "ai.letter_drafted", subject(pid), &a.device, if res.is_ok() { "ok" } else { "failed" }).await;
    match res {
        Ok(text) => Ok(Json(json!({ "text": text, "model": ai::model() }))),
        Err(AiError::Offline) => Err(AppError::ConflictWith("ai_offline", "Can’t reach the local AI. Check the AI computer is on and Ollama is running, and its address in Settings › Local AI.", json!({ "url": ai::url() }))),
        Err(AiError::NoModel) => Err(AppError::ConflictWith("ai_no_model", "The AI model isn’t installed on the server yet.", json!({ "model": ai::model() }))),
        Err(AiError::Failed | AiError::NoVision) => Err(AppError::ConflictWith("ai_failed", "The AI couldn’t finish the draft. Try again, or write the letter yourself.", Value::Null)),
    }
}

/// Owner, and therapists with AI on: is the local AI there? `{available, url, model, reason?, models, speech}`
/// (`speech`: the speech-to-text server, `{available, url, reason?}`).
pub async fn ai_status(a: Auth) -> ApiResult<Json<Value>> {
    // Whoever may use any AI feature (the assistant included) may see whether the AI PC is on.
    if !auth::assistant_allowed(&a.staff) {
        return Err(AppError::Forbidden);
    }
    let mut v = ai::status().await;
    v["speech"] = crate::stt::status().await;
    v["assistant"] = assistant_status(&v).await;
    Ok(Json(v))
}

/// The assistant's model (13 Nov 2026): its own if the owner chose one, else the main one. `{model, available, own}`.
async fn assistant_status(main: &Value) -> Value {
    match ai::assistant_model_chosen() {
        Some(m) if m != ai::model() => {
            let c = ai::check(&ai::url(), &m).await;
            json!({ "model": m, "available": c["available"], "reason": c["reason"], "own": true })
        }
        _ => json!({ "model": ai::model(), "available": main["available"], "reason": main["reason"], "own": false }),
    }
}

#[derive(Deserialize)]
pub struct SpeechReq { url: Option<String> }

fn speech_input(u: Option<&str>) -> ApiResult<Option<String>> {
    match u.map(str::trim) {
        None | Some("") => Ok(None),
        Some(u) => crate::ai::normalise_port(u, crate::stt::DEFAULT_PORT).map(Some)
            .ok_or(AppError::BadRequest("Use the address of a computer in the clinic, like 192.168.1.66 or 100.86.97.21:8178. Internet addresses aren’t allowed.")),
    }
}

/// Owner: try a speech-to-text address before saving it. Nothing is stored.
pub async fn speech_test(a: Auth, Json(r): Json<SpeechReq>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let url = speech_input(r.url.as_deref())?.unwrap_or_else(crate::stt::env_url);
    Ok(Json(crate::stt::check(&url).await))
}

/// Owner: where the local AI is and which model it uses, and where that comes from.
pub async fn ai_settings(a: Auth) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let (url_in_app, model_in_app) = ai::chosen_in_app();
    Ok(Json(json!({
        "url": ai::url(), "model": ai::model(), "url_in_app": url_in_app, "model_in_app": model_in_app,
        "server_url": ai::env_url(), "server_model": ai::env_model(),
        "speech_url": crate::stt::url(), "speech_url_in_app": crate::stt::chosen_in_app(), "server_speech_url": crate::stt::env_url(),
        "assistant_model": ai::assistant_model_chosen(),
    })))
}

#[derive(Deserialize)]
pub struct AiSettingsReq {
    url: Option<String>,
    model: Option<String>,
    /// The speech-to-text server (absent = unchanged, "" = back to WHISPER_URL).
    speech_url: Option<String>,
    /// The assistant's own model (absent = unchanged, "" = the main model).
    assistant_model: Option<String>,
}

fn ai_input(r: &AiSettingsReq) -> ApiResult<(Option<String>, Option<String>)> {
    let url = match r.url.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(u) => Some(ai::normalise(u).ok_or(AppError::BadRequest("Use the address of a computer in the clinic, like 100.86.97.21 or 192.168.1.20:11434. Internet addresses aren’t allowed."))?),
    };
    let model = match r.model.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(m) if ai::model_ok(m) => Some(m.to_string()),
        Some(_) => return Err(AppError::BadRequest("That model name isn’t valid. Use a name like gemma3:12b.")),
    };
    Ok((url, model))
}

/// Owner: try an address and model before saving them. Nothing is stored.
pub async fn ai_test(a: Auth, Json(r): Json<AiSettingsReq>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let (url, model) = ai_input(&r)?;
    // Testing the assistant's own model: on the address in use unless another one is being tried.
    if let Some(m) = r.assistant_model.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        if !ai::model_ok(m) {
            return Err(AppError::BadRequest("That model name isn’t valid. Use a name like gemma3:12b."));
        }
        return Ok(Json(ai::check(&url.unwrap_or_else(ai::url), m).await));
    }
    Ok(Json(ai::check(&url.unwrap_or_else(ai::env_url), &model.unwrap_or_else(ai::env_model)).await))
}

/// Owner: save the address and model (empty = go back to the server's settings). Logged (address only).
pub async fn put_ai_settings(State(s): State<AppState>, a: Auth, Json(r): Json<AiSettingsReq>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let (url, model) = ai_input(&r)?;
    let speech = r.speech_url.as_deref().map(|u| speech_input(Some(u))).transpose()?;
    let assistant = match r.assistant_model.as_deref().map(str::trim) {
        None => None,
        Some("") => Some(None),
        Some(m) if ai::model_ok(m) => Some(Some(m.to_string())),
        Some(_) => return Err(AppError::BadRequest("That model name isn’t valid. Use a name like gemma3:12b.")),
    };
    // Only the speech-to-text address or the assistant's model sent: the AI's address and model stay as they are.
    if (r.speech_url.is_none() && r.assistant_model.is_none()) || r.url.is_some() || r.model.is_some() {
        sqlx::query("UPDATE clinic SET ai_url = $1, ai_model = $2 WHERE id = 1").bind(&url).bind(&model).execute(&s.pool).await?;
        ai::set_chosen(url.clone(), model.clone());
    }
    let mut detail = format!("{} · {}", ai::url(), ai::model());
    if let Some(m) = assistant {
        sqlx::query("UPDATE clinic SET ai_assistant_model = $1 WHERE id = 1").bind(&m).execute(&s.pool).await?;
        ai::set_assistant_model(m);
        detail.push_str(&format!(" · assistant {}", ai::assistant_model()));
    }
    if let Some(sp) = speech {
        sqlx::query("UPDATE clinic SET stt_url = $1 WHERE id = 1").bind(&sp).execute(&s.pool).await?;
        crate::stt::set_chosen(sp);
        detail.push_str(&format!(" · speech {}", crate::stt::url()));
    }
    auth::log(&s.pool, Some(a.staff.id), "ai.settings_changed", None, &a.device, &detail).await;
    let mut v = ai::status().await;
    v["speech"] = crate::stt::status().await;
    v["assistant"] = assistant_status(&v).await;
    Ok(Json(v))
}

const LETTER_JSON: &str = "json_build_object('id', l.id, 'lang', l.lang, 'addressee', l.addressee, 'body', l.body, 'ai_drafted', l.ai_drafted,
        'created_by', l.created_by, 'created_by_name', cb.name, 'created_at', l.created_at, 'updated_at', l.updated_at,
        'approved_by_name', ab.name, 'approved_at', l.approved_at,
        'document_id', (SELECT id FROM documents WHERE letter_id = l.id))";

/// Owner/therapist: this patient's letters, newest first (drafts and approved).
pub async fn list_letters(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let (v,): (Value,) = sqlx::query_as(&format!(
        "SELECT COALESCE(json_agg({LETTER_JSON} ORDER BY l.created_at DESC), '[]')
         FROM discharge_letters l LEFT JOIN staff cb ON cb.id = l.created_by LEFT JOIN staff ab ON ab.id = l.approved_by
         WHERE l.patient_id = $1")).bind(pid).fetch_one(&s.pool).await?;
    let (pro,): (String,) = sqlx::query_as("SELECT professional_id FROM staff WHERE id = $1").bind(a.staff.id).fetch_one(&s.pool).await?;
    Ok(Json(json!({ "letters": v, "professional_id": pro })))
}

#[derive(Deserialize)]
pub struct LetterReq { lang: Option<String>, addressee: Option<String>, body: Option<String>, ai_drafted: Option<bool> }

fn check_letter(r: &LetterReq) -> ApiResult<()> {
    if r.lang.as_deref().is_some_and(|l| l != "pt" && l != "en") {
        return Err(AppError::BadRequest("Choose Portuguese or English."));
    }
    if r.body.as_ref().is_some_and(|b| b.len() > 20000) || r.addressee.as_ref().is_some_and(|b| b.len() > 300) {
        return Err(AppError::BadRequest("The letter is too long."));
    }
    Ok(())
}

/// Owner/therapist: save a new draft.
pub async fn create_letter(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Json(r): Json<LetterReq>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    check_letter(&r)?;
    facts(&s, pid, None, None).await?; // must be a live, named patient
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO discharge_letters (id, patient_id, lang, addressee, body, ai_drafted, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(id).bind(pid).bind(r.lang.as_deref().unwrap_or("pt")).bind(r.addressee.unwrap_or_default().trim())
        .bind(r.body.unwrap_or_default().trim()).bind(r.ai_drafted.unwrap_or(false)).bind(a.staff.id)
        .execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "letter.created", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "id": id })))
}

async fn letter_patient(s: &AppState, id: Uuid) -> ApiResult<(Uuid, Option<Uuid>, bool)> {
    let r: Option<(Uuid, Option<Uuid>, bool)> = sqlx::query_as("SELECT patient_id, created_by, approved_at IS NOT NULL FROM discharge_letters WHERE id = $1")
        .bind(id).fetch_optional(&s.pool).await?;
    r.ok_or(AppError::NotFound)
}

/// Owner, or the therapist who started it: change a draft. Approved letters can't change (database trigger).
pub async fn update_letter(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(r): Json<LetterReq>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    check_letter(&r)?;
    let (pid, author, approved) = letter_patient(&s, id).await?;
    if approved {
        return Err(AppError::Conflict("This letter is approved and can’t be changed. Write a new letter instead."));
    }
    if a.staff.role != "owner" && author != Some(a.staff.id) {
        return Err(AppError::Conflict("Only the owner or whoever started this draft can change it."));
    }
    sqlx::query("UPDATE discharge_letters SET lang = COALESCE($2, lang), addressee = COALESCE($3, addressee), body = COALESCE($4, body),
                 ai_drafted = ai_drafted OR COALESCE($5, FALSE), updated_at = now() WHERE id = $1 AND approved_at IS NULL")
        .bind(id).bind(r.lang).bind(r.addressee.map(|x| x.trim().to_string())).bind(r.body.map(|x| x.trim().to_string())).bind(r.ai_drafted)
        .execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "letter.updated", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

/// Owner, or the therapist who started it: throw away a draft.
pub async fn delete_letter(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let (pid, author, approved) = letter_patient(&s, id).await?;
    if approved {
        return Err(AppError::Conflict("Approved letters are kept."));
    }
    if a.staff.role != "owner" && author != Some(a.staff.id) {
        return Err(AppError::Conflict("Only the owner or whoever started this draft can discard it."));
    }
    sqlx::query("DELETE FROM discharge_letters WHERE id = $1 AND approved_at IS NULL").bind(id).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "letter.draft_deleted", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

/// The PDF of an approved letter.
async fn letter_pdf(s: &AppState, id: Uuid) -> ApiResult<(Uuid, String, Vec<u8>)> {
    let (v,): (Value,) = sqlx::query_as(&format!(
        "SELECT json_build_object('patient_id', p.id, 'number', p.number, 'name', p.name, 'birth', to_char(p.birth_date, 'DD/MM/YYYY'),
            'sns', p.sns_number, 'clinic', (SELECT name FROM clinic LIMIT 1), 'clinic_info', (SELECT json_build_object('name', c.name, 'nif', c.nif, 'ers', c.ers_number, 'address', c.address, 'phone', c.phone, 'email', c.email, 'logo', encode(c.logo, 'base64')) FROM clinic c WHERE c.id = 1), 'lang', l.lang, 'addressee', l.addressee, 'body', l.body,
            'signer', ab.name, 'pro', ab.professional_id,
            'date', to_char(l.approved_at AT TIME ZONE '{0}', 'DD/MM/YYYY'),
            'day', to_char(l.approved_at AT TIME ZONE '{0}', 'YYYY-MM-DD'),
            'hhmm', to_char(l.approved_at AT TIME ZONE '{0}', 'HH24\"h\"MI'))
         FROM discharge_letters l JOIN patients p ON p.id = l.patient_id LEFT JOIN staff ab ON ab.id = l.approved_by
         WHERE l.id = $1 AND l.approved_at IS NOT NULL AND p.anonymised_at IS NULL", tz()))
        .bind(id).fetch_optional(&s.pool).await?.ok_or(AppError::NotFound)?;
    let pid: Uuid = st(&v, "patient_id").parse().map_err(|_| AppError::NotFound)?;
    let en = st(&v, "lang") == "en";
    let t = |pt: &str, e: &str| if en { e.to_string() } else { pt.to_string() };
    let mut l = vec![Line::Title(t("Carta de alta de fisioterapia", "Physiotherapy discharge letter"))];
    l.extend(crate::export::clinic_header(&v["clinic_info"], &st(&v, "clinic")));
    l.push(Line::Text(st(&v, "date")));
    l.push(Line::Gap);
    if !st(&v, "addressee").is_empty() {
        l.extend(export::wrap(&st(&v, "addressee"), 10.0, 483.0).into_iter().map(Line::Text));
        l.push(Line::Gap);
    }
    let mut who = format!("{} ({} {})", st(&v, "name"), t("utente n.º", "patient no."), v["number"]);
    if !st(&v, "birth").is_empty() {
        who.push_str(&format!(", {} {}", t("nascido(a) a", "born"), st(&v, "birth")));
    }
    if !st(&v, "sns").is_empty() {
        who.push_str(&format!(", {} {}", t("N.º SNS", "NHS no."), st(&v, "sns")));
    }
    l.push(Line::Heading(t("Assunto: alta de fisioterapia", "Re: discharge from physiotherapy")));
    l.extend(export::wrap(&who, 10.0, 483.0).into_iter().map(Line::Text));
    l.push(Line::Gap);
    l.push(Line::Text(t("Exmo.(a) Colega,", "Dear colleague,")));
    l.push(Line::Gap);
    for para in st(&v, "body").lines() {
        if para.trim().is_empty() {
            l.push(Line::Gap);
        } else {
            l.extend(export::wrap(para, 10.0, 483.0).into_iter().map(Line::Text));
        }
    }
    l.push(Line::Gap);
    l.push(Line::Text(t("Com os melhores cumprimentos,", "Yours sincerely,")));
    l.push(Line::Gap);
    l.push(Line::Gap);
    l.push(Line::Text(st(&v, "signer")));
    let pro = st(&v, "pro");
    l.push(Line::Text(match (pro.is_empty(), en) {
        (true, false) => "Fisioterapeuta".into(),
        (true, true) => "Physiotherapist".into(),
        (false, false) => format!("Fisioterapeuta — cédula profissional n.º {pro}"),
        (false, true) => format!("Physiotherapist — professional licence no. {pro}"),
    }));
    if !st(&v, "clinic").is_empty() {
        l.push(Line::Text(st(&v, "clinic")));
    }
    Ok((pid, format!("{} Carta de alta {}.pdf", st(&v, "day"), st(&v, "hhmm")), export::pdf(&l)))
}

/// Save the approved letter as PDF in `Cartas/` and index it. Safe to call again (one PDF per letter).
async fn save_letter(s: &AppState, a: &Auth, letter: Uuid) -> ApiResult<Uuid> {
    if let Some((id,)) = sqlx::query_as::<_, (Uuid,)>("SELECT id FROM documents WHERE letter_id = $1").bind(letter).fetch_optional(&s.pool).await? {
        return Ok(id);
    }
    let (pid, mut name, bytes) = letter_pdf(s, letter).await?;
    if tokio::fs::try_exists(s.archive.join(format!("{}/Cartas/{name}", super::documents::patient_dir(s, pid).await?))).await.unwrap_or(false) {
        name = format!("{} ({}).pdf", name.trim_end_matches(".pdf"), &letter.simple().to_string()[..8]);
    }
    let rel = write(s, pid, "Cartas", &name, &bytes).await?;
    let id = Uuid::new_v4();
    let sha = hex::encode(Sha256::digest(&bytes));
    sqlx::query("INSERT INTO documents (id, patient_id, kind, filename, mime, bytes, path, sha256, uploaded_by, letter_id)
                 VALUES ($1,$2,'letter',$3,'application/pdf',$4,$5,$6,$7,$8)")
        .bind(id).bind(pid).bind(&name).bind(bytes.len() as i64).bind(&rel).bind(sha).bind(a.staff.id).bind(letter)
        .execute(&s.pool).await?;
    Ok(id)
}

#[derive(Deserialize)]
pub struct ApproveReq {
    #[serde(default)]
    discharge: bool,
    professional_id: Option<String>,
}

/// Owner only: approve a letter. It can't change afterwards; it's saved as PDF, and the patient can be
/// marked as discharged (inactive; future bookings are left alone).
pub async fn approve_letter(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(r): Json<ApproveReq>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let (pid, _, approved) = letter_patient(&s, id).await?;
    if approved {
        return Err(AppError::Conflict("This letter is already approved."));
    }
    let (body,): (String,) = sqlx::query_as("SELECT body FROM discharge_letters WHERE id = $1").bind(id).fetch_one(&s.pool).await?;
    if body.trim().is_empty() {
        return Err(AppError::BadRequest("Write the letter first."));
    }
    if body.contains(TODO_PT) || body.contains(TODO_EN) {
        return Err(AppError::BadRequest("Fill in the parts marked [a completar] before approving."));
    }
    if let Some(pro) = r.professional_id.as_deref() {
        if pro.len() > 40 {
            return Err(AppError::BadRequest("Some details are too long."));
        }
        sqlx::query("UPDATE staff SET professional_id = $2 WHERE id = $1").bind(a.staff.id).bind(pro.trim()).execute(&s.pool).await?;
    }
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query("UPDATE discharge_letters SET approved_by = $2, approved_at = now(), updated_at = now() WHERE id = $1 AND approved_at IS NULL")
        .bind(id).bind(a.staff.id).execute(&mut *tx).await?.rows_affected();
    if n == 0 {
        return Err(AppError::Conflict("This letter is already approved."));
    }
    if r.discharge {
        sqlx::query("UPDATE patients SET active = FALSE, updated_at = now() WHERE id = $1").bind(pid).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "letter.approved", subject(pid), &a.device, if r.discharge { "discharged" } else { "" }).await;
    let doc = match save_letter(&s, &a, id).await {
        Ok(d) => Some(d),
        Err(e) => { tracing::warn!("discharge letter PDF not saved: {e:?}"); None }
    };
    Ok(Json(json!({ "document_id": doc, "discharged": r.discharge })))
}

/// Owner/therapist: save the PDF of an approved letter again (after a failed first try).
pub async fn letter_pdf_again(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let (pid, _, approved) = letter_patient(&s, id).await?;
    if !approved {
        return Err(AppError::Conflict("Approve the letter first."));
    }
    let doc = save_letter(&s, &a, id).await?;
    auth::log(&s.pool, Some(a.staff.id), "letter.pdf_saved", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "document_id": doc })))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn template_marks_gaps_and_uses_facts() {
        let v = json!({
            "p": { "name": "Ana Lopes", "age": 42, "lang": "pt", "sex": "f" },
            "an": { "main_complaint": "Cervicalgia", "regions": [{ "region": "cervical", "symptoms": ["pain", "stiffness"] }] },
            "visits": [{ "on": "04/09/2026" }, { "on": "30/10/2026" }],
            "tests": [{ "code": "ndi", "score": 32, "max": 50, "on": "04/09/2026" }, { "code": "ndi", "score": 12, "max": 50, "on": "30/10/2026" }],
            "notes": [{ "blocks": [{ "type": "checklist", "value": ["Terapia manual", "Exercício"] }] }, { "blocks": [{ "type": "checklist", "value": ["exercício"] }] }],
            "rx": [],
        });
        let t = letter_template(&v, false);
        assert!(t.contains("Ana Lopes, de 42 anos, foi acompanhada em"));
        assert!(t.contains("entre 04/09/2026 e 30/10/2026, num total de 2 sessões"));
        assert!(t.contains("dor e rigidez — coluna cervical"));
        assert!(t.contains("exercício e terapia manual"));
        assert!(t.contains("• NDI: de 32/50 para 12/50 (04/09/2026 – 30/10/2026)"));
        assert!(t.contains(TODO_PT));
        let en = letter_template(&v, true);
        assert!(en.contains("pain and stiffness in the cervical spine") && en.contains(TODO_EN));
        let r = pack_report(&v, None);
        assert!(r.starts_with("Ana Lopes realizou 2 sessões") && !r.contains(TODO_PT));
        assert_eq!(date_pt("2026-11-08"), "08/11/2026");
        assert!(ai_facts(&v, false).contains("Sessões realizadas: 2"));
    }
}
