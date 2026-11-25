//! Schedule: appointments, recurring series, overlaps, group classes, waitlist, booking requests, Today alerts.
//!
//! Times travel as RFC 3339 strings. Series are generated in the clinic's time zone (CLINIC_TZ,
//! default Europe/Lisbon) so a weekly 09:00 session stays at 09:00 across summer-time changes.

use super::util::{clinical, int_array, sets, K};
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{extract::{Path, Query, State}, Json};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

pub fn tz() -> &'static str {
    static TZ: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TZ.get_or_init(|| std::env::var("CLINIC_TZ").ok().filter(|t| t.chars().all(|c| c.is_ascii_alphanumeric() || "/_+-".contains(c))).unwrap_or_else(|| "Europe/Lisbon".into()))
}

const KINDS: &[&str] = &["session", "assessment", "class", "home", "block"];
const MAX_OCCURRENCES: i64 = 200;

// ───────── list ─────────

#[derive(Deserialize)]
pub struct ListQuery { from: String, to: String, therapist_id: Option<Uuid>, include_cancelled: Option<bool> }

/// Everyone: appointments between `from` and `to` (at most 62 days).
pub async fn list(State(s): State<AppState>, _a: Auth, Query(q): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let (ok,): (bool,) = sqlx::query_as("SELECT $2::timestamptz > $1::timestamptz AND $2::timestamptz - $1::timestamptz <= interval '62 days'")
        .bind(&q.from).bind(&q.to).fetch_one(&s.pool).await?;
    if !ok {
        return Err(AppError::BadRequest("Choose a period of up to two months."));
    }
    let (v,): (Value,) = sqlx::query_as(LIST_SQL)
        .bind(&q.from).bind(&q.to).bind(q.therapist_id).bind(q.include_cancelled.unwrap_or(false))
        .fetch_one(&s.pool).await?;
    Ok(Json(v))
}

const LIST_SQL: &str = "
    SELECT COALESCE(json_agg(x ORDER BY x.starts_at), '[]') FROM (
      SELECT ap.id, ap.kind, ap.starts_at, ap.ends_at, ap.status, ap.therapist_id, st.name AS therapist_name,
             ap.patient_id, p.name AS patient_name, p.number AS patient_number, ap.series_id, ap.prescription_id,
             ap.title, ap.color, ap.address, ap.travel_minutes, ap.capacity, ap.admin_note,
             ap.plan_id, ap.charge_cents, ap.charge_waived,
             (SELECT COALESCE(sum(py.amount_cents), 0) FROM payments py WHERE py.appointment_id = ap.id AND py.voided_at IS NULL
                AND (ap.kind = 'class' OR py.patient_id = ap.patient_id)) AS paid_cents,
             CASE WHEN ap.kind = 'class' THEN (
               SELECT COALESCE(json_agg(json_build_object('patient_id', ca.patient_id, 'name', pp.name, 'attended', ca.attended, 'plan_id', ca.plan_id, 'charge_cents', ca.charge_cents) ORDER BY pp.name), '[]')
               FROM class_attendees ca JOIN patients pp ON pp.id = ca.patient_id WHERE ca.appointment_id = ap.id) END AS attendees
      FROM appointments ap
      LEFT JOIN staff st ON st.id = ap.therapist_id
      LEFT JOIN patients p ON p.id = ap.patient_id
      WHERE ap.starts_at < $2::timestamptz AND ap.ends_at > $1::timestamptz
        AND ($3::uuid IS NULL OR ap.therapist_id = $3)
        AND (ap.status <> 'cancelled' OR $4)) x";

// ───────── overlaps ─────────

/// Booked appointments of the same therapist that overlap any of `ids` (travel time around home visits counts).
pub(crate) async fn overlaps(tx: &mut Transaction<'_, Postgres>, ids: &[Uuid]) -> ApiResult<Value> {
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(DISTINCT jsonb_build_object('id', o.id, 'kind', o.kind, 'starts_at', o.starts_at, 'ends_at', o.ends_at,
                                                      'title', o.title, 'patient_name', p.name)), '[]')
         FROM appointments n
         JOIN appointments o ON o.therapist_id = n.therapist_id AND o.status = 'booked' AND o.id <> ALL($1)
              AND tstzrange(o.starts_at - make_interval(mins => o.travel_minutes), o.ends_at + make_interval(mins => o.travel_minutes))
               && tstzrange(n.starts_at - make_interval(mins => n.travel_minutes), n.ends_at + make_interval(mins => n.travel_minutes))
         LEFT JOIN patients p ON p.id = o.patient_id
         WHERE n.id = ANY($1) AND n.status = 'booked'")
        .bind(ids).fetch_one(&mut **tx).await?;
    Ok(v)
}

async fn check_overlaps(tx: &mut Transaction<'_, Postgres>, ids: &[Uuid], allow: bool) -> ApiResult<Value> {
    let o = overlaps(tx, ids).await?;
    if !allow && o.as_array().is_some_and(|a| !a.is_empty()) {
        return Err(AppError::ConflictWith("overlap", "This overlaps another appointment for the same therapist.", o));
    }
    Ok(o)
}

// ───────── create ─────────

#[derive(Deserialize, Clone)]
pub struct Repeat { pub(crate) weekdays: Vec<i32>, pub(crate) until: Option<String>, pub(crate) count: Option<i64> }

#[derive(Deserialize, Clone)]
pub struct NewAppointment {
    pub(crate) kind: String,
    pub(crate) starts_at: String,
    pub(crate) ends_at: String,
    pub(crate) therapist_id: Option<Uuid>,
    pub(crate) patient_id: Option<Uuid>,
    pub(crate) prescription_id: Option<Uuid>,
    #[serde(default)] pub(crate) title: String,
    #[serde(default)] pub(crate) color: String,
    #[serde(default)] pub(crate) address: String,
    #[serde(default)] pub(crate) travel_minutes: i32,
    pub(crate) capacity: Option<i32>,
    #[serde(default)] pub(crate) admin_note: String,
    pub(crate) repeat: Option<Repeat>,
    #[serde(default)] pub(crate) allow_overlap: bool,
}

/// Creates one appointment or a whole series inside `tx`. Returns the new ids and any overlaps that were allowed.
pub async fn create_in(tx: &mut Transaction<'_, Postgres>, a: &Auth, r: &NewAppointment) -> ApiResult<(Vec<Uuid>, Option<Uuid>, Value)> {
    if !KINDS.contains(&r.kind.as_str()) {
        return Err(AppError::BadRequest("Choose the type of appointment."));
    }
    if matches!(r.kind.as_str(), "session" | "assessment" | "home") && r.patient_id.is_none() {
        return Err(AppError::BadRequest("Choose the patient."));
    }
    if r.kind != "block" && r.therapist_id.is_none() {
        return Err(AppError::BadRequest("Choose the therapist."));
    }
    if r.kind == "class" && r.capacity.unwrap_or(0) < 1 {
        return Err(AppError::BadRequest("Enter how many people fit in the class."));
    }
    if !(0..=240).contains(&r.travel_minutes) {
        return Err(AppError::BadRequest("Travel time must be between 0 and 240 minutes."));
    }
    let (ok,): (bool,) = sqlx::query_as("SELECT $2::timestamptz > $1::timestamptz AND $2::timestamptz - $1::timestamptz <= interval '24 hours'")
        .bind(&r.starts_at).bind(&r.ends_at).fetch_one(&mut **tx).await?;
    if !ok {
        return Err(AppError::BadRequest("The end must be after the start, on the same day."));
    }

    // Occurrences: just this one, or a weekly pattern until a date or for N sessions.
    let mut series_id = None;
    let times: Vec<(OffsetDateTime, OffsetDateTime)> = match &r.repeat {
        None => sqlx::query_as("SELECT $1::timestamptz, $2::timestamptz").bind(&r.starts_at).bind(&r.ends_at).fetch_all(&mut **tx).await?,
        Some(rep) => {
            if rep.weekdays.is_empty() || rep.weekdays.iter().any(|d| !(1..=7).contains(d)) {
                return Err(AppError::BadRequest("Choose the days of the week to repeat on."));
            }
            if rep.until.is_none() && rep.count.is_none() {
                return Err(AppError::BadRequest("Choose an end date or a number of sessions."));
            }
            let limit = rep.count.unwrap_or(MAX_OCCURRENCES).clamp(1, MAX_OCCURRENCES);
            let t: Vec<(OffsetDateTime, OffsetDateTime)> = sqlx::query_as(
                "WITH b AS (SELECT $1::timestamptz AS s, $2::timestamptz AS e, $3::text AS tz)
                 SELECT ((d::date + (b.s AT TIME ZONE b.tz)::time) AT TIME ZONE b.tz),
                        ((d::date + (b.s AT TIME ZONE b.tz)::time) AT TIME ZONE b.tz) + (b.e - b.s)
                 FROM b, generate_series((b.s AT TIME ZONE b.tz)::date,
                                         LEAST(COALESCE(NULLIF($4, '')::date, (b.s AT TIME ZONE b.tz)::date + 366), (b.s AT TIME ZONE b.tz)::date + 366),
                                         interval '1 day') d
                 WHERE extract(isodow FROM d)::int = ANY($5::int[]) OR d::date = (b.s AT TIME ZONE b.tz)::date
                 ORDER BY d LIMIT $6")
                .bind(&r.starts_at).bind(&r.ends_at).bind(tz()).bind(rep.until.clone().unwrap_or_default()).bind(&rep.weekdays).bind(limit)
                .fetch_all(&mut **tx).await?;
            let sid = Uuid::new_v4();
            sqlx::query("INSERT INTO series (id, weekdays, until_date, count, created_by) VALUES ($1, $2, NULLIF($3, '')::date, $4, $5)")
                .bind(sid).bind(&rep.weekdays).bind(rep.until.clone().unwrap_or_default()).bind(rep.count.map(|c| c as i32)).bind(a.staff.id)
                .execute(&mut **tx).await?;
            series_id = Some(sid);
            t
        }
    };
    let ids: Vec<Uuid> = times.iter().map(|_| Uuid::new_v4()).collect();
    let starts: Vec<OffsetDateTime> = times.iter().map(|t| t.0).collect();
    let ends: Vec<OffsetDateTime> = times.iter().map(|t| t.1).collect();
    sqlx::query(
        "INSERT INTO appointments (id, starts_at, ends_at, kind, therapist_id, patient_id, series_id, prescription_id,
                                   title, color, address, travel_minutes, capacity, admin_note, created_by)
         SELECT u.id, u.s, u.e, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15
         FROM unnest($1::uuid[], $2::timestamptz[], $3::timestamptz[]) AS u(id, s, e)")
        .bind(&ids).bind(&starts).bind(&ends)
        .bind(&r.kind).bind(r.therapist_id).bind(r.patient_id).bind(series_id).bind(r.prescription_id)
        .bind(r.title.trim()).bind(r.color.trim()).bind(r.address.trim()).bind(r.travel_minutes)
        .bind(if r.kind == "class" { r.capacity } else { None }).bind(r.admin_note.trim()).bind(a.staff.id)
        .execute(&mut **tx).await?;
    let o = check_overlaps(tx, &ids, r.allow_overlap || r.kind == "block").await?;
    Ok((ids, series_id, o))
}

/// Everyone: book one appointment or a series. Answers 409 `overlap` with the clashes unless `allow_overlap`.
pub async fn create(State(s): State<AppState>, a: Auth, Json(r): Json<NewAppointment>) -> ApiResult<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    let (ids, series_id, o) = create_in(&mut tx, &a, &r).await?;
    tx.commit().await?;
    let subj = r.patient_id.map(|p| ("patient", p.to_string()));
    auth::log(&s.pool, Some(a.staff.id), "appointment.created", subj, &a.device, &format!("{} x{}", r.kind, ids.len())).await;
    Ok(Json(json!({ "ids": ids, "series_id": series_id, "overlaps": o })))
}

// ───────── change, move, cancel ─────────

const APPT_FIELDS: &[(&str, K)] = &[
    ("therapist_id", K::Opt("uuid")), ("prescription_id", K::Opt("uuid")), ("title", K::Text), ("color", K::Text),
    ("address", K::Text), ("travel_minutes", K::Req("int")), ("capacity", K::Opt("int")), ("admin_note", K::Text),
    // Phase 3: which package a done appointment used; waiving its charge is the owner's call.
    ("plan_id", K::Opt("uuid")), ("charge_waived", K::Req("boolean")),
];

/// Everyone: change an appointment, or this and the following ones of a series (`scope`: one | following | all).
/// Moving a series keeps each occurrence on its own day (shifted by the same number of days) at the new time.
pub async fn update(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    let scope = body.get("scope").and_then(Value::as_str).unwrap_or("one");
    let allow = body.get("allow_overlap").and_then(Value::as_bool).unwrap_or(false);
    let status = body.get("status").and_then(Value::as_str);
    if let Some(st) = status {
        if !["booked", "done", "cancelled", "no_show"].contains(&st) {
            return Err(AppError::BadRequest("Choose a status."));
        }
        if st != "cancelled" && scope != "one" {
            return Err(AppError::BadRequest("Mark appointments as done or missed one at a time."));
        }
    }
    if body.contains_key("charge_waived") && a.staff.role != "owner" {
        return Err(AppError::Forbidden);
    }
    let mut tx = s.pool.begin().await?;
    let row: Option<(Option<Uuid>, Option<Uuid>, String)> = sqlx::query_as("SELECT series_id, patient_id, kind FROM appointments WHERE id = $1 FOR UPDATE")
        .bind(id).fetch_optional(&mut *tx).await?;
    let Some((series, patient, kind)) = row else { return Err(AppError::NotFound) };
    let ids: Vec<Uuid> = match (scope, series) {
        ("one", _) | (_, None) => vec![id],
        ("following", Some(sid)) => sqlx::query_scalar(
            "SELECT id FROM appointments WHERE series_id = $1 AND (id = $2 OR (status = 'booked' AND starts_at >= (SELECT starts_at FROM appointments WHERE id = $2)))")
            .bind(sid).bind(id).fetch_all(&mut *tx).await?,
        ("all", Some(sid)) => sqlx::query_scalar("SELECT id FROM appointments WHERE series_id = $1 AND (id = $2 OR status = 'booked')")
            .bind(sid).bind(id).fetch_all(&mut *tx).await?,
        _ => return Err(AppError::BadRequest("Choose which appointments to change.")),
    };

    // Fields that apply to every appointment in scope.
    let (mut parts, mut binds) = sets(&body, APPT_FIELDS, 2)?;
    if let Some(st) = status {
        binds.push(Some(st.to_string()));
        parts.push(format!("status = ${}", binds.len() + 1));
    }
    // New time.
    let starts = body.get("starts_at").and_then(Value::as_str);
    let ends = body.get("ends_at").and_then(Value::as_str);
    if starts.is_some() != ends.is_some() {
        return Err(AppError::BadRequest("Send both the new start and the new end."));
    }
    if let (Some(st), Some(en)) = (starts, ends) {
        let (ok,): (bool,) = sqlx::query_as("SELECT $2::timestamptz > $1::timestamptz AND $2::timestamptz - $1::timestamptz <= interval '24 hours'")
            .bind(st).bind(en).fetch_one(&mut *tx).await?;
        if !ok {
            return Err(AppError::BadRequest("The end must be after the start, on the same day."));
        }
        binds.push(Some(st.to_string()));
        let ns = binds.len() + 1;
        binds.push(Some(en.to_string()));
        let ne = binds.len() + 1;
        binds.push(Some(tz().to_string()));
        let ntz = binds.len() + 1;
        binds.push(Some(id.to_string()));
        let nref = binds.len() + 1;
        // day shift = new local date − the edited appointment's old local date
        let new_start = format!(
            "(((starts_at AT TIME ZONE ${ntz})::date + ((${ns}::timestamptz AT TIME ZONE ${ntz})::date
                - (SELECT (r.starts_at AT TIME ZONE ${ntz})::date FROM appointments r WHERE r.id = ${nref}::uuid)))
              + (${ns}::timestamptz AT TIME ZONE ${ntz})::time) AT TIME ZONE ${ntz}");
        parts.push(format!("starts_at = {new_start}"));
        parts.push(format!("ends_at = {new_start} + (${ne}::timestamptz - ${ns}::timestamptz)"));
    }
    if !parts.is_empty() {
        let sql = format!("UPDATE appointments SET {} WHERE id = ANY($1)", parts.join(", "));
        let mut q = sqlx::query(&sql).bind(&ids);
        for b in binds {
            q = q.bind(b);
        }
        q.execute(&mut *tx).await?;
    }
    // Phase 3: what it used and costs.
    if let Some(st) = status {
        super::money::on_status(&mut tx, &ids, st).await?;
    } else if body.contains_key("plan_id") {
        for x in &ids {
            super::money::apply_done(&mut tx, *x, false).await?;
        }
    }
    let o = check_overlaps(&mut tx, &ids, allow || kind == "block").await?;
    tx.commit().await?;

    let action = match status { Some("cancelled") => "appointment.cancelled", Some("done") => "appointment.done", Some("no_show") => "appointment.no_show", _ => "appointment.updated" };
    auth::log(&s.pool, Some(a.staff.id), action, patient.map(|p| ("patient", p.to_string())), &a.device, &format!("{scope} x{}", ids.len())).await;

    // A freed slot: who on the waitlist fits it.
    let waitlist = if status == Some("cancelled") && ids.len() == 1 { waitlist_matches(&s, id).await? } else { json!([]) };
    Ok(Json(json!({ "ids": ids, "overlaps": o, "waitlist_matches": waitlist })))
}

// ───────── group classes ─────────

#[derive(Deserialize)]
pub struct AttendeeReq { patient_id: Uuid }

pub async fn add_attendee(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(r): Json<AttendeeReq>) -> ApiResult<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    let row: Option<(String, Option<i32>, i64)> = sqlx::query_as(
        "SELECT kind, capacity, (SELECT count(*) FROM class_attendees WHERE appointment_id = $1) FROM appointments WHERE id = $1 FOR UPDATE")
        .bind(id).fetch_optional(&mut *tx).await?;
    let Some((kind, cap, n)) = row else { return Err(AppError::NotFound) };
    if kind != "class" {
        return Err(AppError::BadRequest("Only group classes have attendees."));
    }
    if n >= cap.unwrap_or(0) as i64 {
        return Err(AppError::Conflict("This class is full."));
    }
    sqlx::query("INSERT INTO class_attendees (appointment_id, patient_id) VALUES ($1, $2)").bind(id).bind(r.patient_id).execute(&mut *tx).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "class.attendee_added", Some(("patient", r.patient_id.to_string())), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct AttendedReq { attended: Option<bool> }

pub async fn set_attended(State(s): State<AppState>, a: Auth, Path((id, pid)): Path<(Uuid, Uuid)>, Json(r): Json<AttendedReq>) -> ApiResult<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query("UPDATE class_attendees SET attended = $3 WHERE appointment_id = $1 AND patient_id = $2")
        .bind(id).bind(pid).bind(r.attended).execute(&mut *tx).await?.rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    // Phase 3: a class credit or the drop-in price.
    super::money::on_attended(&mut tx, id, pid, r.attended).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "class.attendance", Some(("patient", pid.to_string())), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove_attendee(State(s): State<AppState>, a: Auth, Path((id, pid)): Path<(Uuid, Uuid)>) -> ApiResult<Json<Value>> {
    sqlx::query("DELETE FROM class_attendees WHERE appointment_id = $1 AND patient_id = $2").bind(id).bind(pid).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "class.attendee_removed", Some(("patient", pid.to_string())), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── waitlist ─────────

/// Waiting people whose preferences fit a freed appointment slot, first come first served.
async fn waitlist_matches(s: &AppState, appointment: Uuid) -> ApiResult<Value> {
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('id', w.id, 'patient_id', w.patient_id, 'name', COALESCE(p.name, w.name),
                                                    'phone', COALESCE(p.phone, w.phone)) ORDER BY w.created_at), '[]')
         FROM appointments ap
         JOIN waitlist w ON w.status = 'waiting'
           AND (w.therapist_id IS NULL OR w.therapist_id = ap.therapist_id)
           AND (cardinality(w.weekdays) = 0 OR extract(isodow FROM ap.starts_at AT TIME ZONE $2)::int = ANY(w.weekdays))
           AND (w.from_time IS NULL OR (ap.starts_at AT TIME ZONE $2)::time >= w.from_time)
           AND (w.to_time IS NULL OR (ap.ends_at AT TIME ZONE $2)::time <= w.to_time)
           AND (w.patient_id IS NULL OR w.patient_id IS DISTINCT FROM ap.patient_id)
         LEFT JOIN patients p ON p.id = w.patient_id
         WHERE ap.id = $1 AND ap.kind IN ('session', 'assessment') AND ap.starts_at > now()")
        .bind(appointment).bind(tz()).fetch_one(&s.pool).await?;
    Ok(v)
}

pub async fn waitlist(State(s): State<AppState>, _a: Auth) -> ApiResult<Json<Value>> {
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(x ORDER BY x.created_at), '[]') FROM (
           SELECT w.id, w.patient_id, COALESCE(p.name, w.name) AS name, COALESCE(p.phone, w.phone) AS phone, COALESCE(p.email, w.email) AS email,
                  w.therapist_id, st.name AS therapist_name, w.weekdays, w.from_time, w.to_time, w.note, w.status,
                  w.offered_appointment, w.offer_expires_at, w.created_at
           FROM waitlist w LEFT JOIN patients p ON p.id = w.patient_id LEFT JOIN staff st ON st.id = w.therapist_id
           WHERE w.status IN ('waiting', 'offered')) x")
        .fetch_one(&s.pool).await?;
    Ok(Json(v))
}

const WAIT_FIELDS: &[(&str, K)] = &[
    ("patient_id", K::Opt("uuid")), ("name", K::Text), ("phone", K::Text), ("email", K::Text), ("therapist_id", K::Opt("uuid")),
    ("weekdays", K::IntArray), ("from_time", K::Opt("time")), ("to_time", K::Opt("time")), ("note", K::Text), ("status", K::Req("text")),
];

pub async fn add_waitlist(State(s): State<AppState>, a: Auth, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    if body.get("patient_id").is_none_or(Value::is_null) && body.get("name").and_then(Value::as_str).unwrap_or("").trim().is_empty() {
        return Err(AppError::BadRequest("Choose a patient or enter a name and phone."));
    }
    if let Some(w) = body.get("weekdays") {
        if int_array(w)?.is_none() && !w.is_null() {
            return Err(AppError::BadRequest("Choose the days of the week."));
        }
    }
    let id = Uuid::new_v4();
    let (parts, binds) = sets(&body, WAIT_FIELDS, 3)?;
    let cols: Vec<&str> = parts.iter().map(|p| p.split(" = ").next().unwrap_or("")).collect();
    let vals: Vec<&str> = parts.iter().map(|p| p.split_once(" = ").map(|x| x.1).unwrap_or("NULL")).collect();
    let sep = if cols.is_empty() { "" } else { ", " };
    let sql = format!("INSERT INTO waitlist (id, created_by{sep}{}) VALUES ($1, $2{sep}{})", cols.join(", "), vals.join(", "));
    let mut q = sqlx::query(&sql).bind(id).bind(a.staff.id);
    for b in binds {
        q = q.bind(b);
    }
    q.execute(&s.pool).await?;
    let pid = body.get("patient_id").and_then(Value::as_str).map(|p| ("patient", p.to_string()));
    auth::log(&s.pool, Some(a.staff.id), "waitlist.added", pid, &a.device, &id.to_string()).await;
    Ok(Json(json!({ "id": id })))
}

pub async fn update_waitlist(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    if !super::util::update_row(&s.pool, "waitlist", id, sets(&body, WAIT_FIELDS, 2)?, "", "").await? {
        return Err(AppError::NotFound);
    }
    auth::log(&s.pool, Some(a.staff.id), "waitlist.updated", None, &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── booking requests ─────────

#[derive(Deserialize)]
pub struct RequestsQuery { status: Option<String> }

pub async fn requests(State(s): State<AppState>, _a: Auth, Query(q): Query<RequestsQuery>) -> ApiResult<Json<Value>> {
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(to_jsonb(b) || jsonb_build_object('about_starts_at', ab.starts_at) ORDER BY b.preferred_at), '[]')
         FROM booking_requests b LEFT JOIN appointments ab ON ab.id = b.about_appointment_id WHERE b.status = $1")
        .bind(q.status.unwrap_or_else(|| "pending".into())).fetch_one(&s.pool).await?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct NewRequest { name: String, #[serde(default)] phone: String, #[serde(default)] email: String, patient_id: Option<Uuid>,
                        #[serde(default)] service: String, preferred_at: String, #[serde(default)] message: String }

/// Everyone: enter a request by hand (phone, email). The patient portal will create these later.
pub async fn add_request(State(s): State<AppState>, a: Auth, Json(r): Json<NewRequest>) -> ApiResult<Json<Value>> {
    if r.name.trim().is_empty() || (r.phone.trim().is_empty() && r.email.trim().is_empty()) {
        return Err(AppError::BadRequest("Enter a name and a phone or email."));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO booking_requests (id, name, phone, email, patient_id, service, preferred_at, message)
                 VALUES ($1, $2, $3, $4, $5, COALESCE(NULLIF($6, ''), 'session'), $7::timestamptz, $8)")
        .bind(id).bind(r.name.trim()).bind(r.phone.trim()).bind(r.email.trim()).bind(r.patient_id).bind(r.service.trim()).bind(&r.preferred_at).bind(r.message.trim())
        .execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "booking_request.added", None, &a.device, &id.to_string()).await;
    Ok(Json(json!({ "id": id })))
}

/// Everyone: accept a request by booking it (same rules as a normal booking, overlaps included).
pub async fn accept_request(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(r): Json<NewAppointment>) -> ApiResult<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    let st: Option<(String,)> = sqlx::query_as("SELECT status FROM booking_requests WHERE id = $1 FOR UPDATE").bind(id).fetch_optional(&mut *tx).await?;
    match st {
        None => return Err(AppError::NotFound),
        Some((st,)) if st != "pending" => return Err(AppError::Conflict("This request was already answered.")),
        _ => {}
    }
    let r = NewAppointment { repeat: None, ..r };
    let (ids, _, o) = create_in(&mut tx, &a, &r).await?;
    sqlx::query("UPDATE booking_requests SET status = 'accepted', appointment_id = $2, decided_by = $3, decided_at = now(), patient_id = COALESCE(patient_id, $4) WHERE id = $1")
        .bind(id).bind(ids[0]).bind(a.staff.id).bind(r.patient_id).execute(&mut *tx).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "booking_request.accepted", r.patient_id.map(|p| ("patient", p.to_string())), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "appointment_id": ids[0], "overlaps": o })))
}

pub async fn decline_request(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let n = sqlx::query("UPDATE booking_requests SET status = 'declined', decided_by = $2, decided_at = now() WHERE id = $1 AND status = 'pending'")
        .bind(id).bind(a.staff.id).execute(&s.pool).await?.rows_affected();
    if n == 0 {
        return Err(AppError::Conflict("This request was already answered."));
    }
    auth::log(&s.pool, Some(a.staff.id), "booking_request.declined", None, &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

/// Everyone: Undo for accept/decline. An appointment made by accepting is removed if nothing happened to it yet,
/// and a patient file the one-press booking made goes too if nothing else uses it (`agenda::reopen_in`).
pub async fn reopen_request(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    let dropped = super::agenda::reopen_in(&mut tx, id).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "booking_request.reopened", None, &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true, "patient_removed": dropped })))
}

// ───────── Today alerts ─────────

/// Everyone: alerts for the Today page. Worsening scores (clinical) only for owner and therapists.
pub async fn alerts(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    let (mut v,): (Value,) = sqlx::query_as(ALERTS_SQL).bind(clinical(&a)).fetch_one(&s.pool).await?;
    // Phase 3: packages running out (everyone) and money owed (owner).
    let extra = super::money::alerts_extra(&s, &a).await?;
    v["plans_low"] = extra["plans_low"].clone();
    v["money"] = extra["money"].clone();
    // ERS deadlines for complaints (owner only; null for everyone else).
    v["complaints"] = super::complaints::alerts(&s, &a).await?;
    // Backups that stopped or failed (owner only; null otherwise). Audit C1.
    v["backup"] = super::system::alert(&a).await;
    // Devices waiting for approval (owner only; null otherwise). PhysioPulse plan, phase 3.
    v["devices"] = super::devices::alert(&s, &a).await?;
    Ok(Json(v))
}

const ALERTS_SQL: &str = "
  SELECT json_build_object(
    'booking_requests', (SELECT count(*) FROM booking_requests WHERE status = 'pending'),
    -- Patients seen in the next 7 days without a valid treatment and data-processing consent.
    'consents_missing', (SELECT COALESCE(json_agg(json_build_object('patient_id', x.id, 'name', x.name, 'number', x.number, 'missing', x.missing)), '[]') FROM (
        SELECT p.id, p.name, p.number,
               array_remove(ARRAY[
                 CASE WHEN NOT EXISTS (SELECT 1 FROM consents c WHERE c.patient_id = p.id AND c.kind = 'treatment' AND c.granted AND c.revoked_at IS NULL
                                       AND (c.expires_on IS NULL OR c.expires_on >= CURRENT_DATE)) THEN 'treatment' END,
                 CASE WHEN NOT EXISTS (SELECT 1 FROM consents c WHERE c.patient_id = p.id AND c.kind = 'data' AND c.revoked_at IS NULL
                                       AND (c.expires_on IS NULL OR c.expires_on >= CURRENT_DATE)) THEN 'data' END], NULL) AS missing
        FROM patients p
        WHERE p.active AND EXISTS (SELECT 1 FROM appointments ap WHERE ap.patient_id = p.id AND ap.status = 'booked'
                                   AND ap.starts_at BETWEEN now() AND now() + interval '7 days')) x
      WHERE cardinality(x.missing) > 0),
    'consents_expiring', (SELECT COALESCE(json_agg(json_build_object('patient_id', p.id, 'name', p.name, 'kind', c.kind, 'expires_on', c.expires_on)), '[]')
        FROM consents c JOIN patients p ON p.id = c.patient_id
        WHERE p.active AND c.revoked_at IS NULL AND c.expires_on BETWEEN CURRENT_DATE AND CURRENT_DATE + 30
          AND NOT EXISTS (SELECT 1 FROM consents c2 WHERE c2.patient_id = c.patient_id AND c2.kind = c.kind AND c2.signed_at > c.signed_at)),
    -- Everyone (reception books against prescriptions, owner decision 4 Nov 2026).
    'prescriptions', (SELECT COALESCE(json_agg(json_build_object('patient_id', p.id, 'name', p.name, 'prescription_id', pr.id,
                                   'remaining', pr.sessions_total - u.used, 'valid_until', pr.valid_until)), '[]')
        FROM prescriptions pr JOIN patients p ON p.id = pr.patient_id,
             LATERAL (SELECT count(*) FILTER (WHERE status = 'done') AS used FROM appointments WHERE prescription_id = pr.id) u
        WHERE p.active AND pr.sessions_total - u.used > 0
          AND (pr.sessions_total - u.used <= 2 OR pr.valid_until BETWEEN CURRENT_DATE AND CURRENT_DATE + 14)),
    'worse_scores', CASE WHEN $1 THEN (SELECT COALESCE(json_agg(json_build_object('patient_id', p.id, 'name', p.name, 'test_code', x.test_code,
                                   'score', x.score, 'previous', x.prev, 'taken_on', x.taken_on)), '[]')
        FROM (SELECT DISTINCT ON (patient_id, test_code) patient_id, test_code, score, taken_on, higher_is_worse,
                     lead(score) OVER (PARTITION BY patient_id, test_code ORDER BY taken_on DESC, created_at DESC) AS prev
              FROM test_results ORDER BY patient_id, test_code, taken_on DESC, created_at DESC) x
        JOIN patients p ON p.id = x.patient_id
        WHERE p.active AND x.taken_on >= CURRENT_DATE - 30 AND x.prev IS NOT NULL
          AND ((x.higher_is_worse AND x.score > x.prev) OR (NOT x.higher_is_worse AND x.score < x.prev))) END
  )";
