use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{extract::{Path, Query, State}, http::{header, HeaderValue}, response::{IntoResponse, Response}, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Serialize, sqlx::FromRow)]
pub struct StaffRow {
    id: Uuid, email: String, name: String, role: String, active: bool, totp_enabled: bool, ai_enabled: bool,
    #[serde(with = "time::serde::rfc3339")] created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")] last_seen: Option<OffsetDateTime>,
}

/// Owner only: everyone with an account, and when they were last active.
pub async fn list(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Vec<StaffRow>>> {
    a.require(&["owner"])?;
    let rows = sqlx::query_as(
        "SELECT st.id, st.email, st.name, st.role, st.active, st.totp_enabled, st.ai_enabled, st.created_at,
                (SELECT max(last_seen) FROM sessions WHERE staff_id = st.id) AS last_seen
         FROM staff st ORDER BY st.created_at")
        .fetch_all(&s.pool).await?;
    Ok(Json(rows))
}

/// Everyone: who can be booked (active owner and therapists). Names only, for the schedule.
pub async fn therapists(State(s): State<AppState>, _a: Auth) -> ApiResult<Json<Value>> {
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('id', id, 'name', name, 'role', role) ORDER BY created_at), '[]')
         FROM staff WHERE active AND role IN ('owner', 'therapist')")
        .fetch_one(&s.pool).await?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct InviteReq { role: String, email: Option<String> }

/// Owner only: a one-time code, valid 7 days. Only its hash is stored, so it's shown once.
pub async fn invite(State(s): State<AppState>, a: Auth, Json(r): Json<InviteReq>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    if !["therapist", "reception", "owner"].contains(&r.role.as_str()) { return Err(AppError::BadRequest("Choose a role.")); }
    let code = auth::new_invite_code();
    let email = r.email.map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty());
    sqlx::query("INSERT INTO invites (id, code_hash, role, email, created_by, expires_at) VALUES ($1,$2,$3,$4,$5, now() + interval '7 days')")
        .bind(Uuid::new_v4()).bind(auth::sha256(&code)).bind(&r.role).bind(&email).bind(a.staff.id)
        .execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "invite.created", None, &a.device, &r.role).await;
    Ok(Json(json!({ "code": code, "role": r.role, "expires_in_days": 7 })))
}

#[derive(Deserialize)]
pub struct UpdateReq { active: Option<bool>, role: Option<String>, ai_enabled: Option<bool> }

/// Owner only: change a role or deactivate someone (which also signs them out everywhere).
pub async fn update(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(r): Json<UpdateReq>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    if id == a.staff.id { return Err(AppError::BadRequest("You can’t change your own role or deactivate yourself.")); }
    // The AI switch (PhysioPulse plan, phase 2). The database keeps it on for owners and off for reception.
    if let Some(on) = r.ai_enabled {
        let role: Option<(String,)> = sqlx::query_as("SELECT role FROM staff WHERE id = $1").bind(id).fetch_optional(&s.pool).await?;
        match role.map(|r| r.0).as_deref() {
            None => return Err(AppError::NotFound),
            Some("therapist") | Some("reception") => {}
            Some(_) => return Err(AppError::BadRequest("The owner always has the AI switch on.")),
        }
        sqlx::query("UPDATE staff SET ai_enabled = $2 WHERE id = $1").bind(id).bind(on).execute(&s.pool).await?;
        auth::log(&s.pool, Some(a.staff.id), if on { "staff.ai_on" } else { "staff.ai_off" }, Some(("staff", id.to_string())), &a.device, "").await;
    }
    if let Some(role) = &r.role {
        if !["therapist", "reception", "owner"].contains(&role.as_str()) { return Err(AppError::BadRequest("Choose a role.")); }
        sqlx::query("UPDATE staff SET role = $2 WHERE id = $1").bind(id).bind(role).execute(&s.pool).await?;
        auth::log(&s.pool, Some(a.staff.id), "staff.role_changed", Some(("staff", id.to_string())), &a.device, role).await;
    }
    if let Some(active) = r.active {
        sqlx::query("UPDATE staff SET active = $2 WHERE id = $1").bind(id).bind(active).execute(&s.pool).await?;
        if !active {
            sqlx::query("DELETE FROM sessions WHERE staff_id = $1").bind(id).execute(&s.pool).await?;
            sqlx::query("DELETE FROM trusted_devices WHERE staff_id = $1").bind(id).execute(&s.pool).await?;
        }
        auth::log(&s.pool, Some(a.staff.id), if active { "staff.reactivated" } else { "staff.deactivated" }, Some(("staff", id.to_string())), &a.device, "").await;
    }
    Ok(Json(json!({ "ok": true })))
}

/// Owner only: sign someone out of every computer. Also forgets their trusted computers,
/// so the next sign-in anywhere asks for the 6-digit code again.
pub async fn end_sessions(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let n = sqlx::query("DELETE FROM sessions WHERE staff_id = $1").bind(id).execute(&s.pool).await?.rows_affected();
    sqlx::query("DELETE FROM trusted_devices WHERE staff_id = $1").bind(id).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "staff.signed_out_everywhere", Some(("staff", id.to_string())), &a.device, &n.to_string()).await;
    Ok(Json(json!({ "ended": n })))
}

/// The person's signed-in iPhones (phase 5): one row per phone session. The owner sees anyone's (Team); everyone
/// sees their own. Phone sessions last 30 days with no idle timeout, so the owner needs a way to end one.
pub async fn phones(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    if a.staff.role != "owner" && a.staff.id != id { return Err(AppError::Forbidden); }
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('id', se.id, 'device_id', d.id, 'name', d.name, 'code', d.code,
                  'signed_in', se.created_at, 'last_seen', se.last_seen, 'expires_at', se.expires_at, 'this_device', se.id = $2)
                ORDER BY se.last_seen DESC), '[]')
         FROM sessions se JOIN devices d ON d.id = se.device_id WHERE se.staff_id = $1 AND se.phone")
        .bind(id).bind(a.session_id).fetch_one(&s.pool).await?;
    Ok(Json(v))
}

/// Owner only: "Sign Out This iPhone" in Team. Ends that phone session now; the phone asks for the password and
/// two-step code next time (its Face ID unlock finds the session gone). The phone itself stays approved.
pub async fn sign_out_phone(State(s): State<AppState>, a: Auth, Path((id, sid)): Path<(Uuid, Uuid)>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let gone: Option<(Uuid, String)> = sqlx::query_as(
        "DELETE FROM sessions se USING devices d WHERE se.id = $1 AND se.staff_id = $2 AND se.phone AND d.id = se.device_id RETURNING d.id, d.name")
        .bind(sid).bind(id).fetch_optional(&s.pool).await?;
    let (device_id, name) = gone.ok_or(AppError::NotFound)?;
    auth::log(&s.pool, Some(a.staff.id), "staff.phone_signed_out", Some(("staff", id.to_string())), &a.device, &format!("{name} · {device_id}")).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct LogRow {
    id: i64, #[serde(with = "time::serde::rfc3339")] at: OffsetDateTime,
    staff_name: Option<String>, action: String, subject_type: Option<String>, subject_id: Option<String>, device: String, detail: String,
    /// For a patient subject: "0042 Ana Silva", so the owner sees whose file was opened (empty once deleted or anonymised).
    subject_label: Option<String>,
}

/// Filters for the access log (audit phase C). All optional; they combine with AND.
/// `from`/`to`: days (YYYY-MM-DD, clinic time zone, both included). `actions`: comma list, each an exact action
/// (`login.failed`) or a prefix ending in a dot (`patient.`). `q`: words in the person, patient, computer, action or detail.
/// `before`: id of the last row already shown, for "Show more".
#[derive(Deserialize, Default)]
pub struct LogQuery {
    subject_type: Option<String>, subject_id: Option<String>, limit: Option<i64>,
    from: Option<String>, to: Option<String>, staff_id: Option<Uuid>, actions: Option<String>, q: Option<String>, before: Option<i64>,
}

const LOG_SQL: &str =
    "SELECT l.id, l.at, st.name AS staff_name, l.action, l.subject_type, l.subject_id, l.device, l.detail,
            CASE WHEN p.id IS NOT NULL AND p.deleted_at IS NULL AND p.anonymised_at IS NULL THEN lpad(p.number::text, 4, '0') || ' ' || p.name
                 WHEN dv.id IS NOT NULL THEN dv.name || ' (' || dv.code || ')' END AS subject_label
     FROM access_log l
     LEFT JOIN staff st ON st.id = l.staff_id
     LEFT JOIN patients p ON l.subject_type = 'patient' AND p.id::text = l.subject_id
     LEFT JOIN devices dv ON l.subject_type = 'device' AND dv.id::text = l.subject_id
     WHERE ($1::text IS NULL OR l.subject_type = $1) AND ($2::text IS NULL OR l.subject_id = $2)
       AND ($3::date IS NULL OR l.at >= ($3::date)::timestamp AT TIME ZONE $11)
       AND ($4::date IS NULL OR l.at < ($4::date + 1)::timestamp AT TIME ZONE $11)
       AND ($5::uuid IS NULL OR l.staff_id = $5)
       AND (cardinality($6::text[]) = 0 OR l.action = ANY($6) OR l.action LIKE ANY($7))
       AND ($8::text IS NULL OR concat_ws(' ', st.name, p.name, lpad(p.number::text, 4, '0'), dv.name, l.device, l.action, l.detail) ILIKE $8)
       AND ($9::bigint IS NULL OR l.id < $9)
     ORDER BY l.id DESC LIMIT $10";

fn parse_day(v: &Option<String>) -> ApiResult<Option<String>> {
    match v.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        None => Ok(None),
        Some(d) if d.len() == 10 && time::Date::parse(d, &time::macros::format_description!("[year]-[month]-[day]")).is_ok() => Ok(Some(d.to_string())),
        Some(_) => Err(AppError::BadRequest("Choose a valid date.")),
    }
}

async fn query_log(s: &AppState, q: &LogQuery, limit: i64) -> ApiResult<Vec<LogRow>> {
    let (from, to) = (parse_day(&q.from)?, parse_day(&q.to)?);
    if let (Some(f), Some(t)) = (&from, &to) {
        if f > t { return Err(AppError::BadRequest("The first day must be before the last day.")); }
    }
    let (mut exact, mut prefix) = (Vec::<String>::new(), Vec::<String>::new());
    for a in q.actions.as_deref().unwrap_or("").split(',').map(str::trim).filter(|a| !a.is_empty()).take(40) {
        if !a.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c == '.') { return Err(AppError::BadRequest("Unknown kind of entry.")); }
        // A prefix also adds an empty exact entry, so the list isn't empty and the filter applies.
        if a.ends_with('.') { prefix.push(format!("{a}%")); exact.push(String::new()); } else { exact.push(a.to_string()); }
    }
    let words = q.q.as_deref().map(str::trim).filter(|w| !w.is_empty()).map(|w| {
        let esc: String = w.chars().take(80).flat_map(|c| if matches!(c, '%' | '_' | '\\') { vec!['\\', c] } else { vec![c] }).collect();
        format!("%{}%", esc.split_whitespace().collect::<Vec<_>>().join("%"))
    });
    Ok(sqlx::query_as(LOG_SQL)
        .bind(&q.subject_type).bind(&q.subject_id).bind(from).bind(to).bind(q.staff_id)
        .bind(&exact).bind(&prefix).bind(words).bind(q.before).bind(limit).bind(super::schedule::tz())
        .fetch_all(&s.pool).await?)
}

/// Owner only: the access log, newest first, with filters; filter by subject for a patient's own log.
pub async fn access_log(State(s): State<AppState>, a: Auth, Query(q): Query<LogQuery>) -> ApiResult<Json<Vec<LogRow>>> {
    a.require(&["owner"])?;
    let rows = query_log(&s, &q, q.limit.unwrap_or(200).clamp(1, 1000)).await?;
    Ok(Json(rows))
}

/// Owner only: the filtered access log as a CSV (semicolons, opens in Excel), saved to Downloads by the desktop app.
/// Contains patient names, so the export itself is logged as `access_log.exported`.
pub async fn access_log_export(State(s): State<AppState>, a: Auth, Query(q): Query<LogQuery>) -> ApiResult<Response> {
    a.require(&["owner"])?;
    const MAX: i64 = 100_000;
    let rows = query_log(&s, &q, MAX).await?;
    let (zone,): (String,) = sqlx::query_as("SELECT to_char(now() AT TIME ZONE $1, 'YYYY-MM-DD')").bind(super::schedule::tz()).fetch_one(&s.pool).await?;
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    // Local times from the database, so summer time is right.
    let locals: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, to_char(at AT TIME ZONE $2, 'YYYY-MM-DD HH24:MI:SS') FROM access_log WHERE id = ANY($1)")
        .bind(&ids).bind(super::schedule::tz()).fetch_all(&s.pool).await?;
    let local: std::collections::HashMap<i64, String> = locals.into_iter().collect();
    let out: Vec<Vec<String>> = rows.iter().map(|r| vec![
        local.get(&r.id).cloned().unwrap_or_default(),
        r.staff_name.clone().unwrap_or_default(),
        r.action.clone(),
        r.subject_label.clone().unwrap_or_else(|| match (&r.subject_type, &r.subject_id) { (Some(t), Some(i)) => format!("{t} {i}"), _ => String::new() }),
        r.device.clone(),
        r.detail.clone(),
    ]).collect();
    let body = crate::export::csv(&["Data e hora", "Quem", "Ação", "Utente ou assunto", "Computador", "Detalhe"], &out);
    // What was filtered (never the search words: they can be a patient's name).
    let mut filters = Vec::new();
    if let Some(f) = q.from.as_deref().filter(|x| !x.is_empty()) { filters.push(format!("from {f}")); }
    if let Some(t) = q.to.as_deref().filter(|x| !x.is_empty()) { filters.push(format!("to {t}")); }
    if let Some(k) = q.actions.as_deref().filter(|x| !x.is_empty()) { filters.push(format!("kinds {k}")); }
    if q.staff_id.is_some() { filters.push("one person".into()); }
    if q.q.as_deref().is_some_and(|x| !x.trim().is_empty()) { filters.push("search".into()); }
    if q.subject_id.is_some() { filters.push("one subject".into()); }
    let filters = filters.join("; ");
    auth::log(&s.pool, Some(a.staff.id), "access_log.exported", None, &a.device,
              &format!("{} rows{}", out.len(), if filters.is_empty() { String::new() } else { format!("; {filters}") })).await;
    let mut res = body.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv; charset=utf-8"));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"registo-acessos-{zone}.csv\"")) { h.insert(header::CONTENT_DISPOSITION, v); }
    Ok(res)
}
