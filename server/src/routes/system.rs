//! Settings › Server and backups: what the server really knows about itself (audit C1, 10 Nov 2026).
//!
//! The backup container (server/backup) writes `status.json` into a folder shared with this server
//! (`BACKUP_CONTROL`, default /backup-control) after every run, and watches the same folder for a
//! `request` file: that is how "Back Up Now" reaches it. The server never sees the backups themselves
//! (they stay in a root-only folder) and never holds the key that opens them.

use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::path::PathBuf;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// A backup older than this shows a warning on Today (owner).
pub const STALE_HOURS: i64 = 36;

fn control_dir() -> PathBuf {
    PathBuf::from(std::env::var("BACKUP_CONTROL").unwrap_or_else(|_| "/backup-control".into()))
}

/// What the backup container last wrote, checked. `state`:
/// - "ok": the newest backup is younger than 36 hours;
/// - "old": it's older (the nightly run stopped);
/// - "failed": the last run failed (`last_error` is newer than `last_ok`);
/// - "running": a backup is being made now;
/// - "unknown": no status file (the backup container isn't running, or is older than this server).
pub async fn backup_status() -> Value {
    let dir = control_dir();
    let raw = tokio::fs::read_to_string(dir.join("status.json")).await.ok();
    let st: Value = raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or(Value::Null);
    let requested = tokio::fs::metadata(dir.join("request")).await.is_ok();
    if !st.is_object() {
        return json!({ "state": "unknown", "requested": requested });
    }
    let parse = |k: &str| st[k].as_str().and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok());
    let last_ok = parse("last_ok");
    let last_error = parse("last_error");
    let now = OffsetDateTime::now_utc();
    let state = if st["running"] == true {
        "running"
    } else if matches!((last_ok, last_error), (Some(ok), Some(err)) if err > ok) || (last_ok.is_none() && last_error.is_some()) {
        "failed"
    } else if last_ok.is_some_and(|t| (now - t).whole_hours() < STALE_HOURS) {
        "ok"
    } else {
        "old"
    };
    let fmt = |t: Option<OffsetDateTime>| t.and_then(|t| t.format(&Rfc3339).ok());
    // Only these fields go to the app: file names and times, never paths or keys.
    let name = |k: &str| st[k].as_str().filter(|s| s.len() < 80 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))).map(str::to_string);
    json!({
        "state": state,
        "requested": requested,
        "last_ok": fmt(last_ok),
        "last_error": fmt(last_error),
        "db_file": name("db_file"),
        "files_file": name("files_file"),
        "bytes": st["bytes"].as_i64(),
        "kept_days": st["kept_days"].as_i64(),
        "offsite": st["offsite"].as_str().filter(|s| ["off", "ok", "failed"].contains(s)).unwrap_or("off"),
        "next_at": st["next_at"].as_str().and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok()).and_then(|t| t.format(&Rfc3339).ok()),
    })
}

/// Owner: the server's version and address and the real backup state.
pub async fn status(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let (db,): (String,) = sqlx::query_as("SELECT split_part(version(), ' ', 2)").fetch_one(&s.pool).await?;
    let secrets: Vec<(String,)> = sqlx::query_as("SELECT totp_secret FROM staff").fetch_all(&s.pool).await?;
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        // Set by docker-compose: the clinic's HTTPS address (phase 3), e.g. https://….duckdns.org.
        "address": std::env::var("PUBLIC_ADDRESS").ok().filter(|v| !v.trim().is_empty() && !v.starts_with(':')),
        "database": format!("PostgreSQL {db}"),
        "archive": s.archive.display().to_string(),
        "backup": backup_status().await,
        "stale_hours": STALE_HOURS,
        // Phase 3: are the two-step secrets encrypted with the server key?
        "secrets_encrypted": crate::secret::enabled() && secrets.iter().all(|(v,)| crate::secret::is_sealed(v)),
    })))
}

/// Owner: ask the backup container for a backup now. It checks every 30 seconds.
pub async fn backup_now(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let dir = control_dir();
    if tokio::fs::metadata(dir.join("status.json")).await.is_err() {
        return Err(AppError::Conflict("The backup service isn’t answering. Update the server, then try again."));
    }
    let stamp = OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_default();
    tokio::fs::write(dir.join("request"), stamp.as_bytes()).await
        .map_err(|e| AppError::Internal(format!("backup request: {e}")))?;
    auth::log(&s.pool, Some(a.staff.id), "system.backup_requested", None, &a.device, "").await;
    Ok(Json(json!({ "ok": true, "backup": backup_status().await })))
}

/// For Today's alerts (owner only): null when the backup is fine.
pub async fn alert(a: &Auth) -> Value {
    if a.staff.role != "owner" {
        return Value::Null;
    }
    let b = backup_status().await;
    match b["state"].as_str() {
        Some("ok") | Some("running") => Value::Null,
        _ => json!({ "state": b["state"], "last_ok": b["last_ok"] }),
    }
}
