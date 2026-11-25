//! Patients and the clinical record.
//!
//! Rules (see claude/SECURITY.md):
//! - Every read of a patient file goes through [`open_file`], which writes `patient.viewed` to the access log.
//! - Reception gets a reduced file built on the server: contacts, consents, appointments and prescriptions
//!   (owner decision, 4 Nov 2026: reception books against ADSE/insurer prescriptions), no clinical fields.
//! - Clinical writes need the owner or therapist role.

use super::util::{changed, clinical, one_json, require_clinical, sets, update_row, K};
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{extract::{Path, Query, State}, Json};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use uuid::Uuid;

fn subject(id: Uuid) -> Option<(&'static str, String)> {
    Some(("patient", id.to_string()))
}

const PATIENT_FIELDS: &[(&str, K)] = &[
    ("name", K::Req("text")),
    ("birth_date", K::Opt("date")),
    ("sex", K::Opt("text")),
    ("nif", K::Opt("text")),
    ("sns_number", K::Opt("text")),
    ("phone", K::Opt("text")),
    ("email", K::Opt("text")),
    ("address", K::Opt("text")),
    ("guardian_name", K::Opt("text")),
    ("guardian_phone", K::Opt("text")),
    ("lang", K::Req("text")),
    ("reminders", K::Req("text")),
    ("therapist_id", K::Opt("uuid")),
    ("admin_note", K::Text),
    ("active", K::Req("boolean")),
];

// ───────── list, create, update ─────────

#[derive(Deserialize)]
pub struct ListQuery { q: Option<String>, include_inactive: Option<bool>, limit: Option<i64>, offset: Option<i64>, id: Option<Uuid> }

/// Everyone: the patient list (contacts only). Search by name, number, phone, email or NIF.
pub async fn list(State(s): State<AppState>, a: Auth, Query(q): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let term = q.q.unwrap_or_default().trim().to_lowercase();
    let row: (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(x ORDER BY lower(x.name)), '[]') FROM (
           SELECT p.id, p.number, p.name, p.birth_date, p.phone, p.email, p.active, p.therapist_id,
                  st.name AS therapist_name,
                  (SELECT min(ap.starts_at) FROM appointments ap WHERE ap.patient_id = p.id AND ap.status = 'booked' AND ap.starts_at > now()) AS next_at,
                  (SELECT max(ap.starts_at) FROM appointments ap WHERE ap.patient_id = p.id AND ap.status = 'done') AS last_at
           FROM patients p LEFT JOIN staff st ON st.id = p.therapist_id
           WHERE ($1 = '' OR lower(p.name) LIKE '%' || $1 || '%' OR p.number::text = $1
                  OR replace(COALESCE(p.phone,''), ' ', '') LIKE '%' || replace($1, ' ', '') || '%'
                  OR lower(COALESCE(p.email,'')) LIKE '%' || $1 || '%' OR COALESCE(p.nif,'') = replace($1, ' ', ''))
             AND (p.active OR $2 OR $5::uuid IS NOT NULL) AND p.deleted_at IS NULL AND ($5::uuid IS NULL OR p.id = $5)
           ORDER BY lower(p.name), p.number LIMIT $3 OFFSET $4) x")
        .bind(&term).bind(q.include_inactive.unwrap_or(false)).bind(q.limit.unwrap_or(500).clamp(1, 2000)).bind(q.offset.unwrap_or(0).clamp(0, 1_000_000)).bind(q.id)
        .fetch_one(&s.pool).await?;
    // The list holds names and contacts: record that it was read (never the search text).
    let n = row.0.as_array().map(|v| v.len()).unwrap_or(0);
    auth::log(&s.pool, Some(a.staff.id), "patients.listed", None, &a.device, &format!("{n} {}", if term.is_empty() { "all" } else { "search" })).await;
    Ok(Json(row.0))
}

/// A Portuguese NIF (9 digits, valid check digit; spaces ignored), or a foreign tax number that starts with its
/// two-letter country code (e.g. ES12345678Z), which can't be checked here. Answers the tidied number.
pub fn tidy_nif(raw: &str) -> Result<String, AppError> {
    let n: String = raw.chars().filter(|c| !c.is_whitespace() && *c != '.' && *c != '-').collect::<String>().to_uppercase();
    let n = n.strip_prefix("PT").filter(|r| r.len() == 9).map(str::to_string).unwrap_or(n);
    let foreign = n.len() > 2 && n[..2].chars().all(|c| c.is_ascii_alphabetic()) && n.len() <= 20 && n.chars().all(|c| c.is_ascii_alphanumeric());
    if n.is_empty() || foreign || super::clinic::nif_ok(&n) {
        Ok(n)
    } else {
        Err(AppError::BadRequest("That NIF isn’t valid. Check the 9 digits."))
    }
}

/// Tidies the NIF in a create/update body and refuses one that another patient already has (owner decision, 10 Nov 2026).
async fn check_nif(s: &AppState, body: &mut Map<String, Value>, me: Option<Uuid>) -> ApiResult<()> {
    let Some(raw) = body.get("nif").and_then(Value::as_str).map(str::to_string) else { return Ok(()) };
    let nif = tidy_nif(&raw)?;
    body.insert("nif".into(), Value::String(nif.clone()));
    if nif.is_empty() {
        return Ok(());
    }
    let other: Option<(Uuid, i32, String)> = sqlx::query_as(
        "SELECT id, number, name FROM patients WHERE nif = $1 AND deleted_at IS NULL AND anonymised_at IS NULL AND ($2::uuid IS NULL OR id <> $2) LIMIT 1")
        .bind(&nif).bind(me).fetch_optional(&s.pool).await?;
    if let Some((id, number, name)) = other {
        return Err(AppError::ConflictWith("duplicate_nif", "There’s already a patient with this NIF. Open that file instead.", json!({ "id": id, "number": number, "name": name })));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct DupQuery { nif: Option<String>, phone: Option<String>, name: Option<String>, birth_date: Option<String>, exclude: Option<Uuid> }

/// Everyone: patients that look like the one being registered (same NIF, same phone, or same name and birth date),
/// so the app can say "this may be a duplicate" before creating a second file (audit M11).
pub async fn duplicates(State(s): State<AppState>, a: Auth, Query(q): Query<DupQuery>) -> ApiResult<Json<Value>> {
    let nif = q.nif.as_deref().map(tidy_nif).transpose().unwrap_or(None).unwrap_or_default();
    let phone: String = q.phone.unwrap_or_default().chars().filter(|c| c.is_ascii_digit()).collect();
    let phone = if phone.len() >= 9 { phone[phone.len() - 9..].to_string() } else { String::new() };
    let name = q.name.unwrap_or_default().trim().to_lowercase();
    let birth = q.birth_date.unwrap_or_default();
    let birth = if birth.len() == 10 { birth } else { String::new() };
    if nif.is_empty() && phone.is_empty() && (name.is_empty() || birth.is_empty()) {
        return Ok(Json(json!([])));
    }
    let row: (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(x ORDER BY x.number), '[]') FROM (
           SELECT p.id, p.number, p.name, p.birth_date, p.phone,
                  CASE WHEN $1 <> '' AND p.nif = $1 THEN 'nif'
                       WHEN $2 <> '' AND right(regexp_replace(COALESCE(p.phone, ''), '[^0-9]', '', 'g'), 9) = $2 THEN 'phone'
                       ELSE 'name_birth' END AS reason
           FROM patients p
           WHERE p.deleted_at IS NULL AND p.anonymised_at IS NULL AND ($5::uuid IS NULL OR p.id <> $5)
             AND (($1 <> '' AND p.nif = $1)
               OR ($2 <> '' AND right(regexp_replace(COALESCE(p.phone, ''), '[^0-9]', '', 'g'), 9) = $2)
               OR ($3 <> '' AND $4 <> '' AND lower(trim(p.name)) = $3 AND p.birth_date = $4::date))
           ORDER BY p.number LIMIT 5) x")
        .bind(&nif).bind(&phone).bind(&name).bind(&birth).bind(q.exclude).fetch_one(&s.pool).await?;
    let n = row.0.as_array().map(|v| v.len()).unwrap_or(0);
    if n > 0 {
        auth::log(&s.pool, Some(a.staff.id), "patients.duplicates_checked", None, &a.device, &n.to_string()).await;
    }
    Ok(Json(row.0))
}

/// Everyone: register a patient.
pub async fn create(State(s): State<AppState>, a: Auth, Json(mut body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    let name = body.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("Enter the patient’s name."));
    }
    check_nif(&s, &mut body, None).await?;
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO patients (id, name, created_by) VALUES ($1, $2, $3)").bind(id).bind(&name).bind(a.staff.id).execute(&mut *tx).await?;
    update_row(&mut *tx, "patients", id, sets(&body, PATIENT_FIELDS, 2)?, "", "").await?;
    let (number,): (i32,) = sqlx::query_as("SELECT number FROM patients WHERE id = $1").bind(id).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "patient.created", subject(id), &a.device, "").await;
    Ok(Json(json!({ "id": id, "number": number })))
}

/// Everyone: change contact details (reception keeps contacts up to date).
pub async fn update(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(mut body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    if body.get("name").is_some_and(|v| v.as_str().map(str::trim).unwrap_or("").is_empty()) {
        return Err(AppError::BadRequest("Enter the patient’s name."));
    }
    check_nif(&s, &mut body, Some(id)).await?;
    let found = update_row(&s.pool, "patients", id, sets(&body, PATIENT_FIELDS, 2)?, "updated_at = now()", "anonymised_at IS NULL").await?;
    if !found {
        return Err(AppError::NotFound);
    }
    auth::log(&s.pool, Some(a.staff.id), "patient.updated", subject(id), &a.device, &changed(&body, PATIENT_FIELDS)).await;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::tidy_nif;
    #[test]
    fn nif_tidying() {
        assert_eq!(tidy_nif("123 456 789").unwrap(), "123456789");
        assert_eq!(tidy_nif("PT123456789").unwrap(), "123456789");
        assert_eq!(tidy_nif("es12345678z").unwrap(), "ES12345678Z");
        assert_eq!(tidy_nif("").unwrap(), "");
        assert!(tidy_nif("123456780").is_err());
        assert!(tidy_nif("12345").is_err());
    }
}

// ───────── the patient file (the one read path) ─────────

const FILE_BASE: &str = "
    SELECT json_build_object(
      'patient', (SELECT to_jsonb(p) - 'created_by' || jsonb_build_object('therapist_name', st.name)
                  FROM patients p LEFT JOIN staff st ON st.id = p.therapist_id WHERE p.id = $1 AND p.deleted_at IS NULL),
      'consents', (SELECT COALESCE(json_agg(json_build_object(
                     'id', c.id, 'kind', c.kind, 'granted', c.granted, 'signed_by_name', c.signed_by_name,
                     'signed_at', c.signed_at, 'expires_on', c.expires_on, 'revoked_at', c.revoked_at,
                     'version', t.version, 'title', t.title,
                     'pdf_id', (SELECT d.id FROM documents d WHERE d.consent_id = c.id AND d.consent_event = 'signed'),
                     'revoked_pdf_id', (SELECT d.id FROM documents d WHERE d.consent_id = c.id AND d.consent_event = 'revoked'),
                     'state', CASE WHEN c.revoked_at IS NOT NULL THEN 'revoked'
                                   WHEN c.expires_on IS NOT NULL AND c.expires_on < CURRENT_DATE THEN 'expired'
                                   WHEN NOT c.granted THEN 'refused' ELSE 'valid' END)
                   ORDER BY c.signed_at DESC), '[]')
                   FROM consents c JOIN consent_texts t ON t.id = c.text_id WHERE c.patient_id = $1),
      'appointments', (SELECT COALESCE(json_agg(x ORDER BY x.starts_at DESC), '[]') FROM (
                   SELECT ap.id, ap.kind, ap.starts_at, ap.ends_at, ap.status, ap.therapist_id, st.name AS therapist_name, ap.series_id
                   FROM appointments ap LEFT JOIN staff st ON st.id = ap.therapist_id
                   WHERE ap.patient_id = $1 OR ap.id IN (SELECT appointment_id FROM class_attendees WHERE patient_id = $1)
                   ORDER BY ap.starts_at DESC LIMIT 100) x),
      'prescriptions', (SELECT COALESCE(json_agg(x ORDER BY x.issued_on DESC), '[]') FROM (
                    SELECT pr.*, u.used, u.booked, pr.sessions_total - u.used AS remaining
                    FROM prescriptions pr,
                    LATERAL (SELECT count(*) FILTER (WHERE status = 'done') AS used,
                                    count(*) FILTER (WHERE status = 'booked') AS booked
                             FROM appointments WHERE prescription_id = pr.id) u
                    WHERE pr.patient_id = $1) x)";

const FILE_CLINICAL: &str = ",
      'anamnesis', (SELECT to_jsonb(an) || jsonb_build_object('updated_by_name', st.name)
                    FROM anamnesis an LEFT JOIN staff st ON st.id = an.updated_by WHERE an.patient_id = $1),
      'notes', (SELECT COALESCE(json_agg(json_build_object(
                    'id', n.id, 'appointment_id', n.appointment_id, 'template', n.template, 'blocks', n.blocks, 'text', n.text, 'ai_assisted', n.ai_assisted,
                    'created_at', n.created_at, 'updated_at', n.updated_at, 'created_by', n.created_by, 'created_by_name', cb.name,
                    'signed_at', n.signed_at, 'signed_by_name', sb.name) ORDER BY n.created_at DESC), '[]')
                FROM notes n LEFT JOIN staff cb ON cb.id = n.created_by LEFT JOIN staff sb ON sb.id = n.signed_by WHERE n.patient_id = $1),
      'tests', (SELECT COALESCE(json_agg(json_build_object(
                    'id', t.id, 'test_code', t.test_code, 'answers', t.answers, 'score', t.score, 'max_score', t.max_score,
                    'higher_is_worse', t.higher_is_worse, 'taken_on', t.taken_on, 'recorded_by_name', st.name)
                    ORDER BY t.test_code, t.taken_on, t.created_at), '[]')
                FROM test_results t LEFT JOIN staff st ON st.id = t.recorded_by WHERE t.patient_id = $1)";

/// Documents: everything for clinical roles; scanned prescriptions only for reception.
const FILE_DOCUMENTS: &str = ",
      'documents', (SELECT COALESCE(json_agg(json_build_object('id', d.id, 'kind', d.kind, 'filename', d.filename, 'mime', d.mime,
                    'bytes', d.bytes, 'uploaded_at', d.uploaded_at, 'uploaded_by', d.uploaded_by, 'uploaded_by_name', st.name,
                    'note_id', d.note_id, 'consent_id', d.consent_id, 'letter_id', d.letter_id) ORDER BY d.uploaded_at DESC), '[]')
                FROM documents d LEFT JOIN staff st ON st.id = d.uploaded_by WHERE d.patient_id = $1";

/// The single function that reads a patient file. It writes `patient.viewed` to the access log
/// and leaves out clinical fields for reception.
pub async fn open_file(s: &AppState, a: &Auth, id: Uuid) -> ApiResult<Value> {
    let full = clinical(a);
    let sql = format!("{FILE_BASE}{}{FILE_DOCUMENTS}{}), 'view', '{}')", if full { FILE_CLINICAL } else { "" },
                      if full { "" } else { " AND d.kind IN ('prescription', 'consent')" }, if full { "full" } else { "reduced" });
    let v = one_json(&s.pool, &sql, id).await?;
    if v.get("patient").is_none_or(Value::is_null) {
        return Err(AppError::NotFound);
    }
    auth::log(&s.pool, Some(a.staff.id), "patient.viewed", subject(id), &a.device, if full { "full" } else { "reduced" }).await;
    Ok(v)
}

pub async fn get_file(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(open_file(&s, &a, id).await?))
}

async fn exists(s: &AppState, id: Uuid) -> ApiResult<()> {
    let r: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM patients WHERE id = $1 AND anonymised_at IS NULL").bind(id).fetch_optional(&s.pool).await?;
    r.map(|_| ()).ok_or(AppError::NotFound)
}

/// Which patient a row belongs to (for role checks and the access log).
async fn owner_of(s: &AppState, table: &str, id: Uuid) -> ApiResult<Uuid> {
    let r: Option<(Uuid,)> = sqlx::query_as(&format!("SELECT patient_id FROM {table} WHERE id = $1")).bind(id).fetch_optional(&s.pool).await?;
    r.map(|x| x.0).ok_or(AppError::NotFound)
}

// ───────── anamnesis ─────────

const ANAMNESIS_FIELDS: &[(&str, K)] = &[
    ("main_complaint", K::Text), ("history", K::Text), ("medication", K::Text), ("medical_history", K::Text),
    ("goals", K::Text), ("regions", K::Json), ("red_flags", K::Json),
];

/// Owner/therapist: save the anamnesis (creates it the first time).
pub async fn put_anamnesis(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    exists(&s, pid).await?;
    for key in ["regions", "red_flags"] {
        if body.get(key).is_some_and(|v| !v.is_array()) {
            return Err(AppError::BadRequest("Some details aren’t in the right format. Check dates, times and numbers."));
        }
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO anamnesis (patient_id) VALUES ($1) ON CONFLICT DO NOTHING").bind(pid).execute(&mut *tx).await?;
    let (mut parts, mut binds) = sets(&body, ANAMNESIS_FIELDS, 3)?;
    parts.push("updated_by = $2".into());
    parts.push("updated_at = now()".into());
    let sql = format!("UPDATE anamnesis SET {} WHERE patient_id = $1", parts.join(", "));
    let mut q = sqlx::query(&sql).bind(pid).bind(a.staff.id);
    for b in binds.drain(..) {
        q = q.bind(b);
    }
    q.execute(&mut *tx).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "anamnesis.updated", subject(pid), &a.device, &changed(&body, ANAMNESIS_FIELDS)).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── progress notes ─────────

const NOTE_FIELDS: &[(&str, K)] = &[("appointment_id", K::Opt("uuid")), ("template", K::Text), ("blocks", K::Json), ("text", K::Text), ("ai_assisted", K::Req("boolean"))];

/// Owner/therapist: start a note (a draft until signed).
pub async fn create_note(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    exists(&s, pid).await?;
    if body.get("blocks").is_some_and(|v| !v.is_array()) {
        return Err(AppError::BadRequest("Some details aren’t in the right format. Check dates, times and numbers."));
    }
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO notes (id, patient_id, created_by) VALUES ($1, $2, $3)").bind(id).bind(pid).bind(a.staff.id).execute(&mut *tx).await?;
    update_row(&mut *tx, "notes", id, sets(&body, NOTE_FIELDS, 2)?, "", "").await?;
    tx.commit().await?;
    // A note written from a recording's AI draft points at it, so signing it deletes the recording.
    super::recordings::link_note(&s, &a, pid, id, body.get("recording_id")).await?;
    auth::log(&s.pool, Some(a.staff.id), "note.created", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "id": id })))
}

/// Owner, or the therapist who wrote it: change a draft. Signed notes can't change (database trigger).
pub async fn update_note(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let pid = owner_of(&s, "notes", id).await?;
    if body.get("blocks").is_some_and(|v| !v.is_array()) {
        return Err(AppError::BadRequest("Some details aren’t in the right format. Check dates, times and numbers."));
    }
    let cond = if a.staff.role == "owner" { "signed_at IS NULL".to_string() } else { format!("signed_at IS NULL AND created_by = '{}'", a.staff.id) };
    if !update_row(&s.pool, "notes", id, sets(&body, NOTE_FIELDS, 2)?, "updated_at = now()", &cond).await? {
        return Err(AppError::Conflict("This note is signed or belongs to someone else, so it can’t be changed. Add a new note instead."));
    }
    super::recordings::link_note(&s, &a, pid, id, body.get("recording_id")).await?;
    auth::log(&s.pool, Some(a.staff.id), "note.updated", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

/// Owner, or the therapist who wrote it: sign the note. After this it can't be changed.
pub async fn sign_note(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let pid = owner_of(&s, "notes", id).await?;
    let r: Option<(Value,)> = sqlx::query_as(
        "UPDATE notes SET signed_by = $2, signed_at = now(), updated_at = now(),
                -- Scales nobody set (value null) aren't part of the signed record (audit C3).
                blocks = COALESCE((SELECT jsonb_agg(b) FROM jsonb_array_elements(blocks) b
                                   WHERE NOT (b->>'type' = 'slider' AND jsonb_typeof(b->'value') IS DISTINCT FROM 'number')), '[]'::jsonb)
         WHERE id = $1 AND signed_at IS NULL AND (created_by = $2 OR $3) RETURNING to_json(signed_at)")
        .bind(id).bind(a.staff.id).bind(a.staff.role == "owner").fetch_optional(&s.pool).await?;
    let Some((signed_at,)) = r else {
        return Err(AppError::Conflict("This note is already signed or belongs to someone else."));
    };
    auth::log(&s.pool, Some(a.staff.id), "note.signed", subject(pid), &a.device, &id.to_string()).await;
    // Recordings this note was written from are deleted now (owner decision, 11 Nov 2026).
    super::recordings::note_signed(&s, &a, id).await;
    // The signed note is also saved as PDF in the patient's archive folder. If that fails (disk full,
    // folder not writable) the note stays signed; the app says so and offers to try again.
    let pdf = match super::documents::save_signed_note(&s, &a, id).await {
        Ok(doc) => Some(doc),
        Err(e) => { tracing::warn!("signed note PDF not saved: {e:?}"); None }
    };
    Ok(Json(json!({ "signed_at": signed_at, "document_id": pdf })))
}

/// Owner, or the author: save the PDF of a signed note again (after a failed first try). Returns the document.
pub async fn note_pdf(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let pid = owner_of(&s, "notes", id).await?;
    let doc = super::documents::save_signed_note(&s, &a, id).await?;
    auth::log(&s.pool, Some(a.staff.id), "note.pdf_saved", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "document_id": doc })))
}

/// Owner, or the author: throw away a draft.
pub async fn delete_note(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let pid = owner_of(&s, "notes", id).await?;
    let n = sqlx::query("DELETE FROM notes WHERE id = $1 AND signed_at IS NULL AND (created_by = $2 OR $3)")
        .bind(id).bind(a.staff.id).bind(a.staff.role == "owner").execute(&s.pool).await?.rows_affected();
    if n == 0 {
        return Err(AppError::Conflict("Signed notes are kept. Add a new note to correct it."));
    }
    auth::log(&s.pool, Some(a.staff.id), "note.draft_deleted", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── tests and questionnaires ─────────

#[derive(Deserialize)]
pub struct TestReq { test_code: String, answers: Option<Value>, #[serde(default)] score: Option<f64>, max_score: Option<f64>, higher_is_worse: Option<bool>, taken_on: Option<String> }

/// Owner/therapist: record a result. Says whether it's worse than the previous result of the same test.
pub async fn add_test(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Json(r): Json<TestReq>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    exists(&s, pid).await?;
    let code = r.test_code.trim().to_lowercase();
    if code.is_empty() || code.len() > 40 {
        return Err(AppError::BadRequest("Choose a test and enter its score."));
    }
    // A form answered question by question is scored here, never trusted from the app (audit phase C).
    let (score, max, hw, answers) = match crate::forms::score(&code, r.answers.as_ref())? {
        Some(f) => (f.score, Some(f.max), f.higher_is_worse, f.answers),
        None => {
            let sc = r.score.filter(|x| x.is_finite() && *x >= 0.0).ok_or(AppError::BadRequest("Choose a test and enter its score."))?;
            if r.max_score.is_some_and(|m| sc > m) { return Err(AppError::BadRequest("The score is higher than the test’s maximum.")); }
            (sc, r.max_score, r.higher_is_worse.unwrap_or(true), json!({}))
        }
    };
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO test_results (id, patient_id, test_code, answers, score, max_score, higher_is_worse, taken_on, recorded_by)
         VALUES ($1, $2, $3, $4, $5::float8::numeric, $6::float8::numeric, $7, COALESCE(NULLIF($8, '')::date, CURRENT_DATE), $9)")
        .bind(id).bind(pid).bind(&code).bind(answers).bind(score).bind(max).bind(hw)
        .bind(r.taken_on.unwrap_or_default()).bind(a.staff.id).execute(&s.pool).await?;
    let prev: Option<(f64,)> = sqlx::query_as(
        "SELECT score::float8 FROM test_results WHERE patient_id = $1 AND test_code = $2 AND id <> $3
         AND taken_on <= (SELECT taken_on FROM test_results WHERE id = $3) ORDER BY taken_on DESC, created_at DESC LIMIT 1")
        .bind(pid).bind(&code).bind(id).fetch_optional(&s.pool).await?;
    let worse = prev.map(|(p,)| if hw { score > p } else { score < p }).unwrap_or(false);
    auth::log(&s.pool, Some(a.staff.id), "test.recorded", subject(pid), &a.device, &code).await;
    Ok(Json(json!({ "id": id, "score": score, "previous": prev.map(|p| p.0), "worse": worse })))
}

pub async fn delete_test(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let pid = owner_of(&s, "test_results", id).await?;
    sqlx::query("DELETE FROM test_results WHERE id = $1").bind(id).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "test.deleted", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── prescriptions (credenciais) ─────────

const PRESCRIPTION_FIELDS: &[(&str, K)] = &[
    ("doctor", K::Text), ("specialty", K::Text), ("issued_on", K::Req("date")), ("sessions_total", K::Req("int")),
    ("valid_until", K::Opt("date")), ("payer", K::Text), ("document_id", K::Opt("uuid")),
];

/// A prescription can point at its scan: a document of the same patient, filed as a prescription.
async fn check_scan(s: &AppState, pid: Uuid, body: &Map<String, Value>) -> ApiResult<()> {
    let Some(doc) = body.get("document_id").and_then(Value::as_str).filter(|d| !d.is_empty()) else { return Ok(()) };
    let doc: Uuid = doc.parse().map_err(|_| AppError::BadRequest("Choose the scanned prescription again."))?;
    let r: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM documents WHERE id = $1 AND patient_id = $2 AND kind = 'prescription'")
        .bind(doc).bind(pid).fetch_optional(&s.pool).await?;
    r.map(|_| ()).ok_or(AppError::BadRequest("Choose the scanned prescription again."))
}

pub async fn create_prescription(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    exists(&s, pid).await?;
    if !body.contains_key("issued_on") || !body.contains_key("sessions_total") {
        return Err(AppError::BadRequest("Enter the prescription date and the number of sessions."));
    }
    check_scan(&s, pid, &body).await?;
    let id = Uuid::new_v4();
    let (parts, binds) = sets(&body, PRESCRIPTION_FIELDS, 4)?;
    // Insert with every column at once so the NOT NULL / CHECK rules apply to the real values.
    let cols: Vec<&str> = parts.iter().map(|p| p.split(" = ").next().unwrap_or("")).collect();
    let vals: Vec<&str> = parts.iter().map(|p| p.split_once(" = ").map(|x| x.1).unwrap_or("NULL")).collect();
    let sql = format!("INSERT INTO prescriptions (id, patient_id, created_by, {}) VALUES ($1, $2, $3, {})", cols.join(", "), vals.join(", "));
    let mut q = sqlx::query(&sql).bind(id).bind(pid).bind(a.staff.id);
    for b in binds {
        q = q.bind(b);
    }
    q.execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "prescription.created", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "id": id })))
}

pub async fn update_prescription(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    let pid = owner_of(&s, "prescriptions", id).await?;
    check_scan(&s, pid, &body).await?;
    update_row(&s.pool, "prescriptions", id, sets(&body, PRESCRIPTION_FIELDS, 2)?, "", "").await?;
    auth::log(&s.pool, Some(a.staff.id), "prescription.updated", subject(pid), &a.device, &changed(&body, PRESCRIPTION_FIELDS)).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── consents ─────────

/// Everyone: the consent texts (newest version of each kind and language first).
pub async fn consent_texts(State(s): State<AppState>, _a: Auth) -> ApiResult<Json<Value>> {
    let r: (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(t ORDER BY t.kind, t.lang, t.version DESC), '[]') FROM
         (SELECT id, kind, version, lang, title, body, valid_months, created_at, approved_at,
                 approved_at IS NULL AS draft,
                 approved_at IS NOT NULL AND version = max(version) FILTER (WHERE approved_at IS NOT NULL) OVER (PARTITION BY kind, lang) AS current
          FROM consent_texts) t")
        .fetch_one(&s.pool).await?;
    Ok(Json(r.0))
}

#[derive(Deserialize)]
pub struct ConsentTextReq { kind: String, lang: String, title: String, body: String, valid_months: Option<i32> }

/// Owner: a new version of a consent text. Old versions stay, because signed consents point to them.
/// The owner wrote it, so it's approved straight away (drafts only come from the server's first install).
pub async fn add_consent_text(State(s): State<AppState>, a: Auth, Json(r): Json<ConsentTextReq>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    if r.title.trim().is_empty() || r.body.trim().is_empty() {
        return Err(AppError::BadRequest("Enter a title and the text."));
    }
    let id = Uuid::new_v4();
    let (version,): (i32,) = sqlx::query_as(
        "INSERT INTO consent_texts (id, kind, version, lang, title, body, valid_months, created_by, approved_at, approved_by)
         VALUES ($1, $2, (SELECT COALESCE(max(version), 0) + 1 FROM consent_texts WHERE kind = $2 AND lang = $3), $3, $4, $5, $6, $7, now(), $7)
         RETURNING version")
        .bind(id).bind(&r.kind).bind(&r.lang).bind(r.title.trim()).bind(r.body.trim()).bind(r.valid_months).bind(a.staff.id)
        .fetch_one(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "consent_text.created", None, &a.device, &format!("{} v{version} {}", r.kind, r.lang)).await;
    Ok(Json(json!({ "id": id, "version": version })))
}

/// Owner: approve a draft text as it is, so it can be signed.
pub async fn approve_consent_text(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let r: Option<(String, i32, String)> = sqlx::query_as(
        "UPDATE consent_texts SET approved_at = now(), approved_by = $2 WHERE id = $1 AND approved_at IS NULL RETURNING kind, version, lang")
        .bind(id).bind(a.staff.id).fetch_optional(&s.pool).await?;
    let Some((kind, version, lang)) = r else {
        return Err(AppError::Conflict("This text is already approved."));
    };
    auth::log(&s.pool, Some(a.staff.id), "consent_text.approved", None, &a.device, &format!("{kind} v{version} {lang}")).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ConsentReq { text_id: Uuid, granted: bool, signed_by_name: String, signature_png: String }

/// Everyone (reception collects signatures at the desk): record a signed consent.
pub async fn sign_consent(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Json(r): Json<ConsentReq>) -> ApiResult<Json<Value>> {
    exists(&s, pid).await?;
    if r.signed_by_name.trim().is_empty() {
        return Err(AppError::BadRequest("Enter the name of the person signing."));
    }
    if !r.signature_png.starts_with("data:image/png;base64,") || r.signature_png.len() > 400_000 {
        return Err(AppError::BadRequest("The signature is missing. Ask the patient to sign again."));
    }
    let id = Uuid::new_v4();
    let row: Option<(String, Option<Value>)> = sqlx::query_as(
        "INSERT INTO consents (id, patient_id, text_id, kind, granted, signed_by_name, signature_png, expires_on, recorded_by)
         SELECT $1, $2, t.id, t.kind, $4, $5, $6,
                CASE WHEN t.valid_months IS NULL THEN NULL ELSE (CURRENT_DATE + make_interval(months => t.valid_months))::date END, $7
         FROM consent_texts t WHERE t.id = $3 AND t.approved_at IS NOT NULL
         RETURNING kind, to_json(expires_on)")
        .bind(id).bind(pid).bind(r.text_id).bind(r.granted).bind(r.signed_by_name.trim()).bind(&r.signature_png).bind(a.staff.id)
        .fetch_optional(&s.pool).await?;
    let Some((kind, expires_on)) = row else {
        return Err(AppError::BadRequest("This consent text isn’t approved yet. The owner approves it in Settings › Consents."));
    };
    auth::log(&s.pool, Some(a.staff.id), "consent.signed", subject(pid), &a.device, &format!("{kind} {}", if r.granted { "granted" } else { "refused" })).await;
    // Like signed notes: saved as PDF in the patient's folder. If that fails the consent is still recorded
    // and the app offers to try again.
    let pdf = match super::documents::save_consent(&s, &a, id, "signed").await {
        Ok(doc) => Some(doc),
        Err(e) => { tracing::warn!("consent PDF not saved: {e:?}"); None }
    };
    Ok(Json(json!({ "id": id, "expires_on": expires_on, "document_id": pdf })))
}

/// Everyone: one signed consent with its signature and text (logged as a read of the file).
pub async fn get_consent(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let pid = owner_of(&s, "consents", id).await?;
    let v = one_json(&s.pool,
        "SELECT to_jsonb(c) || jsonb_build_object('title', t.title, 'body', t.body, 'version', t.version, 'lang', t.lang)
         FROM consents c JOIN consent_texts t ON t.id = c.text_id WHERE c.id = $1", id).await?;
    auth::log(&s.pool, Some(a.staff.id), "patient.viewed", subject(pid), &a.device, "consent").await;
    Ok(Json(v))
}

/// Everyone: the patient withdraws a consent. The signed record stays.
pub async fn revoke_consent(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let pid = owner_of(&s, "consents", id).await?;
    let n = sqlx::query("UPDATE consents SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL").bind(id).execute(&s.pool).await?;
    if n.rows_affected() == 0 {
        return Err(AppError::Conflict("This consent was already withdrawn."));
    }
    auth::log(&s.pool, Some(a.staff.id), "consent.revoked", subject(pid), &a.device, &id.to_string()).await;
    // DGS 015/2013: a withdrawal is recorded in the clinical record → its own PDF next to the signed one.
    let pdf = match super::documents::save_consent(&s, &a, id, "revoked").await {
        Ok(doc) => Some(doc),
        Err(e) => { tracing::warn!("withdrawal PDF not saved: {e:?}"); None }
    };
    Ok(Json(json!({ "ok": true, "document_id": pdf })))
}

/// Everyone who can record consents: save the PDFs of a consent again (after a failed first try).
pub async fn consent_pdf(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let pid = owner_of(&s, "consents", id).await?;
    let (revoked,): (bool,) = sqlx::query_as("SELECT revoked_at IS NOT NULL FROM consents WHERE id = $1").bind(id).fetch_one(&s.pool).await?;
    let doc = super::documents::save_consent(&s, &a, id, "signed").await?;
    let revoked_doc = if revoked { Some(super::documents::save_consent(&s, &a, id, "revoked").await?) } else { None };
    auth::log(&s.pool, Some(a.staff.id), "consent.pdf_saved", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "document_id": doc, "revoked_document_id": revoked_doc })))
}
