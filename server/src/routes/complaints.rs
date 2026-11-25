//! Complaints log (Livro de Reclamações), owner only. SPEC.md › Legal tools; LEGAL.md › Complaints.
//!
//! Each complaint gets a deadline 10 working days after it was received (weekends and national holidays
//! don't count): paper complaints must be entered in the ERS system by then, with the reply sent to the
//! person. A complaint is done when it was sent to the ERS (paper) or answered (electronic and other ways).

use super::util::{changed, sets, update_row, K};
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use crate::workdays;
use axum::{extract::{Path, State}, Json};
use serde_json::{json, Map, Value};
use uuid::Uuid;

pub const ERS_WORKING_DAYS: u32 = 10;
const BAD_DATE: &str = "Enter the date the complaint was received.";

fn subject(id: Uuid) -> Option<(&'static str, String)> {
    Some(("complaint", id.to_string()))
}

const FIELDS: &[(&str, K)] = &[
    ("source", K::Req("text")), ("book_ref", K::Text), ("patient_id", K::Opt("uuid")), ("complainant", K::Text),
    ("summary", K::Text), ("reply", K::Text), ("replied_on", K::Opt("date")), ("sent_to_ers_on", K::Opt("date")),
];

const LIST_SQL: &str = "
    SELECT COALESCE(json_agg(json_build_object('id', c.id, 'received_on', c.received_on, 'source', c.source, 'book_ref', c.book_ref,
             'patient_id', c.patient_id, 'patient_name', p.name, 'patient_number', p.number, 'complainant', c.complainant,
             'summary', c.summary, 'reply', c.reply, 'replied_on', c.replied_on, 'sent_to_ers_on', c.sent_to_ers_on,
             'due_on', c.due_on, 'created_by_name', st.name, 'created_at', c.created_at, 'updated_at', c.updated_at)
           ORDER BY c.received_on DESC, c.created_at DESC), '[]')
    FROM complaints c LEFT JOIN patients p ON p.id = c.patient_id AND p.deleted_at IS NULL LEFT JOIN staff st ON st.id = c.created_by";

/// Owner: every complaint, newest first, with its ERS deadline.
pub async fn list(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let (v,): (Value,) = sqlx::query_as(LIST_SQL).fetch_one(&s.pool).await?;
    Ok(Json(v))
}

fn received(body: &Map<String, Value>) -> ApiResult<Option<time::Date>> {
    match body.get("received_on") {
        None => Ok(None),
        Some(v) => v.as_str().and_then(workdays::parse).map(Some).ok_or(AppError::BadRequest(BAD_DATE)),
    }
}

fn check_source(body: &Map<String, Value>) -> ApiResult<()> {
    match body.get("source").map(|v| v.as_str()) {
        None | Some(Some("paper" | "electronic" | "other")) => Ok(()),
        _ => Err(AppError::BadRequest("Choose where the complaint came from.")),
    }
}

/// Owner: log a complaint. The deadline is worked out here.
pub async fn create(State(s): State<AppState>, a: Auth, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let day = received(&body)?.ok_or(AppError::BadRequest(BAD_DATE))?;
    if body.get("source").is_none() {
        return Err(AppError::BadRequest("Choose where the complaint came from."));
    }
    check_source(&body)?;
    let due = workdays::add_working_days(day, ERS_WORKING_DAYS);
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO complaints (id, received_on, source, due_on, created_by) VALUES ($1, $2::date, 'other', $3::date, $4)")
        .bind(id).bind(workdays::fmt(day)).bind(workdays::fmt(due)).bind(a.staff.id).execute(&mut *tx).await?;
    update_row(&mut *tx, "complaints", id, sets(&body, FIELDS, 2)?, "", "").await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "complaint.created", subject(id), &a.device, "").await;
    Ok(Json(json!({ "id": id, "due_on": workdays::fmt(due) })))
}

/// Owner: change a complaint (reply, sent to the ERS, …). Changing the date of receipt moves the deadline.
pub async fn update(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    check_source(&body)?;
    let extra = match received(&body)? {
        Some(d) => format!("updated_at = now(), received_on = '{}'::date, due_on = '{}'::date", workdays::fmt(d), workdays::fmt(workdays::add_working_days(d, ERS_WORKING_DAYS))),
        None => "updated_at = now()".into(),
    };
    if !update_row(&s.pool, "complaints", id, sets(&body, FIELDS, 2)?, &extra, "").await? {
        return Err(AppError::NotFound);
    }
    let mut what = changed(&body, FIELDS);
    if body.contains_key("received_on") {
        what = if what.is_empty() { "received_on".into() } else { format!("received_on,{what}") };
    }
    auth::log(&s.pool, Some(a.staff.id), "complaint.updated", subject(id), &a.device, &what).await;
    Ok(Json(json!({ "ok": true })))
}

/// Owner: remove a complaint logged by mistake.
pub async fn delete(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let n = sqlx::query("DELETE FROM complaints WHERE id = $1").bind(id).execute(&s.pool).await?.rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    auth::log(&s.pool, Some(a.staff.id), "complaint.deleted", subject(id), &a.device, "").await;
    Ok(Json(json!({ "ok": true })))
}

/// For Today (owner): open complaints due within 3 working days, or overdue.
pub async fn alerts(s: &AppState, a: &Auth) -> ApiResult<Value> {
    if a.staff.role != "owner" {
        return Ok(Value::Null);
    }
    let rows: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT id, received_on::text, due_on::text, complainant FROM complaints
         WHERE CASE WHEN source = 'paper' THEN sent_to_ers_on IS NULL ELSE replied_on IS NULL END ORDER BY due_on")
        .fetch_all(&s.pool).await?;
    let (today,): (String,) = sqlx::query_as("SELECT ((now() AT TIME ZONE $1)::date)::text").bind(super::schedule::tz()).fetch_one(&s.pool).await?;
    let Some(t) = workdays::parse(&today) else { return Ok(json!([])) };
    let out: Vec<Value> = rows.into_iter().filter_map(|(id, rec, due, who)| {
        let d = workdays::parse(&due)?;
        let left = workdays::working_days_between(t, d);
        (left <= 3).then(|| json!({ "id": id, "received_on": rec, "due_on": due, "complainant": who, "working_days_left": left }))
    }).collect();
    Ok(json!(out))
}
