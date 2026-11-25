//! Speech-to-text on the clinic's AI PC: a whisper.cpp server (`whisper-server`), reached over the clinic
//! network only (the same local-address rule as the AI: `ai::parse`). Phase 2 of the PhysioPulse plan.
//!
//! Where it is: Settings › Local AI (`clinic.stt_url`), else WHISPER_URL. Audio goes to it as a 16 kHz mono
//! WAV file; the answer is the text. Nothing is logged, and nothing leaves the clinic.

use serde_json::{json, Value};
use sqlx::PgPool;
use std::{path::Path, sync::RwLock, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const DEFAULT_PORT: u16 = 8178;

pub enum SttError {
    /// No address set, or the program isn't answering there.
    Offline,
    /// It answered, but not with text (wrong program, out of memory, …).
    Failed,
}

static CHOSEN: RwLock<Option<String>> = RwLock::new(None);

pub fn env_url() -> String {
    std::env::var("WHISPER_URL").ok().and_then(|u| crate::ai::normalise_port(&u, DEFAULT_PORT)).unwrap_or_default()
}
/// The address in use ("" = not set up).
pub fn url() -> String {
    CHOSEN.read().unwrap().clone().unwrap_or_else(env_url)
}
pub fn chosen_in_app() -> bool {
    CHOSEN.read().unwrap().is_some()
}
pub fn set_chosen(u: Option<String>) {
    *CHOSEN.write().unwrap() = u;
}
pub async fn load(pool: &PgPool) {
    match sqlx::query_as::<_, (Option<String>,)>("SELECT stt_url FROM clinic WHERE id = 1").fetch_optional(pool).await {
        Ok(Some((u,))) => set_chosen(u.filter(|u| crate::ai::parse(u).is_some())),
        Ok(None) => {}
        Err(e) => tracing::warn!("could not read the speech-to-text address: {e}"),
    }
}

/// Opens a connection to `url` (local addresses only).
async fn connect(url: &str) -> Result<(tokio::net::TcpStream, String, u16), SttError> {
    let (host, port) = crate::ai::parse(url).ok_or(SttError::Offline)?;
    let sock = tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect((host.as_str(), port)))
        .await.map_err(|_| SttError::Offline)?.map_err(|_| SttError::Offline)?;
    Ok((sock, host, port))
}

async fn read_answer(sock: &mut tokio::net::TcpStream) -> Result<(u16, String), SttError> {
    let mut buf = Vec::new();
    sock.read_to_end(&mut buf).await.map_err(|_| SttError::Failed)?;
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, body) = text.split_once("\r\n\r\n").ok_or(SttError::Failed)?;
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or(SttError::Failed)?;
    Ok((status, body.to_string()))
}

/// Is a whisper server answering at `url`? `{available, url, reason?}` — reason: not_set | not_local | offline.
pub async fn check(url: &str) -> Value {
    if url.is_empty() {
        return json!({ "available": false, "url": "", "reason": "not_set" });
    }
    if crate::ai::parse(url).is_none() {
        return json!({ "available": false, "url": url, "reason": "not_local" });
    }
    let run = async {
        let (mut sock, host, port) = connect(url).await?;
        sock.write_all(format!("GET / HTTP/1.0\r\nHost: {host}:{port}\r\n\r\n").as_bytes()).await.map_err(|_| SttError::Offline)?;
        read_answer(&mut sock).await
    };
    match tokio::time::timeout(Duration::from_secs(6), run).await {
        Ok(Ok((s, _))) if s < 500 => json!({ "available": true, "url": url }),
        _ => json!({ "available": false, "url": url, "reason": "offline" }),
    }
}

/// The last answer of `status()`, so screens don't wait 5 s for an AI PC that's off.
static LAST: RwLock<Option<Value>> = RwLock::new(None);

pub async fn status() -> Value {
    let v = check(&url()).await;
    *LAST.write().unwrap() = Some(v.clone());
    v
}

/// The last known status (the worker refreshes it while recordings wait); asks now if never asked.
pub async fn cached_status() -> Value {
    let last = LAST.read().unwrap().clone();
    match last {
        Some(v) if v["url"] == json!(url()) => v,
        _ => status().await,
    }
}

/// The whisper server's answer → plain text, one line per segment (the pauses help the note model tell
/// who is speaking). Accepts `verbose_json` (segments) or plain `json` (`text`).
pub fn answer_text(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body.trim()).ok()?;
    if v.get("error").is_some() {
        return None;
    }
    let lines: Vec<String> = match v.get("segments").and_then(Value::as_array) {
        Some(segs) if !segs.is_empty() => segs.iter().filter_map(|s| s.get("text").and_then(Value::as_str)).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
        _ => v.get("text").and_then(Value::as_str).map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default(),
    };
    // whisper writes these for silence or noise.
    let lines: Vec<String> = lines.into_iter().filter(|l| !matches!(l.as_str(), "[BLANK_AUDIO]" | "[ Silence ]" | "(silence)" | "[MUSIC]")).collect();
    Some(lines.join("\n"))
}

/// Sends a WAV file to the whisper server and returns the text. Long sessions take a while: up to 60 min.
pub async fn transcribe(wav: &Path, lang: &str) -> Result<String, SttError> {
    let url = url();
    if url.is_empty() {
        return Err(SttError::Offline);
    }
    let size = tokio::fs::metadata(wav).await.map_err(|_| SttError::Failed)?.len();
    let boundary = format!("----physiopulse{}", uuid::Uuid::new_v4().simple());
    let field = |name: &str, value: &str| format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n");
    let mut head = String::new();
    head.push_str(&field("response_format", "verbose_json"));
    head.push_str(&field("language", if lang == "en" { "en" } else { "pt" }));
    head.push_str(&field("temperature", "0.0"));
    head.push_str(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"));
    let tail = format!("\r\n--{boundary}--\r\n");
    let len = head.len() as u64 + size + tail.len() as u64;
    let run = async {
        let (mut sock, host, port) = connect(&url).await?;
        let req = format!("POST /inference HTTP/1.0\r\nHost: {host}:{port}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {len}\r\n\r\n");
        sock.write_all(req.as_bytes()).await.map_err(|_| SttError::Offline)?;
        sock.write_all(head.as_bytes()).await.map_err(|_| SttError::Offline)?;
        let mut f = tokio::fs::File::open(wav).await.map_err(|_| SttError::Failed)?;
        tokio::io::copy(&mut f, &mut sock).await.map_err(|_| SttError::Offline)?;
        sock.write_all(tail.as_bytes()).await.map_err(|_| SttError::Offline)?;
        read_answer(&mut sock).await
    };
    let (status, body) = tokio::time::timeout(Duration::from_secs(3600), run).await.map_err(|_| SttError::Failed)??;
    if status != 200 {
        return Err(SttError::Failed);
    }
    answer_text(&body).ok_or(SttError::Failed)
}

/// A WAV header for 16-bit mono PCM at `rate` Hz with `data_len` bytes of samples.
pub fn wav_header(data_len: u32, rate: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn segments_become_lines() {
        let b = r#"{"text":"x","segments":[{"text":" Bom dia. "},{"text":"[BLANK_AUDIO]"},{"text":" Como está o ombro?"}]}"#;
        assert_eq!(answer_text(b).as_deref(), Some("Bom dia.\nComo está o ombro?"));
        assert_eq!(answer_text(r#"{"text":" Olá\n Tudo bem "}"#).as_deref(), Some("Olá\nTudo bem"));
        assert_eq!(answer_text(r#"{"error":"failed to read audio"}"#), None);
        assert_eq!(answer_text("<html>"), None);
    }
    #[test]
    fn header_is_a_wav() {
        let h = wav_header(32000, 16000);
        assert_eq!(&h[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(h[4..8].try_into().unwrap()), 32036);
        assert_eq!(u32::from_le_bytes(h[24..28].try_into().unwrap()), 16000);
        assert_eq!(u32::from_le_bytes(h[28..32].try_into().unwrap()), 32000);
        assert_eq!(u32::from_le_bytes(h[40..44].try_into().unwrap()), 32000);
    }
    #[test]
    fn whisper_addresses() {
        assert_eq!(crate::ai::normalise_port("192.168.1.66", DEFAULT_PORT).as_deref(), Some("http://192.168.1.66:8178"));
        assert_eq!(crate::ai::normalise_port("100.86.97.21:9000", DEFAULT_PORT).as_deref(), Some("http://100.86.97.21:9000"));
        assert_eq!(crate::ai::normalise_port("api.openai.com", DEFAULT_PORT), None);
    }
}
