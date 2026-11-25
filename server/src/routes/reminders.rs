//! Appointment reminders, sent by hand (owner decision 7 Nov 2026: no SMS provider for now).
//!
//! The app lists a day's appointments with each patient's phone and preferred channel, fills in the
//! clinic's message and opens Messages (SMS), WhatsApp or email on the computer; the person presses Send.
//! The server only records that a reminder went out (who, when, which channel) — never the text.
//! Everyone books and reminds, so every role may use this. Reading the list is logged (`reminders.listed`).

use super::schedule::tz;
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{extract::{Query, State}, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct DayQuery { day: Option<String> }

fn valid_day(d: &str) -> bool {
    d.len() == 10 && d.chars().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() })
}

/// Everyone: the people to remind on one day (default: tomorrow, clinic time). Sessions, assessments and home
/// visits, plus each person booked into a class. Booked appointments only.
pub async fn list(State(s): State<AppState>, a: Auth, Query(q): Query<DayQuery>) -> ApiResult<Json<Value>> {
    let day = match q.day {
        Some(d) if valid_day(&d) => d,
        Some(_) => return Err(AppError::BadRequest("Choose a day.")),
        None => sqlx::query_as::<_, (String,)>(&format!("SELECT to_char((now() AT TIME ZONE '{}')::date + 1, 'YYYY-MM-DD')", tz()))
            .fetch_one(&s.pool).await?.0,
    };
    let (v,): (Value,) = sqlx::query_as(&format!(
        "WITH people AS (
            SELECT ap.id AS appointment_id, ap.patient_id, ap.kind, ap.starts_at, ap.therapist_id, ap.address, ''::text AS title
            FROM appointments ap
            WHERE ap.kind IN ('session', 'assessment', 'home') AND ap.status = 'booked'
              AND (ap.starts_at AT TIME ZONE '{0}')::date = $1::date
            UNION ALL
            SELECT ap.id, ca.patient_id, ap.kind, ap.starts_at, ap.therapist_id, '', ap.title
            FROM appointments ap JOIN class_attendees ca ON ca.appointment_id = ap.id
            WHERE ap.kind = 'class' AND ap.status = 'booked' AND (ap.starts_at AT TIME ZONE '{0}')::date = $1::date)
         SELECT json_build_object(
           'day', $1, 'clinic', c.name, 'template_pt', c.reminder_pt, 'template_en', c.reminder_en,
           'items', COALESCE((SELECT json_agg(json_build_object(
               'appointment_id', x.appointment_id, 'patient_id', p.id, 'name', p.name, 'number', p.number,
               'phone', COALESCE(NULLIF(p.phone, ''), NULLIF(p.guardian_phone, '')), 'email', NULLIF(p.email, ''),
               'lang', p.lang, 'prefers', p.reminders, 'kind', x.kind, 'title', x.title, 'starts_at', x.starts_at,
               'therapist_name', st.name, 'home', x.kind = 'home',
               'sent_at', r.sent_at, 'sent_channel', r.channel, 'sent_by_name', sb.name)
               ORDER BY x.starts_at, p.name)
             FROM people x JOIN patients p ON p.id = x.patient_id AND p.deleted_at IS NULL AND p.anonymised_at IS NULL
             LEFT JOIN staff st ON st.id = x.therapist_id
             LEFT JOIN reminders_sent r ON r.appointment_id = x.appointment_id AND r.patient_id = x.patient_id
             LEFT JOIN staff sb ON sb.id = r.sent_by), '[]'))
         FROM clinic c", tz()))
        .bind(&day).fetch_one(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "reminders.listed", None, &a.device, &day).await;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct Sent { appointment_id: Uuid, patient_id: Uuid, channel: Option<String>, sent: bool }

/// Everyone: record that a reminder went out (or undo it).
pub async fn mark(State(s): State<AppState>, a: Auth, Json(r): Json<Sent>) -> ApiResult<Json<Value>> {
    // The patient must really be booked on that appointment (directly or in the class).
    let (booked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM appointments WHERE id = $1 AND patient_id = $2)
             OR EXISTS (SELECT 1 FROM class_attendees WHERE appointment_id = $1 AND patient_id = $2)")
        .bind(r.appointment_id).bind(r.patient_id).fetch_one(&s.pool).await?;
    if !booked {
        return Err(AppError::NotFound);
    }
    let subject = Some(("patient", r.patient_id.to_string()));
    if r.sent {
        let channel = r.channel.as_deref().unwrap_or("other");
        if !["sms", "whatsapp", "email", "call", "other"].contains(&channel) {
            return Err(AppError::BadRequest("Choose how the reminder was sent."));
        }
        sqlx::query("INSERT INTO reminders_sent (appointment_id, patient_id, channel, sent_by) VALUES ($1, $2, $3, $4)
                     ON CONFLICT (appointment_id, patient_id) DO UPDATE SET channel = $3, sent_by = $4, sent_at = now()")
            .bind(r.appointment_id).bind(r.patient_id).bind(channel).bind(a.staff.id).execute(&s.pool).await?;
        auth::log(&s.pool, Some(a.staff.id), "appointment.reminded", subject, &a.device, channel).await;
    } else {
        sqlx::query("DELETE FROM reminders_sent WHERE appointment_id = $1 AND patient_id = $2")
            .bind(r.appointment_id).bind(r.patient_id).execute(&s.pool).await?;
        auth::log(&s.pool, Some(a.staff.id), "appointment.reminder_undone", subject, &a.device, "").await;
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct Template { pt: String, en: String }

/// Owner: the reminder message in each language.
pub async fn set_template(State(s): State<AppState>, a: Auth, Json(t): Json<Template>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let (pt, en) = (t.pt.trim(), t.en.trim());
    if pt.is_empty() || en.is_empty() || pt.chars().count() > 600 || en.chars().count() > 600 {
        return Err(AppError::BadRequest("Write the message in both languages (up to 600 characters)."));
    }
    sqlx::query("UPDATE clinic SET reminder_pt = $1, reminder_en = $2").bind(pt).bind(en).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "reminder_text.updated", None, &a.device, "").await;
    Ok(Json(json!({ "ok": true })))
}
