//! On-device AI for the desktop app: chat and speech-to-text that run on this computer, through the app's own C++
//! module (`native/`, built by `build.rs`: llama.cpp and whisper.cpp, with Metal).
//!
//! - Nothing leaves the computer and nothing is written to disk: no prompt, answer, audio or transcript is logged
//!   or saved.
//! - The models live in the app's data folder (`…/app.physiopulse/models`), put there by `tools/local-ai/setup.sh`:
//!   a `*.gguf` for chat (Gemma 4 E4B by default), a `ggml-*.bin` for speech (Whisper large-v3).
//! - A model is loaded on first use and let go after 10 idle minutes, so the RAM is free again for the rest of the
//!   Mac (an 8–16 GB MacBook Air).
//! - Builds without the module (Windows by default, iOS, or a Mac where setup wasn't run) still have these commands:
//!   they answer `built: false` / "local_ai_unavailable".

use crate::server::ApiError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{ipc::Channel, AppHandle, Emitter, Manager, State};

/// The chat model to use when several are in the folder, best first (see claude/LOCAL_AI.md for why).
const PREFERRED: &[&str] = &["gemma-4-E4B-it-Q4_K_M.gguf", "gemma-4-E4B-it-Q4_0.gguf", "gemma-4-E2B-it-Q4_K_M.gguf"];
/// The speech model, best first: quality over speed (owner, 25 Nov 2026), so the full large-v3 before turbo.
const PREFERRED_STT: &[&str] = &["ggml-large-v3.bin", "ggml-large-v3-q8_0.bin", "ggml-large-v3-q5_0.bin", "ggml-large-v3-turbo.bin"];
/// The longest dictation: 10 minutes of 16 kHz audio.
const MAX_SAMPLES: usize = 16_000 * 600;
/// Words that set the spelling (pt-PT, not pt-BR) and the clinic's vocabulary for the speech model.
const PROMPT_PT: &str = "Consulta de fisioterapia, em português de Portugal. Utente, fisioterapeuta, sessão, joelho, ombro, coluna lombar, cervical, tornozelo, entorse, tendinite, ligamento, alongamentos, exercícios, dor.";
const PROMPT_EN: &str = "Physiotherapy appointment. Patient, physiotherapist, session, knee, shoulder, lower back, neck, ankle sprain, tendinitis, ligament, stretches, exercises, pain.";
/// Context size: a long conversation, a pasted letter. More costs RAM (the KV cache) for little gain here.
const N_CTX: i32 = 8192;
/// Let a model go after this long unused.
const IDLE: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub struct Local {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    chat: Mutex<Option<Loaded<ffi::Chat>>>,
    stt: Mutex<Option<Loaded<ffi::Stt>>>,
    /// Set by `local_transcribe_stop`.
    stt_stop: AtomicBool,
    /// Set by `local_chat_stop`; the running answer stops at its next piece.
    stop: AtomicBool,
}

struct Loaded<T> {
    key: PathBuf,
    handle: T,
    used: Instant,
}

#[derive(Deserialize)]
pub struct Msg {
    role: String,
    content: String,
}

#[derive(Serialize)]
pub struct Status {
    /// The C++ module is in this build.
    built: bool,
    models_dir: String,
    /// The chat model file that would be used, if any.
    chat_model: Option<String>,
    chat_loaded: bool,
    /// The speech-to-text model file that would be used, if any.
    stt_model: Option<String>,
}

#[derive(Serialize)]
pub struct Reply {
    text: String,
    tokens: i32,
    model: String,
    seconds: f32,
    stopped: bool,
}

fn models_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("models"))
}

/// The chat model: the first of PREFERRED that is there, else the first *.gguf by name.
fn find_chat_model(dir: &Path) -> Option<PathBuf> {
    for name in PREFERRED {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "gguf") && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("mmproj")))
        .collect();
    all.sort();
    all.into_iter().next()
}

/// The speech model: the first of PREFERRED_STT that is there, else any `ggml-*.bin`.
fn find_stt_model(dir: &Path) -> Option<PathBuf> {
    for name in PREFERRED_STT {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.file_name().is_some_and(|n| { let n = n.to_string_lossy(); n.starts_with("ggml-") && n.ends_with(".bin") }))
        .collect();
    all.sort();
    all.into_iter().next()
}

fn unavailable() -> ApiError {
    ApiError::local("local_ai_unavailable", "On-device AI isn't in this build of the app.")
}

/// Started once: lets idle models go.
pub fn start(app: &AppHandle) {
    let inner = app.state::<Local>().inner.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(60));
        if let Ok(mut c) = inner.chat.try_lock() {
            if c.as_ref().is_some_and(|l| l.used.elapsed() > IDLE) {
                *c = None;
            }
        }
        if let Ok(mut s) = inner.stt.try_lock() {
            if s.as_ref().is_some_and(|l| l.used.elapsed() > IDLE) {
                *s = None;
            }
        }
    });
}

#[tauri::command]
pub fn local_ai_status(app: AppHandle, local: State<'_, Local>) -> Status {
    let dir = models_dir(&app).unwrap_or_default();
    let chat_model = find_chat_model(&dir).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    let chat_loaded = local.inner.chat.lock().map(|c| c.is_some()).unwrap_or(false);
    let stt_model = find_stt_model(&dir).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    Status { built: ffi::BUILT, models_dir: dir.to_string_lossy().into_owned(), chat_model, chat_loaded, stt_model }
}

/// Opens the models folder in Finder / Explorer (made if missing), so the owner can see or swap the files.
#[cfg(desktop)]
#[tauri::command]
pub fn local_ai_open_folder(app: AppHandle) -> Result<(), ApiError> {
    let dir = models_dir(&app).ok_or_else(unavailable)?;
    let _ = std::fs::create_dir_all(&dir);
    open::that_detached(&dir).map_err(|_| ApiError::local("failed", "Couldn't open the folder."))
}

/// Frees the RAM the model uses now (it loads again on next use).
#[tauri::command]
pub fn local_ai_unload(local: State<'_, Local>) {
    if let Ok(mut c) = local.inner.chat.lock() { *c = None; }
    if let Ok(mut s) = local.inner.stt.lock() { *s = None; }
}

/// Stops the answer being written.
#[tauri::command]
pub fn local_chat_stop(local: State<'_, Local>) {
    local.inner.stop.store(true, Ordering::SeqCst);
}

/// Answers a conversation on this computer. The answer streams to `on_piece` as it's written; the whole text comes
/// back at the end. Only one answer at a time ("busy" otherwise).
#[tauri::command]
pub async fn local_chat(app: AppHandle, local: State<'_, Local>, messages: Vec<Msg>, on_piece: Channel<String>) -> Result<Reply, ApiError> {
    if !ffi::BUILT {
        return Err(unavailable());
    }
    if messages.is_empty() || messages.len() > 60 || messages.iter().any(|m| !matches!(m.role.as_str(), "system" | "user" | "assistant") || m.content.len() > 40_000) {
        return Err(ApiError::local("invalid", "Invalid request."));
    }
    let dir = models_dir(&app).ok_or_else(unavailable)?;
    let path = find_chat_model(&dir).ok_or_else(|| ApiError::local("no_model", "No chat model on this computer yet."))?;
    let inner = local.inner.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut slot = inner.chat.try_lock().map_err(|_| ApiError::local("busy", "Still answering the last question."))?;
        if slot.as_ref().is_some_and(|l| l.key != path) {
            *slot = None; // the model file changed: let the old one go first (RAM)
        }
        if slot.is_none() {
            let handle = ffi::Chat::open(&path, N_CTX).map_err(|_| ApiError::local("load_failed", "The chat model couldn't be loaded (not enough free memory?)."))?;
            *slot = Some(Loaded { key: path.clone(), handle, used: Instant::now() });
        }
        let loaded = slot.as_mut().unwrap();
        loaded.used = Instant::now();
        inner.stop.store(false, Ordering::SeqCst);
        let started = Instant::now();
        let mut text = String::new();
        let pairs: Vec<(&str, &str)> = messages.iter().map(|m| (m.role.as_str(), m.content.as_str())).collect();
        let result = loaded.handle.reply(&pairs, 1500, 0.3, |piece| {
            text.push_str(piece);
            let _ = on_piece.send(piece.to_string());
            !inner.stop.load(Ordering::SeqCst)
        });
        loaded.used = Instant::now();
        let model = loaded.handle.name();
        match result {
            Ok(tokens) => Ok(Reply { text, tokens, model, seconds: started.elapsed().as_secs_f32(), stopped: inner.stop.load(Ordering::SeqCst) }),
            Err(-2) => Err(ApiError::local("too_long", "The conversation is too long. Start a new one.")),
            Err(_) => Err(ApiError::local("failed", "The on-device AI couldn't answer. Try again.")),
        }
    })
    .await
    .map_err(|_| ApiError::local("failed", "The on-device AI couldn't answer. Try again."))?
}

/// Stops the transcription running now.
#[tauri::command]
pub fn local_transcribe_stop(local: State<'_, Local>) {
    local.inner.stt_stop.store(true, Ordering::SeqCst);
}

/// Speech to text on this computer. The body is the audio itself: mono float32 little-endian samples at 16 kHz
/// (the UI records them, `src/lib/localAi.ts`); the `x-lang` header says "pt" or "en". Progress (0–100) goes out as
/// the `local-stt-progress` event. Returns the text. Only one at a time ("busy" otherwise).
#[tauri::command]
pub async fn local_transcribe(app: AppHandle, local: State<'_, Local>, request: tauri::ipc::Request<'_>) -> Result<String, ApiError> {
    if !ffi::BUILT {
        return Err(unavailable());
    }
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(ApiError::local("invalid", "Invalid request."));
    };
    if bytes.len() % 4 != 0 || bytes.len() / 4 < 16_000 / 2 || bytes.len() / 4 > MAX_SAMPLES {
        return Err(ApiError::local(if bytes.len() / 4 < 16_000 / 2 { "too_short" } else { "invalid" }, "Invalid recording."));
    }
    let pcm: Vec<f32> = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).map(|x| if x.is_finite() { x.clamp(-1.0, 1.0) } else { 0.0 }).collect();
    let en = request.headers().get("x-lang").is_some_and(|v| v.as_bytes() == b"en");
    let dir = models_dir(&app).ok_or_else(unavailable)?;
    let path = find_stt_model(&dir).ok_or_else(|| ApiError::local("no_model", "No speech-to-text model on this computer yet."))?;
    let inner = local.inner.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut slot = inner.stt.try_lock().map_err(|_| ApiError::local("busy", "Still transcribing the last recording."))?;
        if slot.as_ref().is_some_and(|l| l.key != path) {
            *slot = None;
        }
        if slot.is_none() {
            let handle = ffi::Stt::open(&path).map_err(|_| ApiError::local("load_failed", "The speech model couldn't be loaded (not enough free memory?)."))?;
            *slot = Some(Loaded { key: path.clone(), handle, used: Instant::now() });
        }
        let loaded = slot.as_mut().unwrap();
        inner.stt_stop.store(false, Ordering::SeqCst);
        let (lang, prompt) = if en { ("en", PROMPT_EN) } else { ("pt", PROMPT_PT) };
        let result = loaded.handle.transcribe(&pcm, lang, prompt, |pct| {
            let _ = app.emit("local-stt-progress", pct.clamp(0, 100));
            !inner.stt_stop.load(Ordering::SeqCst)
        });
        loaded.used = Instant::now();
        match result {
            Ok(text) => Ok(text.trim().to_string()),
            Err(_) if inner.stt_stop.load(Ordering::SeqCst) => Err(ApiError::local("stopped", "Stopped.")),
            Err(_) => Err(ApiError::local("failed", "The recording couldn't be transcribed. Try again.")),
        }
    })
    .await
    .map_err(|_| ApiError::local("failed", "The recording couldn't be transcribed. Try again."))?
}

/// The C interface of `native/include/physiopulse_ai.h`, and safe handles around it.
#[cfg(local_ai)]
mod ffi {
    use std::ffi::{c_char, c_int, c_void, CStr, CString};
    use std::path::Path;

    pub const BUILT: bool = true;

    #[repr(C)]
    struct PpMsg {
        role: *const c_char,
        content: *const c_char,
    }
    enum PpChat {}
    enum PpStt {}
    type ProgressCb = extern "C" fn(c_int, *mut c_void) -> c_int;
    type PieceCb = extern "C" fn(*const c_char, *mut c_void) -> c_int;

    #[link(name = "physiopulse_ai")]
    extern "C" {
        fn pp_chat_open(model_path: *const c_char, n_ctx: c_int, n_gpu_layers: c_int, err: *mut c_char, err_len: usize) -> *mut PpChat;
        fn pp_chat_reply(chat: *mut PpChat, msgs: *const PpMsg, n: usize, max_tokens: c_int, temperature: f32, cb: Option<PieceCb>, user: *mut c_void, err: *mut c_char, err_len: usize) -> c_int;
        fn pp_chat_model_name(chat: *const PpChat) -> *const c_char;
        fn pp_chat_close(chat: *mut PpChat);
        fn pp_stt_open(model_path: *const c_char, err: *mut c_char, err_len: usize) -> *mut PpStt;
        fn pp_stt_transcribe(stt: *mut PpStt, pcm: *const f32, n: usize, language: *const c_char, prompt: *const c_char, cb: Option<ProgressCb>, user: *mut c_void, err: *mut c_char, err_len: usize) -> *mut c_char;
        fn pp_stt_close(stt: *mut PpStt);
        fn pp_free(p: *mut c_void);
    }

    fn cpath(p: &Path) -> Result<CString, i32> {
        CString::new(p.to_string_lossy().as_bytes()).map_err(|_| -1)
    }
    /// Text for C: an inner NUL would cut it short, so drop any.
    fn ctext(s: &str) -> CString {
        CString::new(s.replace('\0', "")).unwrap_or_default()
    }

    pub struct Chat(*mut PpChat);
    // The handle is only used behind a Mutex, one thread at a time (physiopulse_ai.h).
    unsafe impl Send for Chat {}

    impl Chat {
        pub fn open(path: &Path, n_ctx: i32) -> Result<Self, i32> {
            let p = cpath(path)?;
            let mut err = [0 as c_char; 512];
            let h = unsafe { pp_chat_open(p.as_ptr(), n_ctx, -1, err.as_mut_ptr(), err.len()) };
            if h.is_null() { Err(-1) } else { Ok(Chat(h)) }
        }

        pub fn name(&self) -> String {
            unsafe { CStr::from_ptr(pp_chat_model_name(self.0)) }.to_string_lossy().into_owned()
        }

        /// `on_piece` returns false to stop. Ok(tokens) or Err(code): -2 = doesn't fit, -1 = failed.
        pub fn reply(&mut self, msgs: &[(&str, &str)], max_tokens: i32, temperature: f32, mut on_piece: impl FnMut(&str) -> bool) -> Result<i32, i32> {
            let owned: Vec<(CString, CString)> = msgs.iter().map(|(r, c)| (ctext(r), ctext(c))).collect();
            let raw: Vec<PpMsg> = owned.iter().map(|(r, c)| PpMsg { role: r.as_ptr(), content: c.as_ptr() }).collect();

            extern "C" fn trampoline(piece: *const c_char, user: *mut c_void) -> c_int {
                let f = unsafe { &mut *(user as *mut &mut dyn FnMut(&str) -> bool) };
                let s = unsafe { CStr::from_ptr(piece) }.to_string_lossy();
                // A panic must not cross into C++.
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&s))) {
                    Ok(true) => 1,
                    _ => 0,
                }
            }
            let mut f: &mut dyn FnMut(&str) -> bool = &mut on_piece;
            let user = &mut f as *mut &mut dyn FnMut(&str) -> bool as *mut c_void;
            let mut err = [0 as c_char; 512];
            let rc = unsafe { pp_chat_reply(self.0, raw.as_ptr(), raw.len(), max_tokens, temperature, Some(trampoline), user, err.as_mut_ptr(), err.len()) };
            if rc >= 0 { Ok(rc) } else { Err(rc) }
        }
    }

    impl Drop for Chat {
        fn drop(&mut self) {
            unsafe { pp_chat_close(self.0) }
        }
    }

    pub struct Stt(*mut PpStt);
    unsafe impl Send for Stt {}

    impl Stt {
        pub fn open(path: &Path) -> Result<Self, i32> {
            let p = cpath(path)?;
            let mut err = [0 as c_char; 512];
            let h = unsafe { pp_stt_open(p.as_ptr(), err.as_mut_ptr(), err.len()) };
            if h.is_null() { Err(-1) } else { Ok(Stt(h)) }
        }

        /// `on_progress` (0–100) returns false to stop.
        pub fn transcribe(&mut self, pcm: &[f32], lang: &str, prompt: &str, mut on_progress: impl FnMut(i32) -> bool) -> Result<String, i32> {
            extern "C" fn trampoline(pct: c_int, user: *mut c_void) -> c_int {
                let f = unsafe { &mut *(user as *mut &mut dyn FnMut(i32) -> bool) };
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(pct))) {
                    Ok(true) => 1,
                    _ => 0,
                }
            }
            let (l, pr) = (ctext(lang), ctext(prompt));
            let mut f: &mut dyn FnMut(i32) -> bool = &mut on_progress;
            let user = &mut f as *mut &mut dyn FnMut(i32) -> bool as *mut c_void;
            let mut err = [0 as c_char; 512];
            let out = unsafe { pp_stt_transcribe(self.0, pcm.as_ptr(), pcm.len(), l.as_ptr(), pr.as_ptr(), Some(trampoline), user, err.as_mut_ptr(), err.len()) };
            if out.is_null() {
                return Err(-1);
            }
            let text = unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned();
            unsafe { pp_free(out as *mut c_void) };
            Ok(text)
        }
    }

    impl Drop for Stt {
        fn drop(&mut self) {
            unsafe { pp_stt_close(self.0) }
        }
    }

}

/// Without the C++ module: the same names, nothing behind them.
#[cfg(not(local_ai))]
mod ffi {
    use std::path::Path;

    pub const BUILT: bool = false;
    pub struct Chat;
    impl Chat {
        pub fn open(_: &Path, _: i32) -> Result<Self, i32> { Err(-1) }
        pub fn name(&self) -> String { String::new() }
        pub fn reply(&mut self, _: &[(&str, &str)], _: i32, _: f32, _: impl FnMut(&str) -> bool) -> Result<i32, i32> { Err(-1) }
    }
    pub struct Stt;
    impl Stt {
        pub fn open(_: &Path) -> Result<Self, i32> { Err(-1) }
        pub fn transcribe(&mut self, _: &[f32], _: &str, _: &str, _: impl FnMut(i32) -> bool) -> Result<String, i32> { Err(-1) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_preferred_model() {
        let d = std::env::temp_dir().join(format!("pp-local-ai-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a-other.gguf"), b"").unwrap();
        std::fs::write(d.join("mmproj-x.gguf"), b"").unwrap();
        assert_eq!(find_chat_model(&d).unwrap().file_name().unwrap(), "a-other.gguf");
        std::fs::write(d.join("gemma-4-E4B-it-Q4_K_M.gguf"), b"").unwrap();
        assert_eq!(find_chat_model(&d).unwrap().file_name().unwrap(), "gemma-4-E4B-it-Q4_K_M.gguf");
        assert!(find_stt_model(&d).is_none());
        std::fs::write(d.join("ggml-large-v3-turbo.bin"), b"").unwrap();
        std::fs::write(d.join("ggml-large-v3.bin"), b"").unwrap();
        assert_eq!(find_stt_model(&d).unwrap().file_name().unwrap(), "ggml-large-v3.bin");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// End to end through the C++ module, with real models:
    /// PP_TEST_MODEL=…gguf [PP_TEST_STT=…bin PP_TEST_WAV=16 kHz mono 16-bit .wav] cargo test --features local-ai --lib local_ai -- --include-ignored
    #[cfg(local_ai)]
    #[test]
    #[ignore]
    fn native_chat() {
        let model = std::env::var("PP_TEST_MODEL").expect("PP_TEST_MODEL");
        let mut chat = ffi::Chat::open(Path::new(&model), 4096).expect("model loads");
        let mut pieces = 0;
        let n = chat.reply(&[("system", "Responde em português."), ("user", "Olá! Tudo bem?")], 32, 0.3, |_| { pieces += 1; true }).expect("answers");
        assert!(n > 0 && pieces > 0);
        let mut seen = 0;
        chat.reply(&[("user", "Conta até dez.")], 32, 0.3, |_| { seen += 1; seen < 2 }).expect("stops");
        assert_eq!(seen, 2);
        let long = "palavra ".repeat(20_000);
        assert_eq!(chat.reply(&[("user", long.as_str())], 32, 0.3, |_| true), Err(-2));
        if let (Ok(m), Ok(w)) = (std::env::var("PP_TEST_STT"), std::env::var("PP_TEST_WAV")) {
            let wav = std::fs::read(w).unwrap();
            let pcm: Vec<f32> = wav[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
            let mut stt = ffi::Stt::open(Path::new(&m)).expect("speech model loads");
            let mut last = -1;
            stt.transcribe(&pcm, "pt", PROMPT_PT, |p| { last = p; true }).expect("transcribes");
            assert!(last >= 0);
            assert!(stt.transcribe(&pcm, "pt", "", |_| false).is_err(), "stops when asked");
        }
    }
}
