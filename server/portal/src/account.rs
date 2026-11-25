//! The patient area, /conta (PhysioPulse plan, phase 4b, 23 Nov 2026).
//!
//! Owner's rules (DECISIONS.md, 23 Nov): an account starts from an invite code made in the clinic (no date-of-birth
//! check); a forgotten password needs a reset code from reception, the patient's therapist or the owner; two-step
//! codes are optional; under-16s are seen through a guardian's account; patients see their sessions, ask for
//! appointments or other times, cancel up to 24 h before and download invoices — never clinical notes or tests.
//! The whole area stays closed (`accounts_open`) until the owner opens it.
//!
//! Signing in happens here, through the database functions of migration 0024. What a signed-in patient sees or
//! does comes from the staff server's internal API (server/src/routes/portal.rs), which answers only for the
//! patients linked to the session's account. Still no JavaScript; one cookie, only on these pages: the session
//! (HttpOnly, Secure, SameSite=Strict). Every form on a signed-in page carries a token tied to the session.

use crate::html::{self, esc, money};
use crate::{href, Data, Lang, Page, St};
use argon2::{password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString}, Argon2};
use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use chacha20poly1305::{aead::{Aead, KeyInit}, ChaCha20Poly1305, Nonce};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fmt::Write, net::SocketAddr};
use time::OffsetDateTime;
use totp_rs::{Algorithm, Secret, TOTP};

const COOKIE: &str = "pp_conta";

/// The sub-pages, Portuguese and English addresses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sub { Home, SignIn, Code, SignOut, Invite, Reset, Request, Cancel, Security }
pub const SUBS: [(Sub, &str, &str); 9] = [
    (Sub::Home, "/conta", "/en/account"),
    (Sub::SignIn, "/conta/entrar", "/en/account/sign-in"),
    (Sub::Code, "/conta/codigo", "/en/account/code"),
    (Sub::SignOut, "/conta/sair", "/en/account/sign-out"),
    (Sub::Invite, "/conta/convite", "/en/account/invite"),
    (Sub::Reset, "/conta/repor", "/en/account/reset"),
    (Sub::Request, "/conta/pedido", "/en/account/request"),
    (Sub::Cancel, "/conta/cancelar", "/en/account/cancel"),
    (Sub::Security, "/conta/seguranca", "/en/account/security"),
];
pub fn sub(p: Sub, l: Lang) -> &'static str {
    let (_, pt, en) = SUBS.iter().find(|(x, _, _)| *x == p).unwrap();
    if l == Lang::Pt { pt } else { en }
}
pub fn invoice_path(l: Lang) -> &'static str { if l == Lang::Pt { "/conta/fatura/:id" } else { "/en/account/invoice/:id" } }

// ───────── small helpers ─────────

fn sha_hex(s: &str) -> String { hex::encode(Sha256::digest(s.as_bytes())) }

fn hmac_hex(key: &[u8], msg: &str) -> String {
    let mut m = <Hmac::<Sha256> as Mac>::new_from_slice(key).expect("hmac key");
    m.update(msg.as_bytes());
    hex::encode(m.finalize().into_bytes())
}

/// The token on a signed-in page's forms: tied to the session, so another site can't post as the patient.
fn csrf(key: &[u8], token: &str) -> String { hmac_hex(key, &format!("conta:{token}"))[..32].to_string() }

fn cookie_of(h: &HeaderMap) -> Option<String> {
    h.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';')).map(str::trim)
        .find_map(|c| c.strip_prefix(&format!("{COOKIE}=")).map(str::to_string))
        .filter(|t| t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()))
}

fn set_cookie(r: &mut Response, token: Option<&str>, secure: bool) {
    let sec = if secure { "; Secure" } else { "" };
    let v = match token {
        Some(t) => format!("{COOKIE}={t}; Path=/; HttpOnly; SameSite=Strict; Max-Age=43200{sec}"),
        None => format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{sec}"),
    };
    if let Ok(v) = HeaderValue::from_str(&v) { r.headers_mut().append(header::SET_COOKIE, v); }
}

fn new_token() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

/// Passwords: 10 to 200 characters (the person's own choice; Argon2id with the crate's defaults).
fn hash_password(p: &str) -> Option<String> {
    Argon2::default().hash_password(p.as_bytes(), &SaltString::generate(&mut OsRng)).ok().map(|h| h.to_string())
}
fn password_ok(p: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(p.as_bytes(), &h).is_ok())
}
/// Same work as a real check, so an unknown email answers as slowly as a wrong password.
fn waste() { let _ = hash_password("not a real password"); }

// Two-step secrets are kept encrypted (ChaCha20-Poly1305 with a key derived from FORM_KEY).
fn totp_key(key: &[u8]) -> [u8; 32] { Sha256::digest([b"physiopulse-totp:".as_slice(), key].concat()).into() }
fn seal(key: &[u8], secret: &[u8]) -> String {
    let c = ChaCha20Poly1305::new((&totp_key(key)).into());
    let mut n = [0u8; 12];
    OsRng.fill_bytes(&mut n);
    let ct = c.encrypt(Nonce::from_slice(&n), secret).expect("encrypt");
    format!("{}.{}", hex::encode(n), hex::encode(ct))
}
fn open(key: &[u8], sealed: &str) -> Option<Vec<u8>> {
    let (n, ct) = sealed.split_once('.')?;
    let (n, ct) = (hex::decode(n).ok()?, hex::decode(ct).ok()?);
    if n.len() != 12 { return None; }
    ChaCha20Poly1305::new((&totp_key(key)).into()).decrypt(Nonce::from_slice(&n), ct.as_slice()).ok()
}
fn totp(secret: Vec<u8>, email: &str) -> Option<TOTP> {
    TOTP::new(Algorithm::SHA1, 6, 1, 30, secret, Some("PhysioPulse".into()), email.to_string()).ok()
}
fn code_ok(t: &TOTP, code: &str) -> bool {
    let c: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    c.len() == 6 && t.check_current(&c).unwrap_or(false)
}

/// Error codes from the database functions, as text.
fn db_code(e: &sqlx::Error) -> String {
    match e { sqlx::Error::Database(d) => d.message().to_string(), _ => String::new() }
}

/// One count in a rate-limit window (migration 0018's portal.hit); true while under the limit.
async fn hit(st: &St, key: &str, max: i32, minutes: i32) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT portal.hit($1, $2, $3)").bind(key).bind(max).bind(minutes).fetch_one(&st.pool).await.unwrap_or(false)
}
fn who(st: &St, peer: SocketAddr, h: &HeaderMap) -> String {
    let ip = crate::real_ip(&st.proxies, peer.ip(), h.get("x-forwarded-for").and_then(|v| v.to_str().ok()));
    hex::encode(&Sha256::digest([&st.key[..], ip.as_bytes()].concat())[..16])
}

// ───────── the session ─────────

pub struct Session { pub token: String, pub email: String, pub pending: bool, pub totp: Option<String>, pub lang: String }

async fn session(st: &St, h: &HeaderMap) -> Option<Session> {
    let token = cookie_of(h)?;
    let row: Option<(sqlx::types::Uuid, String, bool, Option<String>, String)> =
        sqlx::query_as("SELECT account_id, email, pending_2fa, totp_secret, lang FROM portal.session_get($1)")
            .bind(sha_hex(&token)).fetch_optional(&st.pool).await.ok().flatten();
    row.map(|(_, email, pending, totp, lang)| Session { token, email, pending, totp, lang })
}

// ───────── the staff server's internal API ─────────

async fn api(st: &St, method: &str, path: &str, token: &str, l: Lang, body: Option<Value>) -> Result<reqwest::Response, ()> {
    let Some(key) = st.internal else { return Err(()) };
    let url = format!("{}/api/internal/portal{path}", st.server_url);
    let mut r = if method == "POST" { st.http.post(url) } else { st.http.get(url) };
    r = r.header("x-portal-key", hex::encode(key)).header("x-portal-session", token).header("accept-language", l.code());
    if let Some(b) = body { r = r.json(&b); }
    r.send().await.map_err(|e| tracing::error!("internal API: {e}"))
}

/// The server's message on a refused call, already in the patient's language.
async fn api_error(r: reqwest::Response) -> String {
    r.json::<Value>().await.ok().and_then(|v| v["message"].as_str().map(str::to_string)).unwrap_or_default()
}

// ───────── pages ─────────

async fn data(st: &St) -> Option<Data> {
    crate::load(&st.pool, Page::Account).await.map_err(|e| tracing::error!("database: {e}")).ok()
}

fn page(st: &St, d: &Data, l: Lang, status: StatusCode, title: &str, body: &str) -> Response {
    let _ = st;
    let mut r = (status, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], html::layout(d, Page::Account, l, title, body)).into_response();
    r.headers_mut().insert("x-robots-tag", HeaderValue::from_static("noindex"));
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn unavailable(l: Lang) -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], html::unavailable(l)).into_response()
}

fn errors(list: &[String], l: Lang) -> String {
    if list.is_empty() { return String::new(); }
    let items: String = list.iter().map(|m| format!("<li>{}</li>", esc(m))).collect();
    format!(r#"<div class="errors" role="alert"><p class="h">{}</p><ul>{items}</ul></div>"#, l.t("Não foi possível continuar:", "We couldn't continue:"))
}

fn head(title: &str, sub_text: &str) -> String {
    let p = if sub_text.is_empty() { String::new() } else { format!(r#"<p class="sub">{}</p>"#, sub_text) };
    format!(r#"<section class="phead"><h1>{}</h1>{p}</section>"#, esc(title))
}

/// The area when it's closed, or when the website isn't published.
fn closed(st: &St, d: &Data, l: Lang) -> Response {
    let body = format!(r#"{}<div class="card"><p class="pill">{}</p><p>{}</p>{}</div>"#, head(l.t("Área de utente", "Patient area"), ""),
        l.t("Em breve", "Coming soon"),
        l.t("Em breve vai poder ver as suas marcações e faturas aqui, pedir uma hora e cancelar uma sessão. Até lá, fale connosco:",
            "Soon you'll be able to see your appointments and invoices here, ask for a time and cancel a session. Until then, get in touch:"),
        html::contacts(&d.site, l));
    page(st, d, l, StatusCode::OK, l.t("Área de utente", "Patient area"), &body)
}

fn open_now(d: &Data) -> bool { d.site.published && d.site.accounts_open }

fn sign_in_form(l: Lang, email: &str, errs: &[String]) -> String {
    format!(r#"{head}{errs}<form class="card form narrow" method="post" action="{action}" novalidate>
<div class="f"><label for="f-email">Email</label><input id="f-email" name="email" type="email" autocomplete="username" maxlength="200" value="{email}" required></div>
<div class="f"><label for="f-password">{pwl}</label><input id="f-password" name="password" type="password" autocomplete="current-password" maxlength="200" required></div>
<p class="ctas"><button class="btn pri lg" type="submit">{go}</button></p>
<p class="small"><a href="{invite}">{invitel}</a> · <a href="{reset}">{resetl}</a></p></form>"#,
        head = head(l.t("Área de utente", "Patient area"), l.t("As suas sessões, pedidos e faturas.", "Your sessions, requests and invoices.")),
        errs = errors(errs, l), action = sub(Sub::SignIn, l), email = esc(email), pwl = l.t("Palavra-passe", "Password"), go = l.t("Entrar", "Sign In"),
        invite = sub(Sub::Invite, l), invitel = l.t("Tenho um código de convite", "I have an invite code"),
        reset = sub(Sub::Reset, l), resetl = l.t("Esqueci-me da palavra-passe", "I forgot my password"))
}

fn code_form(l: Lang, errs: &[String]) -> String {
    format!(r#"{head}{errs}<form class="card form narrow" method="post" action="{action}" novalidate>
<div class="f"><label for="f-code">{codel}</label><input id="f-code" name="code" inputmode="numeric" autocomplete="one-time-code" maxlength="8" required></div>
<p class="ctas"><button class="btn pri lg" type="submit">{go}</button></p>
<p class="small">{lost}</p></form>"#,
        head = head(l.t("Código de dois passos", "Two-step code"), l.t("Escreva os 6 algarismos da sua app de autenticação.", "Enter the 6 digits from your authenticator app.")),
        errs = errors(errs, l), action = sub(Sub::Code, l), codel = l.t("Código", "Code"), go = l.t("Continuar", "Continue"),
        lost = l.t("Perdeu o telemóvel? Peça à clínica um código para repor o acesso.", "Lost your phone? Ask the clinic for a code to get back in."))
}

// ───────── GET /conta ─────────

pub async fn home(st: St, l: Lang, h: HeaderMap) -> Response {
    let Some(d) = data(&st).await else { return unavailable(l) };
    if !open_now(&d) { return closed(&st, &d, l); }
    let Some(s) = session(&st, &h).await else {
        return page(&st, &d, l, StatusCode::OK, l.t("Entrar", "Sign in"), &sign_in_form(l, "", &[]));
    };
    if s.pending { return page(&st, &d, l, StatusCode::OK, l.t("Código", "Code"), &code_form(l, &[])); }
    let r = match api(&st, "GET", "/me", &s.token, l, None).await { Ok(r) => r, Err(()) => return unavailable(l) };
    if r.status() == StatusCode::UNAUTHORIZED {
        return page(&st, &d, l, StatusCode::OK, l.t("Entrar", "Sign in"), &sign_in_form(l, "", &[]));
    }
    let Ok(me) = r.json::<Value>().await else { return unavailable(l) };
    let body = dashboard(&me, l, &csrf(&st.key[..], &s.token));
    page(&st, &d, l, StatusCode::OK, l.t("Área de utente", "Patient area"), &body)
}

fn when(iso: &str, l: Lang) -> String {
    let Ok(t) = OffsetDateTime::parse(iso, &time::format_description::well_known::Rfc3339) else { return esc(iso) };
    let t = t.to_offset(crate::lisbon_offset(t));
    const PT: [&str; 7] = ["segunda", "terça", "quarta", "quinta", "sexta", "sábado", "domingo"];
    const EN: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let wd = t.weekday().number_days_from_monday() as usize;
    format!("{}, {} · {:02}:{:02}", if l == Lang::Pt { PT[wd] } else { EN[wd] }, html::date_text(t.date(), l), t.hour(), t.minute())
}
fn day(iso: &str, l: Lang) -> String {
    time::Date::parse(&iso[..iso.len().min(10)], &time::macros::format_description!("[year]-[month]-[day]")).map(|d| html::date_text(d, l)).unwrap_or_else(|_| esc(iso))
}
fn kind_label(k: &str, l: Lang) -> &'static str {
    match k { "assessment" => l.t("Avaliação", "Assessment"), "home" => l.t("Ao domicílio", "Home visit"), "class" => l.t("Aula de grupo", "Group class"), _ => l.t("Sessão", "Session") }
}

fn dashboard(me: &Value, l: Lang, c: &str) -> String {
    let empty = vec![];
    let people = me["people"].as_array().unwrap_or(&empty);
    let appts = me["appointments"].as_array().unwrap_or(&empty);
    let pays = me["payments"].as_array().unwrap_or(&empty);
    let owed = me["owed"].as_array().unwrap_or(&empty);
    let reqs = me["requests"].as_array().unwrap_or(&empty);
    let many = people.len() > 1 || people.iter().any(|p| p["relation"] == "guardian");
    let name_of = |pid: &Value| people.iter().find(|p| p["id"] == *pid).and_then(|p| p["name"].as_str()).unwrap_or("").to_string();
    let first = |n: &str| n.split_whitespace().next().unwrap_or("").to_string();
    let now = OffsetDateTime::now_utc();
    let future = |a: &&Value| a["starts_at"].as_str().and_then(|s| OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()).is_some_and(|t| t > now);

    let mut o = head(l.t("Área de utente", "Patient area"), &esc(me["email"].as_str().unwrap_or("")));
    // What's booked.
    let next: Vec<&Value> = appts.iter().filter(|a| a["status"] == "booked").filter(future).collect();
    let _ = write!(o, r#"<section class="card"><h2>{}</h2>"#, l.t("Próximas sessões", "Upcoming sessions"));
    if next.is_empty() {
        let _ = write!(o, "<p>{}</p>", l.t("Não tem sessões marcadas.", "You have no sessions booked."));
    } else {
        o.push_str(r#"<ul class="list">"#);
        for a in &next {
            let id = esc(a["id"].as_str().unwrap_or(""));
            let who = if many { format!("{} · ", esc(&first(&name_of(&a["patient_id"])))) } else { String::new() };
            let th = a["therapist"].as_str().filter(|t| !t.is_empty()).map(|t| format!(" · {}", esc(t))).unwrap_or_default();
            let mut acts = String::new();
            if a["move_asked"] == true {
                let _ = write!(acts, r#"<span class="tag">{}</span>"#, l.t("Pedido de mudança enviado", "Change requested"));
            } else if a["can_move"] == true {
                let _ = write!(acts, r#"<a class="btn sm" href="{}?mudar={id}">{}</a>"#, sub(Sub::Request, l), l.t("Pedir outra hora", "Ask for Another Time"));
            }
            if a["can_cancel"] == true {
                let _ = write!(acts, r#"<a class="btn sm ghost" href="{}?id={id}">{}</a>"#, sub(Sub::Cancel, l), l.t("Cancelar…", "Cancel…"));
            }
            let _ = write!(o, r#"<li><div><p class="t dt">{}</p><p class="s">{who}{}{th}</p></div><div class="acts">{acts}</div></li>"#,
                when(a["starts_at"].as_str().unwrap_or(""), l), kind_label(a["kind"].as_str().unwrap_or(""), l));
        }
        o.push_str("</ul>");
        let _ = write!(o, r#"<p class="small">{}</p>"#, l.t("Pode cancelar aqui até 24 horas antes. Depois disso, ligue para a clínica.", "You can cancel here up to 24 hours before. After that, call the clinic."));
    }
    let _ = write!(o, r#"<p class="ctas"><a class="btn pri" href="{}">{}</a></p></section>"#, sub(Sub::Request, l), l.t("Pedir marcação", "Request an Appointment"));

    // Requests waiting for the clinic.
    let waiting: Vec<&Value> = reqs.iter().filter(|r| r["status"] == "pending").collect();
    if !waiting.is_empty() {
        let _ = write!(o, r#"<section class="card"><h2>{}</h2><ul class="list">"#, l.t("Pedidos à espera de resposta", "Requests waiting for an answer"));
        for r in waiting {
            let what = if r["kind"] == "move" { l.t("Outra hora para uma sessão", "Another time for a session") } else { kind_label(r["service"].as_str().unwrap_or(""), l) };
            let part = match r["day_part"].as_str().unwrap_or("") { "morning" => l.t("manhã", "morning"), "afternoon" => l.t("tarde", "afternoon"), _ => l.t("qualquer hora", "any time") };
            let _ = write!(o, r#"<li><div><p class="t">{what}</p><p class="s"><span class="dt">{}</span>, {part}</p></div></li>"#, day(r["preferred_at"].as_str().unwrap_or(""), l));
        }
        let _ = write!(o, r#"</ul><p class="small">{}</p></section>"#, l.t("A clínica responde por telefone ou email.", "The clinic will answer by phone or email."));
    }

    // Money.
    let due: i64 = owed.iter().filter_map(|x| x["due_cents"].as_i64()).sum();
    let _ = write!(o, r#"<section class="card"><h2>{}</h2>"#, l.t("Pagamentos e faturas", "Payments and invoices"));
    if due > 0 {
        let _ = write!(o, r#"<p class="due"><span>{}</span><b>{}</b></p>"#, l.t("Por pagar", "To pay"), money(due as i32, l));
    }
    if pays.is_empty() {
        let _ = write!(o, "<p>{}</p>", l.t("Ainda não há pagamentos.", "No payments yet."));
    } else {
        o.push_str(r#"<ul class="list">"#);
        for p in pays.iter().take(24) {
            let who = if many { format!("{} · ", esc(&first(&name_of(&p["patient_id"])))) } else { String::new() };
            let desc = p["description"].as_str().filter(|d| !d.trim().is_empty()).map(esc).unwrap_or_else(|| l.t("Pagamento", "Payment").into());
            let inv = if p["has_invoice"] == true {
                format!(r#"<a class="btn sm" href="{}">{}</a>"#, invoice_path(l).replace(":id", &esc(p["id"].as_str().unwrap_or(""))),
                    l.t("Fatura (PDF)", "Invoice (PDF)"))
            } else if p["invoice_number"].as_str().is_some_and(|n| !n.is_empty()) {
                format!(r#"<span class="s">{} {}</span>"#, l.t("Fatura", "Invoice"), esc(p["invoice_number"].as_str().unwrap_or("")))
            } else { String::new() };
            let _ = write!(o, r#"<li><div><p class="t">{}</p><p class="s">{who}<span class="dt">{}</span> · {desc}</p></div><div class="acts">{inv}</div></li>"#,
                money(p["amount_cents"].as_i64().unwrap_or(0) as i32, l), day(p["received_on"].as_str().unwrap_or(""), l));
        }
        o.push_str("</ul>");
    }
    o.push_str("</section>");

    // The last sessions.
    // Sessions already past (a cancelled future one just disappears from the list above).
    let past: Vec<&Value> = appts.iter().filter(|a| !future(a) && a["status"] != "booked").rev().take(8).collect();
    if !past.is_empty() {
        let _ = write!(o, r#"<section class="card"><h2>{}</h2><ul class="list">"#, l.t("Sessões anteriores", "Past sessions"));
        for a in past {
            let st = match a["status"].as_str().unwrap_or("") { "done" => l.t("Realizada", "Done"), "no_show" => l.t("Falta", "Missed"), "cancelled" => l.t("Cancelada", "Cancelled"), _ => "" };
            let who = if many { format!("{} · ", esc(&first(&name_of(&a["patient_id"])))) } else { String::new() };
            let _ = write!(o, r#"<li><div><p class="t dt">{}</p><p class="s">{who}{} · {st}</p></div></li>"#, when(a["starts_at"].as_str().unwrap_or(""), l), kind_label(a["kind"].as_str().unwrap_or(""), l));
        }
        o.push_str("</ul></section>");
    }
    let _ = write!(o, r#"<form class="foot-acts" method="post" action="{}"><input type="hidden" name="c" value="{c}"><a class="btn ghost" href="{}">{}</a><button class="btn ghost" type="submit">{}</button></form>"#,
        sub(Sub::SignOut, l), sub(Sub::Security, l), l.t("Palavra-passe e segurança", "Password and Security"), l.t("Sair", "Sign Out"));
    o
}

// ───────── sign in, two-step, sign out ─────────

#[derive(Deserialize, Default)]
pub struct SignInForm { #[serde(default)] email: String, #[serde(default)] password: String }

pub async fn sign_in(st: St, l: Lang, peer: SocketAddr, h: HeaderMap, f: SignInForm) -> Response {
    let Some(d) = data(&st).await else { return unavailable(l) };
    if !open_now(&d) { return closed(&st, &d, l); }
    let email = f.email.trim().to_lowercase();
    let w = who(&st, peer, &h);
    let fail = |msg: &str| page(&st, &d, l, StatusCode::UNPROCESSABLE_ENTITY, l.t("Entrar", "Sign in"), &sign_in_form(l, &email, &[msg.to_string()]));
    // 10 tries in 15 minutes from one place, and 10 for one email.
    if !hit(&st, &format!("login:{w}"), 10, 15).await || !hit(&st, &format!("loginmail:{}", sha_hex(&email)), 10, 15).await {
        return fail(l.t("Demasiadas tentativas. Espere 15 minutos e tente de novo.", "Too many tries. Wait 15 minutes and try again."));
    }
    let row: Option<(sqlx::types::Uuid, String, Option<String>)> = sqlx::query_as("SELECT id, password_hash, totp_secret FROM portal.account_by_email($1)")
        .bind(&email).fetch_optional(&st.pool).await.ok().flatten();
    // The same answer for an unknown email and a wrong password.
    let wrong = l.t("O email ou a palavra-passe não estão certos.", "The email or password isn't right.");
    let Some((id, hash, totp_secret)) = row else { waste(); return fail(wrong) };
    if !password_ok(&f.password, &hash) { return fail(wrong); }
    let token = new_token();
    let pending = totp_secret.is_some();
    if sqlx::query("SELECT portal.session_start($1, $2, $3)").bind(id).bind(sha_hex(&token)).bind(pending).execute(&st.pool).await.is_err() {
        return unavailable(l);
    }
    let mut r = Redirect::to(sub(Sub::Home, l)).into_response();
    set_cookie(&mut r, Some(&token), st.secure_cookie);
    r
}

#[derive(Deserialize, Default)]
pub struct CodeForm { #[serde(default)] code: String }

pub async fn two_step(st: St, l: Lang, peer: SocketAddr, h: HeaderMap, f: CodeForm) -> Response {
    let Some(d) = data(&st).await else { return unavailable(l) };
    if !open_now(&d) { return closed(&st, &d, l); }
    let Some(s) = session(&st, &h).await else { return Redirect::to(sub(Sub::Home, l)).into_response() };
    if !s.pending { return Redirect::to(sub(Sub::Home, l)).into_response(); }
    let w = who(&st, peer, &h);
    if !hit(&st, &format!("code:{w}"), 10, 15).await || !hit(&st, &format!("codeacct:{}", sha_hex(&s.email)), 10, 15).await {
        return page(&st, &d, l, StatusCode::TOO_MANY_REQUESTS, l.t("Código", "Code"), &code_form(l, &[l.t("Demasiadas tentativas. Espere 15 minutos.", "Too many tries. Wait 15 minutes.").into()]));
    }
    let ok = s.totp.as_deref().and_then(|x| open(&st.key[..], x)).and_then(|sec| totp(sec, &s.email)).is_some_and(|t| code_ok(&t, &f.code));
    if !ok {
        return page(&st, &d, l, StatusCode::UNPROCESSABLE_ENTITY, l.t("Código", "Code"), &code_form(l, &[l.t("Esse código não está certo. Use o que a app mostra agora.", "That code isn't right. Use the one the app shows now.").into()]));
    }
    let _ = sqlx::query("SELECT portal.session_two_step_ok($1)").bind(sha_hex(&s.token)).execute(&st.pool).await;
    Redirect::to(sub(Sub::Home, l)).into_response()
}

#[derive(Deserialize, Default)]
pub struct TokenForm { #[serde(default)] c: String }

pub async fn sign_out(st: St, l: Lang, h: HeaderMap, f: TokenForm) -> Response {
    if let Some(t) = cookie_of(&h) {
        if f.c == csrf(&st.key[..], &t) {
            let _ = sqlx::query("SELECT portal.session_end($1)").bind(sha_hex(&t)).execute(&st.pool).await;
        } else {
            return Redirect::to(sub(Sub::Home, l)).into_response();
        }
    }
    let mut r = Redirect::to(sub(Sub::Home, l)).into_response();
    set_cookie(&mut r, None, st.secure_cookie);
    r
}

// ───────── invite and reset codes ─────────

#[derive(Deserialize, Default)]
pub struct CodeQuery { #[serde(default)] codigo: String, #[serde(default)] code: String }

#[derive(Deserialize, Default, Clone)]
pub struct InviteForm { #[serde(default)] code: String, #[serde(default)] email: String, #[serde(default)] password: String, #[serde(default)] password2: String, #[serde(default)] privacy: String }

fn invite_form(l: Lang, f: &InviteForm, errs: &[String]) -> String {
    format!(r#"{head}{errs}<form class="card form narrow" method="post" action="{action}" novalidate>
<p class="lead">{lead}</p>
<div class="f"><label for="f-code">{codel}</label><input id="f-code" name="code" autocomplete="off" maxlength="20" value="{code}" required></div>
<div class="f"><label for="f-email">Email</label><input id="f-email" name="email" type="email" autocomplete="username" maxlength="200" value="{email}" required><p class="hint">{emailhint}</p></div>
<div class="f"><label for="f-password">{pwl}</label><input id="f-password" name="password" type="password" autocomplete="new-password" maxlength="200" required><p class="hint">{pwhint}</p></div>
<div class="f"><label for="f-password2">{pw2l}</label><input id="f-password2" name="password2" type="password" autocomplete="new-password" maxlength="200" required></div>
<div class="f"><label class="check"><input type="checkbox" name="privacy"{chk}><span>{priv}</span></label></div>
<p class="ctas"><button class="btn pri lg" type="submit">{go}</button></p></form>"#,
        head = head(l.t("Ativar a área de utente", "Set up your patient area"), ""), errs = errors(errs, l), action = sub(Sub::Invite, l),
        lead = l.t("Escreva o código que a clínica lhe deu e escolha o email e a palavra-passe com que vai entrar. Se já tem conta, use o mesmo email e a palavra-passe que já tem.",
                   "Enter the code the clinic gave you and choose the email and password you'll sign in with. If you already have an account, use the same email and your current password."),
        codel = l.t("Código de convite", "Invite code"), code = esc(&f.code), email = esc(&f.email),
        emailhint = l.t("Só para entrar. A clínica não lhe envia emails por aqui.", "Only for signing in. The clinic doesn't email you from here."),
        pwl = l.t("Palavra-passe", "Password"), pwhint = l.t("Pelo menos 10 caracteres.", "At least 10 characters."),
        pw2l = l.t("Repita a palavra-passe", "Repeat the password"), chk = if f.privacy == "on" { " checked" } else { "" },
        priv = format!("{} <a href=\"{}\">{}</a>.", l.t("Li a", "I have read the"), href(Page::Privacy, l), l.t("política de privacidade", "privacy policy")),
        go = l.t("Ativar", "Set Up"))
}

pub async fn invite_page(st: St, l: Lang, q: CodeQuery) -> Response {
    let Some(d) = data(&st).await else { return unavailable(l) };
    if !open_now(&d) { return closed(&st, &d, l); }
    let f = InviteForm { code: if q.codigo.is_empty() { q.code } else { q.codigo }, ..Default::default() };
    page(&st, &d, l, StatusCode::OK, l.t("Ativar a área de utente", "Set up your patient area"), &invite_form(l, &f, &[]))
}

pub async fn invite(st: St, l: Lang, peer: SocketAddr, h: HeaderMap, f: InviteForm) -> Response {
    let Some(d) = data(&st).await else { return unavailable(l) };
    if !open_now(&d) { return closed(&st, &d, l); }
    let title = l.t("Ativar a área de utente", "Set up your patient area");
    let bad = |msgs: Vec<String>| page(&st, &d, l, StatusCode::UNPROCESSABLE_ENTITY, title, &invite_form(l, &InviteForm { password: String::new(), password2: String::new(), ..f.clone() }, &msgs));
    let w = who(&st, peer, &h);
    if !hit(&st, &format!("invite:{w}"), 10, 60).await {
        return bad(vec![l.t("Demasiadas tentativas. Espere uma hora ou fale com a clínica.", "Too many tries. Wait an hour or talk to the clinic.").into()]);
    }
    let email = f.email.trim().to_lowercase();
    let mut e = vec![];
    if f.code.chars().filter(|c| c.is_ascii_alphanumeric()).count() != 12 { e.push(l.t("Escreva o código de 12 letras e algarismos que a clínica lhe deu.", "Enter the 12-character code the clinic gave you.").to_string()); }
    if email.len() > 200 || !email.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.') && !b.ends_with('.')) || email.contains(' ') {
        e.push(l.t("Verifique o endereço de email.", "Check the email address.").into());
    }
    if f.privacy != "on" { e.push(l.t("Confirme que leu a política de privacidade.", "Confirm you have read the privacy policy.").into()); }
    if !e.is_empty() { return bad(e); }
    // An account with this email already: the password is the one it has, and the patient is added to it.
    let existing: Option<(sqlx::types::Uuid, String, Option<String>)> = sqlx::query_as("SELECT id, password_hash, totp_secret FROM portal.account_by_email($1)")
        .bind(&email).fetch_optional(&st.pool).await.ok().flatten();
    if let Some((id, hash, _)) = existing {
        if !password_ok(&f.password, &hash) {
            return bad(vec![l.t("Já existe uma conta com este email e a palavra-passe não corresponde. Use a palavra-passe dessa conta, ou outro email.",
                "There's already an account with this email and the password doesn't match. Use that account's password, or another email.").into()]);
        }
        return match sqlx::query("SELECT portal.account_link($1, $2)").bind(f.code.trim()).bind(id).execute(&st.pool).await {
            Ok(_) => done(&st, &d, l, l.t("Acrescentado à sua conta", "Added to your account"), l.t("Entre com o seu email e a palavra-passe de sempre.", "Sign in with your email and usual password.")),
            Err(e) => bad(vec![code_msg(&db_code(&e), l)]),
        };
    }
    if f.password.chars().count() < 10 || f.password.chars().count() > 200 { return bad(vec![l.t("Escolha uma palavra-passe com pelo menos 10 caracteres.", "Choose a password with at least 10 characters.").into()]); }
    if f.password != f.password2 { return bad(vec![l.t("As palavras-passe não coincidem.", "The passwords don't match.").into()]); }
    let Some(hash) = hash_password(&f.password) else { return unavailable(l) };
    match sqlx::query("SELECT portal.account_create($1, $2, $3, $4)").bind(f.code.trim()).bind(&email).bind(hash).bind(l.code()).execute(&st.pool).await {
        Ok(_) => done(&st, &d, l, l.t("Conta pronta", "Your account is ready"), l.t("Já pode entrar com o seu email e a palavra-passe que escolheu.", "You can now sign in with your email and the password you chose.")),
        Err(e) => bad(vec![code_msg(&db_code(&e), l)]),
    }
}

fn code_msg(code: &str, l: Lang) -> String {
    match code {
        "bad_code" => l.t("Esse código não é válido ou já foi usado. Os códigos valem 14 dias (os de nova palavra-passe, 24 horas). Peça outro à clínica.",
                          "That code isn't valid or was already used. Codes last 14 days (password codes, 24 hours). Ask the clinic for another."),
        "email_taken" => l.t("Já existe uma conta com este email.", "There's already an account with this email."),
        "bad_email" => l.t("Verifique o endereço de email.", "Check the email address."),
        "closed" => l.t("A área de utente está fechada de momento.", "The patient area is closed right now."),
        _ => l.t("Não foi possível continuar. Tente de novo.", "We couldn't continue. Please try again."),
    }.to_string()
}

fn done(st: &St, d: &Data, l: Lang, title: &str, text: &str) -> Response {
    let body = format!(r#"{}<div class="card done" role="status"><p>{}</p><p><a class="btn pri" href="{}">{}</a></p></div>"#,
        head(title, ""), text, sub(Sub::Home, l), l.t("Entrar", "Sign In"));
    page(st, d, l, StatusCode::OK, title, &body)
}

#[derive(Deserialize, Default, Clone)]
pub struct ResetForm { #[serde(default)] code: String, #[serde(default)] password: String, #[serde(default)] password2: String }

fn reset_form(l: Lang, code: &str, errs: &[String]) -> String {
    format!(r#"{head}{errs}<form class="card form narrow" method="post" action="{action}" novalidate>
<p class="lead">{lead}</p>
<div class="f"><label for="f-code">{codel}</label><input id="f-code" name="code" autocomplete="off" maxlength="20" value="{code}" required></div>
<div class="f"><label for="f-password">{pwl}</label><input id="f-password" name="password" type="password" autocomplete="new-password" maxlength="200" required><p class="hint">{pwhint}</p></div>
<div class="f"><label for="f-password2">{pw2l}</label><input id="f-password2" name="password2" type="password" autocomplete="new-password" maxlength="200" required></div>
<p class="ctas"><button class="btn pri lg" type="submit">{go}</button></p></form>"#,
        head = head(l.t("Nova palavra-passe", "New password"), ""), errs = errors(errs, l), action = sub(Sub::Reset, l),
        lead = l.t("Peça um código à receção, ao seu fisioterapeuta ou à clínica (por telefone ou ao balcão). O código vale 24 horas e desliga os códigos de dois passos.",
                   "Ask reception, your physiotherapist or the clinic for a code (by phone or at the desk). It lasts 24 hours and turns two-step codes off."),
        codel = l.t("Código", "Code"), code = esc(code), pwl = l.t("Nova palavra-passe", "New password"), pwhint = l.t("Pelo menos 10 caracteres.", "At least 10 characters."),
        pw2l = l.t("Repita a palavra-passe", "Repeat the password"), go = l.t("Mudar palavra-passe", "Change Password"))
}

pub async fn reset_page(st: St, l: Lang, q: CodeQuery) -> Response {
    let Some(d) = data(&st).await else { return unavailable(l) };
    if !open_now(&d) { return closed(&st, &d, l); }
    let code = if q.codigo.is_empty() { q.code } else { q.codigo };
    page(&st, &d, l, StatusCode::OK, l.t("Nova palavra-passe", "New password"), &reset_form(l, &code, &[]))
}

pub async fn reset(st: St, l: Lang, peer: SocketAddr, h: HeaderMap, f: ResetForm) -> Response {
    let Some(d) = data(&st).await else { return unavailable(l) };
    if !open_now(&d) { return closed(&st, &d, l); }
    let title = l.t("Nova palavra-passe", "New password");
    let bad = |m: String| page(&st, &d, l, StatusCode::UNPROCESSABLE_ENTITY, title, &reset_form(l, &f.code, &[m]));
    let w = who(&st, peer, &h);
    if !hit(&st, &format!("reset:{w}"), 10, 60).await { return bad(l.t("Demasiadas tentativas. Espere uma hora.", "Too many tries. Wait an hour.").into()); }
    if f.password.chars().count() < 10 || f.password.chars().count() > 200 { return bad(l.t("Escolha uma palavra-passe com pelo menos 10 caracteres.", "Choose a password with at least 10 characters.").into()); }
    if f.password != f.password2 { return bad(l.t("As palavras-passe não coincidem.", "The passwords don't match.").into()); }
    let Some(hash) = hash_password(&f.password) else { return unavailable(l) };
    match sqlx::query("SELECT portal.account_reset($1, $2)").bind(f.code.trim()).bind(hash).execute(&st.pool).await {
        Ok(_) => {
            let mut r = done(&st, &d, l, l.t("Palavra-passe mudada", "Password changed"), l.t("Entre com a nova palavra-passe.", "Sign in with your new password."));
            set_cookie(&mut r, None, st.secure_cookie);
            r
        }
        Err(e) => bad(code_msg(&db_code(&e), l)),
    }
}

// ───────── signed-in pages ─────────

/// The signed-in session (fully, past the two-step code), or a response that sends the person to sign in.
async fn need(st: &St, l: Lang, h: &HeaderMap) -> Result<(Data, Session), Response> {
    let Some(d) = data(st).await else { return Err(unavailable(l)) };
    if !open_now(&d) { return Err(closed(st, &d, l)); }
    match session(st, h).await {
        Some(s) if !s.pending => Ok((d, s)),
        _ => Err(Redirect::to(sub(Sub::Home, l)).into_response()),
    }
}

#[derive(Deserialize, Default)]
pub struct RequestQuery { #[serde(default)] mudar: String, #[serde(default)] r#move: String }

#[derive(Deserialize, Default, Clone)]
pub struct RequestForm {
    #[serde(default)] c: String,
    #[serde(default)] patient: String,
    #[serde(default)] appointment: String,
    #[serde(default)] service: String,
    #[serde(default)] day: String,
    #[serde(default)] part: String,
    #[serde(default)] message: String,
}

fn request_form(me: &Value, l: Lang, c: &str, f: &RequestForm, errs: &[String]) -> String {
    let empty = vec![];
    let people = me["people"].as_array().unwrap_or(&empty);
    let moving = me["appointments"].as_array().unwrap_or(&empty).iter().find(|a| a["id"].as_str() == Some(f.appointment.as_str()) && a["can_move"] == true);
    let now = OffsetDateTime::now_utc();
    let today = now.to_offset(crate::lisbon_offset(now)).date();
    let (min, max) = (today + time::Duration::days(1), today + time::Duration::days(62));
    let mut o = head(if moving.is_some() { l.t("Pedir outra hora", "Ask for another time") } else { l.t("Pedir marcação", "Request an appointment") }, "");
    o.push_str(&errors(errs, l));
    let _ = write!(o, r#"<form class="card form narrow" method="post" action="{}" novalidate><input type="hidden" name="c" value="{c}">"#, sub(Sub::Request, l));
    if let Some(a) = moving {
        let _ = write!(o, r#"<p class="lead">{} <b>{}</b>. {}</p><input type="hidden" name="appointment" value="{}"><input type="hidden" name="patient" value="{}">"#,
            l.t("Sessão marcada para", "Session booked for"), when(a["starts_at"].as_str().unwrap_or(""), l),
            l.t("Diga-nos quando lhe dá mais jeito. A sessão só muda quando a clínica confirmar.", "Tell us when suits you better. The session only changes once the clinic confirms."),
            esc(a["id"].as_str().unwrap_or("")), esc(a["patient_id"].as_str().unwrap_or("")));
    } else {
        let _ = write!(o, r#"<p class="lead">{}</p>"#, l.t("Diga-nos o que precisa e quando lhe dá jeito. A marcação só fica feita quando a clínica a confirmar.", "Tell us what you need and when suits you. It's only booked once the clinic confirms."));
        if people.len() > 1 {
            let _ = write!(o, r#"<fieldset><legend>{}</legend><div class="chips">"#, l.t("Para quem", "For whom"));
            for (i, p) in people.iter().enumerate() {
                let id = p["id"].as_str().unwrap_or("");
                let chk = if f.patient == id || (f.patient.is_empty() && i == 0) { " checked" } else { "" };
                let _ = write!(o, r#"<label class="chip"><input type="radio" name="patient" value="{}"{chk}><span>{}</span></label>"#, esc(id), esc(p["name"].as_str().unwrap_or("")));
            }
            o.push_str("</div></fieldset>");
        } else if let Some(p) = people.first() {
            let _ = write!(o, r#"<input type="hidden" name="patient" value="{}">"#, esc(p["id"].as_str().unwrap_or("")));
        }
        let _ = write!(o, r#"<fieldset><legend>{}</legend><div class="chips">"#, l.t("O que pretende", "What you'd like"));
        for (v, t) in [("session", l.t("Sessão", "Session")), ("assessment", l.t("Avaliação", "Assessment")), ("home", l.t("Ao domicílio", "Home visit"))] {
            let chk = if f.service == v || (f.service.is_empty() && v == "session") { " checked" } else { "" };
            let _ = write!(o, r#"<label class="chip"><input type="radio" name="service" value="{v}"{chk}><span>{t}</span></label>"#);
        }
        o.push_str("</div></fieldset>");
    }
    let mut parts = String::new();
    for (v, t) in [("morning", l.t("Manhã", "Morning")), ("afternoon", l.t("Tarde", "Afternoon")), ("any", l.t("Tanto faz", "Either"))] {
        let chk = if f.part == v || (f.part.is_empty() && v == "any") { " checked" } else { "" };
        let _ = write!(parts, r#"<label class="chip"><input type="radio" name="part" value="{v}"{chk}><span>{t}</span></label>"#);
    }
    let _ = write!(o, r#"<div class="f"><label for="f-day">{dayl}</label><input id="f-day" name="day" type="date" min="{min}" max="{max}" value="{day}" required></div>
<fieldset><legend>{partl}</legend><div class="chips">{parts}</div></fieldset>
<div class="f"><label for="f-message">{msgl}</label><textarea id="f-message" name="message" rows="3" maxlength="1000">{msg}</textarea><p class="hint">{hint}</p></div>
<p class="ctas"><button class="btn pri lg" type="submit">{go}</button> <a class="btn ghost" href="{back}">{backl}</a></p></form>"#,
        dayl = l.t("Dia que prefere", "Preferred day"), day = esc(&f.day), partl = l.t("Período do dia", "Time of day"),
        msgl = l.t("Mensagem (opcional)", "Message (optional)"), msg = esc(&f.message),
        hint = l.t("Não precisa de descrever problemas de saúde aqui.", "There's no need to describe health problems here."),
        go = l.t("Enviar pedido", "Send Request"), back = sub(Sub::Home, l), backl = l.t("Voltar", "Back"));
    o
}

pub async fn request_page(st: St, l: Lang, h: HeaderMap, q: RequestQuery) -> Response {
    let (d, s) = match need(&st, l, &h).await { Ok(x) => x, Err(r) => return r };
    let Ok(r) = api(&st, "GET", "/me", &s.token, l, None).await else { return unavailable(l) };
    let Ok(me) = r.json::<Value>().await else { return unavailable(l) };
    let f = RequestForm { appointment: if q.mudar.is_empty() { q.r#move } else { q.mudar }, ..Default::default() };
    page(&st, &d, l, StatusCode::OK, l.t("Pedir marcação", "Request an appointment"), &request_form(&me, l, &csrf(&st.key[..], &s.token), &f, &[]))
}

pub async fn request(st: St, l: Lang, h: HeaderMap, f: RequestForm) -> Response {
    let (d, s) = match need(&st, l, &h).await { Ok(x) => x, Err(r) => return r };
    let c = csrf(&st.key[..], &s.token);
    if f.c != c { return Redirect::to(sub(Sub::Home, l)).into_response(); }
    let kind = if f.appointment.is_empty() { "book" } else { "move" };
    let body = serde_json::json!({ "patient_id": f.patient, "kind": kind, "appointment_id": if f.appointment.is_empty() { Value::Null } else { Value::String(f.appointment.clone()) },
        "service": f.service, "day": f.day.trim(), "part": if f.part.is_empty() { "any" } else { &f.part }, "message": f.message.trim() });
    let Ok(r) = api(&st, "POST", "/request", &s.token, l, Some(body)).await else { return unavailable(l) };
    if r.status().is_success() {
        let body = format!(r#"{}<div class="card done" role="status"><p>{}</p><p><a class="btn pri" href="{}">{}</a></p></div>"#,
            head(l.t("Pedido enviado", "Request sent"), ""),
            l.t("A clínica vai ver a agenda e responde-lhe por telefone ou email.", "The clinic will check the schedule and answer by phone or email."),
            sub(Sub::Home, l), l.t("Voltar à área de utente", "Back to Your Area"));
        return page(&st, &d, l, StatusCode::OK, l.t("Pedido enviado", "Request sent"), &body);
    }
    let status = r.status();
    let mut msg = api_error(r).await;
    if status == StatusCode::TOO_MANY_REQUESTS || msg.is_empty() {
        msg = l.t("Já enviou vários pedidos hoje. Tente amanhã ou ligue para a clínica.", "You've sent several requests today. Try tomorrow or call the clinic.").into();
    }
    let Ok(r2) = api(&st, "GET", "/me", &s.token, l, None).await else { return unavailable(l) };
    let Ok(me) = r2.json::<Value>().await else { return unavailable(l) };
    page(&st, &d, l, StatusCode::UNPROCESSABLE_ENTITY, l.t("Pedir marcação", "Request an appointment"), &request_form(&me, l, &c, &f, &[msg]))
}

#[derive(Deserialize, Default)]
pub struct IdQuery { #[serde(default)] id: String }
#[derive(Deserialize, Default)]
pub struct CancelForm { #[serde(default)] c: String, #[serde(default)] id: String }

/// Cancelling asks once more (no scripts, so a page of its own).
pub async fn cancel_page(st: St, l: Lang, h: HeaderMap, q: IdQuery) -> Response {
    let (d, s) = match need(&st, l, &h).await { Ok(x) => x, Err(r) => return r };
    let Ok(r) = api(&st, "GET", "/me", &s.token, l, None).await else { return unavailable(l) };
    let Ok(me) = r.json::<Value>().await else { return unavailable(l) };
    let Some(a) = me["appointments"].as_array().and_then(|v| v.iter().find(|a| a["id"].as_str() == Some(q.id.as_str()) && a["can_cancel"] == true)).cloned() else {
        return Redirect::to(sub(Sub::Home, l)).into_response();
    };
    let body = format!(r#"{}<form class="card form narrow" method="post" action="{}"><input type="hidden" name="c" value="{}"><input type="hidden" name="id" value="{}">
<p class="lead">{} <b>{}</b>?</p><p>{}</p><p class="ctas"><button class="btn pri lg danger" type="submit">{}</button> <a class="btn ghost" href="{}">{}</a></p></form>"#,
        head(l.t("Cancelar sessão", "Cancel session"), ""), sub(Sub::Cancel, l), csrf(&st.key[..], &s.token), esc(&q.id),
        l.t("Cancelar a sessão de", "Cancel the session on"), when(a["starts_at"].as_str().unwrap_or(""), l),
        l.t("A clínica é avisada. Para marcar outra, faça um pedido depois.", "The clinic will be told. To book another one, send a request afterwards."),
        l.t("Cancelar Sessão", "Cancel Session"), sub(Sub::Home, l), l.t("Manter", "Keep It"));
    page(&st, &d, l, StatusCode::OK, l.t("Cancelar sessão", "Cancel session"), &body)
}

pub async fn cancel(st: St, l: Lang, h: HeaderMap, f: CancelForm) -> Response {
    let (d, s) = match need(&st, l, &h).await { Ok(x) => x, Err(r) => return r };
    if f.c != csrf(&st.key[..], &s.token) { return Redirect::to(sub(Sub::Home, l)).into_response(); }
    let Ok(r) = api(&st, "POST", "/cancel", &s.token, l, Some(serde_json::json!({ "appointment_id": f.id }))).await else { return unavailable(l) };
    let (title, text) = if r.status().is_success() {
        (l.t("Sessão cancelada", "Session cancelled"), l.t("A clínica já sabe.", "The clinic has been told.").to_string())
    } else {
        let m = api_error(r).await;
        (l.t("Não foi possível cancelar", "Couldn't cancel"), if m.is_empty() { l.t("Ligue para a clínica.", "Call the clinic.").into() } else { m })
    };
    let body = format!(r#"{}<div class="card done" role="status"><p>{}</p><p><a class="btn pri" href="{}">{}</a></p></div>"#,
        head(title, ""), esc(&text), sub(Sub::Home, l), l.t("Voltar à área de utente", "Back to Your Area"));
    page(&st, &d, l, StatusCode::OK, title, &body)
}

pub async fn invoice(st: St, l: Lang, h: HeaderMap, id: String) -> Response {
    let (_, s) = match need(&st, l, &h).await { Ok(x) => x, Err(r) => return r };
    if sqlx::types::Uuid::parse_str(&id).is_err() { return StatusCode::NOT_FOUND.into_response(); }
    let Ok(r) = api(&st, "GET", &format!("/invoice/{id}"), &s.token, l, None).await else { return unavailable(l) };
    if !r.status().is_success() { return StatusCode::NOT_FOUND.into_response(); }
    let ct = r.headers().get(header::CONTENT_TYPE).cloned().unwrap_or(HeaderValue::from_static("application/pdf"));
    let cd = r.headers().get(header::CONTENT_DISPOSITION).cloned().unwrap_or(HeaderValue::from_static("attachment"));
    let Ok(bytes) = r.bytes().await else { return unavailable(l) };
    let mut out = Response::new(Body::from(bytes));
    out.headers_mut().insert(header::CONTENT_TYPE, ct);
    out.headers_mut().insert(header::CONTENT_DISPOSITION, cd);
    out.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    out
}

// ───────── password and two-step ─────────

#[derive(Deserialize, Default, Clone)]
pub struct SecurityForm {
    #[serde(default)] c: String,
    /// "password", "totp_start", "totp_on", "totp_off"
    #[serde(default)] action: String,
    #[serde(default)] current: String,
    #[serde(default)] password: String,
    #[serde(default)] password2: String,
    #[serde(default)] code: String,
    /// The new two-step secret while it's being set up, sealed (so it can't be changed in the page).
    #[serde(default)] pending: String,
}

fn security_page(st: &St, s: &Session, l: Lang, errs: &[String], info: &str, setup: Option<(String, String, String)>) -> String {
    let c = csrf(&st.key[..], &s.token);
    let mut o = head(l.t("Palavra-passe e segurança", "Password and security"), &esc(&s.email));
    o.push_str(&errors(errs, l));
    if !info.is_empty() { let _ = write!(o, r#"<div class="card done" role="status"><p>{}</p></div>"#, esc(info)); }
    let _ = write!(o, r#"<form class="card form narrow" method="post" action="{a}" novalidate><h2>{t}</h2><input type="hidden" name="c" value="{c}"><input type="hidden" name="action" value="password">
<div class="f"><label for="f-cur">{cur}</label><input id="f-cur" name="current" type="password" autocomplete="current-password" maxlength="200" required></div>
<div class="f"><label for="f-new">{new}</label><input id="f-new" name="password" type="password" autocomplete="new-password" maxlength="200" required><p class="hint">{hint}</p></div>
<div class="f"><label for="f-new2">{new2}</label><input id="f-new2" name="password2" type="password" autocomplete="new-password" maxlength="200" required></div>
<p class="ctas"><button class="btn" type="submit">{go}</button></p></form>"#,
        a = sub(Sub::Security, l), t = l.t("Mudar a palavra-passe", "Change your password"), cur = l.t("Palavra-passe atual", "Current password"),
        new = l.t("Nova palavra-passe", "New password"), hint = l.t("Pelo menos 10 caracteres.", "At least 10 characters."), new2 = l.t("Repita a nova palavra-passe", "Repeat the new password"),
        go = l.t("Mudar Palavra-passe", "Change Password"));
    let _ = write!(o, r#"<section class="card"><h2>{}</h2>"#, l.t("Códigos de dois passos", "Two-step codes"));
    if let Some((svg, secret, sealed)) = setup {
        let _ = write!(o, r#"<p>{scan}</p><div class="qr">{svg}</div><p class="small">{manual} <code>{secret}</code></p>
<form class="form" method="post" action="{a}" novalidate><input type="hidden" name="c" value="{c}"><input type="hidden" name="action" value="totp_on"><input type="hidden" name="pending" value="{sealed}">
<div class="f"><label for="f-code">{codel}</label><input id="f-code" name="code" inputmode="numeric" autocomplete="one-time-code" maxlength="8" required></div>
<div class="f"><label for="f-cur2">{cur}</label><input id="f-cur2" name="current" type="password" autocomplete="current-password" maxlength="200" required></div>
<p class="ctas"><button class="btn pri" type="submit">{go}</button></p></form>"#,
            scan = l.t("Leia este código com uma app de autenticação (Palavras-passe da Apple, Google Authenticator, 1Password) e escreva os 6 algarismos que ela mostra.",
                       "Scan this with an authenticator app (Apple Passwords, Google Authenticator, 1Password) and enter the 6 digits it shows."),
            manual = l.t("Sem câmara? Escreva esta chave na app:", "No camera? Type this key into the app:"), secret = esc(&secret), sealed = esc(&sealed),
            a = sub(Sub::Security, l), codel = l.t("Código", "Code"), cur = l.t("Palavra-passe atual", "Current password"), go = l.t("Ativar", "Turn On"));
    } else if s.totp.is_some() {
        let _ = write!(o, r#"<p>{on}</p><form class="form" method="post" action="{a}" novalidate><input type="hidden" name="c" value="{c}"><input type="hidden" name="action" value="totp_off">
<div class="f"><label for="f-cur3">{cur}</label><input id="f-cur3" name="current" type="password" autocomplete="current-password" maxlength="200" required></div>
<p class="ctas"><button class="btn" type="submit">{go}</button></p></form>"#,
            on = l.t("Ligados: além da palavra-passe, a entrada pede o código da sua app.", "On: besides your password, signing in asks for the code from your app."),
            a = sub(Sub::Security, l), cur = l.t("Palavra-passe atual", "Current password"), go = l.t("Desligar", "Turn Off"));
    } else {
        let _ = write!(o, r#"<p>{off}</p><form method="post" action="{a}"><input type="hidden" name="c" value="{c}"><input type="hidden" name="action" value="totp_start"><p class="ctas"><button class="btn" type="submit">{go}</button></p></form>"#,
            off = l.t("Opcional. Com eles, entrar pede também um código da app de autenticação do seu telemóvel.", "Optional. With them on, signing in also asks for a code from an authenticator app on your phone."),
            a = sub(Sub::Security, l), go = l.t("Ativar…", "Turn On…"));
    }
    let _ = write!(o, r#"</section><p><a class="btn ghost" href="{}">{}</a></p>"#, sub(Sub::Home, l), l.t("Voltar à área de utente", "Back to Your Area"));
    o
}

pub async fn security(st: St, l: Lang, h: HeaderMap) -> Response {
    let (d, s) = match need(&st, l, &h).await { Ok(x) => x, Err(r) => return r };
    page(&st, &d, l, StatusCode::OK, l.t("Palavra-passe e segurança", "Password and security"), &security_page(&st, &s, l, &[], "", None))
}

pub async fn security_post(st: St, l: Lang, peer: SocketAddr, h: HeaderMap, f: SecurityForm) -> Response {
    let (d, s) = match need(&st, l, &h).await { Ok(x) => x, Err(r) => return r };
    if f.c != csrf(&st.key[..], &s.token) { return Redirect::to(sub(Sub::Home, l)).into_response(); }
    let title = l.t("Palavra-passe e segurança", "Password and security");
    let show = |errs: Vec<String>, info: &str, setup| page(&st, &d, l, if errs.is_empty() { StatusCode::OK } else { StatusCode::UNPROCESSABLE_ENTITY }, title, &security_page(&st, &s, l, &errs, info, setup));
    if !hit(&st, &format!("sec:{}", who(&st, peer, &h)), 15, 15).await {
        return show(vec![l.t("Demasiadas tentativas. Espere 15 minutos.", "Too many tries. Wait 15 minutes.").into()], "", None);
    }
    // Everything but starting the set-up needs the current password.
    let current_ok = || async {
        let row: Option<(sqlx::types::Uuid, String, Option<String>)> = sqlx::query_as("SELECT id, password_hash, totp_secret FROM portal.account_by_email($1)")
            .bind(&s.email).fetch_optional(&st.pool).await.ok().flatten();
        row.is_some_and(|(_, hash, _)| password_ok(&f.current, &hash))
    };
    let wrong = l.t("A palavra-passe atual não está certa.", "Your current password isn't right.").to_string();
    let th = sha_hex(&s.token);
    match f.action.as_str() {
        "password" => {
            if !current_ok().await { return show(vec![wrong], "", None); }
            if f.password.chars().count() < 10 || f.password.chars().count() > 200 { return show(vec![l.t("Escolha uma palavra-passe com pelo menos 10 caracteres.", "Choose a password with at least 10 characters.").into()], "", None); }
            if f.password != f.password2 { return show(vec![l.t("As palavras-passe não coincidem.", "The passwords don't match.").into()], "", None); }
            let Some(hash) = hash_password(&f.password) else { return unavailable(l) };
            if sqlx::query("SELECT portal.account_update($1, $2, NULL, FALSE, NULL)").bind(&th).bind(hash).execute(&st.pool).await.is_err() { return unavailable(l); }
            show(vec![], l.t("Palavra-passe mudada. Noutros dispositivos terá de entrar de novo.", "Password changed. You'll need to sign in again on other devices."), None)
        }
        "totp_start" => {
            let secret = Secret::generate_secret().to_bytes().unwrap_or_default();
            let Some(t) = totp(secret.clone(), &s.email) else { return unavailable(l) };
            let svg = qrcode::QrCode::new(t.get_url().as_bytes()).map(|q| q.render::<qrcode::render::svg::Color>().min_dimensions(200, 200).build()).unwrap_or_default();
            // The QR code is drawn inline; strip the XML header so it sits inside the page.
            let svg = svg.find("<svg").map(|i| svg[i..].to_string()).unwrap_or_default();
            show(vec![], "", Some((svg, t.get_secret_base32(), seal(&st.key[..], &secret))))
        }
        "totp_on" => {
            let Some(secret) = open(&st.key[..], &f.pending) else { return show(vec![l.t("Comece de novo.", "Start again.").into()], "", None) };
            let Some(t) = totp(secret.clone(), &s.email) else { return unavailable(l) };
            if !current_ok().await { return show(vec![wrong], "", None); }
            if !code_ok(&t, &f.code) { return show(vec![l.t("Esse código não está certo. Comece de novo e use o que a app mostra agora.", "That code isn't right. Start again and use the one the app shows now.").into()], "", None); }
            if sqlx::query("SELECT portal.account_update($1, NULL, $2, TRUE, NULL)").bind(&th).bind(seal(&st.key[..], &secret)).execute(&st.pool).await.is_err() { return unavailable(l); }
            let s2 = Session { totp: Some(String::new()), token: s.token.clone(), email: s.email.clone(), pending: false, lang: s.lang.clone() };
            page(&st, &d, l, StatusCode::OK, title, &security_page(&st, &s2, l, &[], l.t("Códigos de dois passos ligados.", "Two-step codes are on."), None))
        }
        "totp_off" => {
            if !current_ok().await { return show(vec![wrong], "", None); }
            if sqlx::query("SELECT portal.account_update($1, NULL, NULL, TRUE, NULL)").bind(&th).execute(&st.pool).await.is_err() { return unavailable(l); }
            let s2 = Session { totp: None, token: s.token.clone(), email: s.email.clone(), pending: false, lang: s.lang.clone() };
            page(&st, &d, l, StatusCode::OK, title, &security_page(&st, &s2, l, &[], l.t("Códigos de dois passos desligados.", "Two-step codes are off."), None))
        }
        _ => Redirect::to(sub(Sub::Security, l)).into_response(),
    }
}

// ───────── routes ─────────

pub fn routes(mut r: axum::Router<St>) -> axum::Router<St> {
    use axum::routing::{get, post};
    for l in [Lang::Pt, Lang::En] {
        r = r
            .route(sub(Sub::Home, l), get(move |State(st): State<St>, h: HeaderMap| async move { home(st, l, h).await }))
            .route(sub(Sub::SignIn, l), post(move |State(st): State<St>, ConnectInfo(p): ConnectInfo<SocketAddr>, h: HeaderMap, Form(f): Form<SignInForm>| async move { sign_in(st, l, p, h, f).await })
                .get(move || async move { Redirect::to(sub(Sub::Home, l)) }))
            .route(sub(Sub::Code, l), post(move |State(st): State<St>, ConnectInfo(p): ConnectInfo<SocketAddr>, h: HeaderMap, Form(f): Form<CodeForm>| async move { two_step(st, l, p, h, f).await })
                .get(move || async move { Redirect::to(sub(Sub::Home, l)) }))
            .route(sub(Sub::SignOut, l), post(move |State(st): State<St>, h: HeaderMap, Form(f): Form<TokenForm>| async move { sign_out(st, l, h, f).await }))
            .route(sub(Sub::Invite, l), get(move |State(st): State<St>, Query(q): Query<CodeQuery>| async move { invite_page(st, l, q).await })
                .post(move |State(st): State<St>, ConnectInfo(p): ConnectInfo<SocketAddr>, h: HeaderMap, Form(f): Form<InviteForm>| async move { invite(st, l, p, h, f).await }))
            .route(sub(Sub::Reset, l), get(move |State(st): State<St>, Query(q): Query<CodeQuery>| async move { reset_page(st, l, q).await })
                .post(move |State(st): State<St>, ConnectInfo(p): ConnectInfo<SocketAddr>, h: HeaderMap, Form(f): Form<ResetForm>| async move { reset(st, l, p, h, f).await }))
            .route(sub(Sub::Request, l), get(move |State(st): State<St>, h: HeaderMap, Query(q): Query<RequestQuery>| async move { request_page(st, l, h, q).await })
                .post(move |State(st): State<St>, h: HeaderMap, Form(f): Form<RequestForm>| async move { request(st, l, h, f).await }))
            .route(sub(Sub::Cancel, l), get(move |State(st): State<St>, h: HeaderMap, Query(q): Query<IdQuery>| async move { cancel_page(st, l, h, q).await })
                .post(move |State(st): State<St>, h: HeaderMap, Form(f): Form<CancelForm>| async move { cancel(st, l, h, f).await }))
            .route(sub(Sub::Security, l), get(move |State(st): State<St>, h: HeaderMap| async move { security(st, l, h).await })
                .post(move |State(st): State<St>, ConnectInfo(p): ConnectInfo<SocketAddr>, h: HeaderMap, Form(f): Form<SecurityForm>| async move { security_post(st, l, p, h, f).await }))
            .route(invoice_path(l), get(move |State(st): State<St>, h: HeaderMap, Path(id): Path<String>| async move { invoice(st, l, h, id).await }));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealing() {
        let k = [3u8; 32];
        let s = seal(&k, b"secret bytes");
        assert_eq!(open(&k, &s).unwrap(), b"secret bytes");
        assert!(open(&[4u8; 32], &s).is_none(), "another key can't open it");
        let mut bad = s.clone(); bad.pop(); bad.push(if s.ends_with('0') { '1' } else { '0' });
        assert!(open(&k, &bad).is_none(), "a changed secret is refused");
    }

    #[test]
    fn passwords() {
        let h = hash_password("a long enough password").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(password_ok("a long enough password", &h));
        assert!(!password_ok("another password", &h));
    }

    #[test]
    fn cookies() {
        let mut h = HeaderMap::new();
        let t = "a".repeat(64);
        h.insert(header::COOKIE, HeaderValue::from_str(&format!("x=1; {COOKIE}={t}")).unwrap());
        assert_eq!(cookie_of(&h).as_deref(), Some(t.as_str()));
        h.insert(header::COOKIE, HeaderValue::from_static("pp_conta=short"));
        assert!(cookie_of(&h).is_none());
    }

    #[test]
    fn form_token_is_per_session() {
        let k = [1u8; 32];
        assert_ne!(csrf(&k, "a"), csrf(&k, "b"));
        assert_eq!(csrf(&k, "a"), csrf(&k, "a"));
    }

    #[test]
    fn every_sub_page_has_both_addresses() {
        for (p, pt, en) in SUBS {
            assert!(pt.starts_with("/conta") && en.starts_with("/en/account"));
            assert_eq!(sub(p, Lang::Pt), pt);
        }
    }
}
