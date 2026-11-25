//! PhysioPulse public website (PhysioPulse plan, phase 4a).
//!
//! Plain HTML pages made on the server from the clinic's database — no JavaScript, no cookies, nothing from
//! other companies — so prices and texts change the moment the owner edits them in the app, and the pages
//! work on any phone. Portuguese at `/…`, English at `/en/…`.
//!
//! It signs in to PostgreSQL as `physiopulse_portal`, a login that can only read the `portal` views (clinic
//! details, website texts, the price list, the team page) and write through `portal.request_booking()` and
//! `portal.hit()` (migration 0018). Patient files, staff accounts, money and the access log are out of its reach.
//! Patient accounts (`/conta`, phase 4b) are in `account.rs`.

mod account;
mod html;

use axum::{
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Form, Router,
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{env, net::{IpAddr, SocketAddr}, sync::Arc, time::Duration};
use time::{Date, OffsetDateTime};
use tower_http::{limit::RequestBodyLimitLayer, set_header::SetResponseHeaderLayer};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang { Pt, En }
impl Lang {
    pub fn code(self) -> &'static str { if self == Lang::Pt { "pt" } else { "en" } }
    /// Picks the Portuguese or English text.
    pub fn t<'a>(self, pt: &'a str, en: &'a str) -> &'a str { if self == Lang::Pt { pt } else { en } }
}

/// Every page, with its Portuguese and English address.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page { Home, Services, Team, Book, Privacy, Terms, Account }
pub const PAGES: [(Page, &str, &str); 7] = [
    (Page::Home, "/", "/en"),
    (Page::Services, "/servicos", "/en/services"),
    (Page::Team, "/equipa", "/en/team"),
    (Page::Book, "/marcar", "/en/book"),
    (Page::Privacy, "/privacidade", "/en/privacy"),
    (Page::Terms, "/termos", "/en/terms"),
    (Page::Account, "/conta", "/en/account"),
];
pub fn href(p: Page, l: Lang) -> &'static str {
    let (_, pt, en) = PAGES.iter().find(|(x, _, _)| *x == p).unwrap();
    if l == Lang::Pt { pt } else { en }
}

// ───────── what the database gives the site ─────────

#[derive(sqlx::FromRow, Clone, Debug)]
pub struct Site {
    pub published: bool, pub requests_open: bool, pub home_visits: bool, pub opening_soon: bool, pub show_prices: bool,
    pub tagline_pt: String, pub tagline_en: String, pub intro_pt: String, pub intro_en: String,
    pub home_text_pt: String, pub home_text_en: String,
    pub clinic_name: String, pub nif: String, pub ers_number: String, pub address: String, pub phone: String, pub email: String,
    pub opening_hours: serde_json::Value,
    /// Phase 4b: the patient area (/conta) is open.
    pub accounts_open: bool,
}
impl Site {
    pub fn name(&self) -> &str { if self.clinic_name.trim().is_empty() { "PhysioPulse" } else { self.clinic_name.trim() } }
}

#[derive(sqlx::FromRow, Clone, Debug)]
pub struct Service {
    pub code: String, pub kind: String, pub visit_kind: Option<String>, pub applies_to: String,
    pub name_pt: String, pub name_en: String, pub price_cents: i32, pub sessions: Option<i32>, pub valid_months: Option<i32>,
    pub vat_exempt: bool,
}

#[derive(sqlx::FromRow, Clone, Debug)]
pub struct Person { pub name: String, pub is_owner: bool, pub title_pt: String, pub title_en: String, pub bio_pt: String, pub bio_en: String }

#[derive(sqlx::FromRow, Clone, Debug)]
pub struct Closure { pub starts_on: Date, pub ends_on: Date, pub label: String }

pub struct Data { pub site: Site, pub services: Vec<Service>, pub team: Vec<Person>, pub closures: Vec<Closure> }

pub(crate) async fn load(pool: &PgPool, page: Page) -> Result<Data, sqlx::Error> {
    let site: Site = sqlx::query_as("SELECT published, requests_open, home_visits, opening_soon, show_prices, tagline_pt, tagline_en, intro_pt, intro_en,
            home_text_pt, home_text_en, clinic_name, nif, ers_number, address, phone, email, opening_hours, accounts_open FROM portal.site")
        .fetch_one(pool).await?;
    let services = if matches!(page, Page::Home | Page::Services | Page::Book) {
        sqlx::query_as("SELECT code, kind, visit_kind, applies_to, name_pt, name_en, price_cents, sessions, valid_months, vat_exempt FROM portal.services ORDER BY sort, name_pt")
            .fetch_all(pool).await?
    } else { vec![] };
    let team = if page == Page::Team {
        sqlx::query_as("SELECT name, is_owner, title_pt, title_en, bio_pt, bio_en FROM portal.team ORDER BY NOT is_owner, name").fetch_all(pool).await?
    } else { vec![] };
    let closures = if matches!(page, Page::Home | Page::Book) {
        sqlx::query_as("SELECT starts_on, ends_on, label FROM portal.closures ORDER BY starts_on LIMIT 5").fetch_all(pool).await?
    } else { vec![] };
    Ok(Data { site, services, team, closures })
}

// ───────── state, form token, visitor address ─────────

#[derive(Clone)]
struct St {
    pool: PgPool, key: Arc<[u8; 32]>, proxies: Arc<Vec<(IpAddr, u8)>>,
    /// Phase 4b: the staff server's internal API for signed-in patients (routes/portal.rs there).
    http: reqwest::Client, server_url: Arc<str>, internal: Option<[u8; 32]>,
    /// The session cookie is `Secure` (HTTPS through Nginx Proxy Manager); COOKIE_INSECURE=1 only for local tests.
    secure_cookie: bool,
}

/// The form carries the time it was shown and a signature of it: a submission must come 3 s to 2 h after the
/// page was shown (robots post at once; old pages expire). No cookie is needed for this.
fn sign(key: &[u8], t: i64) -> String {
    let mut m = Hmac::<Sha256>::new_from_slice(key).expect("hmac key");
    m.update(format!("book:{t}").as_bytes());
    hex::encode(m.finalize().into_bytes())
}
fn token_ok(key: &[u8], t: &str, s: &str, now: i64) -> bool {
    let Ok(t) = t.parse::<i64>() else { return false };
    let Ok(sig) = hex::decode(s) else { return false };
    let mut m = Hmac::<Sha256>::new_from_slice(key).expect("hmac key");
    m.update(format!("book:{t}").as_bytes());
    m.verify_slice(&sig).is_ok() && (3..=7200).contains(&(now - t))
}

fn parse_cidrs(v: &str) -> Vec<(IpAddr, u8)> {
    v.split(',').filter_map(|c| {
        let c = c.trim();
        let (ip, bits) = c.split_once('/').unwrap_or((c, if c.contains(':') { "128" } else { "32" }));
        Some((ip.parse().ok()?, bits.parse().ok()?))
    }).collect()
}
fn in_cidr(ip: IpAddr, (net, bits): (IpAddr, u8)) -> bool {
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) => { let m = if bits == 0 { 0 } else { u32::MAX << (32 - bits.min(32)) }; (u32::from(a) & m) == (u32::from(n) & m) }
        (IpAddr::V6(a), IpAddr::V6(n)) => { let m = if bits == 0 { 0 } else { u128::MAX << (128 - bits.min(128)) }; (u128::from(a) & m) == (u128::from(n) & m) }
        _ => false,
    }
}
/// The visitor's address: the connection's, or the last one Nginx Proxy Manager added when the connection is NPM.
pub(crate) fn real_ip(proxies: &[(IpAddr, u8)], peer: IpAddr, forwarded: Option<&str>) -> String {
    let peer = match peer { IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(peer), p => p };
    if proxies.iter().any(|c| in_cidr(peer, *c)) {
        if let Some(last) = forwarded.and_then(|f| f.rsplit(',').next()).and_then(|x| x.trim().parse::<IpAddr>().ok()) {
            return last.to_string();
        }
    }
    peer.to_string()
}

// ───────── pages ─────────

fn html_response(status: StatusCode, body: String, index: bool) -> Response {
    let mut r = (status, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response();
    if !index {
        r.headers_mut().insert("x-robots-tag", HeaderValue::from_static("noindex"));
    }
    r
}

async fn show(st: &St, page: Page, lang: Lang, extra: html::Extra) -> Response {
    match load(&st.pool, page).await {
        Ok(d) => {
            let published = d.site.published;
            let status = if published { extra.status } else { StatusCode::OK };
            html_response(status, html::render(&d, page, lang, &extra), published)
        }
        Err(e) => {
            tracing::error!("database: {e}");
            html_response(StatusCode::SERVICE_UNAVAILABLE, html::unavailable(lang), false)
        }
    }
}

#[derive(Deserialize, Default)]
struct BookQuery { enviado: Option<String>, sent: Option<String>, servico: Option<String>, service: Option<String>,
    /// From the home page's request stub (phase 4b redesign): the day and part of the day already chosen.
    dia: Option<String>, day: Option<String>, periodo: Option<String>, part: Option<String> }

async fn get_page(st: St, page: Page, lang: Lang, q: BookQuery) -> Response {
    let mut extra = html::Extra::default();
    if page == Page::Book {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        extra.token = Some((now.to_string(), sign(&st.key[..], now)));
        extra.sent = q.enviado.is_some() || q.sent.is_some();
        extra.form.service = q.servico.or(q.service).filter(|s| ["assessment", "session", "home"].contains(&s.as_str())).unwrap_or_default();
        extra.form.day = q.dia.or(q.day).filter(|d| d.len() == 10 && Date::parse(d, &time::macros::format_description!("[year]-[month]-[day]")).is_ok()).unwrap_or_default();
        extra.form.part = q.periodo.or(q.part).filter(|p| ["morning", "afternoon", "any"].contains(&p.as_str())).unwrap_or_default();
    }
    show(&st, page, lang, extra).await
}

#[derive(Deserialize, Default, Clone, Debug)]
pub struct BookForm {
    #[serde(default)] pub service: String,
    #[serde(default)] pub name: String,
    #[serde(default)] pub phone: String,
    #[serde(default)] pub email: String,
    #[serde(default)] pub day: String,
    #[serde(default)] pub part: String,
    #[serde(default)] pub address: String,
    #[serde(default)] pub message: String,
    #[serde(default)] pub privacy: String,
    #[serde(default)] t: String,
    #[serde(default)] s: String,
    /// Hidden from people; robots fill it in.
    #[serde(default)] website: String,
}

/// Field-by-field checks, with the sentence shown next to the field.
fn check(f: &BookForm, lang: Lang, today: Date) -> Vec<(&'static str, String)> {
    let mut e: Vec<(&'static str, String)> = Vec::new();
    let l = |pt: &str, en: &str| lang.t(pt, en).to_string();
    if !["assessment", "session", "home"].contains(&f.service.as_str()) {
        e.push(("service", l("Escolha o que pretende marcar.", "Choose what you'd like to book.")));
    }
    let name = f.name.trim();
    if name.chars().count() < 2 || name.chars().count() > 120 {
        e.push(("name", l("Escreva o seu nome.", "Write your name.")));
    }
    let digits = f.phone.chars().filter(|c| c.is_ascii_digit()).count();
    let phone_bad = !f.phone.trim().is_empty() && (!(9..=15).contains(&digits) || f.phone.chars().any(|c| !(c.is_ascii_digit() || " +()-".contains(c))));
    if phone_bad {
        e.push(("phone", l("Verifique o número de telemóvel (9 algarismos, ou com o indicativo do país).", "Check the phone number (9 digits, or with the country code).")));
    }
    let email = f.email.trim();
    let email_bad = !email.is_empty() && (email.len() > 200 || email.contains(' ') || !email.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.') && !b.ends_with('.')));
    if email_bad {
        e.push(("email", l("Verifique o endereço de email.", "Check the email address.")));
    }
    if f.phone.trim().is_empty() && email.is_empty() {
        e.push(("phone", l("Deixe um telemóvel ou um email para lhe respondermos.", "Leave a phone number or an email so we can answer you.")));
    }
    match Date::parse(f.day.trim(), &time::macros::format_description!("[year]-[month]-[day]")) {
        Ok(d) if d > today && d <= today + time::Duration::days(62) => {}
        Ok(_) => e.push(("day", l("Escolha um dia a partir de amanhã e nos próximos dois meses.", "Choose a day from tomorrow and within the next two months."))),
        Err(_) => e.push(("day", l("Escolha o dia que prefere.", "Choose the day you'd prefer."))),
    }
    if !["morning", "afternoon", "any"].contains(&f.part.as_str()) {
        e.push(("part", l("Escolha manhã, tarde ou tanto faz.", "Choose morning, afternoon or either.")));
    }
    if f.service == "home" && f.address.trim().chars().count() < 5 {
        e.push(("address", l("Para uma visita ao domicílio, escreva a morada.", "For a home visit, write the address.")));
    }
    if f.address.chars().count() > 300 { e.push(("address", l("A morada é demasiado longa.", "The address is too long."))); }
    if f.message.chars().count() > 1000 { e.push(("message", l("A mensagem é demasiado longa (máximo 1000 caracteres).", "The message is too long (1,000 characters at most)."))); }
    if f.privacy != "on" {
        e.push(("privacy", l("Confirme que leu a política de privacidade.", "Confirm you have read the privacy policy.")));
    }
    e
}

async fn post_book(st: St, lang: Lang, peer: SocketAddr, headers: HeaderMap, f: BookForm) -> Response {
    let now = OffsetDateTime::now_utc();
    let ok_page = format!("{}?{}=1", href(Page::Book, lang), lang.t("enviado", "sent"));
    // A robot: answer as if it worked, store nothing.
    if !f.website.is_empty() {
        return Redirect::to(&ok_page).into_response();
    }
    let mut extra = html::Extra { form: f.clone(), status: StatusCode::UNPROCESSABLE_ENTITY, ..Default::default() };
    let fresh = now.unix_timestamp();
    extra.token = Some((fresh.to_string(), sign(&st.key[..], fresh)));
    if !token_ok(&st.key[..], &f.t, &f.s, now.unix_timestamp()) {
        extra.errors.push(("form", lang.t("A página esteve aberta muito tempo ou foi enviada depressa demais. Confirme os dados e envie de novo.",
            "The page was open too long or sent too quickly. Check your details and send it again.").to_string()));
        return show(&st, Page::Book, lang, extra).await;
    }
    let today = now.to_offset(lisbon_offset(now)).date();
    extra.errors = check(&f, lang, today);
    if !extra.errors.is_empty() {
        return show(&st, Page::Book, lang, extra).await;
    }
    // At most 5 requests an hour and 10 a day from one address; only a hash of the address is kept, for 2 days.
    let ip = real_ip(&st.proxies, peer.ip(), headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()));
    let who = hex::encode(&Sha256::digest([&st.key[..], ip.as_bytes()].concat())[..16]);
    for (k, max, mins) in [(format!("book:{who}"), 5, 60), (format!("bookday:{who}"), 10, 1440)] {
        match sqlx::query_scalar::<_, bool>("SELECT portal.hit($1, $2, $3)").bind(&k).bind(max).bind(mins).fetch_one(&st.pool).await {
            Ok(true) => {}
            Ok(false) => {
                extra.status = StatusCode::TOO_MANY_REQUESTS;
                extra.errors.push(("form", lang.t("Já recebemos vários pedidos deste local. Tente mais tarde ou ligue-nos.",
                    "We've already had several requests from here. Try later or give us a call.").to_string()));
                return show(&st, Page::Book, lang, extra).await;
            }
            Err(e) => { tracing::error!("rate limit: {e}"); return html_response(StatusCode::SERVICE_UNAVAILABLE, html::unavailable(lang), false); }
        }
    }
    let day = Date::parse(f.day.trim(), &time::macros::format_description!("[year]-[month]-[day]")).expect("checked");
    let r = sqlx::query_scalar::<_, sqlx::types::Uuid>("SELECT portal.request_booking($1, $2, $3, $4, $5, $6, $7, $8, $9)")
        .bind(&f.service).bind(f.name.trim()).bind(f.phone.trim()).bind(f.email.trim()).bind(day).bind(&f.part)
        .bind(f.address.trim()).bind(f.message.trim()).bind(lang.code())
        .fetch_one(&st.pool).await;
    match r {
        Ok(_) => Redirect::to(&ok_page).into_response(),
        Err(sqlx::Error::Database(e)) => {
            let code = e.message().to_string();
            let msg = match code.as_str() {
                "closed" => lang.t("De momento não estamos a receber pedidos pelo site. Ligue-nos ou envie um email.", "We aren't taking requests on the website right now. Call or email us."),
                "busy" => lang.t("Recebemos muitos pedidos hoje. Tente amanhã ou ligue-nos.", "We've had a lot of requests today. Try tomorrow or give us a call."),
                "bad_day" => lang.t("Escolha um dia a partir de amanhã e nos próximos dois meses.", "Choose a day from tomorrow and within the next two months."),
                "bad_service" => lang.t("Esse serviço não está disponível. Escolha outro.", "That service isn't available. Choose another."),
                _ => lang.t("Alguns dados não estão certos. Verifique e envie de novo.", "Some details aren't right. Check them and send again."),
            };
            extra.errors.push(("form", msg.to_string()));
            show(&st, Page::Book, lang, extra).await
        }
        Err(e) => { tracing::error!("request: {e}"); html_response(StatusCode::SERVICE_UNAVAILABLE, html::unavailable(lang), false) }
    }
}

/// Mainland Portugal: UTC+1 from the last Sunday of March to the last Sunday of October (01:00 UTC).
pub fn lisbon_offset(now: OffsetDateTime) -> time::UtcOffset {
    let y = now.year();
    let last_sunday = |m: time::Month| {
        let mut d = Date::from_calendar_date(y, m, m.length(y)).unwrap();
        while d.weekday() != time::Weekday::Sunday { d = d.previous_day().unwrap(); }
        d.with_hms(1, 0, 0).unwrap().assume_utc()
    };
    if now >= last_sunday(time::Month::March) && now < last_sunday(time::Month::October) {
        time::UtcOffset::from_hms(1, 0, 0).unwrap()
    } else {
        time::UtcOffset::UTC
    }
}

async fn downtime(State(st): State<St>) -> Response {
    let phone = sqlx::query_scalar::<_, String>("SELECT phone FROM portal.site").fetch_one(&st.pool).await.unwrap_or_default();
    let mut r = html_response(StatusCode::OK, html::downtime(&phone), false);
    // A page meant to be copied elsewhere: everything inline, nothing else loaded.
    r.headers_mut().insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; frame-ancestors 'none'"));
    r
}

async fn health(State(st): State<St>) -> Response {
    let db = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&st.pool).await.is_ok();
    let body = serde_json::json!({ "ok": db, "version": env!("CARGO_PKG_VERSION"), "service": "site" }).to_string();
    (if db { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE }, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

async fn not_found(State(st): State<St>, uri: axum::http::Uri) -> Response {
    let lang = if uri.path().starts_with("/en") { Lang::En } else { Lang::Pt };
    let name = sqlx::query_scalar::<_, String>("SELECT clinic_name FROM portal.site").fetch_one(&st.pool).await.unwrap_or_default();
    html_response(StatusCode::NOT_FOUND, html::not_found(lang, &name), false)
}

fn font(body: &'static [u8]) -> Response {
    ([(header::CONTENT_TYPE, "font/woff2"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], body).into_response()
}

fn asset(ct: &'static str, body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "public, max-age=3600")], body).into_response()
}

fn app(st: St) -> Router {
    let mut r = Router::new()
        .route("/health", get(health))
        .route("/site.css", get(|| async { asset("text/css; charset=utf-8", include_str!("../static/site.css")) }))
        .route("/icon.svg", get(|| async { asset("image/svg+xml", include_str!("../static/icon.svg")) }))
        // The brand's face (Manrope, OFL, variable), served from here: nothing is loaded from other servers.
        .route("/fonts/manrope.woff2", get(|| async { font(include_bytes!("../static/fonts/manrope-latin.woff2")) }))
        .route("/fonts/manrope-ext.woff2", get(|| async { font(include_bytes!("../static/fonts/manrope-latin-ext.woff2")) }))
        // Removes the old site's service worker from returning visitors' browsers (it was registered at /sw.js).
        .route("/sw.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], include_str!("../static/sw-reset.js")).into_response() }))
        .route("/robots.txt", get(|State(st): State<St>| async move {
            let open = sqlx::query_scalar::<_, bool>("SELECT published FROM portal.site").fetch_one(&st.pool).await.unwrap_or(false);
            ([(header::CONTENT_TYPE, "text/plain")], if open { "User-agent: *\nDisallow: /conta\nDisallow: /en/account\n" } else { "User-agent: *\nDisallow: /\n" }).into_response()
        }))
        .route("/downtime.html", get(downtime));
    r = account::routes(r);
    for (page, pt, en) in PAGES {
        // The patient area has its own pages (account.rs).
        if page == Page::Account { continue; }
        for (path, lang) in [(pt, Lang::Pt), (en, Lang::En)] {
            let g = move |State(st): State<St>, Query(q): Query<BookQuery>| async move { get_page(st, page, lang, q).await };
            r = if page == Page::Book {
                r.route(path, get(g).post(move |State(st): State<St>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Form(f): Form<BookForm>| async move {
                    post_book(st, lang, peer, headers, f).await
                }))
            } else {
                r.route(path, get(g))
            };
        }
    }
    r.fallback(not_found)
        .with_state(st)
        .layer(RequestBodyLimitLayer::new(16 * 1024))
        // No scripts, no frames, no outside resources; forms only post back here.
        .layer(SetResponseHeaderLayer::if_not_present(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(
            "default-src 'none'; style-src 'self'; font-src 'self'; img-src 'self' data:; form-action 'self'; base-uri 'none'; frame-ancestors 'none'")))
        .layer(SetResponseHeaderLayer::if_not_present(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
        .layer(SetResponseHeaderLayer::overriding(header::HeaderName::from_static("permissions-policy"), HeaderValue::from_static("camera=(), microphone=(), geolocation=(), interest-cohort=()")))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(env::var("RUST_LOG").unwrap_or_else(|_| "info".into())).init();
    let url = env::var("DATABASE_URL").expect("DATABASE_URL is required (the physiopulse_portal login)");
    // The staff server creates the login at start-up; wait for it instead of failing.
    let pool = loop {
        match PgPoolOptions::new().max_connections(5).acquire_timeout(Duration::from_secs(5)).connect(&url).await {
            Ok(p) => break p,
            Err(e) => { tracing::warn!("waiting for the database: {e}"); tokio::time::sleep(Duration::from_secs(5)).await; }
        }
    };
    // FORM_KEY signs the request form and hashes visitor addresses; a random one works too (open forms expire on restart).
    let mut key = [0u8; 32];
    match env::var("FORM_KEY").ok().and_then(|k| hex::decode(k.trim()).ok()).filter(|k| k.len() == 32) {
        Some(k) => key.copy_from_slice(&k),
        None => { use rand::RngCore; rand::thread_rng().fill_bytes(&mut key); }
    }
    let proxies = parse_cidrs(&env::var("TRUSTED_PROXIES").unwrap_or_else(|_| "127.0.0.1/32,::1/128,172.16.0.0/12".into()));
    // The internal API key: derived from this login's database password, like the staff server does (db.rs).
    let internal = url.split_once("://").and_then(|(_, rest)| rest.split_once('@')).and_then(|(cred, _)| cred.split_once(':'))
        .map(|(_, pw)| pw.to_string()).filter(|p| p.len() >= 16 && p.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(|p| -> [u8; 32] { Sha256::digest(format!("physiopulse-portal-internal:{p}").as_bytes()).into() });
    let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().expect("http client");
    let server_url: Arc<str> = env::var("SERVER_URL").unwrap_or_else(|_| "http://server:8080".into()).trim_end_matches('/').into();
    let secure_cookie = env::var("COOKIE_INSECURE").map(|v| v != "1").unwrap_or(true);
    let st = St { pool, key: Arc::new(key), proxies: Arc::new(proxies), http, server_url, internal, secure_cookie };
    let addr: SocketAddr = env::var("BIND").unwrap_or_else(|_| "0.0.0.0:8090".into()).parse().expect("BIND must be host:port");
    tracing::info!("PhysioPulse website listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    axum::serve(listener, app(st).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; })
        .await
        .expect("server");
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    #[test]
    fn form_token() {
        let k = [7u8; 32];
        let s = sign(&k, 1000);
        assert!(token_ok(&k, "1000", &s, 1010));
        assert!(!token_ok(&k, "1000", &s, 1001), "too quick");
        assert!(!token_ok(&k, "1000", &s, 1000 + 7201), "too old");
        assert!(!token_ok(&k, "1001", &s, 1010), "time changed");
        assert!(!token_ok(&[8u8; 32], "1000", &s, 1010), "other key");
    }

    #[test]
    fn lisbon_time() {
        assert_eq!(lisbon_offset(datetime!(2026-07-01 12:00 UTC)).whole_hours(), 1);
        assert_eq!(lisbon_offset(datetime!(2026-11-11 12:00 UTC)).whole_hours(), 0);
        assert_eq!(lisbon_offset(datetime!(2026-03-29 00:59 UTC)).whole_hours(), 0);
        assert_eq!(lisbon_offset(datetime!(2026-03-29 01:00 UTC)).whole_hours(), 1);
        assert_eq!(lisbon_offset(datetime!(2026-10-25 01:00 UTC)).whole_hours(), 0);
    }

    #[test]
    fn form_checks() {
        let today = date!(2026-11-11);
        let good = BookForm { service: "assessment".into(), name: "Ana Lopes".into(), phone: "912 345 678".into(), day: "2026-11-12".into(), part: "morning".into(), privacy: "on".into(), ..Default::default() };
        assert!(check(&good, Lang::Pt, today).is_empty());
        let f = BookForm { day: "2026-11-11".into(), ..good.clone() };
        assert_eq!(check(&f, Lang::Pt, today)[0].0, "day", "not today");
        let f = BookForm { day: "2027-01-13".into(), ..good.clone() };
        assert_eq!(check(&f, Lang::Pt, today)[0].0, "day", "not beyond two months");
        let f = BookForm { phone: "".into(), email: "".into(), ..good.clone() };
        assert_eq!(check(&f, Lang::En, today)[0].0, "phone");
        let f = BookForm { phone: "".into(), email: "ana@example".into(), ..good.clone() };
        assert_eq!(check(&f, Lang::En, today)[0].0, "email");
        let f = BookForm { service: "home".into(), ..good.clone() };
        assert_eq!(check(&f, Lang::Pt, today)[0].0, "address");
        let f = BookForm { privacy: "".into(), ..good.clone() };
        assert_eq!(check(&f, Lang::Pt, today)[0].0, "privacy");
        let f = BookForm { phone: "91234".into(), ..good };
        assert_eq!(check(&f, Lang::Pt, today)[0].0, "phone");
    }

    #[test]
    fn visitor_address() {
        let p = parse_cidrs("172.16.0.0/12");
        assert_eq!(real_ip(&p, "172.18.0.2".parse().unwrap(), Some("1.1.1.1, 203.0.113.7")), "203.0.113.7");
        assert_eq!(real_ip(&p, "192.168.1.9".parse().unwrap(), Some("203.0.113.7")), "192.168.1.9");
    }

    #[test]
    fn every_page_has_both_addresses() {
        for (p, pt, en) in PAGES {
            assert!(pt.starts_with('/') && en.starts_with("/en"));
            assert_eq!(href(p, Lang::Pt), pt);
            assert_eq!(href(p, Lang::En), en);
        }
    }
}
