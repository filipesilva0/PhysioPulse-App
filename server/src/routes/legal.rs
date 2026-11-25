//! Legal tools on the patient file (owner decision, 7 Nov 2026):
//!
//! - **Export the clinical record** (owner and therapists): a ZIP with a PDF of the record, summary or full.
//!   The full export also holds the original documents and the PDFs of signed notes. No reason is asked;
//!   every export is written to the access log (`patient.exported`).
//! - **Anonymise** (owner only): the clinical content stays for the clinic's records and statistics, but
//!   everything that names or reaches the person goes: name, contacts, NIF/SNS, guardian, signatures on
//!   consents, documents (they show faces, names and letters), the archive folder, future bookings.
//!   Birth date keeps the year only.
//! - **Delete** (owner only): everything about the person is removed, including signed notes and the
//!   archive folder. Payments are fiscal records and stay (without a name): a patient with payments is
//!   kept as a nameless placeholder ("Utente apagado", `deleted_at`); one without is removed outright.
//!
//! Anonymise and delete need the patient's current name typed again (`confirm_name`), checked here on the
//! server. Both run in one transaction with `physiopulse.legal_tool = 'on'`, the only way past the locks on
//! signed notes and their PDFs. The archive folder is removed after the transaction commits.

use super::money::pct_encode;
use super::schedule::tz;
use super::util::require_clinical;
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use crate::export::{self, Line, Zip};
use axum::{
    extract::{Path, State},
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub const ANONYMISED_NAME: &str = "Utente anonimizado";
pub const DELETED_NAME: &str = "Utente apagado";

fn subject(id: Uuid) -> Option<(&'static str, String)> {
    Some(("patient", id.to_string()))
}

/// The patient's folder in the archive (`Utentes/0007-…`), found by number, if there is one.
async fn folder_of(s: &AppState, number: i32) -> Option<std::path::PathBuf> {
    let prefix = format!("{number:04}-");
    let mut rd = tokio::fs::read_dir(s.archive.join("Utentes")).await.ok()?;
    while let Ok(Some(e)) = rd.next_entry().await {
        let is_dir = e.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && e.file_name().to_str().is_some_and(|n| n.starts_with(&prefix)) {
            return Some(e.path());
        }
    }
    None
}

/// Removes the patient's archive folder. Failure is reported, not hidden: the database change already happened.
async fn remove_folder(s: &AppState, number: i32) -> bool {
    match folder_of(s, number).await {
        Some(dir) => match tokio::fs::remove_dir_all(&dir).await {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("could not remove a patient archive folder: {}", e.kind());
                false
            }
        },
        None => true,
    }
}

/// Spaces and letter case don't matter when typing a name to confirm.
fn same_name(typed: &str, name: &str) -> bool {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    !typed.trim().is_empty() && norm(typed) == norm(name)
}

struct Who { number: i32, name: String, anonymised: bool }

async fn live_patient(s: &AppState, id: Uuid) -> ApiResult<Who> {
    let row: Option<(i32, String, bool)> = sqlx::query_as("SELECT number, name, anonymised_at IS NOT NULL FROM patients WHERE id = $1 AND deleted_at IS NULL")
        .bind(id).fetch_optional(&s.pool).await?;
    let (number, name, anonymised) = row.ok_or(AppError::NotFound)?;
    Ok(Who { number, name, anonymised })
}

// ───────── what will happen (shown in the confirmation) ─────────

/// Owner: what anonymising or deleting this patient would touch, so the app can say it plainly.
pub async fn check(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let w = live_patient(&s, id).await?;
    let (v,): (Value,) = sqlx::query_as(
        "SELECT json_build_object(
            'notes_signed', (SELECT count(*) FROM notes WHERE patient_id = $1 AND signed_at IS NOT NULL),
            'notes_draft', (SELECT count(*) FROM notes WHERE patient_id = $1 AND signed_at IS NULL),
            'documents', (SELECT count(*) FROM documents WHERE patient_id = $1),
            'tests', (SELECT count(*) FROM test_results WHERE patient_id = $1),
            'consents', (SELECT count(*) FROM consents WHERE patient_id = $1),
            'upcoming', (SELECT count(*) FROM appointments WHERE patient_id = $1 AND status = 'booked' AND starts_at > now()),
            'visits', (SELECT count(*) FROM appointments WHERE patient_id = $1 AND status = 'done'),
            'payments', (SELECT count(*) FROM payments WHERE patient_id = $1),
            'plans_active', (SELECT count(*) FROM patient_plans WHERE patient_id = $1 AND status IN ('active', 'paused')),
            -- Last contact: the clinical record is kept 20 years after it (LEGAL.md); the app warns before deleting.
            'last_contact', (SELECT max(d)::text FROM (
                SELECT (starts_at AT TIME ZONE $2)::date AS d FROM appointments WHERE patient_id = $1 AND status IN ('done', 'no_show')
                UNION ALL SELECT (ap.starts_at AT TIME ZONE $2)::date FROM class_attendees ca JOIN appointments ap ON ap.id = ca.appointment_id
                          WHERE ca.patient_id = $1 AND ca.attended
                UNION ALL SELECT (created_at AT TIME ZONE $2)::date FROM notes WHERE patient_id = $1
                UNION ALL SELECT taken_on FROM test_results WHERE patient_id = $1) x))")
        .bind(id).bind(tz()).fetch_one(&s.pool).await?;
    let owed = super::money::owed_for(&s, id).await?;
    let mut v = v;
    v["number"] = json!(w.number);
    v["name"] = json!(w.name);
    v["anonymised"] = json!(w.anonymised);
    v["owed_cents"] = json!(owed);
    Ok(Json(v))
}

// ───────── anonymise and delete ─────────

#[derive(Deserialize)]
pub struct Confirm { confirm_name: String }

/// Removes everything that identifies or reaches the person; keeps clinical content. Shared by both tools.
async fn scrub(tx: &mut Transaction<'_, Postgres>, id: Uuid, name: &str) -> ApiResult<()> {
    sqlx::query("SET LOCAL physiopulse.legal_tool = 'on'").execute(&mut **tx).await?;
    sqlx::query(
        "UPDATE patients SET name = $2, nif = NULL, sns_number = NULL, phone = NULL, email = NULL, address = NULL,
                guardian_name = NULL, guardian_phone = NULL, admin_note = '', reminders = 'none', active = FALSE,
                birth_date = date_trunc('year', birth_date)::date,
                anonymised_at = COALESCE(anonymised_at, now()), updated_at = now()
         WHERE id = $1")
        .bind(id).bind(name).execute(&mut **tx).await?;
    // Signatures and the signer's name identify the person; the consent itself (what, when, granted) stays.
    sqlx::query("UPDATE consents SET signed_by_name = '', signature_png = '' WHERE patient_id = $1").bind(id).execute(&mut **tx).await?;
    // Documents and signed-note PDFs carry names, faces and letters: removed with the folder.
    sqlx::query("DELETE FROM documents WHERE patient_id = $1").bind(id).execute(&mut **tx).await?;
    // Discharge letters are written to a named person (and their doctor): removed like the other letters.
    sqlx::query("DELETE FROM discharge_letters WHERE patient_id = $1").bind(id).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM class_attendees WHERE patient_id = $1 AND appointment_id IN (SELECT id FROM appointments WHERE status = 'booked' AND starts_at > now())")
        .bind(id).execute(&mut **tx).await?;
    sqlx::query("UPDATE appointments SET status = 'cancelled' WHERE patient_id = $1 AND status = 'booked' AND starts_at > now()").bind(id).execute(&mut **tx).await?;
    sqlx::query("UPDATE appointments SET address = '', admin_note = '' WHERE patient_id = $1").bind(id).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM waitlist WHERE patient_id = $1").bind(id).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM booking_requests WHERE patient_id = $1").bind(id).execute(&mut **tx).await?;
    sqlx::query("UPDATE patient_plans SET note = '' WHERE patient_id = $1").bind(id).execute(&mut **tx).await?;
    sqlx::query("UPDATE patient_plans SET shared_with = NULL WHERE shared_with = $1").bind(id).execute(&mut **tx).await?;
    Ok(())
}

/// Owner: anonymise a patient (can't be undone).
pub async fn anonymise(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(c): Json<Confirm>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let w = live_patient(&s, id).await?;
    if w.anonymised {
        return Err(AppError::Conflict("This patient is already anonymised."));
    }
    if !same_name(&c.confirm_name, &w.name) {
        return Err(AppError::BadRequest("The name doesn’t match. Type the patient’s name exactly as it appears in the file."));
    }
    let mut tx = s.pool.begin().await?;
    scrub(&mut tx, id, ANONYMISED_NAME).await?;
    let recs = super::recordings::mark_deleted(&mut tx, "patient_id::text = $1 AND $2 <> ''", "patient", &id.to_string()).await?;
    tx.commit().await?;
    super::recordings::remove_files(&s, &a, &recs, "patient").await;
    let folder = remove_folder(&s, w.number).await;
    auth::log(&s.pool, Some(a.staff.id), "patient.anonymised", subject(id), &a.device, if folder { "" } else { "archive folder not removed" }).await;
    Ok(Json(json!({ "ok": true, "number": w.number, "folder_removed": folder })))
}

/// Owner: delete a patient (can't be undone). Payments stay, without a name.
pub async fn delete(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(c): Json<Confirm>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let w = live_patient(&s, id).await?;
    if !same_name(&c.confirm_name, &w.name) {
        return Err(AppError::BadRequest("The name doesn’t match. Type the patient’s name exactly as it appears in the file."));
    }
    let (payments,): (i64,) = sqlx::query_as("SELECT count(*) FROM payments WHERE patient_id = $1").bind(id).fetch_one(&s.pool).await?;
    let mut tx = s.pool.begin().await?;
    // Session recordings first (their files go after the commit).
    let recs = super::recordings::mark_deleted(&mut tx, "patient_id::text = $1 AND $2 <> ''", "patient", &id.to_string()).await?;
    if payments > 0 {
        scrub(&mut tx, id, DELETED_NAME).await?;
        for sql in [
            "DELETE FROM notes WHERE patient_id = $1",
            "DELETE FROM test_results WHERE patient_id = $1",
            "DELETE FROM anamnesis WHERE patient_id = $1",
            "DELETE FROM consents WHERE patient_id = $1",
            "DELETE FROM class_attendees WHERE patient_id = $1",
            "DELETE FROM appointments WHERE patient_id = $1",
            "DELETE FROM prescriptions WHERE patient_id = $1",
            "UPDATE patients SET deleted_at = now(), birth_date = NULL, sex = NULL, therapist_id = NULL WHERE id = $1",
        ] {
            sqlx::query(sql).bind(id).execute(&mut *tx).await?;
        }
    } else {
        sqlx::query("SET LOCAL physiopulse.legal_tool = 'on'").execute(&mut *tx).await?;
        sqlx::query("DELETE FROM booking_requests WHERE patient_id = $1").bind(id).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM patients WHERE id = $1").bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    super::recordings::remove_files(&s, &a, &recs, "patient").await;
    let folder = remove_folder(&s, w.number).await;
    let detail = format!("{}{}", if payments > 0 { format!("{payments} payments kept without a name") } else { "removed".into() },
                         if folder { "" } else { "; archive folder not removed" });
    auth::log(&s.pool, Some(a.staff.id), "patient.deleted", subject(id), &a.device, &detail).await;
    Ok(Json(json!({ "ok": true, "number": w.number, "payments_kept": payments, "folder_removed": folder })))
}

// ───────── export ─────────

fn st(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string().replace('.', ","),
        _ => String::new(),
    }
}

fn region_pt(k: &str) -> &str {
    match k {
        "cervical" => "Coluna cervical", "thoracic" => "Coluna dorsal", "lumbar" => "Coluna lombar",
        "shoulder_r" => "Ombro direito", "shoulder_l" => "Ombro esquerdo", "elbow_r" => "Cotovelo direito",
        "elbow_l" => "Cotovelo esquerdo", "wrist_hand" => "Punho e mão", "hip" => "Anca",
        "knee_r" => "Joelho direito", "knee_l" => "Joelho esquerdo", "ankle_foot" => "Tornozelo e pé",
        o => o,
    }
}
fn symptom_pt(k: &str) -> &str {
    match k {
        "pain" => "dor", "stiffness" => "rigidez", "weakness" => "fraqueza", "tingling" => "formigueiro",
        "numbness" => "dormência", "swelling" => "edema", "instability" => "instabilidade", "cramps" => "cãibras",
        o => o,
    }
}
fn red_flag_pt(k: &str) -> &str {
    match k {
        "weight_loss" => "Perda de peso", "fever" => "Febre", "night_pain" => "Dor noturna",
        "neuro" => "Alterações neurológicas", "trauma" => "Trauma recente", o => o,
    }
}
fn consent_pt(k: &str) -> &str {
    match k {
        "treatment" => "Consentimento informado para tratamento", "data" => "Tratamento de dados pessoais (RGPD)",
        "media" => "Fotografias e vídeos", "marketing" => "Comunicações da clínica",
        "guardian" => "Representante legal (menores)", "invasive" => "Técnicas invasivas (punção seca)", o => o,
    }
}
fn test_pt(k: &str) -> String {
    match k {
        "nprs" => "Escala numérica da dor (0–10)".into(), "ndi" => "Neck Disability Index".into(), "odi" => "Oswestry Disability Index (%)".into(),
        "quickdash" => "QuickDASH".into(), "spadi" => "SPADI (%)".into(), "lefs" => "LEFS".into(), "koos" => "KOOS (0–100)".into(),
        "psfs" => "Escala funcional específica (PSFS)".into(), o => o.to_uppercase(),
    }
}
fn kind_pt(k: &str) -> &str {
    match k { "session" => "Sessão", "assessment" => "Avaliação", "class" => "Aula", "home" => "Domicílio", o => o }
}
fn status_pt(k: &str) -> &str {
    match k { "booked" => "marcada", "done" => "realizada", "cancelled" => "desmarcada", "no_show" => "faltou", o => o }
}
fn doc_kind_pt(k: &str) -> &str {
    match k { "photo" => "Fotografia", "report" => "Relatório", "imaging" => "Exame", "prescription" => "Prescrição",
              "letter" => "Carta", "note" => "Nota assinada", "consent" => "Consentimento", _ => "Outro" }
}

/// Paragraphs wrapped to the page, with an optional label on the first line.
fn para(lines: &mut Vec<Line>, text: &str) {
    for p in text.lines() {
        if p.trim().is_empty() {
            lines.push(Line::Gap);
        } else {
            lines.extend(export::wrap(p, 10.0, 483.0).into_iter().map(Line::Text));
        }
    }
}
fn labelled(lines: &mut Vec<Line>, label: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    if text.contains('\n') || export::wrap(text, 10.0, 330.0).len() > 1 {
        lines.push(Line::Text(format!("{label}:")));
        para(lines, text);
    } else {
        lines.push(Line::Row(label.into(), text.into()));
    }
}

const RECORD_SQL: &str = "
    SELECT json_build_object(
      'p', (SELECT json_build_object('number', p.number, 'name', p.name, 'birth', to_char(p.birth_date, 'DD/MM/YYYY'),
                'sex', p.sex, 'nif', p.nif, 'sns', p.sns_number, 'phone', p.phone, 'email', p.email, 'address', p.address,
                'guardian', p.guardian_name, 'guardian_phone', p.guardian_phone, 'therapist', st.name,
                'since', to_char(p.created_at AT TIME ZONE $2, 'DD/MM/YYYY'), 'anonymised', p.anonymised_at IS NOT NULL)
            FROM patients p LEFT JOIN staff st ON st.id = p.therapist_id WHERE p.id = $1 AND p.deleted_at IS NULL),
      'clinic', (SELECT name FROM clinic LIMIT 1), 'clinic_info', (SELECT json_build_object('name', c.name, 'nif', c.nif, 'ers', c.ers_number, 'address', c.address, 'phone', c.phone, 'email', c.email, 'logo', encode(c.logo, 'base64')) FROM clinic c WHERE c.id = 1),
      'now', to_char(now() AT TIME ZONE $2, 'DD/MM/YYYY HH24:MI'),
      'today', to_char(now() AT TIME ZONE $2, 'YYYY-MM-DD'),
      'an', (SELECT json_build_object('main_complaint', an.main_complaint, 'history', an.history, 'medication', an.medication,
                'medical_history', an.medical_history, 'goals', an.goals, 'regions', an.regions, 'red_flags', an.red_flags,
                'updated', to_char(an.updated_at AT TIME ZONE $2, 'DD/MM/YYYY'), 'by', st.name)
             FROM anamnesis an LEFT JOIN staff st ON st.id = an.updated_by WHERE an.patient_id = $1),
      'tests', (SELECT COALESCE(json_agg(json_build_object('code', t.test_code, 'score', t.score, 'max', t.max_score,
                'on', to_char(t.taken_on, 'DD/MM/YYYY'), 'by', st.name, 'sub', t.answers->'sub', 'acts', t.answers->'activities') ORDER BY t.test_code, t.taken_on, t.created_at), '[]')
             FROM test_results t LEFT JOIN staff st ON st.id = t.recorded_by WHERE t.patient_id = $1),
      'rx', (SELECT COALESCE(json_agg(json_build_object('doctor', pr.doctor, 'specialty', pr.specialty, 'payer', pr.payer,
                'issued', to_char(pr.issued_on, 'DD/MM/YYYY'), 'valid', to_char(pr.valid_until, 'DD/MM/YYYY'),
                'total', pr.sessions_total, 'used', (SELECT count(*) FROM appointments WHERE prescription_id = pr.id AND status = 'done'))
                ORDER BY pr.issued_on), '[]')
             FROM prescriptions pr WHERE pr.patient_id = $1),
      'consents', (SELECT COALESCE(json_agg(json_build_object('kind', c.kind, 'version', t.version, 'granted', c.granted,
                'signed', to_char(c.signed_at AT TIME ZONE $2, 'DD/MM/YYYY'), 'by', c.signed_by_name,
                'expires', to_char(c.expires_on, 'DD/MM/YYYY'), 'revoked', to_char(c.revoked_at AT TIME ZONE $2, 'DD/MM/YYYY'))
                ORDER BY c.signed_at), '[]')
             FROM consents c JOIN consent_texts t ON t.id = c.text_id WHERE c.patient_id = $1),
      'appts', (SELECT COALESCE(json_agg(json_build_object('at', to_char(ap.starts_at AT TIME ZONE $2, 'DD/MM/YYYY HH24:MI'),
                'kind', ap.kind, 'status', ap.status, 'therapist', st.name, 'title', ap.title) ORDER BY ap.starts_at), '[]')
             FROM appointments ap LEFT JOIN staff st ON st.id = ap.therapist_id
             WHERE ap.patient_id = $1 OR ap.id IN (SELECT appointment_id FROM class_attendees WHERE patient_id = $1)),
      'notes', (SELECT COALESCE(json_agg(json_build_object('template', n.template, 'blocks', n.blocks, 'text', n.text,
                'written', to_char(n.created_at AT TIME ZONE $2, 'DD/MM/YYYY HH24:MI'), 'author', cb.name,
                'signed', to_char(n.signed_at AT TIME ZONE $2, 'DD/MM/YYYY HH24:MI'), 'signer', sb.name) ORDER BY n.signed_at), '[]')
             FROM notes n LEFT JOIN staff cb ON cb.id = n.created_by LEFT JOIN staff sb ON sb.id = n.signed_by
             WHERE n.patient_id = $1 AND n.signed_at IS NOT NULL),
      'drafts', (SELECT count(*) FROM notes WHERE patient_id = $1 AND signed_at IS NULL),
      'docs', (SELECT COALESCE(json_agg(json_build_object('kind', d.kind, 'filename', d.filename, 'path', d.path,
                'on', to_char(d.uploaded_at AT TIME ZONE $2, 'DD/MM/YYYY')) ORDER BY d.uploaded_at), '[]')
             FROM documents d WHERE d.patient_id = $1))";

/// The clinical record as PDF lines (Portuguese, the clinic's language).
fn record_lines(v: &Value, full: bool) -> Vec<Line> {
    let p = &v["p"];
    let empty = vec![];
    let arr = |k: &str| v[k].as_array().unwrap_or(&empty);
    let mut l = vec![Line::Title(if full { "Processo clínico".into() } else { "Processo clínico — resumo".into() })];
    l.extend(crate::export::clinic_header(&v["clinic_info"], &st(v, "clinic")));
    l.push(Line::Text(format!("Exportado a {}", st(v, "now"))));

    l.push(Line::Heading("Identificação".into()));
    l.push(Line::Row("Utente".into(), format!("{} (n.º {})", st(p, "name"), p["number"])));
    labelled(&mut l, "Data de nascimento", &st(p, "birth"));
    labelled(&mut l, "Sexo", match p["sex"].as_str() { Some("f") => "Feminino", Some("m") => "Masculino", Some("x") => "Outro", _ => "" });
    labelled(&mut l, "NIF", &st(p, "nif"));
    labelled(&mut l, "N.º SNS", &st(p, "sns"));
    labelled(&mut l, "Telefone", &st(p, "phone"));
    labelled(&mut l, "Email", &st(p, "email"));
    labelled(&mut l, "Morada", &st(p, "address"));
    if !st(p, "guardian").is_empty() {
        labelled(&mut l, "Encarregado de educação", &format!("{} {}", st(p, "guardian"), st(p, "guardian_phone")).trim().to_string());
    }
    labelled(&mut l, "Fisioterapeuta", &st(p, "therapist"));
    labelled(&mut l, "Utente desde", &st(p, "since"));
    if p["anonymised"].as_bool() == Some(true) {
        l.push(Line::Text("Este processo foi anonimizado: os dados que identificavam a pessoa foram removidos.".into()));
    }

    let an = &v["an"];
    if an.is_object() {
        l.push(Line::Heading("Anamnese".into()));
        labelled(&mut l, "Queixa principal", &st(an, "main_complaint"));
        labelled(&mut l, "História atual", &st(an, "history"));
        labelled(&mut l, "Antecedentes", &st(an, "medical_history"));
        labelled(&mut l, "Medicação", &st(an, "medication"));
        labelled(&mut l, "Objetivos", &st(an, "goals"));
        for r in an["regions"].as_array().unwrap_or(&empty) {
            let syms: Vec<&str> = r["symptoms"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(symptom_pt).collect()).unwrap_or_default();
            let mut text = syms.join(", ");
            if !st(r, "notes").is_empty() {
                text = if text.is_empty() { st(r, "notes") } else { format!("{text}. {}", st(r, "notes")) };
            }
            labelled(&mut l, region_pt(&st(r, "region")), if text.is_empty() { "—" } else { &text });
        }
        let flags: Vec<&str> = an["red_flags"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(red_flag_pt).collect()).unwrap_or_default();
        if !flags.is_empty() {
            labelled(&mut l, "Sinais de alerta", &flags.join(", "));
        }
        if !st(an, "updated").is_empty() {
            l.push(Line::Text(format!("Atualizada a {}{}", st(an, "updated"), if st(an, "by").is_empty() { String::new() } else { format!(" por {}", st(an, "by")) })));
        }
    }

    if !arr("tests").is_empty() {
        l.push(Line::Heading("Testes e questionários".into()));
        for t in arr("tests") {
            let max = if t["max"].is_null() { String::new() } else { format!(" / {}", st(t, "max")) };
            l.push(Line::Row(format!("{} — {}", st(t, "on"), test_pt(&st(t, "code"))), format!("{}{max}", st(t, "score"))));
            // Forms answered question by question: subscales (SPADI, KOOS) and the PSFS activities.
            if let Some(sub) = t["sub"].as_object().filter(|m| !m.is_empty()) {
                let name = |k: &str| match k { "pain" => "dor", "disability" => "incapacidade", "symptoms" => "sintomas", "adl" => "atividades diárias", "sport" => "desporto e lazer", "qol" => "qualidade de vida", o => o }.to_string();
                l.push(Line::Text(format!("    {}", sub.iter().map(|(k, v)| format!("{} {}", name(k), v)).collect::<Vec<_>>().join(" · "))));
            }
            if let Some(a) = t["acts"].as_array().filter(|a| !a.is_empty()) {
                l.push(Line::Text(format!("    Atividades: {}", a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; "))));
            }
        }
    }

    if !arr("rx").is_empty() {
        l.push(Line::Heading("Prescrições (credenciais)".into()));
        for r in arr("rx") {
            let who = [st(r, "doctor"), st(r, "specialty")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
            l.push(Line::Row(format!("{} — {}", st(r, "issued"), if who.is_empty() { "Prescrição".to_string() } else { who }),
                             format!("{} de {} sessões", st(r, "used"), st(r, "total"))));
            let extra = [st(r, "payer"), if st(r, "valid").is_empty() { String::new() } else { format!("válida até {}", st(r, "valid")) }]
                .into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
            if !extra.is_empty() {
                l.push(Line::Text(format!("   {extra}")));
            }
        }
    }

    if !arr("consents").is_empty() {
        l.push(Line::Heading("Consentimentos".into()));
        for c in arr("consents") {
            let state = if !st(c, "revoked").is_empty() { format!("retirado a {}", st(c, "revoked")) }
                        else if c["granted"].as_bool() == Some(false) { "recusado".into() }
                        else if !st(c, "expires").is_empty() { format!("aceite, válido até {}", st(c, "expires")) }
                        else { "aceite".into() };
            l.push(Line::Row(format!("{} — {} (v{})", st(c, "signed"), consent_pt(&st(c, "kind")), st(c, "version")), state));
            if full && !st(c, "by").is_empty() {
                l.push(Line::Text(format!("   Assinado por {}", st(c, "by"))));
            }
        }
    }

    let appts = arr("appts");
    if !appts.is_empty() {
        l.push(Line::Heading("Marcações".into()));
        let count = |s: &str| appts.iter().filter(|a| a["status"] == s).count();
        l.push(Line::Text(format!("{} realizadas · {} marcadas · {} desmarcadas · {} faltas",
                                  count("done"), count("booked"), count("cancelled"), count("no_show"))));
        let done: Vec<&Value> = appts.iter().filter(|a| a["status"] == "done").collect();
        if let (Some(f), Some(last)) = (done.first(), done.last()) {
            l.push(Line::Text(format!("Primeira sessão a {} · última a {}", &st(f, "at")[..10.min(st(f, "at").len())], &st(last, "at")[..10.min(st(last, "at").len())])));
        }
        if full {
            l.push(Line::Gap);
            for a in appts {
                let what = if st(a, "title").is_empty() { kind_pt(&st(a, "kind")).to_string() } else { format!("{} — {}", kind_pt(&st(a, "kind")), st(a, "title")) };
                let who = if st(a, "therapist").is_empty() { String::new() } else { format!(" · {}", st(a, "therapist")) };
                l.push(Line::Row(format!("{} — {what}{who}", st(a, "at")), status_pt(&st(a, "status")).into()));
            }
        }
    }

    let notes = arr("notes");
    if !notes.is_empty() {
        l.push(Line::Heading(format!("Notas de evolução assinadas ({})", notes.len())));
        for n in notes {
            if full {
                l.push(Line::Gap);
                l.push(Line::Text(format!("{} — assinada por {}", st(n, "signed"), st(n, "signer"))));
                for b in n["blocks"].as_array().unwrap_or(&empty) {
                    let label = st(b, "label");
                    let value = match b["type"].as_str() {
                        Some("checklist") => b["value"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
                        Some("slider") if b["value"].is_number() => format!("{}{}", st(b, "value"), st(b, "unit")),
                        Some("slider") => String::new(),
                        _ => st(b, "value"),
                    };
                    if !value.trim().is_empty() {
                        for (i, x) in export::wrap(&format!("{label}: {value}"), 10.0, 483.0).into_iter().enumerate() {
                            l.push(Line::Text(if i == 0 { x } else { format!("   {x}") }));
                        }
                    }
                }
                para(&mut l, &st(n, "text"));
            } else {
                l.push(Line::Text(format!("{} — {}", st(n, "signed"), st(n, "signer"))));
            }
        }
    }
    let drafts = v["drafts"].as_i64().unwrap_or(0);
    if drafts > 0 {
        l.push(Line::Text(format!("{drafts} rascunho(s) por assinar não fazem parte do processo e não foram exportados.")));
    }

    let docs = arr("docs");
    if !docs.is_empty() {
        l.push(Line::Heading("Documentos".into()));
        for d in docs {
            l.push(Line::Row(format!("{} — {}", st(d, "on"), st(d, "filename")), doc_kind_pt(&st(d, "kind")).into()));
        }
        if !full {
            l.push(Line::Text("Os ficheiros originais vêm na exportação completa.".into()));
        }
    }
    l
}

/// Owner and therapists: the clinical record as a ZIP (`kind` = `summary` or `full`). Saved straight to
/// the computer's Downloads by the desktop app; logged as `patient.exported`.
pub async fn export(State(s): State<AppState>, a: Auth, Path((id, kind)): Path<(Uuid, String)>) -> ApiResult<Response> {
    require_clinical(&a)?;
    let full = match kind.as_str() {
        "full" => true,
        "summary" => false,
        _ => return Err(AppError::BadRequest("Choose a full or summary export.")),
    };
    let (v,): (Value,) = sqlx::query_as(RECORD_SQL).bind(id).bind(tz()).fetch_one(&s.pool).await?;
    if v["p"].is_null() {
        return Err(AppError::NotFound);
    }
    let today = st(&v, "today");
    let (y, mo, d) = (today.get(..4).and_then(|x| x.parse().ok()).unwrap_or(2026), today.get(5..7).and_then(|x| x.parse().ok()).unwrap_or(1), today.get(8..10).and_then(|x| x.parse().ok()).unwrap_or(1));
    let number = v["p"]["number"].as_i64().unwrap_or(0);
    let root = format!("Processo {number:04} {today}");
    let mut zip = Zip::new(y, mo, d);
    zip.add(&format!("{root}/Processo clínico{}.pdf", if full { "" } else { " (resumo)" }), &export::pdf(&record_lines(&v, full)));

    let mut missing = Vec::new();
    if full {
        let empty = vec![];
        for doc in v["docs"].as_array().unwrap_or(&empty) {
            let path = st(doc, "path");
            let folder = match doc["kind"].as_str() { Some("note") => "Notas", Some("consent") => "Consentimentos", _ => "Documentos" };
            match (!path.contains("..")).then(|| s.archive.join(&path)) {
                Some(p) => match tokio::fs::read(&p).await {
                    Ok(bytes) => {
                        zip.add(&format!("{root}/{folder}/{}", export::safe_name(&st(doc, "filename"))), &bytes);
                    }
                    Err(_) => missing.push(st(doc, "filename")),
                },
                None => missing.push(st(doc, "filename")),
            }
        }
    }
    let mut readme = format!(
        "Processo clínico do utente n.º {number:04}, exportado a {} ({}).\r\n\r\n\
         Processo clínico{}.pdf — {}.\r\n",
        st(&v, "now"), if full { "completo" } else { "resumo" }, if full { "" } else { " (resumo)" },
        if full { "identificação, anamnese, testes, prescrições, consentimentos, todas as marcações e o texto de cada nota assinada" }
        else { "identificação, anamnese, testes, prescrições, consentimentos, totais de marcações e a lista de notas assinadas" });
    if full {
        readme.push_str("Documentos/ — os ficheiros originais (relatórios, exames, fotografias, prescrições, cartas).\r\nNotas/ — o PDF de cada nota de evolução assinada.\r\nConsentimentos/ — o PDF de cada consentimento assinado, com a assinatura, e de cada retirada.\r\n");
    }
    if !missing.is_empty() {
        readme.push_str(&format!("\r\nNão foi possível incluir {} ficheiro(s) que faltam no arquivo da clínica:\r\n", missing.len()));
        for m in &missing {
            readme.push_str(&format!("- {m}\r\n"));
        }
    }
    readme.push_str("\r\nContém dados de saúde. Guarde e envie este ficheiro com cuidado e apague-o quando já não for preciso.\r\n");
    zip.add(&format!("{root}/LEIA-ME.txt"), readme.as_bytes());

    auth::log(&s.pool, Some(a.staff.id), "patient.exported", subject(id), &a.device,
              &format!("{}{}", if full { "full" } else { "summary" }, if missing.is_empty() { String::new() } else { format!("; {} files missing", missing.len()) })).await;
    let filename = format!("Processo clínico {number:04} {}{today}.zip", if full { "" } else { "resumo " });
    let mut res = zip.finish().into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    if let Ok(val) = HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{}", pct_encode(&filename))) {
        h.insert(header::CONTENT_DISPOSITION, val);
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::same_name;
    #[test]
    fn confirm_name() {
        assert!(same_name("  maria   SILVA ", "Maria Silva"));
        assert!(!same_name("Maria", "Maria Silva"));
        assert!(!same_name("", ""));
        assert!(same_name("José Conceição", "josé conceição"));
    }
}
