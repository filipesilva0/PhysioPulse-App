//! The clinic's local AI: Ollama, on this server (`docker compose --profile ai`) or on another clinic computer.
//!
//! Where it is and which model to use are set by the owner in the app (Settings › Local AI, stored in
//! `clinic.ai_url` / `clinic.ai_model`); until then the server's OLLAMA_URL / OLLAMA_MODEL are used.
//!
//! Health data never leaves the clinic: the address must be the `ollama` container, this machine or a private
//! network address over plain HTTP, never the internet. Prompts and answers are never logged.
//! A minimal HTTP/1.0 client (no extra dependencies): one request, `stream: false`, read to the end.

use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::RwLock;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub enum AiError {
    /// Ollama isn't running or isn't reachable, or the address isn't a local one.
    Offline,
    /// The model isn't downloaded yet (`ollama pull <model>`).
    NoModel,
    /// It took too long or answered something unreadable.
    Failed,
    /// A picture was sent but the chosen model can't read pictures.
    NoVision,
}

/// What the owner chose in the app (None = use the server's environment).
static CHOSEN: RwLock<(Option<String>, Option<String>)> = RwLock::new((None, None));
/// The model the assistant uses, if the owner chose one apart (Settings › Local AI, 13 Nov 2026): it needs a model
/// that can use tools (qwen3, llama3.1…), while letters and notes may stay on another. None = the main model.
static ASSISTANT: RwLock<Option<String>> = RwLock::new(None);

const DEFAULT_URL: &str = "http://ollama:11434";
const DEFAULT_MODEL: &str = "gemma3:12b";

pub fn env_url() -> String {
    std::env::var("OLLAMA_URL").ok().filter(|u| !u.trim().is_empty()).unwrap_or_else(|| DEFAULT_URL.into())
}
pub fn env_model() -> String {
    std::env::var("OLLAMA_MODEL").ok().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| DEFAULT_MODEL.into())
}

/// The address in use: the app's choice, else OLLAMA_URL, else the `ollama` container.
pub fn url() -> String {
    CHOSEN.read().unwrap().0.clone().unwrap_or_else(env_url)
}
/// The model in use: the app's choice, else OLLAMA_MODEL, else gemma3:12b.
pub fn model() -> String {
    CHOSEN.read().unwrap().1.clone().unwrap_or_else(env_model)
}
/// Was the address / model chosen in the app (true) or does it come from the server's settings (false)?
pub fn chosen_in_app() -> (bool, bool) {
    let c = CHOSEN.read().unwrap();
    (c.0.is_some(), c.1.is_some())
}

pub fn set_chosen(url: Option<String>, model: Option<String>) {
    *CHOSEN.write().unwrap() = (url, model);
}

/// The assistant's model: its own if the owner chose one, else the main model.
pub fn assistant_model() -> String {
    ASSISTANT.read().unwrap().clone().unwrap_or_else(model)
}
/// Did the owner choose a model just for the assistant?
pub fn assistant_model_chosen() -> Option<String> {
    ASSISTANT.read().unwrap().clone()
}
pub fn set_assistant_model(m: Option<String>) {
    *ASSISTANT.write().unwrap() = m;
}

/// At start-up: read what the owner chose in the app.
pub async fn load(pool: &PgPool) {
    match sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>)>("SELECT ai_url, ai_model, ai_assistant_model FROM clinic WHERE id = 1").fetch_optional(pool).await {
        Ok(Some((u, m, am))) => {
            set_chosen(u.filter(|u| parse(u).is_some()), m.filter(|m| model_ok(m)));
            set_assistant_model(am.filter(|m| model_ok(m)));
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("could not read the AI settings: {e}"),
    }
}

/// What people type ("100.86.97.21", "100.86.97.21:11434", "http://pc-ia:11434/") → "http://host:port",
/// only if the host is local or private. None otherwise.
pub fn normalise(input: &str) -> Option<String> {
    normalise_port(input, 11434)
}

/// The same, with another default port (the speech-to-text server's is 8178).
pub fn normalise_port(input: &str, default_port: u16) -> Option<String> {
    let t = input.trim().trim_end_matches('/');
    let rest = t.strip_prefix("http://").unwrap_or(t);
    if rest.is_empty() || rest.contains("://") {
        return None;
    }
    let with_port = if rest.rsplit_once(':').is_some_and(|(_, p)| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty()) { rest.to_string() } else { format!("{rest}:{default_port}") };
    let url = format!("http://{with_port}");
    parse(&url).map(|_| url)
}

/// `http://host:port` → (host, port), only if the host is local or private.
pub(crate) fn parse(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("http://")?.trim_end_matches('/');
    if rest.contains('/') || rest.contains('@') || rest.contains('?') || rest.contains(char::is_whitespace) {
        return None;
    }
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None => (rest.to_string(), 80),
    };
    (port != 0 && is_local(&host)).then_some((host, port))
}

/// A model name like "gemma3:12b" or "qwen2.5vl:7b" (letters, digits and . _ - : /).
pub fn model_ok(m: &str) -> bool {
    !m.is_empty() && m.len() <= 100 && m.chars().all(|c| c.is_ascii_alphanumeric() || "._-:/".contains(c))
}

/// The container name, localhost, or a private / Tailscale (100.64.0.0/10) IPv4 address.
/// Never a public address or a name that DNS could send outside the clinic.
fn is_local(host: &str) -> bool {
    if matches!(host, "ollama" | "localhost") {
        return true;
    }
    let Ok(ip) = host.parse::<std::net::Ipv4Addr>() else { return false };
    let o = ip.octets();
    ip.is_loopback() || ip.is_private() || (o[0] == 100 && (64..128).contains(&o[1]))
}

async fn request(url: &str, method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<(u16, Value), AiError> {
    let (host, port) = parse(url).ok_or(AiError::Offline)?;
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let run = async {
        let mut sock = tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect((host.as_str(), port)))
            .await.map_err(|_| AiError::Offline)?.map_err(|_| AiError::Offline)?;
        let req = if body.is_some() {
            format!("{method} {path} HTTP/1.0\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", payload.len())
        } else {
            format!("{method} {path} HTTP/1.0\r\nHost: {host}:{port}\r\n\r\n")
        };
        sock.write_all(req.as_bytes()).await.map_err(|_| AiError::Offline)?;
        sock.write_all(payload.as_bytes()).await.map_err(|_| AiError::Offline)?;
        let mut buf = Vec::new();
        sock.read_to_end(&mut buf).await.map_err(|_| AiError::Failed)?;
        Ok::<_, AiError>(buf)
    };
    let buf = tokio::time::timeout(timeout, run).await.map_err(|_| AiError::Failed)??;
    let text = String::from_utf8_lossy(&buf);
    let (head, body) = text.split_once("\r\n\r\n").ok_or(AiError::Failed)?;
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or(AiError::Failed)?;
    Ok((status, merge_stream(body)))
}

/// Ollama answers one JSON object, or (with `stream: true`) one object per line: the text arrives in pieces in
/// `response`. This joins the pieces into one object, like a non-streamed answer. An `error` line wins.
fn merge_stream(body: &str) -> Value {
    let t = body.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return v;
    }
    let mut text = String::new();
    let mut said = String::new();
    let mut calls: Vec<Value> = Vec::new();
    let mut last = Value::Null;
    for line in t.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("error").is_some() {
            return v;
        }
        if let Some(r) = v.get("response").and_then(Value::as_str) {
            text.push_str(r);
        }
        // `/api/chat`: the text comes in `message.content`, tool calls in `message.tool_calls` (usually in one piece).
        if let Some(m) = v.get("message") {
            if let Some(c) = m.get("content").and_then(Value::as_str) { said.push_str(c); }
            if let Some(tc) = m.get("tool_calls").and_then(Value::as_array) { calls.extend(tc.iter().cloned()); }
        }
        last = v;
    }
    if last.is_null() {
        return Value::Null;
    }
    if last.get("message").is_some() {
        last["message"] = json!({ "role": "assistant", "content": said, "tool_calls": calls });
    } else {
        last["response"] = Value::String(text);
    }
    last
}

/// One answer from the model for a system instruction and a prompt. Low temperature: facts, not flourish.
pub async fn generate(system: &str, prompt: &str) -> Result<String, AiError> {
    ask(system, prompt, &[], None, Long::No).await
}

/// How much text goes in: a session transcript needs a bigger context and more time than a note.
#[derive(Clone, Copy)]
pub enum Long {
    No,
    /// Context size in tokens (8192–32768) for a long prompt such as a session transcript.
    Ctx(u32),
}

/// `generate` for a long prompt (a session transcript): bigger context, up to 10 minutes.
pub async fn generate_long(system: &str, prompt: &str, ctx: u32) -> Result<String, AiError> {
    ask(system, prompt, &[], None, Long::Ctx(ctx)).await
}

/// `generate_json` for a long prompt.
pub async fn generate_json_long(system: &str, prompt: &str, schema: &Value, ctx: u32) -> Result<Value, AiError> {
    let out = ask(system, prompt, &[], Some(schema), Long::Ctx(ctx)).await?;
    json_object(&out)
}

fn json_object(out: &str) -> Result<Value, AiError> {
    let t = out.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let v: Value = serde_json::from_str(t).map_err(|_| AiError::Failed)?;
    if v.is_object() { Ok(v) } else { Err(AiError::Failed) }
}

/// An answer that must follow a JSON schema (Ollama's structured outputs). Returns the parsed object.
pub async fn generate_json(system: &str, prompt: &str, images: &[String], schema: &Value) -> Result<Value, AiError> {
    let out = ask(system, prompt, images, Some(schema), Long::No).await?;
    // Some models wrap JSON in ``` fences even when asked not to.
    json_object(&out)
}

/// `images`: base64 JPEG/PNG/WebP, for models that read pictures (receipts).
async fn ask(system: &str, prompt: &str, images: &[String], schema: Option<&Value>, long: Long) -> Result<String, AiError> {
    // Streamed, so that if we stop waiting (timeout) the connection closes and Ollama stops generating at once —
    // a non-streamed request kept the AI computer busy long after the app had given up (9 Nov 2026).
    // num_predict caps the length: a structured answer can otherwise run on forever.
    // think: false — gemma4 is a "thinking" model: left on, it thinks first (slowly) and, asked for JSON, can think
    // without end. Off, the assistant answered in 3 s on the AI PC (9 Nov 2026). Models that don't think ignore it.
    let (ctx, wait) = match long { Long::No => (8192, 300), Long::Ctx(c) => (c.clamp(8192, 32768), 600) };
    let mut body = json!({
        "model": model(), "system": system, "prompt": prompt, "stream": true, "think": false,
        "options": { "temperature": if schema.is_some() { 0.0 } else { 0.3 }, "num_ctx": ctx, "num_predict": if schema.is_some() { 700 } else { 2500 } },
    });
    if !images.is_empty() {
        body["images"] = json!(images);
    }
    if let Some(f) = schema {
        body["format"] = f.clone();
    }
    // A computer without a GPU can take a few minutes for a one-page letter.
    let (mut status, mut v) = request(&url(), "POST", "/api/generate", Some(&body), Duration::from_secs(wait)).await?;
    if status == 400 && v.get("error").and_then(Value::as_str).is_some_and(|e| e.to_lowercase().contains("think")) {
        // An Ollama that doesn't know the `think` option: ask again without it.
        body.as_object_mut().map(|o| o.remove("think"));
        (status, v) = request(&url(), "POST", "/api/generate", Some(&body), Duration::from_secs(wait)).await?;
    }
    if status == 404 {
        return Err(AiError::NoModel);
    }
    if status != 200 {
        // Ollama answers 400/500 with "…does not support images…" (or similar) for a text-only model.
        let msg = v.get("error").and_then(Value::as_str).unwrap_or("").to_lowercase();
        if !images.is_empty() && (msg.contains("image") || msg.contains("vision") || msg.contains("multimodal")) {
            return Err(AiError::NoVision);
        }
        return Err(AiError::Failed);
    }
    let out = v.get("response").and_then(Value::as_str).ok_or(AiError::Failed)?;
    // Some models think out loud between <think> tags first: keep only the answer.
    let out = match out.rfind("</think>") { Some(i) => &out[i + 8..], None => out };
    let out = out.trim();
    if out.is_empty() { Err(AiError::Failed) } else { Ok(out.to_string()) }
}


// ───────── chat with tools (the assistant, 13 Nov 2026) ─────────

/// What the model said in one chat step: text, and/or the tools it wants to call (name, arguments).
#[derive(Debug, Default)]
pub struct ChatOut { pub content: String, pub calls: Vec<(String, Value)> }

pub enum ChatError {
    Ai(AiError),
    /// The chosen model can't use tools (Ollama: "… does not support tools").
    NoTools,
}

/// One step of a conversation with tools (Ollama `/api/chat`). `messages` as Ollama takes them
/// (`{role, content}`, assistant `tool_calls`, `{role: "tool", content, tool_name}`), `tools` in its function format.
/// Temperature 0: the assistant looks things up and fills in details, it doesn't write prose.
pub async fn chat(model: &str, messages: &[Value], tools: &Value, ctx: u32) -> Result<ChatOut, ChatError> {
    let ctx = ctx.clamp(8192, 32768);
    let mut body = json!({
        "model": model, "messages": messages, "tools": tools, "stream": true, "think": false,
        "options": { "temperature": 0.0, "num_ctx": ctx, "num_predict": 900 },
    });
    let wait = Duration::from_secs(240);
    let (mut status, mut v) = request(&url(), "POST", "/api/chat", Some(&body), wait).await.map_err(ChatError::Ai)?;
    if status == 400 && err_text(&v).contains("think") && !err_text(&v).contains("tools") {
        body.as_object_mut().map(|o| o.remove("think"));
        (status, v) = request(&url(), "POST", "/api/chat", Some(&body), wait).await.map_err(ChatError::Ai)?;
    }
    if status == 404 && !err_text(&v).contains("tools") {
        return Err(ChatError::Ai(AiError::NoModel));
    }
    if err_text(&v).contains("does not support tools") || err_text(&v).contains("tools are not supported") {
        return Err(ChatError::NoTools);
    }
    if status != 200 {
        return Err(ChatError::Ai(AiError::Failed));
    }
    Ok(chat_out(&v))
}

fn err_text(v: &Value) -> String {
    v.get("error").and_then(Value::as_str).unwrap_or("").to_lowercase()
}

/// The text and tool calls of a (joined) chat answer. Some models write the call as JSON text instead of a real
/// tool call ("{"name": "agenda", "arguments": {…}}", sometimes inside <tool_call> tags): that counts too.
fn chat_out(v: &Value) -> ChatOut {
    let msg = &v["message"];
    let mut calls: Vec<(String, Value)> = msg["tool_calls"].as_array().map(|a| a.iter().filter_map(|c| {
        let f = &c["function"];
        let name = f["name"].as_str()?.trim().to_string();
        let args = match &f["arguments"] { Value::String(s) => serde_json::from_str(s).unwrap_or(json!({})), Value::Object(_) => f["arguments"].clone(), _ => json!({}) };
        Some((name, args))
    }).collect()).unwrap_or_default();
    let raw = msg["content"].as_str().unwrap_or("");
    let raw = match raw.rfind("</think>") { Some(i) => &raw[i + 8..], None => raw };
    let mut content = raw.trim().to_string();
    if calls.is_empty() {
        let t = content.trim_start_matches("<tool_call>").trim_end_matches("</tool_call>").trim()
            .trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
        if let Ok(j) = serde_json::from_str::<Value>(t) {
            if let (Some(n), Some(a)) = (j["name"].as_str(), j.get("arguments").or(j.get("parameters"))) {
                calls.push((n.to_string(), if a.is_object() { a.clone() } else { json!({}) }));
                content.clear();
            }
        }
    }
    ChatOut { content, calls }
}

/// Is Ollama reachable at `url`, and does it have `model`? Also lists the models it has (for the picker).
/// `{available, url, model, reason?, models}` — reason: not_local | offline | no_model | error.
pub async fn check(url: &str, model: &str) -> Value {
    if parse(url).is_none() {
        return json!({ "available": false, "reason": "not_local", "url": url, "model": model, "models": [] });
    }
    let models: Vec<String> = match request(url, "GET", "/api/tags", None, Duration::from_secs(5)).await {
        Ok((200, v)) => v.get("models").and_then(Value::as_array).map(|a| a.iter().filter_map(|m| m.get("name").and_then(Value::as_str).map(str::to_string)).collect()).unwrap_or_default(),
        Ok(_) => vec![],
        Err(AiError::Offline) => return json!({ "available": false, "reason": "offline", "url": url, "model": model, "models": [] }),
        Err(_) => vec![],
    };
    match request(url, "POST", "/api/show", Some(&json!({ "model": model })), Duration::from_secs(5)).await {
        Ok((200, _)) => json!({ "available": true, "url": url, "model": model, "models": models }),
        Ok((404, _)) => json!({ "available": false, "reason": "no_model", "url": url, "model": model, "models": models }),
        Err(AiError::Offline) => json!({ "available": false, "reason": "offline", "url": url, "model": model, "models": [] }),
        _ => json!({ "available": false, "reason": "error", "url": url, "model": model, "models": models }),
    }
}

/// Is the AI in use reachable, and is the model there? (For Settings and the letter sheet.)
pub async fn status() -> Value {
    check(&url(), &model()).await
}

#[cfg(test)]
mod tests {
    use super::{is_local, model_ok, normalise};
    #[test]
    fn only_local_addresses() {
        for ok in ["ollama", "localhost", "127.0.0.1", "192.168.1.64", "10.0.0.2", "172.18.0.5", "100.68.180.126"] {
            assert!(is_local(ok), "{ok}");
        }
        for bad in ["api.openai.com", "8.8.8.8", "100.128.0.1", "example.com", "1.1.1.1", "pc-ia", "12345"] {
            assert!(!is_local(bad), "{bad}");
        }
    }
    #[test]
    fn typed_addresses() {
        assert_eq!(normalise("100.86.97.21").as_deref(), Some("http://100.86.97.21:11434"));
        assert_eq!(normalise(" 100.86.97.21:11434 ").as_deref(), Some("http://100.86.97.21:11434"));
        assert_eq!(normalise("http://100.86.97.21:11434/").as_deref(), Some("http://100.86.97.21:11434"));
        assert_eq!(normalise("http://ollama").as_deref(), Some("http://ollama:11434"));
        assert_eq!(normalise("192.168.1.20:8000").as_deref(), Some("http://192.168.1.20:8000"));
        assert_eq!(normalise("pc-ia"), None);
        assert_eq!(normalise("https://100.86.97.21"), None);
        assert_eq!(normalise("8.8.8.8"), None);
        assert_eq!(normalise("http://api.openai.com"), None);
        assert_eq!(normalise("100.86.97.21/api"), None);
        assert_eq!(normalise(""), None);
    }
    #[test]
    fn streamed_answers_are_joined() {
        let body = "{\"response\":\"Ol\",\"done\":false}\n{\"response\":\"á\",\"done\":false}\n{\"response\":\"\",\"done\":true,\"eval_count\":2}\n";
        let v = super::merge_stream(body);
        assert_eq!(v["response"], "Olá");
        assert_eq!(v["eval_count"], 2);
        assert_eq!(super::merge_stream("{\"response\":\"x\"}\n{\"error\":\"boom\"}")["error"], "boom");
        assert_eq!(super::merge_stream("{\"models\":[]}")["models"], serde_json::json!([]));
    }
    #[test]
    fn chat_answers_are_joined() {
        let body = "{\"message\":{\"role\":\"assistant\",\"content\":\"Ol\"},\"done\":false}\n{\"message\":{\"role\":\"assistant\",\"content\":\"á\",\"tool_calls\":[{\"function\":{\"name\":\"agenda\",\"arguments\":{\"from\":\"2026-11-14\"}}}]},\"done\":false}\n{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n";
        let out = super::chat_out(&super::merge_stream(body));
        assert_eq!(out.content, "Olá");
        assert_eq!(out.calls, vec![("agenda".to_string(), serde_json::json!({ "from": "2026-11-14" }))]);
        // A call written as text (some models do that) still counts.
        let v = serde_json::json!({ "message": { "content": "<tool_call>{\"name\": \"search_patients\", \"arguments\": {\"query\": \"Celestina\"}}</tool_call>" } });
        let out = super::chat_out(&v);
        assert_eq!(out.calls[0].0, "search_patients");
        assert!(out.content.is_empty());
        // Arguments as a JSON string.
        let v = serde_json::json!({ "message": { "content": "", "tool_calls": [{ "function": { "name": "x", "arguments": "{\"a\":1}" } }] } });
        assert_eq!(super::chat_out(&v).calls[0].1["a"], 1);
    }
    #[test]
    fn model_names() {
        assert!(model_ok("gemma4:12b") && model_ok("qwen2.5vl:7b") && model_ok("library/llama3.1:8b"));
        assert!(!model_ok("") && !model_ok("a b") && !model_ok("x;rm"));
    }
}
