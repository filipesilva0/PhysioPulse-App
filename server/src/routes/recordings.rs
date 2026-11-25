//! Phase 2 of the PhysioPulse plan: record a session, turn it into text on the AI PC, and draft a plain
//! report for the therapist to review (DECISIONS.md, 11 Nov 2026).
//!
//! - Who: the owner, and therapists whose AI switch is on (Team). Reception never (`Auth::require_ai`).
//!   Each recording is visible to the person who made it and to the owner.
//! - The patient is asked first; the app only records on a yes (no record of that answer is kept: owner's risk).
//! - Capture: the app sends 16 kHz mono 16-bit PCM in pieces while recording (`PUT …/chunks/:n`), so a
//!   crash loses at most the last piece. `finish` joins them into one WAV and queues it.
//! - A worker (`spawn_worker`) sends each queued recording to the whisper server (`crate::stt`), then the
//!   transcript to the note model, which writes a plain report (`crate::style`), proposed values for the
//!   note template's blocks and a short plan for next session. When the AI PC is off, it waits.
//! - Storage: AUDIO_DIR/<id>/ holds audio.wav, transcript.txt and draft.json. That folder is never backed up
//!   and the database holds no clinical text about a recording, so deleting the folder deletes it all.
//! - Deleted when the note made from it is signed, by hand, after 30 days, or with the patient / day.
//! - Access log: started, uploaded, opened (draft read), played, deleted (with why) — never the content.

use super::util::require_clinical;
use crate::ai::{self, AiError};
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use crate::{stt, style};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

/// 16 kHz × 2 bytes, mono.
const BYTES_PER_SECOND: i64 = 32_000;
/// One piece is at most ~2 minutes of audio.
const MAX_CHUNK: usize = 4 * 1024 * 1024;
/// A session is at most 2 h 30.
const MAX_BYTES: i64 = 150 * 60 * BYTES_PER_SECOND;
/// Shorter than this isn't a session.
const MIN_SECONDS: i64 = 10;
/// The transcript the note model reads, at most (about 20 000 tokens).
const MAX_TRANSCRIPT: usize = 60_000;

fn dir(s: &AppState, id: Uuid) -> PathBuf {
    s.audio.join(id.to_string())
}
fn subject(pid: Uuid) -> Option<(&'static str, String)> {
    Some(("patient", pid.to_string()))
}

/// The recording, if this person may use it (author, or owner). (patient_id, status, created_by)
async fn mine(s: &AppState, a: &Auth, id: Uuid) -> ApiResult<(Uuid, String, i64)> {
    let r: Option<(Uuid, String, Option<Uuid>, i64)> = sqlx::query_as("SELECT patient_id, status, created_by, bytes FROM recordings WHERE id = $1 AND deleted_at IS NULL")
        .bind(id).fetch_optional(&s.pool).await?;
    let (pid, status, by, bytes) = r.ok_or(AppError::NotFound)?;
    if a.staff.role != "owner" && by != Some(a.staff.id) {
        return Err(AppError::NotFound);
    }
    Ok((pid, status, bytes))
}

// ───────── recording ─────────

#[derive(Deserialize)]
pub struct StartReq {
    #[serde(default)]
    template: String,
    /// The template's blocks as the app shows them (labels, ranges, options): what the AI may fill in.
    #[serde(default)]
    blocks: Value,
    appointment_id: Option<Uuid>,
    lang: Option<String>,
}

/// Owner, or a therapist with AI on: start a recording for this patient. Returns its id.
pub async fn start(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Json(r): Json<StartReq>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    a.require_ai()?;
    let (ok,): (bool,) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM patients WHERE id = $1 AND deleted_at IS NULL AND anonymised_at IS NULL)")
        .bind(pid).fetch_one(&s.pool).await?;
    if !ok {
        return Err(AppError::NotFound);
    }
    let blocks = template_blocks(&r.blocks);
    let id = Uuid::new_v4();
    let lang = if r.lang.as_deref() == Some("en") { "en" } else { "pt" };
    let folder = dir(&s, id);
    tokio::fs::create_dir_all(&folder).await.map_err(|e| AppError::Internal(format!("audio folder: {e}")))?;
    tokio::fs::write(folder.join("blocks.json"), blocks.to_string()).await.map_err(|e| AppError::Internal(format!("audio folder: {e}")))?;
    let template: String = r.template.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(40).collect();
    sqlx::query("INSERT INTO recordings (id, patient_id, appointment_id, template, lang, created_by) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(id).bind(pid).bind(r.appointment_id).bind(&template).bind(lang).bind(a.staff.id).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "recording.started", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct ChunkReq { data: String }

/// The author: the next piece of audio (base64 of 16 kHz mono s16le PCM). Pieces arrive in order from 0;
/// a piece sent twice (an answer lost on the way) is accepted once.
pub async fn chunk(State(s): State<AppState>, a: Auth, Path((id, n)): Path<(Uuid, i32)>, Json(r): Json<ChunkReq>) -> ApiResult<Json<Value>> {
    a.require_ai()?;
    let row: Option<(Option<Uuid>, String, i32, i64)> = sqlx::query_as("SELECT created_by, status, chunks, bytes FROM recordings WHERE id = $1 AND deleted_at IS NULL")
        .bind(id).fetch_optional(&s.pool).await?;
    let (by, status, have, bytes) = row.ok_or(AppError::NotFound)?;
    if by != Some(a.staff.id) {
        return Err(AppError::NotFound);
    }
    if status != "recording" {
        return Err(AppError::Conflict("This recording has already finished."));
    }
    if n < have {
        return Ok(Json(json!({ "chunks": have, "seconds": bytes / BYTES_PER_SECOND })));
    }
    if n > have {
        return Err(AppError::ConflictWith("chunk_order", "Part of the recording is missing. Send it again.", json!({ "expected": have })));
    }
    let raw = base64::engine::general_purpose::STANDARD.decode(r.data.trim()).map_err(|_| AppError::BadRequest("The audio couldn’t be read."))?;
    if raw.is_empty() || raw.len() > MAX_CHUNK || raw.len() % 2 != 0 {
        return Err(AppError::BadRequest("The audio couldn’t be read."));
    }
    if bytes + raw.len() as i64 > MAX_BYTES {
        return Err(AppError::BadRequest("A recording can be at most 2 h 30. Stop and start a new one."));
    }
    tokio::fs::write(dir(&s, id).join(format!("{n:05}.pcm")), &raw).await.map_err(|e| AppError::Internal(format!("audio chunk: {e}")))?;
    let (chunks, total): (i32, i64) = sqlx::query_as(
        "UPDATE recordings SET chunks = chunks + 1, bytes = bytes + $2, seconds = ((bytes + $2) / 32000)::int WHERE id = $1 AND chunks = $3 RETURNING chunks, bytes")
        .bind(id).bind(raw.len() as i64).bind(n).fetch_one(&s.pool).await?;
    Ok(Json(json!({ "chunks": chunks, "seconds": total / BYTES_PER_SECOND })))
}

/// The author: the recording is over. Joins the pieces into one WAV and queues it for the AI PC.
pub async fn finish(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    a.require_ai()?;
    let row: Option<(Uuid, Option<Uuid>, String, i32, i64)> = sqlx::query_as("SELECT patient_id, created_by, status, chunks, bytes FROM recordings WHERE id = $1 AND deleted_at IS NULL")
        .bind(id).fetch_optional(&s.pool).await?;
    let (pid, by, status, chunks, bytes) = row.ok_or(AppError::NotFound)?;
    if by != Some(a.staff.id) {
        return Err(AppError::NotFound);
    }
    if status != "recording" {
        return Ok(Json(json!({ "status": status })));
    }
    if bytes < MIN_SECONDS * BYTES_PER_SECOND {
        return Err(AppError::BadRequest("The recording is too short. Record the session, or delete this recording."));
    }
    join_wav(&dir(&s, id), chunks, bytes).await.map_err(|e| AppError::Internal(format!("audio join: {e}")))?;
    sqlx::query("UPDATE recordings SET status = 'queued', finished_at = now() WHERE id = $1").bind(id).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "recording.uploaded", subject(pid), &a.device, &format!("{id} · {} s", bytes / BYTES_PER_SECOND)).await;
    Ok(Json(json!({ "status": "queued", "seconds": bytes / BYTES_PER_SECOND })))
}

async fn join_wav(folder: &std::path::Path, chunks: i32, bytes: i64) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let tmp = folder.join("audio.wav.part");
    let mut out = tokio::fs::File::create(&tmp).await?;
    out.write_all(&stt::wav_header(bytes as u32, 16_000)).await?;
    for n in 0..chunks {
        let piece = tokio::fs::read(folder.join(format!("{n:05}.pcm"))).await?;
        out.write_all(&piece).await?;
    }
    out.flush().await?;
    drop(out);
    tokio::fs::rename(&tmp, folder.join("audio.wav")).await?;
    for n in 0..chunks {
        let _ = tokio::fs::remove_file(folder.join(format!("{n:05}.pcm"))).await;
    }
    Ok(())
}

// ───────── reading ─────────

/// Owner, or a therapist with AI on: this patient's recordings (their own; the owner sees all). No content.
pub async fn list(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    a.require_ai()?;
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object(
                'id', r.id, 'status', r.status, 'seconds', r.seconds, 'chunks', r.chunks, 'template', r.template, 'lang', r.lang,
                'error', r.error, 'note_id', r.note_id, 'created_by', r.created_by, 'created_by_name', st.name,
                'created_at', r.created_at, 'ready_at', r.ready_at, 'delete_after', r.delete_after) ORDER BY r.created_at DESC), '[]')
         FROM recordings r LEFT JOIN staff st ON st.id = r.created_by
         WHERE r.patient_id = $1 AND r.deleted_at IS NULL AND (r.created_by = $2 OR $3)")
        .bind(pid).bind(a.staff.id).bind(a.staff.role == "owner").fetch_one(&s.pool).await?;
    Ok(Json(json!({ "recordings": v, "speech": stt::cached_status().await })))
}

/// The author or the owner: the AI's draft (report, proposed block values, plan) and the transcript, once ready.
pub async fn get(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    a.require_ai()?;
    let (pid, status, _) = mine(&s, &a, id).await?;
    if status != "ready" {
        return Err(AppError::Conflict("The draft isn’t ready yet."));
    }
    let folder = dir(&s, id);
    let draft: Value = tokio::fs::read_to_string(folder.join("draft.json")).await.ok().and_then(|t| serde_json::from_str(&t).ok())
        .ok_or(AppError::Conflict("The draft isn’t ready yet."))?;
    let transcript = tokio::fs::read_to_string(folder.join("transcript.txt")).await.unwrap_or_default();
    let (template, lang): (String, String) = sqlx::query_as("SELECT template, lang FROM recordings WHERE id = $1").bind(id).fetch_one(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "recording.opened", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({
        "id": id, "template": template, "lang": lang, "report": draft["report"], "plan": draft["plan"],
        "values": draft["values"], "model": draft["model"], "transcript": transcript,
    })))
}

/// The author or the owner: the audio (WAV), to listen inside the app. Logged.
pub async fn audio(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Response> {
    require_clinical(&a)?;
    a.require_ai()?;
    let (pid, status, _) = mine(&s, &a, id).await?;
    if status == "recording" {
        return Err(AppError::Conflict("The recording hasn’t finished yet."));
    }
    let bytes = tokio::fs::read(dir(&s, id).join("audio.wav")).await.map_err(|_| AppError::NotFound)?;
    auth::log(&s.pool, Some(a.staff.id), "recording.played", subject(pid), &a.device, &id.to_string()).await;
    let mut res = Body::from(bytes).into_response();
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/wav"));
    Ok(res)
}

/// The author or the owner: a failed recording goes back in the queue.
pub async fn retry(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    a.require_ai()?;
    let (_, status, _) = mine(&s, &a, id).await?;
    if status != "failed" {
        return Err(AppError::Conflict("Only a recording that failed can be tried again."));
    }
    sqlx::query("UPDATE recordings SET status = 'queued', attempts = 0, error = '' WHERE id = $1").bind(id).execute(&s.pool).await?;
    Ok(Json(json!({ "status": "queued" })))
}

/// The author or the owner: delete the recording now (audio, transcript and draft).
pub async fn delete(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    require_clinical(&a)?;
    mine(&s, &a, id).await?;
    purge(&s, id, "by_hand", Some(&a)).await?;
    Ok(Json(json!({ "ok": true })))
}

// ───────── deleting ─────────

/// Removes the recording's folder and marks the row deleted. Logged with why (never the content).
pub async fn purge(s: &AppState, id: Uuid, why: &str, a: Option<&Auth>) -> ApiResult<()> {
    let folder = dir(s, id);
    if let Err(e) = tokio::fs::remove_dir_all(&folder).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(AppError::Internal(format!("audio delete: {e}")));
        }
    }
    let r: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE recordings SET status = 'deleted', deleted_at = now(), deleted_why = $2 WHERE id = $1 AND deleted_at IS NULL RETURNING patient_id")
        .bind(id).bind(why).fetch_optional(&s.pool).await?;
    if let Some((pid,)) = r {
        let (who, device) = a.map(|a| (Some(a.staff.id), a.device.as_str())).unwrap_or((None, "server"));
        auth::log(&s.pool, who, "recording.deleted", subject(pid), device, &format!("{id} · {why}")).await;
    }
    Ok(())
}

/// A note was signed: the recordings it was written from are deleted (owner decision, 11 Nov).
pub async fn note_signed(s: &AppState, a: &Auth, note: Uuid) {
    let ids: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM recordings WHERE note_id = $1 AND deleted_at IS NULL").bind(note).fetch_all(&s.pool).await.unwrap_or_default();
    for (id,) in ids {
        if let Err(e) = purge(s, id, "signed", Some(a)).await {
            tracing::warn!("recording not deleted after signing: {e:?}");
        }
    }
}

/// A note draft points at the recording it came from (`recording_id` in the note body). Author or owner only,
/// same patient, and only a recording whose draft is ready.
pub async fn link_note(s: &AppState, a: &Auth, pid: Uuid, note: Uuid, recording: Option<&Value>) -> ApiResult<()> {
    let Some(rid) = recording.and_then(Value::as_str).and_then(|r| Uuid::parse_str(r).ok()) else { return Ok(()) };
    sqlx::query("UPDATE recordings SET note_id = $1 WHERE id = $2 AND patient_id = $3 AND deleted_at IS NULL AND status = 'ready' AND (created_by = $4 OR $5)")
        .bind(note).bind(rid).bind(pid).bind(a.staff.id).bind(a.staff.role == "owner").execute(&s.pool).await?;
    Ok(())
}

/// The owner's legal tools (delete or anonymise a patient, delete a day): the recordings they cover, marked deleted
/// inside the tool's transaction (`mark_sql` with $1 = patient id or day, $2 = time zone). Returns their ids.
pub async fn mark_deleted(tx: &mut sqlx::PgConnection, filter: &str, why: &str, arg: &str) -> ApiResult<Vec<Uuid>> {
    let sql = format!("UPDATE recordings SET status = 'deleted', deleted_at = now(), deleted_why = $3 WHERE deleted_at IS NULL AND {filter} RETURNING id");
    let ids: Vec<(Uuid,)> = sqlx::query_as(&sql).bind(arg).bind(super::schedule::tz()).bind(why).fetch_all(tx).await?;
    Ok(ids.into_iter().map(|x| x.0).collect())
}

/// After the tool committed: the folders go, and each removal is logged.
pub async fn remove_files(s: &AppState, a: &Auth, ids: &[Uuid], why: &str) {
    for id in ids {
        let _ = tokio::fs::remove_dir_all(dir(s, *id)).await;
        auth::log(&s.pool, Some(a.staff.id), "recording.deleted", None, &a.device, &format!("{id} · {why}")).await;
    }
}

/// Folders whose recording is gone (patient deleted, row removed) or marked deleted: removed.
pub async fn sweep(s: &AppState) {
    let Ok(mut rd) = tokio::fs::read_dir(&s.audio).await else { return };
    let live: Vec<(Uuid,)> = match sqlx::query_as("SELECT id FROM recordings WHERE deleted_at IS NULL").fetch_all(&s.pool).await {
        Ok(v) => v,
        Err(_) => return,
    };
    let live: std::collections::HashSet<Uuid> = live.into_iter().map(|x| x.0).collect();
    while let Ok(Some(e)) = rd.next_entry().await {
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(id) = Uuid::parse_str(&name) else { continue };
        // A folder made a moment ago may not have its row yet.
        let young = e.metadata().await.ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|d| d < Duration::from_secs(120));
        if !live.contains(&id) && !young {
            let _ = tokio::fs::remove_dir_all(e.path()).await;
        }
    }
}

// ───────── the worker ─────────

/// Starts the background worker: one recording at a time (the AI PC has one GPU), oldest first.
pub fn spawn_worker(s: AppState) {
    let every = std::env::var("RECORDING_POLL_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(15u64);
    tokio::spawn(async move {
        // Work interrupted by a restart starts again.
        let _ = sqlx::query("UPDATE recordings SET status = 'queued' WHERE status IN ('transcribing', 'drafting')").execute(&s.pool).await;
        let mut last_sweep = std::time::Instant::now() - Duration::from_secs(3600);
        loop {
            if last_sweep.elapsed() > Duration::from_secs(600) || every < 5 {
                expire(&s).await;
                sweep(&s).await;
                last_sweep = std::time::Instant::now();
            }
            let worked = run_next(&s).await;
            if !worked {
                tokio::time::sleep(Duration::from_secs(every)).await;
            }
        }
    });
}

/// Recordings past their 30 days (never signed): deleted.
async fn expire(s: &AppState) {
    let ids: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM recordings WHERE deleted_at IS NULL AND delete_after < now()").fetch_all(&s.pool).await.unwrap_or_default();
    for (id,) in ids {
        let _ = purge(s, id, "expired", None).await;
    }
}

/// Does the next queued recording if the AI PC can take it. True if something was done.
async fn run_next(s: &AppState) -> bool {
    let next: Option<(Uuid, String, i32)> = sqlx::query_as(
        "SELECT id, lang, attempts FROM recordings WHERE status = 'queued' AND deleted_at IS NULL ORDER BY finished_at, created_at LIMIT 1")
        .fetch_optional(&s.pool).await.ok().flatten();
    let Some((id, lang, attempts)) = next else { return false };
    let folder = dir(s, id);
    let has_text = tokio::fs::metadata(folder.join("transcript.txt")).await.is_ok();
    // Nothing to do until the program it needs answers (the AI PC may be asleep): it stays queued.
    if !has_text && stt::status().await["available"] != true {
        return false;
    }
    if ai::status().await["available"] != true && has_text {
        return false;
    }
    let first = if has_text { "drafting" } else { "transcribing" };
    if sqlx::query("UPDATE recordings SET status = $2, attempts = attempts + 1 WHERE id = $1 AND deleted_at IS NULL").bind(id).bind(first).execute(&s.pool).await.is_err() {
        return false;
    }
    if !has_text {
        match stt::transcribe(&folder.join("audio.wav"), &lang).await {
            Ok(text) if !text.trim().is_empty() => {
                if tokio::fs::write(folder.join("transcript.txt"), &text).await.is_err() {
                    return fail_or_requeue(s, id, attempts, "stt_failed").await;
                }
            }
            Ok(_) => return fail(s, id, "no_speech").await,
            Err(stt::SttError::Offline) => { requeue(s, id).await; return false; }
            Err(stt::SttError::Failed) => return fail_or_requeue(s, id, attempts, "stt_failed").await,
        }
    }
    let _ = sqlx::query("UPDATE recordings SET status = 'drafting' WHERE id = $1 AND deleted_at IS NULL").bind(id).execute(&s.pool).await;
    let transcript = tokio::fs::read_to_string(folder.join("transcript.txt")).await.unwrap_or_default();
    let blocks: Value = tokio::fs::read_to_string(folder.join("blocks.json")).await.ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!([]));
    match draft(&transcript, &blocks, lang == "en").await {
        Ok(d) => {
            if tokio::fs::write(folder.join("draft.json"), d.to_string()).await.is_err() {
                return fail_or_requeue(s, id, attempts, "ai_failed").await;
            }
            let _ = sqlx::query("UPDATE recordings SET status = 'ready', ready_at = now(), error = '', model = $2 WHERE id = $1 AND deleted_at IS NULL")
                .bind(id).bind(ai::model()).execute(&s.pool).await;
            true
        }
        Err(AiError::Offline) => { requeue(s, id).await; false }
        Err(AiError::NoModel) => fail(s, id, "ai_no_model").await,
        Err(_) => fail_or_requeue(s, id, attempts, "ai_failed").await,
    }
}

/// The AI PC went away mid-way: back in the queue, and that try doesn't count.
async fn requeue(s: &AppState, id: Uuid) {
    let _ = sqlx::query("UPDATE recordings SET status = 'queued', attempts = greatest(attempts - 1, 0) WHERE id = $1 AND deleted_at IS NULL").bind(id).execute(&s.pool).await;
}
async fn fail(s: &AppState, id: Uuid, why: &str) -> bool {
    let _ = sqlx::query("UPDATE recordings SET status = 'failed', error = $2 WHERE id = $1 AND deleted_at IS NULL").bind(id).bind(why).execute(&s.pool).await;
    true
}
/// Three tries, then it waits for someone to press Try Again.
async fn fail_or_requeue(s: &AppState, id: Uuid, attempts: i32, why: &str) -> bool {
    if attempts + 1 >= 3 { fail(s, id, why).await } else {
        let _ = sqlx::query("UPDATE recordings SET status = 'queued', error = $2 WHERE id = $1 AND deleted_at IS NULL").bind(id).bind(why).execute(&s.pool).await;
        true
    }
}

// ───────── the AI's draft ─────────

const REPORT_PT: &str = "És um fisioterapeuta em Portugal a escrever a nota de evolução de uma sessão, para o processo do utente, \
a partir da transcrição automática da conversa da sessão. A transcrição não diz quem fala: é uma conversa entre o fisioterapeuta e o utente \
(às vezes com um acompanhante); percebe pelo contexto quem disse o quê — o fisioterapeuta pergunta, explica, mede e orienta exercícios; \
o utente descreve dor, sintomas e o dia a dia. A transcrição pode ter palavras mal reconhecidas: se algo não fizer sentido ou um valor não for claro, não o uses. \
Usa apenas o que foi dito ou medido na sessão: não inventes valores, testes, diagnósticos, técnicas nem respostas do utente. \
Ignora conversa de circunstância. Não escrevas o plano para a próxima sessão (é pedido à parte). Um a quatro parágrafos curtos.";
const REPORT_EN: &str = "You are a physiotherapist writing up the progress note for one session, for the patient's record, from an automatic \
transcript of the session's conversation. The transcript doesn't say who is speaking: it is a conversation between the physiotherapist and the patient \
(sometimes with a companion); work out from the context who said what — the physiotherapist asks, explains, measures and guides exercises; \
the patient describes pain, symptoms and daily life. The transcript may have misheard words: if something makes no sense or a value isn't clear, leave it out. \
Use only what was said or measured in the session: never invent values, tests, diagnoses, techniques or what the patient said. \
Ignore small talk. Don't write the plan for next session (it is asked for separately). One to four short paragraphs.";

const FIELDS_PT: &str = "Lês a transcrição automática de uma sessão de fisioterapia (conversa entre fisioterapeuta e utente, sem indicação de quem fala) \
e preenches os campos da nota. Responde só com o JSON pedido. Em cada campo, põe um valor só se foi dito ou medido claramente nesta sessão; \
senão usa null nas escalas, \"\" nas escolhas e no texto, e [] nas listas. Nas escolhas e listas usa só as opções dadas, escritas igual. \
plan: o plano para a próxima sessão em uma ou duas frases de texto corrido, em português europeu, no presente, com «o utente»; \"\" se não se falou disso.";
const FIELDS_EN: &str = "You read the automatic transcript of a physiotherapy session (a conversation between physiotherapist and patient, with no speaker labels) \
and fill in the note's fields. Answer only with the JSON asked for. In each field, put a value only if it was clearly said or measured in this session; \
otherwise use null for scales, \"\" for choices and text, and [] for lists. For choices and lists use only the options given, written exactly the same. \
plan: the plan for next session in one or two sentences of plain text, British English, present tense, \"the patient\"; \"\" if it wasn't discussed.";

/// The template's blocks, cleaned: what the AI is allowed to fill in (no values; at most 20 blocks).
pub(crate) fn template_blocks(v: &Value) -> Value {
    let clean = |s: &str, n: usize| s.chars().filter(|c| !c.is_control()).take(n).collect::<String>().trim().to_string();
    let mut out = Vec::new();
    for b in v.as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().take(20) {
        let key: String = b["key"].as_str().unwrap_or("").chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(40).collect();
        let label = clean(b["label"].as_str().unwrap_or(""), 80);
        if key.is_empty() || label.is_empty() { continue; }
        let opts = || json!(b["options"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(|o| clean(o, 80)).filter(|o| !o.is_empty()).take(20).collect::<Vec<_>>()).unwrap_or_default());
        let blk = match b["type"].as_str().unwrap_or("") {
            "slider" => {
                let min = b["min"].as_f64().filter(|x| x.is_finite()).unwrap_or(0.0);
                let max = b["max"].as_f64().filter(|x| x.is_finite() && *x > min).unwrap_or(10.0);
                json!({ "type": "slider", "key": key, "label": label, "min": min, "max": max, "step": b["step"].as_f64().filter(|x| *x > 0.0).unwrap_or(1.0), "unit": clean(b["unit"].as_str().unwrap_or(""), 10) })
            }
            "choice" => json!({ "type": "choice", "key": key, "label": label, "options": opts() }),
            "checklist" => json!({ "type": "checklist", "key": key, "label": label, "options": opts() }),
            "text" => json!({ "type": "text", "key": key, "label": label }),
            _ => continue,
        };
        out.push(blk);
    }
    Value::Array(out)
}

/// The JSON schema the note model answers in: one property per block, plus `plan`.
fn fields_schema(blocks: &Value) -> Value {
    let mut props = serde_json::Map::new();
    for b in blocks.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let k = b["key"].as_str().unwrap_or("").to_string();
        let sch = match b["type"].as_str().unwrap_or("") {
            "slider" => json!({ "type": ["number", "null"] }),
            "choice" => { let mut o = b["options"].as_array().cloned().unwrap_or_default(); o.push(json!("")); json!({ "type": "string", "enum": o }) }
            "checklist" => json!({ "type": "array", "items": { "type": "string", "enum": b["options"] } }),
            _ => json!({ "type": "string" }),
        };
        props.insert(k, sch);
    }
    let keys: Vec<String> = props.keys().cloned().collect();
    json!({
        "type": "object",
        "properties": { "values": { "type": "object", "properties": props, "required": keys }, "plan": { "type": "string" } },
        "required": ["values", "plan"]
    })
}

fn fields_prompt(blocks: &Value, transcript: &str, en: bool) -> String {
    let mut lines = Vec::new();
    for b in blocks.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let (k, l) = (b["key"].as_str().unwrap_or(""), b["label"].as_str().unwrap_or(""));
        let opts = |b: &Value| b["options"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" | ")).unwrap_or_default();
        lines.push(match b["type"].as_str().unwrap_or("") {
            "slider" => format!("- {k}: {l} — {} {}–{}{}", if en { "number" } else { "número" }, b["min"], b["max"], b["unit"].as_str().unwrap_or("")),
            "choice" => format!("- {k}: {l} — {}: {}", if en { "one of" } else { "uma de" }, opts(b)),
            "checklist" => format!("- {k}: {l} — {}: {}", if en { "any of" } else { "várias de" }, opts(b)),
            _ => format!("- {k}: {l} — {}", if en { "short text" } else { "texto curto" }),
        });
    }
    let fields = if lines.is_empty() { (if en { "(no fields)" } else { "(sem campos)" }).to_string() } else { lines.join("\n") };
    if en { format!("Fields:\n{fields}\n\nTranscript:\n{transcript}") } else { format!("Campos:\n{fields}\n\nTranscrição:\n{transcript}") }
}

/// What the model proposed → only values that fit the block (in range, one of the options). Others are left out.
pub(crate) fn check_values(blocks: &Value, v: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for b in blocks.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let k = b["key"].as_str().unwrap_or("");
        let x = &v[k];
        let opts: Vec<&str> = b["options"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let good = match b["type"].as_str().unwrap_or("") {
            "slider" => x.as_f64().filter(|n| n.is_finite()).and_then(|n| {
                let (min, max, step) = (b["min"].as_f64()?, b["max"].as_f64()?, b["step"].as_f64().unwrap_or(1.0));
                (n >= min && n <= max).then(|| { let r = min + ((n - min) / step).round() * step; json!(if r.fract() == 0.0 { json!(r as i64) } else { json!(r) }) })
            }),
            "choice" => x.as_str().filter(|s| opts.contains(s)).map(|s| json!(s)),
            "checklist" => x.as_array().map(|a| {
                let picked: Vec<&str> = opts.iter().copied().filter(|o| a.iter().any(|y| y.as_str() == Some(o))).collect();
                json!(picked)
            }).filter(|p| p.as_array().is_some_and(|a| !a.is_empty())),
            "text" => x.as_str().map(|s| style::tidy(s).replace('\n', " ")).map(|s| s.chars().take(500).collect::<String>()).filter(|s| !s.trim().is_empty() && style::problem(s).is_none()).map(|s| json!(s)),
            _ => None,
        };
        if let Some(g) = good { out.insert(k.to_string(), g); }
    }
    Value::Object(out)
}

/// The draft: a plain report (style-checked), proposed values for the blocks and a plan. The values and the
/// plan are a bonus: if that second answer fails, the report still comes back with them empty.
async fn draft(transcript: &str, blocks: &Value, en: bool) -> Result<Value, AiError> {
    let t: String = transcript.chars().take(MAX_TRANSCRIPT).collect();
    let ctx = ((t.chars().count() / 3) as u32 + 3000).clamp(8192, 32768);
    let (sys, rules, example) = if en { (REPORT_EN, style::RULES_EN, style::EXAMPLE_EN) } else { (REPORT_PT, style::RULES_PT, style::EXAMPLE_PT) };
    let system = format!("{sys} {rules}\n\n{example}");
    let prompt = if en { format!("Session transcript:\n{t}\n\nWrite the note.") } else { format!("Transcrição da sessão:\n{t}\n\nEscreve a nota.") };
    let report = style::generate_plain_ctx(&system, &prompt, en, Some(ctx)).await?;
    let (values, plan) = match ai::generate_json_long(if en { FIELDS_EN } else { FIELDS_PT }, &fields_prompt(blocks, &t, en), &fields_schema(blocks), ctx).await {
        Ok(v) => {
            let plan = style::tidy(v["plan"].as_str().unwrap_or(""));
            let plan = if style::problem(&plan).is_none() { plan.replace('\n', " ").chars().take(600).collect::<String>() } else { String::new() };
            (check_values(blocks, &v["values"]), plan)
        }
        Err(AiError::Offline) => return Err(AiError::Offline),
        Err(_) => (json!({}), String::new()),
    };
    Ok(json!({ "report": report.trim(), "plan": plan.trim(), "values": values, "model": ai::model() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn blocks() -> Value {
        template_blocks(&json!([
            { "type": "slider", "key": "pain", "label": "Dor (0–10)", "value": 6, "min": 0, "max": 10 },
            { "type": "slider", "key": "rotation", "label": "Rotação", "min": 0, "max": 90, "step": 5, "unit": "°" },
            { "type": "choice", "key": "slr", "label": "Lasègue", "value": "", "options": ["Negativo", "Positivo à direita"] },
            { "type": "checklist", "key": "techniques", "label": "Técnicas", "value": [], "options": ["Mobilização articular", "Taping"] },
            { "type": "text", "key": "note", "label": "Observações" },
            { "type": "slider", "key": "", "label": "sem chave" },
            { "type": "video", "key": "x", "label": "?" }
        ]))
    }
    #[test]
    fn only_known_blocks_without_values() {
        let b = blocks();
        assert_eq!(b.as_array().unwrap().len(), 5);
        assert!(b[0].get("value").is_none());
        assert_eq!(b[1]["step"], 5.0);
    }
    #[test]
    fn values_must_fit() {
        let v = check_values(&blocks(), &json!({
            "pain": 4, "rotation": 72, "slr": "Positivo à direita", "techniques": ["Taping", "Ventosas"], "note": "**Sem** queixas de tonturas."
        }));
        assert_eq!(v, json!({ "pain": 4, "rotation": 70, "slr": "Positivo à direita", "techniques": ["Taping"], "note": "Sem queixas de tonturas." }));
        let v = check_values(&blocks(), &json!({ "pain": 14, "rotation": null, "slr": "Talvez", "techniques": [], "note": "" }));
        assert_eq!(v, json!({}));
    }
    #[test]
    fn schema_lists_every_block() {
        let s = fields_schema(&blocks());
        assert_eq!(s["properties"]["values"]["required"].as_array().unwrap().len(), 5);
        assert_eq!(s["properties"]["values"]["properties"]["slr"]["enum"], json!(["Negativo", "Positivo à direita", ""]));
    }
}
