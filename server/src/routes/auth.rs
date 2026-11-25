use super::devices;
use crate::auth::{self, AppState, Auth, Staff, MIN_PASSWORD};
use crate::error::{ApiResult, AppError};
use axum::{extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub(crate) fn device(h: &HeaderMap) -> String {
    h.get("x-device").and_then(|v| v.to_str().ok()).unwrap_or("").chars().take(80).collect()
}
pub(crate) fn clean_email(e: &str) -> String {
    e.trim().to_lowercase()
}

#[derive(Serialize)]
pub struct Status { setup_needed: bool, clinic: Option<String>, version: &'static str }

/// Public: lets the app show "Set up the clinic" on first run.
pub async fn status(State(s): State<AppState>) -> ApiResult<Json<Status>> {
    let n: (i64,) = sqlx::query_as("SELECT count(*) FROM staff").fetch_one(&s.pool).await?;
    let clinic: Option<(String,)> = sqlx::query_as("SELECT name FROM clinic WHERE id = 1").fetch_optional(&s.pool).await?;
    Ok(Json(Status { setup_needed: n.0 == 0, clinic: clinic.map(|c| c.0), version: env!("CARGO_PKG_VERSION") }))
}

/// Two-step setup the app shows as a QR code. The secret is also returned for typing in by hand.
#[derive(Serialize)]
pub struct Enrol { challenge: Uuid, otpauth_uri: String, secret: String, qr_svg: String }

pub(crate) async fn start_enrol(s: &AppState, staff: &Staff, device: &str) -> ApiResult<Enrol> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO challenges (id, staff_id, kind, device, expires_at) VALUES ($1, $2, 'enrol', $3, now() + interval '15 minutes')")
        .bind(id).bind(staff.id).bind(device).execute(&s.pool).await?;
    let uri = auth::totp_uri(&staff.totp_secret, &staff.email)?;
    Ok(Enrol { challenge: id, qr_svg: auth::qr_svg(&uri), otpauth_uri: uri, secret: auth::plain_totp_secret(&staff.totp_secret)? })
}

#[derive(Deserialize)]
pub struct SetupReq { clinic_name: String, name: String, email: String, password: String }

/// First run only: creates the clinic and the owner account. Refused once any account exists.
pub async fn setup(State(s): State<AppState>, h: HeaderMap, Json(r): Json<SetupReq>) -> ApiResult<Json<Enrol>> {
    let dkey = devices::key_or_update(&h)?;
    if r.password.chars().count() < MIN_PASSWORD { return Err(AppError::BadRequest("Choose a password with at least 12 characters.")); }
    if r.name.trim().is_empty() || !r.email.contains('@') || r.clinic_name.trim().is_empty() { return Err(AppError::BadRequest("Fill in the clinic name, your name and a valid email.")); }
    let mut tx = s.pool.begin().await?;
    // Lock the table so two first-run requests can't both create an owner.
    sqlx::query("LOCK TABLE staff IN EXCLUSIVE MODE").execute(&mut *tx).await?;
    let n: (i64,) = sqlx::query_as("SELECT count(*) FROM staff").fetch_one(&mut *tx).await?;
    if n.0 > 0 { return Err(AppError::Conflict("The clinic is already set up. Sign in instead.")); }
    sqlx::query("INSERT INTO clinic (id, name) VALUES (1, $1) ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name").bind(r.clinic_name.trim()).execute(&mut *tx).await?;
    let staff: Staff = sqlx::query_as("INSERT INTO staff (id, email, name, role, password_hash, totp_secret) VALUES ($1,$2,$3,'owner',$4,$5) RETURNING *")
        .bind(Uuid::new_v4()).bind(clean_email(&r.email)).bind(r.name.trim()).bind(auth::hash_password(&r.password)?).bind(auth::new_totp_secret())
        .fetch_one(&mut *tx).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(staff.id), "setup.owner_created", None, &device(&h), "").await;
    // The computer the clinic is set up on is the first approved device (phase 3).
    devices::approve_now(&s, &dkey, &device(&h), auth::device_kind(&h), Some(staff.id), "setup", &auth::client_ip(&h)).await?;
    Ok(Json(start_enrol(&s, &staff, &device(&h)).await?))
}

#[derive(Deserialize)]
pub struct SignupReq { invite: String, name: String, email: String, password: String }

/// Staff join with the invite code the owner gave them.
pub async fn signup(State(s): State<AppState>, h: HeaderMap, Json(r): Json<SignupReq>) -> ApiResult<Json<Enrol>> {
    let key = format!("invite:{}", auth::client_ip(&h));
    if auth::limited(&s, &key).await? { return Err(AppError::TooMany); }
    if r.password.chars().count() < MIN_PASSWORD { return Err(AppError::BadRequest("Choose a password with at least 12 characters.")); }
    if r.name.trim().is_empty() || !r.email.contains('@') { return Err(AppError::BadRequest("Fill in your name and a valid email.")); }
    let code = r.invite.trim().to_uppercase();
    let mut tx = s.pool.begin().await?;
    let inv: Option<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT id, role, email FROM invites WHERE code_hash = $1 AND used_at IS NULL AND revoked_at IS NULL AND expires_at > now() FOR UPDATE")
        .bind(auth::sha256(&code)).fetch_optional(&mut *tx).await?;
    let Some((inv_id, role, inv_email)) = inv else {
        auth::record_fail(&s, &key).await;
        return Err(AppError::BadRequest("That invite code isn’t valid or has expired. Ask the clinic owner for a new one."));
    };
    let email = clean_email(&r.email);
    if let Some(e) = inv_email { if e != email { return Err(AppError::BadRequest("Use the email address the invite was sent to.")); } }
    let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM staff WHERE email = $1").bind(&email).fetch_optional(&mut *tx).await?;
    if exists.is_some() { return Err(AppError::Conflict("An account with this email already exists. Sign in instead.")); }
    let staff: Staff = sqlx::query_as("INSERT INTO staff (id, email, name, role, password_hash, totp_secret) VALUES ($1,$2,$3,$4,$5,$6) RETURNING *")
        .bind(Uuid::new_v4()).bind(&email).bind(r.name.trim()).bind(&role).bind(auth::hash_password(&r.password)?).bind(auth::new_totp_secret())
        .fetch_one(&mut *tx).await?;
    sqlx::query("UPDATE invites SET used_at = now() WHERE id = $1").bind(inv_id).execute(&mut *tx).await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(staff.id), "staff.joined", Some(("staff", staff.id.to_string())), &device(&h), &role).await;
    Ok(Json(start_enrol(&s, &staff, &device(&h)).await?))
}

#[derive(Deserialize)]
pub struct LoginReq {
    email: String,
    password: String,
    /// Sent by the desktop app's Rust core when this computer was trusted after an earlier code.
    #[serde(default)]
    device_token: Option<String>,
}

/// Step 1: email + password. Returns a challenge for the 6-digit code,
/// or the code setup if this account hasn't set up two-step yet,
/// or — on an approved computer trusted in the last 30 days — the session straight away (`next: "session"`).
/// Phase 3: the app must send its device key; a device the owner removed is refused.
pub async fn login(State(s): State<AppState>, h: HeaderMap, Json(r): Json<LoginReq>) -> ApiResult<Json<Value>> {
    let email = clean_email(&r.email);
    let key = format!("login:{email}");
    let ip = auth::client_ip(&h);
    let ipkey = format!("ip:{ip}");
    if auth::limited(&s, &key).await? || auth::limited_n(&s, &ipkey, auth::MAX_FAILS_PER_ADDRESS).await? { return Err(AppError::TooMany); }
    let dkey = devices::key_or_update(&h)?;
    let staff: Option<Staff> = sqlx::query_as("SELECT * FROM staff WHERE email = $1").bind(&email).fetch_optional(&s.pool).await?;
    let ok = match &staff {
        Some(st) => auth::verify_password(&r.password, &st.password_hash) && st.active,
        None => { auth::dummy_verify(&r.password); false }
    };
    if !ok {
        auth::record_fail(&s, &key).await;
        auth::record_fail(&s, &ipkey).await;
        auth::log(&s.pool, staff.as_ref().map(|x| x.id), "login.failed", None, &device(&h), &ip).await;
        return Err(AppError::Unauthorized("The email or password is wrong."));
    }
    let staff = staff.unwrap();
    let dev = devices::find(&s, &dkey).await?;
    if dev.as_ref().is_some_and(|d| d.status == "revoked") {
        auth::log(&s.pool, Some(staff.id), "login.device_refused", None, &device(&h), &ip).await;
        return Err(devices::removed());
    }
    if !staff.totp_enabled {
        return Ok(Json(json!({ "next": "enrol", "enrol": start_enrol(&s, &staff, &device(&h)).await? })));
    }
    if let Some(dt) = r.device_token.as_deref() {
        if let Some(carried_over) = auth::is_trusted(&s, staff.id, dt, dev.as_ref().map(|d| d.id)).await? {
            let approved = match &dev {
                Some(d) if d.status == "approved" => Some(d.id),
                // A computer trusted before phase 3 approves its device on its first sign-in (owner, 11 Nov 2026).
                _ if carried_over => Some(devices::approve_now(&s, &dkey, &device(&h), auth::device_kind(&h), Some(staff.id), "carried_over", &ip).await?.id),
                _ => None,
            };
            if let Some(device_id) = approved {
                if carried_over {
                    sqlx::query("UPDATE trusted_devices SET carried_over = FALSE, device_id = $2 WHERE token_hash = $1")
                        .bind(auth::sha256(dt)).bind(device_id).execute(&s.pool).await?;
                }
                auth::clear_fails(&s, &key).await;
                let token = auth::create_session(&s, staff.id, &device(&h), device_id).await?;
                auth::log(&s.pool, Some(staff.id), "login.ok", None, &device(&h), "trusted_computer").await;
                return Ok(Json(json!({ "next": "session", "token": token, "staff": staff, "idle_minutes": s.idle_minutes })));
            }
        }
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO challenges (id, staff_id, kind, device, expires_at) VALUES ($1, $2, 'login', $3, now() + interval '5 minutes')")
        .bind(id).bind(staff.id).bind(device(&h)).execute(&s.pool).await?;
    Ok(Json(json!({ "next": "code", "challenge": id })))
}

#[derive(Deserialize)]
pub struct CodeReq {
    challenge: Uuid,
    code: String,
    /// The desktop app asks to trust this computer, so the next sign-ins skip the code.
    #[serde(default)]
    trust_device: bool,
}

/// Step 2: the 6-digit code. Works for normal sign-in and for confirming a new code setup.
/// On an approved device the answer is the session; on a new device it is a ticket to wait for the owner's
/// approval (`next: "pending"`, see `routes::devices`).
pub async fn verify(State(s): State<AppState>, h: HeaderMap, Json(r): Json<CodeReq>) -> ApiResult<Json<Value>> {
    let dkey = devices::key_or_update(&h)?;
    let row: Option<(Uuid, String, i32)> = sqlx::query_as(
        "UPDATE challenges SET attempts = attempts + 1 WHERE id = $1 AND expires_at > now() AND attempts < 5 RETURNING staff_id, kind, attempts")
        .bind(r.challenge).fetch_optional(&s.pool).await?;
    let Some((staff_id, kind, _)) = row else { return Err(AppError::Unauthorized("This step expired. Sign in again.")); };
    let staff: Staff = sqlx::query_as("SELECT * FROM staff WHERE id = $1 AND active").bind(staff_id).fetch_optional(&s.pool).await?
        .ok_or(AppError::Unauthorized("This account is no longer active."))?;
    let Some(step) = auth::check_totp(&staff.totp_secret, &staff.email, &r.code, staff.totp_last_step)? else {
        auth::log(&s.pool, Some(staff.id), "login.bad_code", None, &device(&h), "").await;
        return Err(AppError::Unauthorized("That code isn’t right. Check the time on your phone and try the newest code."));
    };
    sqlx::query("UPDATE staff SET totp_last_step = $2, totp_enabled = TRUE WHERE id = $1").bind(staff.id).bind(step).execute(&s.pool).await?;
    sqlx::query("DELETE FROM challenges WHERE id = $1").bind(r.challenge).execute(&s.pool).await?;
    auth::clear_fails(&s, &format!("login:{}", staff.email)).await;
    let enrol = kind == "enrol";
    if enrol {
        auth::log(&s.pool, Some(staff.id), "login.two_step_set_up", None, &device(&h), "").await;
    }
    let staff = Staff { totp_enabled: true, ..staff };
    match devices::find(&s, &dkey).await? {
        Some(d) if d.status == "approved" => {
            if !enrol { auth::log(&s.pool, Some(staff.id), "login.ok", None, &device(&h), "").await; }
            Ok(Json(devices::session_answer(&s, &staff, d.id, &device(&h), r.trust_device, enrol).await?))
        }
        Some(d) if d.status == "revoked" => {
            auth::log(&s.pool, Some(staff.id), "login.device_refused", None, &device(&h), &auth::client_ip(&h)).await;
            Err(devices::removed())
        }
        _ => {
            let d = devices::find_or_request(&s, &dkey, &device(&h), auth::device_kind(&h), staff.id, &auth::client_ip(&h)).await?;
            Ok(Json(devices::pending_answer(&s, &staff, &d, r.trust_device, enrol).await?))
        }
    }
}

#[derive(Deserialize)]
pub struct ForgetReq { device_token: String }

/// "Forget this computer": drops its trusted-computer token. No session needed (you hold the token);
/// always answers ok so it can't be used to test tokens.
pub async fn forget_device(State(s): State<AppState>, h: HeaderMap, Json(r): Json<ForgetReq>) -> ApiResult<Json<Value>> {
    let gone: Option<(Uuid,)> = sqlx::query_as("DELETE FROM trusted_devices WHERE token_hash = $1 RETURNING staff_id")
        .bind(auth::sha256(&r.device_token)).fetch_optional(&s.pool).await?;
    if let Some((staff_id,)) = gone {
        auth::log(&s.pool, Some(staff_id), "login.computer_forgotten", None, &device(&h), "").await;
    }
    Ok(Json(json!({ "ok": true })))
}

pub async fn me(a: Auth) -> Json<Staff> {
    Json(a.staff)
}

/// Signing out deletes the session on the server. The app also calls this when it closes.
pub async fn logout(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    sqlx::query("DELETE FROM sessions WHERE id = $1").bind(a.session_id).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "logout", None, &a.device, "").await;
    Ok(Json(json!({ "ok": true })))
}
