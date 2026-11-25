//! Device approval (PhysioPulse plan, phase 3). The staff server faces the internet, so a password and a
//! 6-digit code are not enough: the device must also be approved by the owner.
//!
//! - The app's Rust core makes a random key on its first start and sends it as `X-Device-Key` with every
//!   request. The server keeps only its SHA-256 (`devices.key_hash`).
//! - After password + code on a device the server doesn't know, the device is saved as *pending* and the
//!   app gets a ticket (kept by the Rust core) instead of a session. The waiting screen shows a short code.
//! - The owner approves it from an approved device (Settings › Devices, or the alert on Today). The waiting
//!   app swaps its ticket for a session (`POST /auth/pending`), with no new password or code.
//! - If the owner has no approved device left: a printed recovery code on the waiting screen
//!   (`POST /auth/pending/recover`), or the server console (`physiopulse-server devices` / `approve CODE`).
//! - Approved once per device, for everyone who uses it (a shared reception PC is approved once).
//! - Removing a device ends its sessions at once and forgets its trusted-computer tokens.
//! - Computers trusted before phase 3 keep working: their first sign-in approves them (`carried_over`).

use super::account::{codes_left, new_recovery_codes, use_recovery_code};
use super::auth::device as device_name;
use crate::auth::{self, AppState, Auth, Staff};
use crate::error::{ApiResult, AppError};
use axum::{extract::{Path, State}, http::HeaderMap, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

/// How long the waiting app may swap its ticket for a session.
pub const PENDING_MINUTES: i32 = 30;

#[derive(sqlx::FromRow, Clone)]
pub struct Device {
    pub id: Uuid,
    pub status: String,
    pub code: String,
    pub name: String,
}

/// A short code like "K7Q-M2X" to match the waiting screen with the owner's list (not a secret).
fn new_code() -> String {
    let c = auth::new_invite_code();
    format!("{}-{}", &c[0..3], &c[5..8])
}

pub async fn find(s: &AppState, key: &str) -> ApiResult<Option<Device>> {
    Ok(sqlx::query_as("SELECT id, status, code, name FROM devices WHERE key_hash = $1").bind(auth::sha256(key)).fetch_optional(&s.pool).await?)
}

/// The device for this key, saved as pending if it's new. A device the owner removed stays removed.
/// `kind` is "phone" or "computer" (`auth::device_kind`); it is set here, when the device is first saved, and never after.
pub async fn find_or_request(s: &AppState, key: &str, name: &str, kind: &str, staff_id: Uuid, ip: &str) -> ApiResult<Device> {
    if let Some(d) = find(s, key).await? {
        return Ok(d);
    }
    let d: Device = sqlx::query_as(
        "INSERT INTO devices (id, key_hash, name, code, status, requested_by, last_ip, kind) VALUES ($1, $2, $3, $4, 'pending', $5, $6, $7)
         ON CONFLICT (key_hash) DO UPDATE SET key_hash = EXCLUDED.key_hash RETURNING id, status, code, name")
        .bind(Uuid::new_v4()).bind(auth::sha256(key)).bind(name).bind(new_code()).bind(staff_id).bind(ip).bind(kind)
        .fetch_one(&s.pool).await?;
    auth::log(&s.pool, Some(staff_id), "device.requested", Some(("device", d.id.to_string())), name, &format!("{} · {kind} · {ip}", d.code)).await;
    Ok(d)
}

/// Approves a device (new or pending) straight away: the first owner's device at setup, a computer trusted
/// before phase 3, the owner's recovery code, the server console.
pub async fn approve_now(s: &AppState, key: &str, name: &str, kind: &str, staff_id: Option<Uuid>, how: &str, ip: &str) -> ApiResult<Device> {
    let d: Device = sqlx::query_as(
        "INSERT INTO devices (id, key_hash, name, code, status, how, requested_by, approved_by, approved_at, last_ip, kind)
         VALUES ($1, $2, $3, $4, 'approved', $5, $6, $6, now(), $7, $8)
         ON CONFLICT (key_hash) DO UPDATE SET status = 'approved', how = EXCLUDED.how, approved_by = EXCLUDED.approved_by,
           approved_at = now(), revoked_at = NULL
         RETURNING id, status, code, name")
        .bind(Uuid::new_v4()).bind(auth::sha256(key)).bind(name).bind(new_code()).bind(how).bind(staff_id).bind(ip).bind(kind)
        .fetch_one(&s.pool).await?;
    auth::log(&s.pool, staff_id, "device.approved", Some(("device", d.id.to_string())), name, how).await;
    Ok(d)
}

/// The app is too old to send a device key (before 0.8.0): it must be updated.
pub fn key_or_update(h: &HeaderMap) -> ApiResult<String> {
    auth::device_key(h).ok_or(AppError::DeviceRefused("app_update", "Update the PhysioPulse app on this computer to sign in."))
}

pub fn removed() -> AppError {
    AppError::DeviceRefused("device_revoked", "The clinic owner removed this device. Ask them to approve it again in Settings › Devices.")
}

/// After password and code on a device that isn't approved yet: a ticket to wait with.
pub async fn pending_answer(s: &AppState, staff: &Staff, d: &Device, trust: bool, enrol: bool) -> ApiResult<Value> {
    let ticket = auth::new_token();
    sqlx::query("DELETE FROM pending_signins WHERE staff_id = $1 AND device_id = $2").bind(staff.id).bind(d.id).execute(&s.pool).await?;
    sqlx::query("INSERT INTO pending_signins (id, ticket_hash, staff_id, device_id, trust, enrol, expires_at)
                 VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(mins => $7))")
        .bind(Uuid::new_v4()).bind(auth::sha256(&ticket)).bind(staff.id).bind(d.id).bind(trust).bind(enrol).bind(PENDING_MINUTES)
        .execute(&s.pool).await?;
    Ok(json!({
        "next": "pending",
        "ticket": ticket,
        "device": { "code": d.code, "name": d.name },
        "owner": staff.role == "owner",
        "expires_in_minutes": PENDING_MINUTES,
    }))
}

/// Session for an approved device (+ trusted-computer token and an owner's first recovery codes).
pub async fn session_answer(s: &AppState, staff: &Staff, device_id: Uuid, dev_name: &str, trust: bool, enrol: bool) -> ApiResult<Value> {
    let token = auth::create_session(s, staff.id, dev_name, device_id).await?;
    let device_token = if trust {
        let t = auth::trust_device(s, staff.id, dev_name, device_id).await?;
        auth::log(&s.pool, Some(staff.id), "login.computer_trusted", None, dev_name, &format!("{} days", auth::TRUST_DAYS)).await;
        Some(t)
    } else { None };
    // An owner who has just set up two-step and has no recovery codes left gets ten (audit C4).
    let recovery_codes = if enrol && staff.role == "owner" && codes_left(s, staff.id).await? == 0 {
        let c = new_recovery_codes(s, staff.id).await?;
        auth::log(&s.pool, Some(staff.id), "account.recovery_codes_made", Some(("staff", staff.id.to_string())), dev_name, "setup").await;
        Some(c)
    } else { None };
    let staff = Staff { totp_enabled: true, ..staff.clone() };
    let mut v = json!({ "next": "session", "token": token, "staff": staff, "idle_minutes": s.idle_minutes });
    if let Some(t) = device_token { v["device_token"] = json!(t); }
    if let Some(c) = recovery_codes { v["recovery_codes"] = json!(c); }
    Ok(v)
}

// ───────── signed out: the waiting app ─────────

#[derive(Deserialize)]
pub struct TicketReq { ticket: String }

async fn ticket_row(s: &AppState, h: &HeaderMap, ticket: &str) -> ApiResult<(Uuid, Staff, Device, bool, bool)> {
    let key = format!("pending:{}", auth::client_ip(h));
    if auth::limited_n(s, &key, auth::MAX_FAILS_PER_ADDRESS).await? { return Err(AppError::TooMany); }
    let dkey = key_or_update(h)?;
    let row: Option<(Uuid, Uuid, Uuid, bool, bool)> = sqlx::query_as(
        "SELECT p.id, p.staff_id, p.device_id, p.trust, p.enrol FROM pending_signins p JOIN devices d ON d.id = p.device_id
         WHERE p.ticket_hash = $1 AND p.expires_at > now() AND d.key_hash = $2")
        .bind(auth::sha256(ticket)).bind(auth::sha256(&dkey)).fetch_optional(&s.pool).await?;
    let Some((pid, staff_id, device_id, trust, enrol)) = row else {
        auth::record_fail(s, &key).await;
        return Err(AppError::Unauthorized("This step expired. Sign in again."));
    };
    let staff: Staff = sqlx::query_as("SELECT * FROM staff WHERE id = $1 AND active").bind(staff_id).fetch_optional(&s.pool).await?
        .ok_or(AppError::Unauthorized("This account is no longer active."))?;
    let d: Device = sqlx::query_as("SELECT id, status, code, name FROM devices WHERE id = $1").bind(device_id).fetch_one(&s.pool).await?;
    Ok((pid, staff, d, trust, enrol))
}

/// The waiting screen asks every few seconds: still waiting, approved (→ session), or declined.
pub async fn pending(State(s): State<AppState>, h: HeaderMap, Json(r): Json<TicketReq>) -> ApiResult<Json<Value>> {
    let (pid, staff, d, trust, enrol) = ticket_row(&s, &h, &r.ticket).await?;
    match d.status.as_str() {
        "approved" => {
            sqlx::query("DELETE FROM pending_signins WHERE id = $1").bind(pid).execute(&s.pool).await?;
            auth::log(&s.pool, Some(staff.id), "login.ok", None, &device_name(&h), "device_approved").await;
            Ok(Json(session_answer(&s, &staff, d.id, &device_name(&h), trust, enrol).await?))
        }
        "revoked" => {
            sqlx::query("DELETE FROM pending_signins WHERE id = $1").bind(pid).execute(&s.pool).await?;
            Err(removed())
        }
        _ => Ok(Json(json!({ "next": "pending", "device": { "code": d.code, "name": d.name }, "owner": staff.role == "owner" }))),
    }
}

#[derive(Deserialize)]
pub struct RecoverReq { ticket: String, recovery_code: String }

/// The owner's way out when no approved device is left: one printed recovery code approves this device.
pub async fn pending_recover(State(s): State<AppState>, h: HeaderMap, Json(r): Json<RecoverReq>) -> ApiResult<Json<Value>> {
    let (pid, staff, d, trust, enrol) = ticket_row(&s, &h, &r.ticket).await?;
    if staff.role != "owner" {
        return Err(AppError::Forbidden);
    }
    let key = format!("recover:{}", staff.id);
    if auth::limited(&s, &key).await? { return Err(AppError::TooMany); }
    if !use_recovery_code(&s, staff.id, &r.recovery_code).await? {
        auth::record_fail(&s, &key).await;
        auth::log(&s.pool, Some(staff.id), "device.bad_recovery_code", Some(("device", d.id.to_string())), &device_name(&h), "").await;
        return Err(AppError::Unauthorized("That recovery code isn’t right, or it was already used."));
    }
    auth::clear_fails(&s, &key).await;
    sqlx::query("UPDATE devices SET status = 'approved', how = 'recovery_code', approved_by = $2, approved_at = now(), revoked_at = NULL WHERE id = $1")
        .bind(d.id).bind(staff.id).execute(&s.pool).await?;
    sqlx::query("DELETE FROM pending_signins WHERE id = $1").bind(pid).execute(&s.pool).await?;
    let left = codes_left(&s, staff.id).await?;
    auth::log(&s.pool, Some(staff.id), "device.approved", Some(("device", d.id.to_string())), &device_name(&h), &format!("recovery_code · {left} left")).await;
    auth::log(&s.pool, Some(staff.id), "login.ok", None, &device_name(&h), "device_approved").await;
    let mut v = session_answer(&s, &staff, d.id, &device_name(&h), trust, enrol).await?;
    v["codes_left"] = json!(left);
    Ok(Json(v))
}

// ───────── owner: the device list ─────────

/// Every device, pending first. Owner only.
pub async fn list(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(x ORDER BY x.rank, x.last_seen DESC NULLS LAST, x.created_at DESC), '[]') FROM (
           SELECT d.id, d.name, d.code, d.status, d.how, d.kind, d.created_at, d.approved_at, d.revoked_at, d.last_seen, d.last_ip,
                  rq.name AS requested_by_name, rq.role AS requested_by_role, ap.name AS approved_by_name,
                  (d.id = $1) AS this_device,
                  (SELECT count(*) FROM sessions se WHERE se.device_id = d.id) AS sessions,
                  (SELECT COALESCE(json_agg(DISTINCT st.name), '[]') FROM sessions se JOIN staff st ON st.id = se.staff_id WHERE se.device_id = d.id) AS signed_in,
                  CASE d.status WHEN 'pending' THEN 0 WHEN 'approved' THEN 1 ELSE 2 END AS rank
           FROM devices d LEFT JOIN staff rq ON rq.id = d.requested_by LEFT JOIN staff ap ON ap.id = d.approved_by
           WHERE d.status <> 'revoked' OR d.revoked_at > now() - interval '90 days') x")
        .bind(a.device_id).fetch_one(&s.pool).await?;
    Ok(Json(v))
}

/// How many devices wait for approval (owner's alert on Today; null for everyone else).
pub async fn alert(s: &AppState, a: &Auth) -> ApiResult<Value> {
    if a.staff.role != "owner" {
        return Ok(Value::Null);
    }
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('id', d.id, 'name', d.name, 'code', d.code, 'kind', d.kind, 'requested_by_name', st.name) ORDER BY d.created_at), '[]')
         FROM devices d LEFT JOIN staff st ON st.id = d.requested_by WHERE d.status = 'pending'")
        .fetch_one(&s.pool).await?;
    Ok(v)
}

async fn one(s: &AppState, id: Uuid) -> ApiResult<Device> {
    sqlx::query_as("SELECT id, status, code, name FROM devices WHERE id = $1").bind(id).fetch_optional(&s.pool).await?.ok_or(AppError::NotFound)
}

/// Owner approves a waiting device (or one removed earlier). Its waiting sign-in goes through by itself.
pub async fn approve(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let d = one(&s, id).await?;
    if d.status == "approved" {
        return Ok(Json(json!({ "ok": true, "status": "approved" })));
    }
    sqlx::query("UPDATE devices SET status = 'approved', how = 'owner', approved_by = $2, approved_at = now(), revoked_at = NULL WHERE id = $1")
        .bind(id).bind(a.staff.id).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "device.approved", Some(("device", id.to_string())), &a.device, &format!("{} · {}", d.code, d.name)).await;
    Ok(Json(json!({ "ok": true, "status": "approved" })))
}

/// Owner removes a device (or declines a waiting one): its sessions end now, its trusted-computer tokens are
/// forgotten, and the next sign-in from it is refused until the owner approves it again.
pub async fn revoke(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    if id == a.device_id {
        return Err(AppError::BadRequest("You’re using this device. Remove it from another approved device."));
    }
    let d = one(&s, id).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE devices SET status = 'revoked', revoked_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
    let ended = sqlx::query("DELETE FROM sessions WHERE device_id = $1").bind(id).execute(&mut *tx).await?.rows_affected();
    sqlx::query("DELETE FROM trusted_devices WHERE device_id = $1").bind(id).execute(&mut *tx).await?;
    // Waiting sign-ins are kept until they expire, so the waiting screen can say "declined" instead of "expired".
    tx.commit().await?;
    let action = if d.status == "pending" { "device.declined" } else { "device.revoked" };
    auth::log(&s.pool, Some(a.staff.id), action, Some(("device", id.to_string())), &a.device, &format!("{} · {} · {ended} sessions ended", d.code, d.name)).await;
    Ok(Json(json!({ "ok": true, "status": "revoked", "sessions_ended": ended })))
}

#[derive(Deserialize)]
pub struct RenameReq { name: String }

/// Owner gives a device a clearer name ("PC da receção").
pub async fn rename(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(r): Json<RenameReq>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let name = r.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > 60 {
        return Err(AppError::BadRequest("Give the device a name (up to 60 characters)."));
    }
    one(&s, id).await?;
    sqlx::query("UPDATE devices SET name = $2 WHERE id = $1").bind(id).bind(&name).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "device.renamed", Some(("device", id.to_string())), &a.device, "").await;
    Ok(Json(json!({ "ok": true, "name": name })))
}

// ───────── server console ─────────

/// `physiopulse-server devices` and `physiopulse-server approve CODE`, for when no approved owner device is
/// left and the recovery codes are lost: run inside the server container on ZimaOS
/// (`docker compose exec server physiopulse-server approve K7Q-M2X`). Returns the exit code.
pub async fn console(pool: &sqlx::PgPool, args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("devices") => {
            let rows: Vec<(String, String, String, Option<String>, String)> = sqlx::query_as(
                "SELECT d.code, d.status, d.name, st.name, to_char(d.created_at, 'YYYY-MM-DD HH24:MI') FROM devices d
                 LEFT JOIN staff st ON st.id = d.requested_by WHERE d.status <> 'revoked' ORDER BY d.status DESC, d.created_at")
                .fetch_all(pool).await.unwrap_or_default();
            if rows.is_empty() { println!("No devices yet."); }
            for (code, status, name, who, at) in rows {
                println!("{code}  {status:<9} {name}  ({}; {at})", who.unwrap_or_default());
            }
            0
        }
        Some("approve") => {
            let Some(code) = args.get(1).map(|c| c.trim().to_uppercase()) else {
                eprintln!("Usage: physiopulse-server approve CODE   (the code on the waiting screen, e.g. K7Q-M2X)");
                return 2;
            };
            let r: Option<(Uuid, String)> = sqlx::query_as(
                "UPDATE devices SET status = 'approved', how = 'console', approved_at = now(), revoked_at = NULL
                 WHERE code = $1 AND status = 'pending' RETURNING id, name")
                .bind(&code).fetch_optional(pool).await.ok().flatten();
            match r {
                Some((id, name)) => {
                    auth::log(pool, None, "device.approved", Some(("device", id.to_string())), "server console", &format!("console · {code} · {name}")).await;
                    println!("Approved {code} ({name}). The waiting app signs in by itself within a few seconds.");
                    0
                }
                None => {
                    eprintln!("No device is waiting with the code {code}. Run `physiopulse-server devices` to see them.");
                    1
                }
            }
        }
        _ => {
            eprintln!("Commands: devices | approve CODE");
            2
        }
    }
}
