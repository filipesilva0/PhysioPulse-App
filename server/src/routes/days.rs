//! "Delete everything from one day" (owner only; File menu). DECISIONS.md 3 Nov: records can be deleted
//! per person or per day. Tax documents are always kept as the law requires.
//!
//! Deleted: that day's appointments (with class lists and reminders), session recordings, progress notes written that day
//! (signed ones and their PDFs too) and test results, and documents added that day.
//! Kept: payments, invoices, expenses and salaries (fiscal records, 10 years), consents and their PDFs,
//! discharge letters, patients themselves, complaints and the access log. A payment for a deleted visit
//! stays in Finance with its description; it just no longer points to the appointment.
//!
//! The owner confirms by sending the same date back (`confirm`). One transaction with
//! `physiopulse.legal_tool = 'on'` (signed notes are locked otherwise); files are removed after it commits.

use super::schedule::tz;
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use crate::workdays;
use axum::{extract::{Path, State}, Json};
use serde::Deserialize;
use serde_json::{json, Value};

const BAD_DAY: &str = "Choose a day.";

fn day_of(s: &str) -> ApiResult<String> {
    workdays::parse(s).map(workdays::fmt).ok_or(AppError::BadRequest(BAD_DAY))
}

// $1 = day, $2 = time zone
const APPTS: &str = "SELECT id FROM appointments WHERE (starts_at AT TIME ZONE $2)::date = $1::date";
const NOTES: &str = "SELECT id FROM notes WHERE (created_at AT TIME ZONE $2)::date = $1::date";
/// Documents added that day, except the PDFs the server makes for consents and letters (kept with them).
/// Note PDFs go with their note.
const DOCS: &str = "SELECT id FROM documents WHERE (uploaded_at AT TIME ZONE $2)::date = $1::date
                    AND kind NOT IN ('note', 'consent') AND letter_id IS NULL";

/// Owner: what deleting this day would remove and keep (for the confirmation).
pub async fn check(State(s): State<AppState>, a: Auth, Path(day): Path<String>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let day = day_of(&day)?;
    let (v,): (Value,) = sqlx::query_as(&format!(
        "SELECT json_build_object(
            'day', $1::text,
            'appointments', (SELECT count(*) FROM appointments WHERE id IN ({APPTS}) AND kind <> 'block'),
            'blocks', (SELECT count(*) FROM appointments WHERE id IN ({APPTS}) AND kind = 'block'),
            'attendees', (SELECT count(*) FROM class_attendees WHERE appointment_id IN ({APPTS})),
            'notes_signed', (SELECT count(*) FROM notes WHERE id IN ({NOTES}) AND signed_at IS NOT NULL),
            'notes_draft', (SELECT count(*) FROM notes WHERE id IN ({NOTES}) AND signed_at IS NULL),
            'tests', (SELECT count(*) FROM test_results WHERE taken_on = $1::date),
            'documents', (SELECT count(*) FROM documents WHERE id IN ({DOCS})),
            'payments_kept', (SELECT count(*) FROM payments WHERE received_on = $1::date OR appointment_id IN ({APPTS})),
            'patients', (SELECT count(DISTINCT pid) FROM (
                SELECT patient_id AS pid FROM appointments WHERE id IN ({APPTS}) AND patient_id IS NOT NULL
                UNION SELECT patient_id FROM class_attendees WHERE appointment_id IN ({APPTS})
                UNION SELECT patient_id FROM notes WHERE id IN ({NOTES})
                UNION SELECT patient_id FROM test_results WHERE taken_on = $1::date
                UNION SELECT patient_id FROM documents WHERE id IN ({DOCS})) x))"))
        .bind(&day).bind(tz()).fetch_one(&s.pool).await?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct Confirm { confirm: String }

/// Owner: delete everything from one day (can't be undone).
pub async fn delete(State(s): State<AppState>, a: Auth, Path(day): Path<String>, Json(c): Json<Confirm>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let day = day_of(&day)?;
    if day_of(&c.confirm).ok().as_deref() != Some(day.as_str()) {
        return Err(AppError::BadRequest("The day doesn’t match. Choose the day again."));
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("SET LOCAL physiopulse.legal_tool = 'on'").execute(&mut *tx).await?;
    // Files to remove once the database change is committed: the documents of the day and the PDFs of its notes.
    let paths: Vec<(String,)> = sqlx::query_as(&format!(
        "SELECT path FROM documents WHERE id IN ({DOCS}) OR note_id IN ({NOTES})"))
        .bind(&day).bind(tz()).fetch_all(&mut *tx).await?;
    let docs = sqlx::query(&format!("DELETE FROM documents WHERE id IN ({DOCS})")).bind(&day).bind(tz()).execute(&mut *tx).await?.rows_affected();
    let notes = sqlx::query(&format!("DELETE FROM notes WHERE id IN ({NOTES})")).bind(&day).bind(tz()).execute(&mut *tx).await?.rows_affected();
    let tests = sqlx::query("DELETE FROM test_results WHERE taken_on = $1::date").bind(&day).execute(&mut *tx).await?.rows_affected();
    let appts = sqlx::query(&format!("DELETE FROM appointments WHERE id IN ({APPTS})")).bind(&day).bind(tz()).execute(&mut *tx).await?.rows_affected();
    // Session recordings made that day (audio, transcript and draft).
    let recs = super::recordings::mark_deleted(&mut tx, "(created_at AT TIME ZONE $2)::date = $1::date", "day", &day).await?;
    tx.commit().await?;
    super::recordings::remove_files(&s, &a, &recs, "day").await;
    let mut left = 0;
    for (p,) in &paths {
        let full = s.archive.join(p);
        if full.starts_with(&s.archive) && tokio::fs::remove_file(&full).await.is_err() && tokio::fs::metadata(&full).await.is_ok() {
            left += 1;
        }
    }
    let detail = format!("{appts} appointments, {notes} notes, {tests} tests, {docs} documents{}", if left > 0 { format!("; {left} files not removed") } else { String::new() });
    auth::log(&s.pool, Some(a.staff.id), "day.deleted", Some(("day", day.clone())), &a.device, &detail).await;
    Ok(Json(json!({ "ok": true, "day": day, "appointments": appts, "notes": notes, "tests": tests, "documents": docs, "files_not_removed": left })))
}
