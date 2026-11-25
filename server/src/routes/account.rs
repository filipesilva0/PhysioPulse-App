//! Account lifecycle (audit C4, 10 Nov 2026; owner's choice: "owner resets + recovery codes").
//!
//! - Everyone: change their own name and password.
//! - Owner: ten printed recovery codes (made when two-step is first set up, or again from Settings).
//!   One code replaces a lost phone at sign-in (two-step is set up again on the new phone) or resets a
//!   forgotten password. Each works once.
//! - Owner, for someone else: a one-time password reset code (24 hours) and "set up two-step again".
//! - Owner: list and withdraw invites that haven't been used.
//!
//! Every step is in the access log. Codes are stored only as SHA-256 hashes.

use super::auth::{clean_email, device, start_enrol, Enrol};
use crate::auth::{self, AppState, Auth, Staff, MIN_PASSWORD};
use crate::error::{ApiResult, AppError};
use axum::{extract::{Path, State}, http::HeaderMap, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub const RECOVERY_CODES: usize = 10;
pub const RESET_HOURS: i32 = 24;

/// "k7qm 2xrp-9tda" → "K7QM-2XRP-9TDA", so codes typed with spaces, dashes or in lower case still match.
pub fn norm_code(c: &str) -> String {
    let s: String = c.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect();
    if s.len() == 12 { format!("{}-{}-{}", &s[0..4], &s[4..8], &s[8..12]) } else { s }
}

fn check_new_password(pw: &str) -> ApiResult<()> {
    if pw.chars().count() < MIN_PASSWORD {
        return Err(AppError::BadRequest("Choose a password with at least 12 characters."));
    }
    if pw.chars().count() > 200 {
        return Err(AppError::BadRequest("That password is too long."));
    }
    Ok(())
}

/// Replaces this person's recovery codes with ten new ones and returns them (shown once).
pub async fn new_recovery_codes(s: &AppState, staff_id: Uuid) -> ApiResult<Vec<String>> {
    let codes: Vec<String> = (0..RECOVERY_CODES).map(|_| auth::new_invite_code()).collect();
    let mut tx = s.pool.begin().await?;
    sqlx::query("DELETE FROM recovery_codes WHERE staff_id = $1").bind(staff_id).execute(&mut *tx).await?;
    for c in &codes {
        sqlx::query("INSERT INTO recovery_codes (id, staff_id, code_hash) VALUES ($1, $2, $3)")
            .bind(Uuid::new_v4()).bind(staff_id).bind(auth::sha256(c)).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(codes)
}

pub async fn codes_left(s: &AppState, staff_id: Uuid) -> ApiResult<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM recovery_codes WHERE staff_id = $1 AND used_at IS NULL").bind(staff_id).fetch_one(&s.pool).await?;
    Ok(n)
}

/// Uses one of this person's recovery codes. True if it was valid and unused.
pub(crate) async fn use_recovery_code(s: &AppState, staff_id: Uuid, code: &str) -> ApiResult<bool> {
    let r: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE recovery_codes SET used_at = now() WHERE staff_id = $1 AND code_hash = $2 AND used_at IS NULL RETURNING id")
        .bind(staff_id).bind(auth::sha256(&norm_code(code))).fetch_optional(&s.pool).await?;
    Ok(r.is_some())
}

// ───────── everyone: their own account ─────────

#[derive(Deserialize)]
pub struct ProfileReq { name: String }

/// Change your own name (how it appears in the schedule, notes and documents).
pub async fn update_me(State(s): State<AppState>, a: Auth, Json(r): Json<ProfileReq>) -> ApiResult<Json<Staff>> {
    let name = r.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::BadRequest("Write your name."));
    }
    let staff: Staff = sqlx::query_as("UPDATE staff SET name = $2 WHERE id = $1 RETURNING *").bind(a.staff.id).bind(&name).fetch_one(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "account.name_changed", Some(("staff", a.staff.id.to_string())), &a.device, "").await;
    Ok(Json(staff))
}

#[derive(Deserialize)]
pub struct PasswordReq { current: String, new: String }

/// Change your own password. Signs you out of every other computer.
pub async fn change_password(State(s): State<AppState>, a: Auth, Json(r): Json<PasswordReq>) -> ApiResult<Json<Value>> {
    let key = format!("password:{}", a.staff.id);
    if auth::limited(&s, &key).await? { return Err(AppError::TooMany); }
    if !auth::verify_password(&r.current, &a.staff.password_hash) {
        auth::record_fail(&s, &key).await;
        auth::log(&s.pool, Some(a.staff.id), "account.password_change_failed", None, &a.device, "").await;
        return Err(AppError::BadRequest("Your current password isn’t right."));
    }
    check_new_password(&r.new)?;
    if r.new == r.current {
        return Err(AppError::BadRequest("Choose a password that’s different from the current one."));
    }
    auth::clear_fails(&s, &key).await;
    sqlx::query("UPDATE staff SET password_hash = $2 WHERE id = $1").bind(a.staff.id).bind(auth::hash_password(&r.new)?).execute(&s.pool).await?;
    let n = sqlx::query("DELETE FROM sessions WHERE staff_id = $1 AND id <> $2").bind(a.staff.id).bind(a.session_id).execute(&s.pool).await?.rows_affected();
    auth::log(&s.pool, Some(a.staff.id), "account.password_changed", Some(("staff", a.staff.id.to_string())), &a.device, &n.to_string()).await;
    Ok(Json(json!({ "ok": true, "other_sessions_ended": n })))
}

/// Owner: how many recovery codes are left.
pub async fn recovery_status(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let (left, total, made): (i64, i64, Option<time::OffsetDateTime>) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE used_at IS NULL), count(*), max(created_at) FROM recovery_codes WHERE staff_id = $1")
        .bind(a.staff.id).fetch_one(&s.pool).await?;
    Ok(Json(json!({ "left": left, "total": total, "created_at": made.and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok()) })))
}

#[derive(Deserialize)]
pub struct ConfirmReq { password: String }

/// Owner: new recovery codes (the old ones stop working). Asks for the password again.
pub async fn make_recovery_codes(State(s): State<AppState>, a: Auth, Json(r): Json<ConfirmReq>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let key = format!("password:{}", a.staff.id);
    if auth::limited(&s, &key).await? { return Err(AppError::TooMany); }
    if !auth::verify_password(&r.password, &a.staff.password_hash) {
        auth::record_fail(&s, &key).await;
        return Err(AppError::BadRequest("Your password isn’t right."));
    }
    let codes = new_recovery_codes(&s, a.staff.id).await?;
    auth::log(&s.pool, Some(a.staff.id), "account.recovery_codes_made", Some(("staff", a.staff.id.to_string())), &a.device, "").await;
    Ok(Json(json!({ "codes": codes })))
}

// ───────── signed out: lost phone, forgotten password ─────────

#[derive(Deserialize)]
pub struct RecoverReq { challenge: Uuid, recovery_code: String }

/// At the 6-digit step: a recovery code instead of the code. Two-step is then set up again on a new phone
/// (the answer is a new QR code), and this person's trusted computers are forgotten.
pub async fn recover(State(s): State<AppState>, h: HeaderMap, Json(r): Json<RecoverReq>) -> ApiResult<Json<Value>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE challenges SET attempts = attempts + 1 WHERE id = $1 AND kind = 'login' AND expires_at > now() AND attempts < 5 RETURNING staff_id")
        .bind(r.challenge).fetch_optional(&s.pool).await?;
    let Some((staff_id,)) = row else { return Err(AppError::Unauthorized("This step expired. Sign in again.")); };
    if !use_recovery_code(&s, staff_id, &r.recovery_code).await? {
        auth::log(&s.pool, Some(staff_id), "login.bad_recovery_code", None, &device(&h), "").await;
        return Err(AppError::Unauthorized("That recovery code isn’t right, or it was already used."));
    }
    let staff: Staff = sqlx::query_as(
        "UPDATE staff SET totp_secret = $2, totp_enabled = FALSE, totp_last_step = 0 WHERE id = $1 AND active RETURNING *")
        .bind(staff_id).bind(auth::new_totp_secret()).fetch_optional(&s.pool).await?
        .ok_or(AppError::Unauthorized("This account is no longer active."))?;
    sqlx::query("DELETE FROM challenges WHERE id = $1").bind(r.challenge).execute(&s.pool).await?;
    sqlx::query("DELETE FROM trusted_devices WHERE staff_id = $1").bind(staff_id).execute(&s.pool).await?;
    let left = codes_left(&s, staff_id).await?;
    auth::log(&s.pool, Some(staff_id), "login.recovery_code_used", Some(("staff", staff_id.to_string())), &device(&h), &format!("{left} left")).await;
    let en: Enrol = start_enrol(&s, &staff, &device(&h)).await?;
    Ok(Json(json!({ "next": "enrol", "enrol": en, "codes_left": left })))
}

#[derive(Deserialize)]
pub struct ResetReq { email: String, code: String, password: String }

/// "Forgot your password?": a reset code from the owner, or (owners) one of your recovery codes.
/// Signs the person out everywhere; the next sign-in still asks for the 6-digit code.
pub async fn reset_password(State(s): State<AppState>, h: HeaderMap, Json(r): Json<ResetReq>) -> ApiResult<Json<Value>> {
    let email = clean_email(&r.email);
    let key = format!("reset:{email}");
    let ipkey = format!("ip:{}", auth::client_ip(&h));
    if auth::limited(&s, &key).await? || auth::limited_n(&s, &ipkey, auth::MAX_FAILS_PER_ADDRESS).await? { return Err(AppError::TooMany); }
    check_new_password(&r.password)?;
    let staff: Option<(Uuid, bool)> = sqlx::query_as("SELECT id, active FROM staff WHERE email = $1").bind(&email).fetch_optional(&s.pool).await?;
    let bad = || AppError::BadRequest("That code isn’t right for this email, or it has expired. Ask the clinic owner for a new one.");
    let Some((staff_id, active)) = staff else {
        auth::record_fail(&s, &key).await;
        return Err(bad());
    };
    let code = norm_code(&r.code);
    let by_owner: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE reset_codes SET used_at = now() WHERE staff_id = $1 AND code_hash = $2 AND used_at IS NULL AND expires_at > now() RETURNING id")
        .bind(staff_id).bind(auth::sha256(&code)).fetch_optional(&s.pool).await?;
    let how = if by_owner.is_some() { "reset_code" } else if use_recovery_code(&s, staff_id, &code).await? { "recovery_code" } else { "" };
    if how.is_empty() || !active {
        auth::record_fail(&s, &key).await;
        auth::record_fail(&s, &ipkey).await;
        auth::log(&s.pool, Some(staff_id), "account.password_reset_failed", None, &device(&h), "").await;
        return Err(bad());
    }
    auth::clear_fails(&s, &key).await;
    auth::clear_fails(&s, &format!("login:{email}")).await;
    sqlx::query("UPDATE staff SET password_hash = $2 WHERE id = $1").bind(staff_id).bind(auth::hash_password(&r.password)?).execute(&s.pool).await?;
    sqlx::query("DELETE FROM sessions WHERE staff_id = $1").bind(staff_id).execute(&s.pool).await?;
    sqlx::query("UPDATE reset_codes SET used_at = now() WHERE staff_id = $1 AND used_at IS NULL").bind(staff_id).execute(&s.pool).await?;
    auth::log(&s.pool, Some(staff_id), "account.password_reset", Some(("staff", staff_id.to_string())), &device(&h), how).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── owner: someone else's account ─────────

async fn other_staff(s: &AppState, a: &Auth, id: Uuid) -> ApiResult<(String, String)> {
    a.require(&["owner"])?;
    if id == a.staff.id {
        return Err(AppError::BadRequest("Use Settings › Account for your own account."));
    }
    sqlx::query_as("SELECT name, email FROM staff WHERE id = $1").bind(id).fetch_optional(&s.pool).await?.ok_or(AppError::NotFound)
}

/// Owner: a one-time code someone uses to choose a new password (valid 24 hours, shown once).
pub async fn reset_code(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let (_, email) = other_staff(&s, &a, id).await?;
    let code = auth::new_invite_code();
    let mut tx = s.pool.begin().await?;
    // Only the newest code works.
    sqlx::query("UPDATE reset_codes SET used_at = now() WHERE staff_id = $1 AND used_at IS NULL").bind(id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO reset_codes (id, staff_id, code_hash, created_by, expires_at) VALUES ($1, $2, $3, $4, now() + make_interval(hours => $5))")
        .bind(Uuid::new_v4()).bind(id).bind(auth::sha256(&code)).bind(a.staff.id).bind(RESET_HOURS).execute(&mut *tx).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "staff.reset_code_made", Some(("staff", id.to_string())), &a.device, "").await;
    Ok(Json(json!({ "code": code, "email": email, "expires_in_hours": RESET_HOURS })))
}

/// Owner: someone lost their phone. Their next sign-in shows a new QR code; they're signed out everywhere
/// and their trusted computers are forgotten.
pub async fn reset_two_step(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    other_staff(&s, &a, id).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE staff SET totp_secret = $2, totp_enabled = FALSE, totp_last_step = 0 WHERE id = $1").bind(id).bind(auth::new_totp_secret()).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM sessions WHERE staff_id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM trusted_devices WHERE staff_id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM challenges WHERE staff_id = $1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "staff.two_step_reset", Some(("staff", id.to_string())), &a.device, "").await;
    Ok(Json(json!({ "ok": true })))
}

/// Owner: invites not used yet (and not withdrawn or expired). The codes themselves can't be shown again.
pub async fn invites(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('id', i.id, 'role', i.role, 'email', i.email, 'created_at', i.created_at,
                'expires_at', i.expires_at, 'created_by_name', st.name) ORDER BY i.created_at DESC), '[]')
         FROM invites i LEFT JOIN staff st ON st.id = i.created_by
         WHERE i.used_at IS NULL AND i.revoked_at IS NULL AND i.expires_at > now()")
        .fetch_one(&s.pool).await?;
    Ok(Json(v))
}

/// Owner: withdraw an invite so its code stops working.
pub async fn revoke_invite(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let r: Option<(String,)> = sqlx::query_as("UPDATE invites SET revoked_at = now() WHERE id = $1 AND used_at IS NULL AND revoked_at IS NULL RETURNING role")
        .bind(id).fetch_optional(&s.pool).await?;
    let Some((role,)) = r else { return Err(AppError::Conflict("This invite was already used or withdrawn.")); };
    auth::log(&s.pool, Some(a.staff.id), "invite.revoked", None, &a.device, &role).await;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::norm_code;
    #[test]
    fn codes_typed_loosely_still_match() {
        assert_eq!(norm_code("k7qm 2xrp-9tda"), "K7QM-2XRP-9TDA");
        assert_eq!(norm_code("K7QM2XRP9TDA"), "K7QM-2XRP-9TDA");
        assert_eq!(norm_code(" K7QM-2XRP-9TDA "), "K7QM-2XRP-9TDA");
        assert_eq!(norm_code("short"), "SHORT");
    }
}
