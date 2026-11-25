//! Accounts and sessions.
//!
//! - Passwords: Argon2id.
//! - Two-step sign-in: TOTP (authenticator app), 6 digits, 30 s; a code can't be reused.
//! - Sessions: random 32-byte token, only its SHA-256 is stored; expires after
//!   `idle_minutes` without use and after 12 hours in any case.
//! - Roles are checked here, on the server, for every request.

use crate::error::{ApiResult, AppError};
use argon2::{password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString}, Argon2};
use axum::{async_trait, extract::FromRequestParts, http::request::Parts};
use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::OffsetDateTime;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub idle_minutes: i64,
    /// Root of the readable archive (PhysioPulse/…) on the server's disk.
    pub archive: std::path::PathBuf,
    /// Session recordings (audio, transcript, AI draft). Never backed up; see `routes::recordings`.
    pub audio: std::path::PathBuf,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Staff {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: String,
    #[serde(skip)]
    pub password_hash: String,
    #[serde(skip)]
    pub totp_secret: String,
    pub totp_enabled: bool,
    #[serde(skip)]
    pub totp_last_step: i64,
    pub active: bool,
    /// The person's AI switch (Team). Always on for the owner (database trigger, 0016/0019). For reception it opens only
    /// the assistant's schedule jobs (`assistant_allowed`).
    pub ai_enabled: bool,
}

/// May this person use the local AI (recording, transcription, drafts)? The owner always; a therapist when
/// the owner switched their AI on in Team (owner, 11 Nov 2026: one switch for all AI features); reception never.
pub fn ai_allowed(st: &Staff) -> bool {
    st.role == "owner" || (st.role == "therapist" && st.ai_enabled)
}

/// May this person use the assistant? The owner always; anyone else whose AI switch is on — reception included
/// (owner, 11 Nov 2026: the assistant's schedule jobs are "aggregated in the AI switch"). For them it only does the
/// schedule (`routes::assistant`); recording, note write-up and letters still need `ai_allowed`.
pub fn assistant_allowed(st: &Staff) -> bool {
    st.role == "owner" || st.ai_enabled
}

pub const ABSOLUTE_HOURS: i64 = 12;
pub const MIN_PASSWORD: usize = 12;

// ───────── passwords ─────────
pub fn hash_password(pw: &str) -> ApiResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(pw.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| AppError::Internal(e.to_string()))
}
pub fn verify_password(pw: &str, hash: &str) -> bool {
    PasswordHash::new(hash).map(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok()).unwrap_or(false)
}
/// Spend the same time on unknown emails so they can't be told apart from wrong passwords.
pub fn dummy_verify(pw: &str) {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let h = DUMMY.get_or_init(|| hash_password("not-a-real-password").unwrap_or_default());
    let _ = verify_password(pw, h);
}

// ───────── tokens ─────────
pub fn new_token() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}
pub fn sha256(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
/// Human-friendly invite code like "K7QM-2XRP-9TDA" (no 0/O/1/I).
pub fn new_invite_code() -> String {
    const A: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut b = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut b);
    let s: String = b.iter().map(|x| A[(*x as usize) % A.len()] as char).collect();
    format!("{}-{}-{}", &s[0..4], &s[4..8], &s[8..12])
}

// ───────── two-step codes ─────────
/// A new two-step secret, ready to store: encrypted with the server key (`secret.rs`, phase 3).
pub fn new_totp_secret() -> String {
    crate::secret::seal(&Secret::generate_secret().to_encoded().to_string())
}
/// The stored two-step secret in clear (base32), e.g. to show it for typing in by hand during setup.
pub fn plain_totp_secret(stored: &str) -> ApiResult<String> {
    crate::secret::open(stored).map_err(AppError::Internal)
}
fn totp(stored: &str, email: &str) -> ApiResult<TOTP> {
    let secret_b32 = plain_totp_secret(stored)?;
    let bytes = Secret::Encoded(secret_b32).to_bytes().map_err(|e| AppError::Internal(e.to_string()))?;
    TOTP::new(Algorithm::SHA1, 6, 1, 30, bytes, Some("PhysioPulse".into()), email.to_string()).map_err(|e| AppError::Internal(e.to_string()))
}
pub fn totp_uri(secret_b32: &str, email: &str) -> ApiResult<String> {
    Ok(totp(secret_b32, email)?.get_url())
}
/// Checks a code against the current step ±1 and refuses steps already used.
/// Returns the accepted step so the caller can store it.
pub fn check_totp(secret_b32: &str, email: &str, code: &str, last_step: i64) -> ApiResult<Option<i64>> {
    let t = totp(secret_b32, email)?;
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    let now = OffsetDateTime::now_utc().unix_timestamp() as u64;
    for skew in [0i64, -1, 1] {
        let ts = (now as i64 + skew * 30) as u64;
        let step = (ts / 30) as i64;
        if step > last_step && t.generate(ts) == code {
            return Ok(Some(step));
        }
    }
    Ok(None)
}
pub fn qr_svg(data: &str) -> String {
    use qrcode::{render::svg, QrCode};
    QrCode::new(data.as_bytes())
        .map(|c| c.render::<svg::Color>().min_dimensions(200, 200).quiet_zone(true).build())
        .unwrap_or_default()
}

// ───────── rate limiting ─────────
// Kept in the database (phase 3), so a restart doesn't forget them. 5 failures per key in 15 minutes;
// the per-address key (`ip:…`) allows more, since a clinic shares one internet address.
const WINDOW_MINUTES: i32 = 15;
const MAX_FAILS: i32 = 5;
pub const MAX_FAILS_PER_ADDRESS: i32 = 30;

pub async fn limited(state: &AppState, key: &str) -> ApiResult<bool> {
    limited_n(state, key, MAX_FAILS).await
}
pub async fn limited_n(state: &AppState, key: &str, max: i32) -> ApiResult<bool> {
    let r: Option<(i32,)> = sqlx::query_as("SELECT fails FROM rate_limits WHERE key = $1 AND window_start > now() - make_interval(mins => $2)")
        .bind(key).bind(WINDOW_MINUTES).fetch_optional(&state.pool).await?;
    Ok(r.is_some_and(|(n,)| n >= max))
}
pub async fn record_fail(state: &AppState, key: &str) {
    let r = sqlx::query(
        "INSERT INTO rate_limits (key, fails, window_start) VALUES ($1, 1, now())
         ON CONFLICT (key) DO UPDATE SET
           fails = CASE WHEN rate_limits.window_start > now() - make_interval(mins => $2) THEN rate_limits.fails + 1 ELSE 1 END,
           window_start = CASE WHEN rate_limits.window_start > now() - make_interval(mins => $2) THEN rate_limits.window_start ELSE now() END")
        .bind(key).bind(WINDOW_MINUTES).execute(&state.pool).await;
    if let Err(e) = r { tracing::error!("could not record a failed attempt: {e}"); }
}
pub async fn clear_fails(state: &AppState, key: &str) {
    let _ = sqlx::query("DELETE FROM rate_limits WHERE key = $1").bind(key).execute(&state.pool).await;
}

// ───────── access log ─────────
pub async fn log(pool: &PgPool, staff: Option<Uuid>, action: &str, subject: Option<(&str, String)>, device: &str, detail: &str) {
    let (st, sid) = subject.map(|(a, b)| (Some(a.to_string()), Some(b))).unwrap_or((None, None));
    if let Err(e) = sqlx::query("INSERT INTO access_log (staff_id, action, subject_type, subject_id, device, detail) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(staff).bind(action).bind(st).bind(sid).bind(device).bind(detail)
        .execute(pool).await
    {
        tracing::error!("could not write access log: {e}");
    }
}

// ───────── sessions ─────────
/// A phone session lasts this many days, with no idle timeout (phase 5, owner 11 Nov 2026): the iPhone app asks for
/// Face ID every time it reopens, and after 30 days for the password and two-step code again.
pub const PHONE_DAYS: i64 = 30;

/// A session always belongs to an approved device (phase 3); removing the device ends it.
/// On a device the owner approved as a phone (`devices.kind`, phase 5) it is a phone session: 30 days, no idle
/// timeout, and one per person per phone (signing in again replaces the old one).
pub async fn create_session(state: &AppState, staff_id: Uuid, device: &str, device_id: Uuid) -> ApiResult<String> {
    let token = new_token();
    let (phone,): (bool,) = sqlx::query_as("SELECT kind = 'phone' FROM devices WHERE id = $1").bind(device_id).fetch_one(&state.pool).await?;
    let mut tx = state.pool.begin().await?;
    if phone {
        sqlx::query("DELETE FROM sessions WHERE staff_id = $1 AND device_id = $2 AND phone").bind(staff_id).bind(device_id).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO sessions (id, token_hash, staff_id, device, expires_at, device_id, phone)
                 VALUES ($1, $2, $3, $4, now() + CASE WHEN $7 THEN make_interval(days => $6::int) ELSE make_interval(hours => $5::int) END, $8, $7)")
        .bind(Uuid::new_v4()).bind(sha256(&token)).bind(staff_id).bind(device).bind(ABSOLUTE_HOURS as i32).bind(PHONE_DAYS as i32).bind(phone).bind(device_id)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(token)
}

// ───────── trusted computers ─────────
/// After the 6-digit code, this computer skips the code for this many days (owner decision, 10 Nov 2026).
pub const TRUST_DAYS: i64 = 30;

/// Remembers this computer for this person. Replaces an older token for the same computer name.
pub async fn trust_device(state: &AppState, staff_id: Uuid, device: &str, device_id: Uuid) -> ApiResult<String> {
    let token = new_token();
    let mut tx = state.pool.begin().await?;
    sqlx::query("DELETE FROM trusted_devices WHERE staff_id = $1 AND (device_id = $2 OR (device <> '' AND device = $3))")
        .bind(staff_id).bind(device_id).bind(device).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO trusted_devices (id, staff_id, token_hash, device, expires_at, device_id) VALUES ($1, $2, $3, $4, now() + make_interval(days => $5::int), $6)")
        .bind(Uuid::new_v4()).bind(staff_id).bind(sha256(&token)).bind(device).bind(TRUST_DAYS as i32).bind(device_id)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(token)
}

/// A live trusted-computer token for this person: `Some(carried_over)` (and marks it used). Tokens made on a
/// device only count on that device; tokens from before phase 3 (`carried_over`) count anywhere, once.
pub async fn is_trusted(state: &AppState, staff_id: Uuid, token: &str, device_id: Option<Uuid>) -> ApiResult<Option<bool>> {
    if token.len() != 64 { return Ok(None); }
    let row: Option<(bool,)> = sqlx::query_as(
        "UPDATE trusted_devices SET last_used = now() WHERE token_hash = $1 AND staff_id = $2 AND expires_at > now()
           AND (carried_over OR device_id = $3) RETURNING carried_over")
        .bind(sha256(token)).bind(staff_id).bind(device_id).fetch_optional(&state.pool).await?;
    Ok(row.map(|r| r.0))
}

/// The signed-in person, taken from `Authorization: Bearer <token>`.
pub struct Auth {
    pub staff: Staff,
    pub session_id: Uuid,
    pub device: String,
    /// The approved device this session belongs to (phase 3).
    pub device_id: Uuid,
}

impl Auth {
    pub fn require(&self, roles: &[&str]) -> ApiResult<()> {
        if roles.contains(&self.staff.role.as_str()) { Ok(()) } else { Err(AppError::Forbidden) }
    }
    /// The local AI (see `ai_allowed`).
    pub fn require_ai(&self) -> ApiResult<()> {
        if ai_allowed(&self.staff) { Ok(()) } else { Err(AppError::Forbidden) }
    }
}

pub fn device_of(parts: &Parts) -> String {
    parts.headers.get("x-device").and_then(|v| v.to_str().ok()).unwrap_or("").chars().take(80).collect()
}

/// What the app says it runs on (`X-Device-Kind`, phase 5): "phone" from the iPhone app, else "computer".
/// Only used when a device is first saved; the owner sees it before approving, and it never changes after.
pub fn device_kind(h: &axum::http::HeaderMap) -> &'static str {
    if h.get("x-device-kind").and_then(|v| v.to_str().ok()).is_some_and(|k| k.trim().eq_ignore_ascii_case("phone")) { "phone" } else { "computer" }
}

/// The device key the app's Rust core sends with every request (64 hex characters), if any.
pub fn device_key(h: &axum::http::HeaderMap) -> Option<String> {
    h.get("x-device-key").and_then(|v| v.to_str().ok()).map(str::trim)
        .filter(|k| k.len() == 64 && k.chars().all(|c| c.is_ascii_hexdigit())).map(str::to_ascii_lowercase)
}

/// The caller's internet address, worked out by `main::client_ip` (behind Nginx Proxy Manager, the real one).
pub fn client_ip(h: &axum::http::HeaderMap) -> String {
    h.get(CLIENT_IP).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
}
pub const CLIENT_IP: &str = "x-physiopulse-client-ip";

#[async_trait]
impl FromRequestParts<AppState> for Auth {
    type Rejection = AppError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = parts.headers.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).map(str::trim)
            .ok_or(AppError::Unauthorized("Sign in to continue."))?;
        // Sliding idle timeout: the session must have been used within `idle_minutes` (not phone sessions, phase 5:
        // the phone asks for Face ID on every reopen instead, and its session ends after 30 days).
        let row: Option<(Uuid, Uuid)> = sqlx::query_as(
            "UPDATE sessions SET last_seen = now()
             WHERE token_hash = $1 AND expires_at > now() AND (phone OR last_seen > now() - make_interval(mins => $2::int))
             RETURNING id, staff_id",
        )
        .bind(sha256(token)).bind(state.idle_minutes as i32)
        .fetch_optional(&state.pool).await?;
        let (session_id, staff_id) = row.ok_or(AppError::Unauthorized("Your session ended. Sign in again."))?;
        // Phase 3: the session only works from its own device, and only while the owner keeps it approved.
        let dev: Option<(Uuid, String, String)> = sqlx::query_as(
            "SELECT d.id, d.status, d.key_hash FROM sessions s JOIN devices d ON d.id = s.device_id WHERE s.id = $1")
            .bind(session_id).fetch_optional(&state.pool).await?;
        let key = device_key(&parts.headers);
        let device_id = match (dev, key) {
            (Some((id, status, hash)), Some(k)) if status == "approved" && hash == sha256(&k) => id,
            (Some((_, status, _)), _) if status != "approved" => {
                sqlx::query("DELETE FROM sessions WHERE id = $1").bind(session_id).execute(&state.pool).await?;
                return Err(AppError::Unauthorized("This device was removed by the clinic owner."));
            }
            _ => return Err(AppError::Unauthorized("Your session ended. Sign in again.")),
        };
        let _ = sqlx::query("UPDATE devices SET last_seen = now(), last_ip = $2 WHERE id = $1 AND (last_seen IS NULL OR last_seen < now() - interval '1 minute' OR last_ip <> $2)")
            .bind(device_id).bind(client_ip(&parts.headers)).execute(&state.pool).await;
        let staff: Staff = sqlx::query_as("SELECT * FROM staff WHERE id = $1 AND active").bind(staff_id).fetch_optional(&state.pool).await?
            .ok_or(AppError::Unauthorized("This account is no longer active."))?;
        Ok(Auth { staff, session_id, device: device_of(parts), device_id })
    }
}
