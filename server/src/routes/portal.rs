//! Phase 4b (23 Nov 2026): patient accounts on the website.
//!
//! Two parts:
//! * **Staff** (in the patient's file): see who has access, make an invite code, make a password-reset code
//!   (reception, the patient's therapist or the owner), take access away.
//! * **Internal API for the website** (`/internal/portal/…`): the website signs patients in (migration 0024) and
//!   then asks here for what a signed-in patient may see or do. Every call carries the website's key (derived from
//!   its database password, `db::portal_internal_key`) and the patient's session cookie; answers cover only the
//!   patients linked to that account, and never clinical notes or tests.

use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use rand::Rng;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use uuid::Uuid;

use super::schedule::tz;

/// The website's key for the internal API (None = the website has no database login, so no internal API).
pub static INTERNAL_KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();

/// Codes: 12 letters and digits without look-alikes (no 0/O, 1/I/L), shown as XXXX-XXXX-XXXX.
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
fn new_code() -> String {
    let mut r = rand::thread_rng();
    let raw: String = (0..12).map(|_| ALPHABET[r.gen_range(0..ALPHABET.len())] as char).collect();
    format!("{}-{}-{}", &raw[..4], &raw[4..8], &raw[8..])
}
/// Same as `portal.code_hash()` in the database: capitals, letters and digits only, SHA-256 in hex.
pub fn code_hash(code: &str) -> String {
    let norm: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase();
    hex::encode(Sha256::digest(norm.as_bytes()))
}

struct Pat { therapist: Option<Uuid>, age: Option<i32>, gone: bool }
async fn patient(s: &AppState, id: Uuid) -> ApiResult<Pat> {
    let row: Option<(Option<Uuid>, Option<i32>, bool)> = sqlx::query_as(
        "SELECT therapist_id, date_part('year', age(birth_date))::int, anonymised_at IS NOT NULL FROM patients WHERE id = $1 AND deleted_at IS NULL")
        .bind(id).fetch_optional(&s.pool).await?;
    let (therapist, age, gone) = row.ok_or(AppError::NotFound)?;
    Ok(Pat { therapist, age, gone })
}

/// Reset codes and taking access away: reception, the patient's own therapist, or the owner (owner, 23 Nov 2026).
fn may_manage(a: &Auth, p: &Pat) -> ApiResult<()> {
    match a.staff.role.as_str() {
        "owner" | "reception" => Ok(()),
        "therapist" if p.therapist == Some(a.staff.id) => Ok(()),
        _ => Err(AppError::BadRequest("Only reception, the patient’s therapist or the owner can do this.")),
    }
}

/// Everyone: who can see this patient on the website, and invites not used yet.
pub async fn get(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let p = patient(&s, id).await?;
    let (v,): (Value,) = sqlx::query_as(
        "SELECT json_build_object(
            'accounts', (SELECT COALESCE(json_agg(json_build_object('id', a.id, 'email', a.email, 'relation', l.relation, 'two_step', a.totp_secret IS NOT NULL,
                            'last_login_at', a.last_login_at, 'created_at', a.created_at) ORDER BY l.created_at), '[]')
                         FROM patient_account_links l JOIN patient_accounts a ON a.id = l.account_id WHERE l.patient_id = $1),
            'invites', (SELECT COALESCE(json_agg(json_build_object('id', c.id, 'relation', c.relation, 'created_at', c.created_at, 'expires_at', c.expires_at,
                            'created_by_name', st.name) ORDER BY c.created_at DESC), '[]')
                        FROM patient_codes c LEFT JOIN staff st ON st.id = c.created_by
                        WHERE c.patient_id = $1 AND c.kind = 'invite' AND c.used_at IS NULL AND c.expires_at > now()),
            'open', (SELECT published AND accounts_open FROM website WHERE id = 1),
            'clinic', (SELECT name FROM clinic WHERE id = 1),
            'patient', (SELECT json_build_object('name', name, 'phone', phone, 'email', email, 'lang', lang) FROM patients WHERE id = $1))")
        .bind(id).fetch_one(&s.pool).await?;
    let mut v = v;
    v["minor"] = json!(p.age.is_some_and(|x| x < 16));
    v["can_manage"] = json!(may_manage(&a, &p).is_ok());
    v["site_url"] = json!(super::website::site_url());
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct InviteReq { #[serde(default)] relation: String }

/// Everyone: an invite code for this patient (or for a parent or guardian), valid 14 days. Shown once.
pub async fn invite(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<InviteReq>) -> ApiResult<Json<Value>> {
    let p = patient(&s, id).await?;
    if p.gone {
        return Err(AppError::BadRequest("This file was anonymised: it can’t take new contacts, documents or appointments."));
    }
    let relation = if b.relation == "guardian" { "guardian" } else { "self" };
    // Under-16s have no login of their own (owner, 23 Nov 2026).
    if relation == "self" && p.age.is_some_and(|x| x < 16) {
        return Err(AppError::BadRequest("Under 16s see their sessions through a parent’s or guardian’s account. Make a guardian invite."));
    }
    let code = new_code();
    let cid = Uuid::new_v4();
    let (expires,): (time::OffsetDateTime,) = sqlx::query_as(
        "INSERT INTO patient_codes (id, kind, code_hash, patient_id, relation, created_by, expires_at) VALUES ($1, 'invite', $2, $3, $4, $5, now() + interval '14 days') RETURNING expires_at")
        .bind(cid).bind(code_hash(&code)).bind(id).bind(relation).bind(a.staff.id).fetch_one(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "portal.invite_created", Some(("patient", id.to_string())), &a.device, relation).await;
    Ok(Json(json!({ "id": cid, "code": code, "relation": relation, "expires_at": expires })))
}

/// Everyone: withdraw an invite that wasn't used yet.
pub async fn drop_invite(State(s): State<AppState>, a: Auth, Path((id, cid)): Path<(Uuid, Uuid)>) -> ApiResult<Json<Value>> {
    let n = sqlx::query("DELETE FROM patient_codes WHERE id = $1 AND patient_id = $2 AND kind = 'invite' AND used_at IS NULL")
        .bind(cid).bind(id).execute(&s.pool).await?.rows_affected();
    if n == 0 { return Err(AppError::NotFound); }
    auth::log(&s.pool, Some(a.staff.id), "portal.invite_withdrawn", Some(("patient", id.to_string())), &a.device, "").await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ResetReq { account_id: Uuid }

/// Reception, the patient's therapist or the owner: a one-time code to choose a new password, valid 24 hours.
/// Using it also turns two-step codes off (whoever made it knew who they were talking to).
pub async fn reset(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<ResetReq>) -> ApiResult<Json<Value>> {
    let p = patient(&s, id).await?;
    may_manage(&a, &p)?;
    let linked: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM patient_account_links WHERE account_id = $1 AND patient_id = $2)")
        .bind(b.account_id).bind(id).fetch_one(&s.pool).await?;
    if !linked {
        return Err(AppError::BadRequest("That account isn’t linked to this patient."));
    }
    let code = new_code();
    // An older reset code for the same account stops working.
    sqlx::query("DELETE FROM patient_codes WHERE kind = 'reset' AND account_id = $1 AND used_at IS NULL").bind(b.account_id).execute(&s.pool).await?;
    let (expires,): (time::OffsetDateTime,) = sqlx::query_as(
        "INSERT INTO patient_codes (id, kind, code_hash, patient_id, account_id, created_by, expires_at) VALUES ($1, 'reset', $2, $3, $4, $5, now() + interval '24 hours') RETURNING expires_at")
        .bind(Uuid::new_v4()).bind(code_hash(&code)).bind(id).bind(b.account_id).bind(a.staff.id).fetch_one(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "portal.reset_created", Some(("patient", id.to_string())), &a.device, "").await;
    Ok(Json(json!({ "code": code, "expires_at": expires })))
}

/// Reception, the patient's therapist or the owner: this account no longer sees this patient. An account left with
/// nobody is removed, and its sessions end.
pub async fn unlink(State(s): State<AppState>, a: Auth, Path((id, account)): Path<(Uuid, Uuid)>) -> ApiResult<Json<Value>> {
    let p = patient(&s, id).await?;
    may_manage(&a, &p)?;
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query("DELETE FROM patient_account_links WHERE account_id = $1 AND patient_id = $2").bind(account).bind(id).execute(&mut *tx).await?.rows_affected();
    if n == 0 { return Err(AppError::NotFound); }
    sqlx::query("DELETE FROM patient_codes WHERE kind = 'reset' AND account_id = $1 AND patient_id = $2").bind(account).bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM patient_accounts a WHERE id = $1 AND NOT EXISTS (SELECT 1 FROM patient_account_links WHERE account_id = $1)").bind(account).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM patient_sessions WHERE account_id = $1").bind(account).execute(&mut *tx).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "portal.access_removed", Some(("patient", id.to_string())), &a.device, "").await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── internal API for the website ─────────

/// The website's call: its key matches, and the cookie is a live, fully signed-in session. Returns the account.
async fn caller(s: &AppState, h: &HeaderMap) -> ApiResult<Uuid> {
    let Some(Some(key)) = INTERNAL_KEY.get() else { return Err(AppError::NotFound) };
    let given = h.get("x-portal-key").and_then(|v| v.to_str().ok()).and_then(|v| hex::decode(v.trim()).ok()).unwrap_or_default();
    // Constant-time compare.
    if given.len() != 32 || given.iter().zip(key.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) != 0 {
        return Err(AppError::NotFound);
    }
    let token = h.get("x-portal-session").and_then(|v| v.to_str().ok()).unwrap_or("").trim();
    if token.len() < 32 {
        return Err(AppError::Unauthorized("Your session ended. Sign in again."));
    }
    let th = hex::encode(Sha256::digest(token.as_bytes()));
    let acc: Option<Uuid> = sqlx::query_scalar(
        "SELECT s.account_id FROM patient_sessions s, website w
         WHERE s.token_hash = $1 AND NOT s.pending_2fa AND s.expires_at > now() AND s.last_seen > now() - interval '30 minutes'
           AND w.id = 1 AND w.published AND w.accounts_open")
        .bind(th).fetch_optional(&s.pool).await?;
    acc.ok_or(AppError::Unauthorized("Your session ended. Sign in again."))
}

/// A patient linked to the account, or NotFound.
async fn linked(s: &AppState, account: Uuid, patient: Uuid) -> ApiResult<()> {
    let ok: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM patient_account_links l JOIN patients p ON p.id = l.patient_id
                                         WHERE l.account_id = $1 AND l.patient_id = $2 AND p.deleted_at IS NULL AND p.anonymised_at IS NULL)")
        .bind(account).bind(patient).fetch_one(&s.pool).await?;
    if ok { Ok(()) } else { Err(AppError::NotFound) }
}

/// Everything the patient area shows: the people on this account, their sessions (the last 12 months and what's
/// booked), payments with an invoice, and what is owed. No notes, tests, prescriptions or documents.
pub async fn me(State(s): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let account = caller(&s, &h).await?;
    let (v,): (Value,) = sqlx::query_as(
        "WITH ppl AS (SELECT p.id, p.name, l.relation FROM patient_account_links l JOIN patients p ON p.id = l.patient_id
                      WHERE l.account_id = $1 AND p.deleted_at IS NULL AND p.anonymised_at IS NULL)
         SELECT json_build_object(
           'email', (SELECT email FROM patient_accounts WHERE id = $1),
           'people', (SELECT COALESCE(json_agg(json_build_object('id', id, 'name', name, 'relation', relation) ORDER BY relation DESC, name), '[]') FROM ppl),
           'appointments', (SELECT COALESCE(json_agg(json_build_object('id', ap.id, 'patient_id', ap.patient_id, 'kind', ap.kind,
                    'starts_at', ap.starts_at, 'ends_at', ap.ends_at, 'status', ap.status, 'therapist', split_part(COALESCE(st.name, ''), ' ', 1),
                    'can_cancel', ap.status = 'booked' AND ap.kind IN ('session', 'assessment', 'home') AND ap.starts_at > now() + interval '24 hours',
                    'can_move', ap.status = 'booked' AND ap.kind IN ('session', 'assessment', 'home') AND ap.starts_at > now(),
                    'move_asked', EXISTS (SELECT 1 FROM booking_requests b WHERE b.about_appointment_id = ap.id AND b.kind = 'move' AND b.status = 'pending'))
                    ORDER BY ap.starts_at), '[]')
                FROM appointments ap JOIN ppl ON ppl.id = ap.patient_id LEFT JOIN staff st ON st.id = ap.therapist_id
                WHERE ap.kind <> 'block' AND ap.starts_at > now() - interval '12 months'),
           'payments', (SELECT COALESCE(json_agg(json_build_object('id', py.id, 'patient_id', py.patient_id, 'received_on', py.received_on,
                    'amount_cents', py.amount_cents, 'description', py.description, 'invoice_number', py.invoice_number,
                    'has_invoice', py.invoice_file_id IS NOT NULL) ORDER BY py.received_on DESC, py.number DESC), '[]')
                FROM payments py JOIN ppl ON ppl.id = py.patient_id WHERE py.voided_at IS NULL),
           'requests', (SELECT COALESCE(json_agg(json_build_object('id', b.id, 'patient_id', b.patient_id, 'kind', b.kind, 'service', b.service,
                    'preferred_at', b.preferred_at, 'day_part', b.day_part, 'status', b.status, 'created_at', b.created_at) ORDER BY b.created_at DESC), '[]')
                FROM booking_requests b JOIN ppl ON ppl.id = b.patient_id WHERE b.source = 'portal' AND b.kind <> 'cancelled' AND b.created_at > now() - interval '60 days'))")
        .bind(account).fetch_one(&s.pool).await?;
    let mut v = v;
    // What is owed, per person, from the same calculation the clinic uses.
    let mut owed = Vec::new();
    for p in v["people"].as_array().cloned().unwrap_or_default() {
        if let Some(pid) = p["id"].as_str().and_then(|x| Uuid::parse_str(x).ok()) {
            for item in super::money::owed(&s, Some(pid)).await?.as_array().cloned().unwrap_or_default() {
                owed.push(json!({ "patient_id": pid, "day": item["day"], "what": item["what"], "name_pt": item["name_pt"], "name_en": item["name_en"],
                                  "month": item["month"], "due_cents": item["due_cents"] }));
            }
        }
    }
    v["owed"] = json!(owed);
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct CancelReq { appointment_id: Uuid }

/// A patient cancels a session up to 24 hours before it. Reception and the therapist see it in Booking Requests.
pub async fn cancel(State(s): State<AppState>, h: HeaderMap, Json(b): Json<CancelReq>) -> ApiResult<Json<Value>> {
    let account = caller(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    let row: Option<(Uuid, String, time::OffsetDateTime, bool)> = sqlx::query_as(
        "SELECT patient_id, kind, starts_at, (status = 'booked' AND starts_at > now() + interval '24 hours') FROM appointments WHERE id = $1 FOR UPDATE")
        .bind(b.appointment_id).fetch_optional(&mut *tx).await?;
    let Some((pid, kind, starts, ok)) = row else { return Err(AppError::NotFound) };
    linked(&s, account, pid).await?;
    if !ok || !["session", "assessment", "home"].contains(&kind.as_str()) {
        return Err(AppError::Conflict("Less than 24 hours to go: call the clinic to cancel."));
    }
    sqlx::query("UPDATE appointments SET status = 'cancelled' WHERE id = $1").bind(b.appointment_id).execute(&mut *tx).await?;
    super::money::on_status(&mut tx, &[b.appointment_id], "cancelled").await?;
    sqlx::query("INSERT INTO booking_requests (id, name, phone, email, patient_id, service, preferred_at, source, kind, about_appointment_id)
                 SELECT $1, p.name, COALESCE(p.phone, ''), COALESCE(p.email, ''), p.id, $3, $4, 'portal', 'cancelled', $5 FROM patients p WHERE p.id = $2")
        .bind(Uuid::new_v4()).bind(pid).bind(&kind).bind(starts).bind(b.appointment_id).execute(&mut *tx).await?;
    tx.commit().await?;
    auth::log(&s.pool, None, "appointment.cancelled", Some(("patient", pid.to_string())), "site", "by the patient on the website").await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct AskReq {
    patient_id: Uuid,
    /// "book" (a new appointment) or "move" (another time for `appointment_id`).
    kind: String,
    appointment_id: Option<Uuid>,
    #[serde(default)] service: String,
    day: String,
    part: String,
    #[serde(default)] message: String,
}

/// A request from a signed-in patient: a new appointment, or another time for a booked one.
pub async fn ask(State(s): State<AppState>, h: HeaderMap, Json(b): Json<AskReq>) -> ApiResult<Json<Value>> {
    let account = caller(&s, &h).await?;
    linked(&s, account, b.patient_id).await?;
    if !["morning", "afternoon", "any"].contains(&b.part.as_str()) || b.message.chars().count() > 1000 {
        return Err(AppError::BadRequest("Some details aren’t right. Check them and send again."));
    }
    let mut service = b.service.clone();
    let mut about = None;
    match b.kind.as_str() {
        "book" => {
            if !["assessment", "session", "home"].contains(&service.as_str()) {
                return Err(AppError::BadRequest("Some details aren’t right. Check them and send again."));
            }
        }
        "move" => {
            let ap = b.appointment_id.ok_or(AppError::NotFound)?;
            let row: Option<(Uuid, String)> = sqlx::query_as("SELECT patient_id, kind FROM appointments WHERE id = $1 AND status = 'booked' AND starts_at > now() AND kind IN ('session', 'assessment', 'home')")
                .bind(ap).fetch_optional(&s.pool).await?;
            let (pid, kind) = row.ok_or(AppError::NotFound)?;
            if pid != b.patient_id { return Err(AppError::NotFound); }
            let asked: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM booking_requests WHERE about_appointment_id = $1 AND kind = 'move' AND status = 'pending')")
                .bind(ap).fetch_one(&s.pool).await?;
            if asked { return Err(AppError::Conflict("You already asked to move this session. The clinic will get back to you.")); }
            service = kind;
            about = Some(ap);
        }
        _ => return Err(AppError::BadRequest("Some details aren’t right. Check them and send again.")),
    }
    // Same window as the public form: from tomorrow to two months ahead; the time of day becomes 09:00, 14:00 or 10:00.
    let ok: bool = sqlx::query_scalar("SELECT $1::date > (now() AT TIME ZONE $2)::date AND $1::date <= (now() AT TIME ZONE $2)::date + 62")
        .bind(&b.day).bind(tz()).fetch_one(&s.pool).await.unwrap_or(false);
    if !ok {
        return Err(AppError::BadRequest("Choose a day from tomorrow and within the next two months."));
    }
    // A ceiling per account, whatever the website does: 10 requests a day.
    let today: i64 = sqlx::query_scalar("SELECT count(*) FROM booking_requests b JOIN patient_account_links l ON l.patient_id = b.patient_id
                                         WHERE l.account_id = $1 AND b.source = 'portal' AND b.created_at > now() - interval '1 day'")
        .bind(account).fetch_one(&s.pool).await?;
    if today >= 10 { return Err(AppError::TooMany); }
    let id = Uuid::new_v4();
    let lang: String = sqlx::query_scalar("SELECT lang FROM patient_accounts WHERE id = $1").bind(account).fetch_one(&s.pool).await?;
    sqlx::query("INSERT INTO booking_requests (id, name, phone, email, patient_id, service, preferred_at, message, source, day_part, lang, kind, about_appointment_id, address)
                 SELECT $1, p.name, COALESCE(p.phone, ''), COALESCE(p.email, ''), p.id, $3,
                        ($4::date + CASE $5 WHEN 'morning' THEN time '09:00' WHEN 'afternoon' THEN time '14:00' ELSE time '10:00' END) AT TIME ZONE $10,
                        $6, 'portal', $5, $7, $8, $9, CASE WHEN $3 = 'home' THEN COALESCE(p.address, '') ELSE '' END
                 FROM patients p WHERE p.id = $2")
        .bind(id).bind(b.patient_id).bind(&service).bind(&b.day).bind(&b.part).bind(b.message.trim()).bind(&lang).bind(&b.kind).bind(about).bind(tz())
        .execute(&s.pool).await?;
    auth::log(&s.pool, None, "booking_request.received", Some(("patient", b.patient_id.to_string())), "site", &b.kind).await;
    Ok(Json(json!({ "id": id })))
}

/// The invoice PDF of a payment, for a patient on this account.
pub async fn invoice(State(s): State<AppState>, h: HeaderMap, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let account = caller(&s, &h).await?;
    let row: Option<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT py.patient_id, f.filename, f.mime, f.path FROM payments py JOIN finance_files f ON f.id = py.invoice_file_id
         WHERE py.id = $1 AND py.voided_at IS NULL AND py.patient_id IS NOT NULL")
        .bind(id).fetch_optional(&s.pool).await?;
    let (pid, filename, mime, path) = row.ok_or(AppError::NotFound)?;
    linked(&s, account, pid).await?;
    if path.contains("..") { return Err(AppError::NotFound); }
    let bytes = tokio::fs::read(s.archive.join(&path)).await.map_err(|_| AppError::NotFound)?;
    auth::log(&s.pool, None, "portal.invoice_downloaded", Some(("patient", pid.to_string())), "site", &id.to_string()).await;
    let safe: String = filename.chars().map(|c| if c.is_ascii_alphanumeric() || ".-_".contains(c) { c } else { '_' }).collect();
    let mut r = Response::new(Body::from(bytes));
    *r.status_mut() = StatusCode::OK;
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    r.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_str(&format!("attachment; filename=\"{safe}\"")).unwrap_or(HeaderValue::from_static("attachment")));
    Ok(r.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codes() {
        let c = new_code();
        assert_eq!(c.len(), 14);
        assert!(c.chars().filter(|c| *c != '-').all(|c| ALPHABET.contains(&(c as u8))));
        // Typed in lower case, without dashes, with spaces: still the same code.
        assert_eq!(code_hash(&c), code_hash(&c.to_lowercase().replace('-', " ")));
        assert_ne!(code_hash("ABCD-EFGH-JKMN"), code_hash("ABCD-EFGH-JKMP"));
    }
}
