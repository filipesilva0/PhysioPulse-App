//! The only code in the desktop app that talks to the clinic server.
//!
//! - The UI never sees the session token: it lives here, in memory only, so quitting the
//!   app always signs out (and we also tell the server on close).
//! - Requests can only go to the configured server and only under `/api/`.
//! - The server address is kept in the app's config folder (it isn't secret).
//! - Trusted computer: after the 6-digit code, the server gives a device token (valid 30 days). It is kept
//!   here and in `account.json` in the config folder, never given to the UI, and sent with the next sign-in
//!   of the same person so they only need their password. The UI only learns the remembered name and email,
//!   for "Bem-vindo de volta, …".
//! - Device key (PhysioPulse plan, phase 3): a random key made on the first start, kept in `device.json` in the
//!   config folder, sent as `X-Device-Key` with every request. The owner approves each device once. While a
//!   device waits for approval, the server's ticket is kept here (never in the UI) and swapped for a session.
//! - Addresses on the internet must use https://; plain http only on the clinic's own network.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::PathBuf, sync::Mutex, time::Duration};
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
pub struct Conn {
    pub base: Mutex<Option<String>>,
    pub token: Mutex<Option<String>>,
    /// This device's key (phase 3); loaded or made in `load`.
    pub key: Mutex<Option<String>>,
    /// The ticket of a sign-in waiting for the owner's approval.
    pub pending: Mutex<Option<String>>,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    server: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct ApiError {
    status: u16,
    message: String,
    /// Machine-readable reason, e.g. "overlap" (so the UI can offer "Book Anyway").
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    /// Extra data for that reason, e.g. the overlapping appointments.
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

impl ApiError {
    /// An error made by the app itself (see `local`).
    pub fn local(code: &str, message: &str) -> Self {
        local(code, message)
    }
}

/// The staff iPhone app (phase 5). The server keeps phone sessions 30 days with no idle timeout, but only for a
/// device the owner approved as a phone: this header says so when the device first asks (`server/src/auth.rs`).
pub const PHONE: bool = cfg!(target_os = "ios");

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("server.json"))
}

/// People who signed in on this computer, most recent first (a shared clinic computer can have a few).
#[derive(Serialize, Deserialize, Default, Clone)]
struct Accounts {
    accounts: Vec<Account>,
}
#[derive(Serialize, Deserialize, Clone)]
struct Account {
    email: String,
    name: String,
    /// The server address this account belongs to.
    server: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    device_token: Option<String>,
}
/// What the UI may know about the remembered person.
#[derive(Serialize)]
pub struct Remembered {
    email: String,
    name: String,
}
const MAX_ACCOUNTS: usize = 5;

fn accounts_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("account.json"))
}
fn load_accounts(app: &AppHandle) -> Accounts {
    accounts_path(app).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}
fn save_accounts(app: &AppHandle, a: &Accounts) {
    let Some(p) = accounts_path(app) else { return };
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    if std::fs::write(&p, serde_json::to_string(a).unwrap()).is_ok() {
        // Only this user can read it (it holds the device token). Windows keeps it in the user's own profile.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
    }
}
fn same_email(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}
/// The device token to send with a sign-in for this email on this server, if this computer is trusted.
fn token_for(app: &AppHandle, server: &str, email: &str) -> Option<String> {
    load_accounts(app).accounts.into_iter().find(|a| a.server == server && same_email(&a.email, email)).and_then(|a| a.device_token)
}
/// After a successful sign-in: put this person first; keep their device token, or store the new one.
fn remember(app: &AppHandle, server: &str, email: &str, name: &str, new_token: Option<String>) {
    let mut all = load_accounts(app);
    let old = all.accounts.iter().position(|a| a.server == server && same_email(&a.email, email)).map(|i| all.accounts.remove(i));
    let device_token = new_token.or_else(|| old.and_then(|a| a.device_token));
    all.accounts.insert(0, Account { email: email.trim().to_lowercase(), name: name.to_string(), server: server.to_string(), device_token });
    all.accounts.truncate(MAX_ACCOUNTS);
    save_accounts(app, &all);
}
/// The server no longer trusts this computer for this person (expired or revoked): drop the token.
fn drop_token(app: &AppHandle, server: &str, email: &str) {
    let mut all = load_accounts(app);
    let mut changed = false;
    for a in all.accounts.iter_mut().filter(|a| a.server == server && same_email(&a.email, email)) {
        changed |= a.device_token.take().is_some();
    }
    if changed { save_accounts(app, &all); }
}

/// The last person who signed in on this computer (for this server), for the welcome-back screen.
#[tauri::command]
pub fn remembered_account(app: AppHandle, conn: State<'_, Conn>) -> Option<Remembered> {
    let base = conn.base.lock().unwrap().clone()?;
    load_accounts(&app).accounts.into_iter().find(|a| a.server == base).map(|a| Remembered { email: a.email, name: a.name })
}

/// "Forget this computer" for one person: removes them from this computer and tells the server to drop
/// the trust (best effort; it expires on its own after 30 days anyway).
#[tauri::command]
pub async fn forget_account(app: AppHandle, conn: State<'_, Conn>, email: String) -> Result<(), ApiError> {
    let base = conn.base.lock().unwrap().clone();
    let mut all = load_accounts(&app);
    let gone: Vec<Account> = all.accounts.iter().filter(|a| Some(&a.server) == base.as_ref() && same_email(&a.email, &email)).cloned().collect();
    all.accounts.retain(|a| !(Some(&a.server) == base.as_ref() && same_email(&a.email, &email)));
    save_accounts(&app, &all);
    if let Some(b) = base {
        for t in gone.into_iter().filter_map(|a| a.device_token) {
            let _ = client().post(format!("{b}/api/auth/forget-device")).header("X-Device", device())
                .json(&serde_json::json!({ "device_token": t })).send().await;
        }
    }
    Ok(())
}

/// The app was called Clínica until 0.6.0, with its settings in a folder named after the old id
/// (`pt.clinica.desktop`). On the first start as PhysioPulse, copy the server address and the
/// remembered accounts (with this computer's trusted-device tokens) from there, so nobody has to
/// connect or do the two-step code again. The old folder is left as it was.
const OLD_ID: &str = "pt.clinica.desktop";
fn carry_over(new_dir: &std::path::Path) {
    let Some(old_dir) = new_dir.parent().map(|p| p.join(OLD_ID)) else { return };
    if new_dir.join("server.json").exists() || !old_dir.join("server.json").exists() {
        return;
    }
    let _ = std::fs::create_dir_all(new_dir);
    for f in ["server.json", "account.json"] {
        let (from, to) = (old_dir.join(f), new_dir.join(f));
        if from.exists() && std::fs::copy(&from, &to).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct DeviceKey {
    key: String,
}

/// This device's key: read from `device.json`, or made now (32 random bytes from the OS) and saved.
fn device_key(dir: &std::path::Path) -> Option<String> {
    let p = dir.join("device.json");
    if let Some(k) = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<DeviceKey>(&s).ok()).map(|d| d.key) {
        if k.len() == 64 && k.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(k);
        }
    }
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).ok()?;
    let key: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let _ = std::fs::create_dir_all(dir);
    std::fs::write(&p, serde_json::to_string(&DeviceKey { key: key.clone() }).ok()?).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    Some(key)
}

pub fn load(app: &AppHandle) {
    if let Ok(dir) = app.path().app_config_dir() {
        carry_over(&dir);
        *app.state::<Conn>().key.lock().unwrap() = device_key(&dir);
    }
    if let Some(p) = config_path(app) {
        if let Ok(s) = std::fs::read_to_string(p) {
            if let Ok(saved) = serde_json::from_str::<Saved>(&s) {
                *app.state::<Conn>().base.lock().unwrap() = saved.server;
            }
        }
    }
}

fn client() -> reqwest::Client {
    client_for(Duration::from_secs(20))
}

fn client_for(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).timeout(timeout).build().expect("http client")
}

/// How long to wait for an answer. The local AI (Ollama on another clinic PC) needs a minute or more for
/// a letter draft, and the server itself gives it up to 300 s (`server/src/ai.rs`); uploads can be large.
fn timeout_for(method: &str, path: &str) -> Duration {
    let p = path.split('?').next().unwrap_or(path);
    if p.ends_with("/ai-draft") || (p.starts_with("/ai/") && p != "/ai/status") {
        Duration::from_secs(330)
    } else if method == "GET" {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(120)
    }
}

/// An error made here (not by the server). The UI shows `message` in the app's language, chosen by `code`
/// (see `localise` in src/lib/api.ts); the English text is the fallback.
fn local(code: &str, message: &str) -> ApiError {
    ApiError { status: 0, message: message.into(), code: Some(code.into()), details: None }
}

/// Tell "the answer took too long" apart from "the server can't be reached".
fn send_error(e: reqwest::Error, ai: bool) -> ApiError {
    if e.is_timeout() && !e.is_connect() {
        if ai {
            local("ai_timeout", "The local AI took too long to answer. Check the AI computer is on, then try again.")
        } else {
            local("timeout", "The server took too long to answer. Try again.")
        }
    } else {
        local("offline", "Can’t reach the server. Check the internet connection.")
    }
}

fn device() -> String {
    let n: String = gethostname::gethostname().to_string_lossy().chars().take(80).collect();
    // iOS gives apps no real device name: "iPhone" (the owner can rename it in Settings › Devices).
    if PHONE && (n.is_empty() || n == "localhost" || n.starts_with("localhost.")) { "iPhone".into() } else { n }
}

/// The headers every request carries: computer name, device key, and on the iPhone the device kind.
fn device_headers(conn: &Conn, mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    req = req.header("X-Device", device());
    if let Some(k) = conn.key.lock().unwrap().clone() {
        req = req.header("X-Device-Key", k);
    }
    if PHONE {
        req = req.header("X-Device-Kind", "phone");
    }
    req
}

/// Ends the session on the server now (best effort, 5 s) and forgets the token here.
pub async fn logout_now(conn: &Conn) {
    let token = conn.token.lock().unwrap().take();
    let base = conn.base.lock().unwrap().clone();
    if let (Some(t), Some(b)) = (token, base) {
        let req = device_headers(conn, client_for(Duration::from_secs(5)).post(format!("{b}/api/auth/logout")).bearer_auth(t));
        let _ = req.send().await;
    }
}

/// Is this host on the clinic's own network (or this computer)? Plain http is only allowed there.
fn local_host(host: &str) -> bool {
    let h = host.trim_start_matches('[').split(']').next().unwrap_or(host);
    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local() || (v.octets()[0] == 100 && (64..128).contains(&v.octets()[1])),
            std::net::IpAddr::V6(v) => v.is_loopback() || (v.segments()[0] & 0xfe00) == 0xfc00,
        };
    }
    // A name without dots (zimaos) or a .local name is on the home network.
    !h.contains('.') || h.ends_with(".local") || h.ends_with(".lan") || h.ends_with(".home.arpa")
}

fn normalise(url: &str) -> Result<String, ApiError> {
    let u = url.trim().trim_end_matches('/').to_string();
    if u.len() > 200 || u.contains(char::is_whitespace) || u.is_empty() {
        return Err(local("bad_address", "Use an address like clinica.duckdns.org or 192.168.1.64:8095."));
    }
    let (scheme, rest) = match u.split_once("://") {
        Some((s, r)) if s.eq_ignore_ascii_case("http") || s.eq_ignore_ascii_case("https") => (Some(s.to_ascii_lowercase()), r.to_string()),
        Some(_) => return Err(local("bad_address", "Use an address like clinica.duckdns.org or 192.168.1.64:8095.")),
        None => (None, u.clone()),
    };
    let host_port = rest.split('/').next().unwrap_or(&rest);
    let host = if host_port.starts_with('[') { host_port.split(']').next().map(|h| format!("{h}]")).unwrap_or_default() } else { host_port.rsplit_once(':').map(|(h, _)| h.to_string()).unwrap_or(host_port.to_string()) };
    if host.is_empty() {
        return Err(local("bad_address", "Use an address like clinica.duckdns.org or 192.168.1.64:8095."));
    }
    let local_net = local_host(&host);
    let scheme = match scheme {
        Some(s) if s == "http" && !local_net => return Err(local("https_needed", "Addresses on the internet must start with https://.")),
        Some(s) => s,
        None if local_net => "http".to_string(),
        None => "https".to_string(),
    };
    Ok(format!("{scheme}://{rest}"))
}

#[tauri::command]
pub fn get_server(conn: State<'_, Conn>) -> Option<String> {
    conn.base.lock().unwrap().clone()
}

/// Checks the address answers like a PhysioPulse server, then saves it.
#[tauri::command]
pub async fn set_server(app: AppHandle, conn: State<'_, Conn>, url: String) -> Result<Value, ApiError> {
    let base = normalise(&url)?;
    let res = client().get(format!("{base}/api/status")).send().await
        .map_err(|_| local("offline_address", "Can’t reach the server. Check the internet connection and that the address is right."))?;
    let status: Value = res.json().await.map_err(|_| local("not_physiopulse", "That address isn’t a PhysioPulse server."))?;
    if status.get("setup_needed").is_none() {
        return Err(local("not_physiopulse", "That address isn’t a PhysioPulse server."));
    }
    *conn.base.lock().unwrap() = Some(base.clone());
    if let Some(p) = config_path(&app) {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(p, serde_json::to_string(&Saved { server: Some(base) }).unwrap());
    }
    Ok(status)
}

/// One call for every API request. The token is attached here and stripped from replies.
#[tauri::command]
pub async fn api(app: AppHandle, conn: State<'_, Conn>, method: String, path: String, body: Option<Value>, lang: Option<String>) -> Result<Value, ApiError> {
    if !path.starts_with('/') || path.contains("..") || path.contains("://") {
        return Err(local("invalid", "Invalid request."));
    }
    let base = conn.base.lock().unwrap().clone().ok_or_else(|| local("no_server", "No server set up yet."))?;
    let url = format!("{base}/api{path}");
    let ai = timeout_for(&method, &path) > Duration::from_secs(120);
    let c = client_for(timeout_for(&method, &path));
    let mut req = match method.as_str() {
        "GET" => c.get(&url),
        "POST" => c.post(&url),
        "PUT" => c.put(&url),
        "PATCH" => c.patch(&url),
        "DELETE" => c.delete(&url),
        _ => return Err(local("invalid", "Invalid request.")),
    };
    req = device_headers(&conn, req);
    // The server answers its error messages in the app's language (audit M13).
    req = req.header("Accept-Language", if lang.as_deref() == Some("en") { "en" } else { "pt-PT" });
    if let Some(t) = conn.token.lock().unwrap().clone() {
        req = req.bearer_auth(t);
    }
    // Sign-in on a trusted computer: add the device token (the UI never has it). After the code: ask to trust.
    let mut body = body;
    let login_email = if path == "/auth/login" { body.as_ref().and_then(|b| b.get("email")).and_then(|e| e.as_str()).map(str::to_string) } else { None };
    if let (Some(email), Some(Value::Object(b))) = (&login_email, body.as_mut()) {
        b.remove("device_token");
        if let Some(t) = token_for(&app, &base, email) {
            b.insert("device_token".into(), Value::String(t));
        }
    }
    // The iPhone never asks to be trusted: its session itself lasts 30 days, behind Face ID (phase 5).
    if path == "/auth/verify" && !PHONE {
        if let Some(Value::Object(b)) = body.as_mut() {
            b.insert("trust_device".into(), Value::Bool(true));
        }
    }
    // Waiting for the owner's approval: the ticket goes in here (the UI never has it).
    if path == "/auth/pending" || path == "/auth/pending/recover" {
        let ticket = conn.pending.lock().unwrap().clone().ok_or_else(|| local("no_pending", "This step expired. Sign in again."))?;
        if body.is_none() { body = Some(Value::Object(Default::default())); }
        if let Some(Value::Object(b)) = body.as_mut() {
            b.insert("ticket".into(), Value::String(ticket));
        }
    }
    if path == "/auth/login" {
        *conn.pending.lock().unwrap() = None;
    }
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = req.send().await.map_err(|e| send_error(e, ai))?;
    let status = res.status().as_u16();
    let mut json: Value = res.json().await.unwrap_or(Value::Null);
    if status >= 400 {
        if status == 401 {
            *conn.token.lock().unwrap() = None;
            // The phone session is over (signed out by the owner, 30 days, phone removed): forget it on the phone too.
            if PHONE && !path.starts_with("/auth/") { crate::phone::forget(&app); }
        }
        let message = json.get("message").and_then(|m| m.as_str()).unwrap_or("Something went wrong.").to_string();
        let code = json.get("error").and_then(|m| m.as_str()).map(str::to_string);
        let details = json.get("details").cloned();
        if path.starts_with("/auth/pending") && (status == 401 || code.as_deref() == Some("device_revoked")) {
            *conn.pending.lock().unwrap() = None;
        }
        return Err(ApiError { status, message, code, details });
    }
    // Keep the session token and the device token in Rust; the UI only gets the person and settings.
    if let Some(obj) = json.as_object_mut() {
        let device_token = match obj.remove("device_token") { Some(Value::String(t)) => Some(t), _ => None };
        // A new device waits for the owner: keep the ticket here.
        if let Some(Value::String(t)) = obj.remove("ticket") {
            *conn.pending.lock().unwrap() = Some(t);
        }
        if let Some(Value::String(t)) = obj.remove("token") {
            *conn.token.lock().unwrap() = Some(t.clone());
            // Signed in (code, or password on a trusted computer): remember who, for "Bem-vindo de volta".
            if path == "/auth/login" || path == "/auth/verify" || path.starts_with("/auth/pending") {
                *conn.pending.lock().unwrap() = None;
                let staff = obj.get("staff");
                let email = staff.and_then(|s| s.get("email")).and_then(|v| v.as_str());
                let name = staff.and_then(|s| s.get("name")).and_then(|v| v.as_str());
                if let (Some(e), Some(n)) = (email, name) {
                    if PHONE {
                        crate::phone::keep(&app, &t, crate::phone::PhoneUser { name: n.into(), email: e.trim().to_lowercase(), server: base.clone() });
                    }
                    remember(&app, &base, e, n, device_token);
                }
            }
        } else if path == "/auth/login" && obj.get("next").and_then(|n| n.as_str()) == Some("code") {
            // The server asked for the code although we sent a token: it expired or was revoked.
            if let Some(email) = &login_email { drop_token(&app, &base, email); }
        }
    }
    if path == "/auth/logout" {
        *conn.token.lock().unwrap() = None;
        if PHONE { crate::phone::forget(&app); }
    }
    Ok(json)
}

/// Downloads a file from the server (e.g. the accountant export or an invoice PDF) straight into the
/// computer's Downloads folder, so the bytes never pass through the UI. Returns where it was saved.
/// Only the paths `file_path_ok` allows.
#[cfg(desktop)]
#[tauri::command]
pub async fn download(app: AppHandle, conn: State<'_, Conn>, path: String, filename: String) -> Result<String, ApiError> {
    if !file_path_ok(&path) {
        return Err(local("invalid", "Invalid request."));
    }
    let bytes = get_file(&conn, &path).await?;
    let dir = app.path().download_dir().map_err(|_| local("no_downloads", "Can’t find the Downloads folder."))?;
    // A plain file name only, never a path; don't overwrite an existing file.
    let clean: String = filename.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.').to_string();
    let clean = if clean.is_empty() { "download".to_string() } else { clean };
    let (stem, ext) = match clean.rfind('.') { Some(i) if i > 0 => (clean[..i].to_string(), clean[i..].to_string()), _ => (clean.clone(), String::new()) };
    let mut target = dir.join(&clean);
    let mut n = 2;
    while target.exists() {
        target = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    std::fs::write(&target, &bytes).map_err(|_| local("save_failed", "Couldn’t save the file in Downloads."))?;
    Ok(target.to_string_lossy().to_string())
}

fn is_id(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Only server files: finance files, patient documents and clinical-record exports, by id; and the
/// access-log CSV with its filters (plain query characters only).
#[cfg(any(desktop, test))]
fn file_path_ok(path: &str) -> bool {
    if let Some(rest) = path.strip_prefix("/access-log/export") {
        return rest.is_empty() || rest.strip_prefix('?').is_some_and(|q| q.len() <= 400 && q.chars().all(|c| c.is_ascii_alphanumeric() || "=&%._-+,".contains(c)));
    }
    ["/finance/files/", "/documents/"].iter()
        .any(|p| path.strip_prefix(p).and_then(|r| r.strip_suffix("/raw")).is_some_and(is_id))
        || ["/export/full", "/export/summary"].iter()
            .any(|k| path.strip_prefix("/patients/").and_then(|r| r.strip_suffix(k)).is_some_and(is_id))
}

/// Fetches one allowed server file (see `file_path_ok`), with the token attached here.
async fn get_file(conn: &State<'_, Conn>, path: &str) -> Result<Vec<u8>, ApiError> {
    let base = conn.base.lock().unwrap().clone().ok_or_else(|| local("no_server", "No server set up yet."))?;
    let mut req = device_headers(conn, client_for(Duration::from_secs(120)).get(format!("{base}/api{path}")));
    if let Some(t) = conn.token.lock().unwrap().clone() {
        req = req.bearer_auth(t);
    }
    let res = req.send().await.map_err(|e| send_error(e, false))?;
    let status = res.status().as_u16();
    if status >= 400 {
        if status == 401 {
            *conn.token.lock().unwrap() = None;
        }
        let json: Value = res.json().await.unwrap_or(Value::Null);
        let message = json.get("message").and_then(|m| m.as_str()).unwrap_or("Something went wrong.").to_string();
        let code = json.get("error").and_then(|m| m.as_str()).map(str::to_string);
        return Err(ApiError { status, message, code, details: None });
    }
    Ok(res.bytes().await.map_err(|_| local("interrupted", "The download was interrupted. Try again."))?.to_vec())
}

/// Shows an invoice PDF or receipt inside the app: the bytes go to the UI as an ArrayBuffer, nothing is
/// written to disk. Only `/finance/files/<id>/raw` and `/recordings/<id>/audio` (patient documents have their own viewer route).
#[tauri::command]
pub async fn view_file(conn: State<'_, Conn>, path: String) -> Result<tauri::ipc::Response, ApiError> {
    let ok = path.strip_prefix("/finance/files/").and_then(|r| r.strip_suffix("/raw")).is_some_and(is_id)
        // A session recording to play (PhysioPulse plan, phase 2).
        || path.strip_prefix("/recordings/").and_then(|r| r.strip_suffix("/audio")).is_some_and(is_id);
    if !ok {
        return Err(local("invalid", "Invalid request."));
    }
    Ok(tauri::ipc::Response::new(get_file(&conn, &path).await?))
}

/// Called when the window closes: end the session on the server, best effort, max 2 s. (Desktop only.)
#[cfg(desktop)]
pub fn sign_out_on_close(app: &AppHandle) {
    let conn = app.state::<Conn>();
    let token = conn.token.lock().unwrap().take();
    let base = conn.base.lock().unwrap().clone();
    let key = conn.key.lock().unwrap().clone().unwrap_or_default();
    if let (Some(t), Some(b)) = (token, base) {
        tauri::async_runtime::block_on(async move {
            let _ = tokio::time::timeout(
                Duration::from_secs(2),
                client().post(format!("{b}/api/auth/logout")).bearer_auth(t).header("X-Device", device()).header("X-Device-Key", key).send(),
            )
            .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "0b9f3c2e-1d4a-4c55-9e7a-2f1b3c4d5e6f";
    #[test]
    fn only_server_files() {
        assert!(file_path_ok(&format!("/finance/files/{ID}/raw")));
        assert!(file_path_ok(&format!("/documents/{ID}/raw")));
        assert!(file_path_ok(&format!("/patients/{ID}/export/full")));
        assert!(!file_path_ok(&format!("/finance/files/{ID}")));
        assert!(!file_path_ok("/finance/files/../../etc/raw"));
        assert!(!file_path_ok(&format!("/patients/{ID}/money")));
        assert!(file_path_ok("/access-log/export"));
        assert!(file_path_ok("/access-log/export?from=2026-11-01&to=2026-11-10&actions=patient.viewed%2Clogin.&q=ana%20silva"));
        assert!(!file_path_ok("/access-log/export/../../patients"));
        assert!(!file_path_ok("/access-log/export?x=/etc/passwd"));
        assert!(!file_path_ok("/access-log"));
    }
    #[test]
    fn carries_over_old_settings_once() {
        let root = std::env::temp_dir().join(format!("pp-carry-{}", std::process::id()));
        let (old, new) = (root.join(OLD_ID), root.join("app.physiopulse"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("server.json"), r#"{"server":"100.68.180.126:8095"}"#).unwrap();
        std::fs::write(old.join("account.json"), r#"{"accounts":[]}"#).unwrap();
        carry_over(&new);
        assert_eq!(std::fs::read_to_string(new.join("server.json")).unwrap(), r#"{"server":"100.68.180.126:8095"}"#);
        assert!(new.join("account.json").exists());
        assert!(old.join("server.json").exists(), "the old folder is left as it was");
        // Once PhysioPulse has its own settings, the old ones are never copied over them again.
        std::fs::write(new.join("server.json"), r#"{"server":"other:8095"}"#).unwrap();
        carry_over(&new);
        assert_eq!(std::fs::read_to_string(new.join("server.json")).unwrap(), r#"{"server":"other:8095"}"#);
        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn https_on_the_internet() {
        assert_eq!(normalise("clinica.duckdns.org").ok().unwrap(), "https://clinica.duckdns.org");
        assert_eq!(normalise("https://clinica.duckdns.org/").ok().unwrap(), "https://clinica.duckdns.org");
        assert!(normalise("http://clinica.duckdns.org").is_err(), "plain http to the internet is refused");
        assert_eq!(normalise("192.168.1.64:8095").ok().unwrap(), "http://192.168.1.64:8095");
        assert_eq!(normalise("100.68.180.126:8095").ok().unwrap(), "http://100.68.180.126:8095", "Tailscale still works until it goes");
        assert_eq!(normalise("zimaos:8080").ok().unwrap(), "http://zimaos:8080");
        assert_eq!(normalise("http://localhost:8080").ok().unwrap(), "http://localhost:8080");
        assert!(normalise("http://8.8.8.8:8080").is_err());
        assert!(normalise("ftp://x").is_err());
        assert!(normalise("").is_err());
    }
    #[test]
    fn device_key_is_made_once() {
        let dir = std::env::temp_dir().join(format!("pp-key-{}", std::process::id()));
        let a = device_key(&dir).unwrap();
        assert_eq!(a.len(), 64);
        assert_eq!(device_key(&dir).unwrap(), a, "the same key on every start");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn ai_waits_longer() {
        assert!(timeout_for("POST", &format!("/patients/{ID}/letters/ai-draft")) >= Duration::from_secs(300));
        assert_eq!(timeout_for("GET", "/ai/status"), Duration::from_secs(30));
        assert!(timeout_for("POST", "/ai/rewrite-note") >= Duration::from_secs(300));
        assert_eq!(timeout_for("GET", "/patients"), Duration::from_secs(30));
        assert_eq!(timeout_for("POST", "/patients"), Duration::from_secs(120));
    }
}
