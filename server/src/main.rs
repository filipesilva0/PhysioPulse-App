//! PhysioPulse server — phase 1: accounts, two-step sign-in, roles, access log.
//! Phase 2: patients, clinical record, consents, prescriptions, schedule.
//! Phase 2 (later): patient documents and the patient archive (uploads, signed notes as PDF); legal tools (export, anonymise, delete).
//! Reimbursement pack and discharge letters (with a local-AI draft the owner approves).
//! Complaints log (ERS deadline in working days), delete everything from one day, VAT-exempt prices.
//! Phase 3: money (prices, packages, payments, expenses, cash close, salaries, month close, accountant export).
//! PhysioPulse plan, phase 2: session recording, speech-to-text on the AI PC, plain AI report; one AI switch per person.
//! PhysioPulse plan, phase 3: public foundations — device approval, rate limits in the database, two-step secrets
//! encrypted with a server key, the real client address behind Nginx Proxy Manager (HTTPS). Tailscale no longer needed.
//! Runs on ZimaOS; reached through Nginx Proxy Manager (HTTPS) at the clinic's public address.

mod ai;
mod auth;
mod db;
mod error;
mod export;
mod forms;
mod i18n;
mod routes;
mod secret;
mod style;
mod stt;
mod workdays;

use axum::{extract::{ConnectInfo, DefaultBodyLimit}, http::{header, HeaderValue, Request}, middleware::Next, response::Response, routing::{delete, get, patch, post, put}, Json, Router};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use std::{env, net::{IpAddr, SocketAddr}, path::PathBuf, sync::OnceLock, time::Duration};
use tower_http::{limit::RequestBodyLimitLayer, set_header::SetResponseHeaderLayer, trace::TraceLayer};

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION"), "phase": 4, "recording": true, "devices": true, "website": true }))
}

/// Addresses allowed to tell us the real client address (`X-Forwarded-For`): Nginx Proxy Manager reaches the
/// server from Docker's private range. `TRUSTED_PROXIES` (comma-separated CIDRs) overrides the default.
static PROXIES: OnceLock<Vec<(IpAddr, u8)>> = OnceLock::new();

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

/// The caller's real address: the connection's own, or — when that is the proxy — the last address the proxy
/// added to `X-Forwarded-For`. Written into an internal header (`auth::CLIENT_IP`); any copy sent by a client is dropped.
fn real_ip(peer: Option<IpAddr>, forwarded: Option<&str>) -> String {
    let proxies = PROXIES.get_or_init(|| parse_cidrs(&env::var("TRUSTED_PROXIES").unwrap_or_else(|_| "127.0.0.1/32,::1/128,172.16.0.0/12".into())));
    let Some(peer) = peer else { return String::new() };
    let peer = match peer { IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(peer), p => p };
    if proxies.iter().any(|c| in_cidr(peer, *c)) {
        if let Some(last) = forwarded.and_then(|f| f.rsplit(',').next()).and_then(|x| x.trim().parse::<IpAddr>().ok()) {
            return last.to_string();
        }
    }
    peer.to_string()
}

async fn client_ip(mut req: Request<axum::body::Body>, next: Next) -> Response {
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    let fwd = req.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(str::to_string);
    let ip = real_ip(peer, fwd.as_deref());
    req.headers_mut().remove(auth::CLIENT_IP);
    if let Ok(v) = HeaderValue::from_str(&ip) {
        req.headers_mut().insert(auth::CLIENT_IP, v);
    }
    next.run(req).await
}

/// Folder layout the AI and the owner can browse on ZimaOS.
async fn ensure_archive(root: &PathBuf) -> std::io::Result<()> {
    for dir in ["Utentes", "Financas", "Financas/Exportacoes", "Agenda", "Relatorios", "Sistema/acessos", "Sistema/reclamacoes"] {
        tokio::fs::create_dir_all(root.join(dir)).await?;
    }
    Ok(())
}

pub fn app(state: auth::AppState) -> Router {
    use routes::{account as acc, agenda as ag, clinic as cl, system as sy, assist as ai4, assistant as asst, auth as a, complaints as cp, days as dy, devices as dv, documents as d, legal as l, letters as lt, money as m, patients as p, reminders as rm, recordings as rec, schedule as sc, staff as st, website as web, portal as pt};
    let api = Router::new()
        .route("/status", get(a::status))
        .route("/setup", post(a::setup))
        .route("/auth/login", post(a::login))
        .route("/auth/verify", post(a::verify))
        .route("/auth/signup", post(a::signup))
        .route("/auth/logout", post(a::logout))
        .route("/auth/forget-device", post(a::forget_device))
        // PhysioPulse plan, phase 3: device approval.
        .route("/auth/pending", post(dv::pending))
        .route("/auth/pending/recover", post(dv::pending_recover))
        .route("/devices", get(dv::list))
        .route("/devices/:id", patch(dv::rename))
        .route("/devices/:id/approve", post(dv::approve))
        .route("/devices/:id/revoke", post(dv::revoke))
        .route("/me", get(a::me).patch(acc::update_me))
        // Audit fixes, phase A: accounts, clinic details, server and backups.
        .route("/me/password", post(acc::change_password))
        .route("/me/recovery-codes", get(acc::recovery_status).post(acc::make_recovery_codes))
        .route("/auth/recover", post(acc::recover))
        .route("/auth/reset", post(acc::reset_password))
        .route("/staff/:id/reset-code", post(acc::reset_code))
        .route("/staff/:id/reset-two-step", post(acc::reset_two_step))
        .route("/invites/:id", delete(acc::revoke_invite))
        .route("/clinic", get(cl::get).put(cl::put))
        .route("/system/status", get(sy::status))
        .route("/system/backup", post(sy::backup_now))
        .route("/staff", get(st::list))
        .route("/therapists", get(st::therapists))
        .route("/staff/:id", patch(st::update))
        .route("/staff/:id/sessions", axum::routing::delete(st::end_sessions))
        .route("/staff/:id/phones", get(st::phones))
        .route("/staff/:id/phones/:sid", axum::routing::delete(st::sign_out_phone))
        .route("/invites", get(acc::invites).post(st::invite))
        .route("/access-log", get(st::access_log))
        .route("/access-log/export", get(st::access_log_export))
        // Phase 2: patients and the clinical record
        .route("/patients", get(p::list).post(p::create))
        .route("/patients/duplicates", get(p::duplicates))
        .route("/patients/:id", get(p::get_file).patch(p::update))
        .route("/patients/:id/anamnesis", put(p::put_anamnesis))
        .route("/patients/:id/notes", post(p::create_note))
        .route("/notes/:id", patch(p::update_note).delete(p::delete_note))
        .route("/notes/:id/sign", post(p::sign_note))
        .route("/notes/:id/pdf", post(p::note_pdf))
        .route("/patients/:id/legal", get(l::check))
        .route("/patients/:id/anonymise", post(l::anonymise))
        .route("/patients/:id/delete", post(l::delete))
        .route("/patients/:id/export/:kind", get(l::export))
        .route("/patients/:id/reimbursement", get(lt::pack_preview).post(lt::make_pack))
        .route("/patients/:id/letters", get(lt::list_letters).post(lt::create_letter))
        .route("/patients/:id/letters/template", get(lt::letter_template_get))
        .route("/patients/:id/letters/ai-draft", post(lt::ai_draft))
        .route("/letters/:id", patch(lt::update_letter).delete(lt::delete_letter))
        .route("/letters/:id/approve", post(lt::approve_letter))
        .route("/letters/:id/pdf", post(lt::letter_pdf_again))
        .route("/ai/status", get(lt::ai_status))
        .route("/ai/settings", get(lt::ai_settings).put(lt::put_ai_settings))
        .route("/ai/test", post(lt::ai_test))
        .route("/documents/:id", get(d::get).patch(d::update).delete(d::delete))
        .route("/documents/:id/raw", get(d::raw))
        .route("/patients/:id/tests", post(p::add_test))
        .route("/tests/:id", delete(p::delete_test))
        .route("/patients/:id/prescriptions", post(p::create_prescription))
        .route("/prescriptions/:id", patch(p::update_prescription))
        .route("/consent-texts", get(p::consent_texts).post(p::add_consent_text))
        .route("/patients/:id/consents", post(p::sign_consent))
        .route("/consents/:id", get(p::get_consent))
        .route("/consents/:id/revoke", post(p::revoke_consent))
        .route("/consents/:id/pdf", post(p::consent_pdf))
        .route("/consent-texts/:id/approve", post(p::approve_consent_text))
        .route("/complaints", get(cp::list).post(cp::create))
        .route("/complaints/:id", patch(cp::update).delete(cp::delete))
        .route("/days/:day", get(dy::check))
        .route("/days/:day/delete", post(dy::delete))
        .route("/reminders", get(rm::list))
        .route("/reminders/sent", post(rm::mark))
        .route("/reminders/template", put(rm::set_template))
        // Phase 2: schedule
        .route("/appointments", get(sc::list).post(sc::create))
        .route("/appointments/:id", patch(sc::update))
        .route("/appointments/:id/attendees", post(sc::add_attendee))
        .route("/appointments/:id/attendees/:pid", patch(sc::set_attended).delete(sc::remove_attendee))
        .route("/waitlist", get(sc::waitlist).post(sc::add_waitlist))
        .route("/waitlist/:id", patch(sc::update_waitlist))
        .route("/booking-requests", get(sc::requests).post(sc::add_request))
        .route("/booking-requests/:id/accept", post(sc::accept_request))
        .route("/booking-requests/:id/decline", post(sc::decline_request))
        .route("/booking-requests/:id/reopen", post(sc::reopen_request))
        .route("/booking-requests/:id/suggest", get(ag::suggest_request))
        .route("/booking-requests/:id/book", post(ag::book_request))
        .route("/alerts", get(sc::alerts))
        // Phase 3: money
        .route("/products", get(m::products).post(m::create_product))
        .route("/products/:id", patch(m::update_product))
        .route("/patients/:id/money", get(m::patient_money))
        .route("/patients/:id/plans", post(m::sell_plan))
        .route("/plans/:id", patch(m::update_plan))
        .route("/payments", get(m::list_payments).post(m::record_payment))
        .route("/payments/:id", patch(m::update_payment))
        .route("/payments/:id/void", post(m::void_payment))
        .route("/finance/overview", get(m::overview))
        .route("/finance/owed", get(m::owed_all))
        .route("/expenses", get(m::list_expenses).post(m::create_expense))
        .route("/expenses/:id", patch(m::update_expense).delete(m::delete_expense))
        .route("/finance/files/:id/raw", get(m::file_raw))
        .route("/finance/cash", get(m::cash_day).post(m::close_day))
        .route("/finance/salaries", get(m::salaries).post(m::pay_salary))
        .route("/finance/salaries/:id", delete(m::delete_salary))
        .route("/finance/salaries/rate", put(m::set_ss_rate))
        .route("/staff/:id/pay", put(m::set_pay))
        .route("/finance/months", get(m::months))
        .route("/finance/months/:month/close", post(m::close_month))
        .route("/finance/months/:month/reopen", post(m::reopen_month))
        .route("/finance/export", post(m::export_zip))
        // Phase 4: the local AI (owner only) and the assistant's hours/closures.
        .route("/ai/rewrite-note", post(ai4::rewrite_note))
        .route("/ai/read-receipt", post(ai4::read_receipt))
        .route("/ai/report", post(ai4::draft_report))
        .route("/ai/assistant", post(asst::ask))
        .route("/finance/reports", get(ai4::list_reports).post(ai4::save_report))
        .route("/finance/reports/figures", get(ai4::report_figures))
        .route("/finance/reports/:id", delete(ai4::delete_report))
        .route("/assistant/changes/:id/apply", post(asst::apply))
        .route("/assistant/changes/:id/cancel", post(asst::cancel))
        .route("/assistant/changes/:id/undo", post(asst::undo))
        .route("/clinic/hours", get(ai4::get_hours).put(ai4::put_hours))
        .route("/closures", post(ai4::add_closure))
        .route("/closures/check", get(ai4::check_closure))
        .route("/closures/:id", delete(ai4::delete_closure))
        // PhysioPulse plan, phase 2: session recordings (owner, and therapists with AI on).
        .route("/patients/:id/recordings", get(rec::list).post(rec::start))
        .route("/recordings/:id", get(rec::get).delete(rec::delete))
        .route("/recordings/:id/finish", post(rec::finish))
        .route("/recordings/:id/retry", post(rec::retry))
        .route("/recordings/:id/audio", get(rec::audio))
        .route("/ai/speech/test", post(lt::speech_test))
        // PhysioPulse plan, phase 4a: the public website (Settings › Website, owner).
        .route("/website", get(web::get).put(web::put))
        .route("/website/team/:id", put(web::put_person))
        .route("/website/products/:id", put(web::put_product))
        // Phase 4b: patient accounts on the website — staff side, in the patient's file.
        .route("/patients/:id/portal", get(pt::get))
        .route("/patients/:id/portal/invite", post(pt::invite))
        .route("/patients/:id/portal/invites/:cid", delete(pt::drop_invite))
        .route("/patients/:id/portal/reset", post(pt::reset))
        .route("/patients/:id/portal/accounts/:aid", delete(pt::unlink))
        // …and the website's internal calls for a signed-in patient (website key + session cookie; routes/portal.rs).
        .route("/internal/portal/me", get(pt::me))
        .route("/internal/portal/cancel", post(pt::cancel))
        .route("/internal/portal/request", post(pt::ask))
        .route("/internal/portal/invoice/:id", get(pt::invoice))
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        // File uploads (invoice PDFs, receipt photos, patient documents) travel as base64 JSON: a bigger limit on this route only.
        .merge(Router::new().route("/finance/files", post(m::upload)).route("/patients/:id/documents", post(d::upload))
            .layer(DefaultBodyLimit::max(15 * 1024 * 1024))
            .layer(RequestBodyLimitLayer::new(15 * 1024 * 1024)))
        // Audio arrives in pieces of up to ~2 minutes (4 MB, base64 in JSON).
        .merge(Router::new().route("/recordings/:id/chunks/:n", put(rec::chunk))
            .layer(DefaultBodyLimit::max(6 * 1024 * 1024))
            .layer(RequestBodyLimitLayer::new(6 * 1024 * 1024)));
    Router::new()
        .route("/health", get(health))
        .nest("/api", api)
        .with_state(state)
        // Error messages in the app's language (Accept-Language: pt → Portuguese; audit M13).
        .layer(axum::middleware::from_fn(i18n::localise))
        // The real client address (behind Nginx Proxy Manager) for rate limits and the device list.
        .layer(axum::middleware::from_fn(client_ip))
        .layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-store")))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        // Spans carry the method and path only, never the query string (a search can contain a patient's name).
        .layer(TraceLayer::new_for_http().make_span_with(|r: &Request<_>| tracing::debug_span!("request", method = %r.method(), path = %r.uri().path())))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=warn".into())).init();

    let url = env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let pool = PgPoolOptions::new().max_connections(10).acquire_timeout(Duration::from_secs(10)).connect(&url).await.expect("cannot connect to PostgreSQL");
    db::migrate(&pool).await.expect("migrations failed");

    // Server console (phase 3): `physiopulse-server devices` / `physiopulse-server approve CODE`.
    let args: Vec<String> = env::args().skip(1).collect();
    if !args.is_empty() {
        std::process::exit(routes::devices::console(&pool, &args).await);
    }

    // Two-step secrets are kept encrypted with SECRET_KEY (phase 3).
    match secret::init(env::var("SECRET_KEY").ok().as_deref()) {
        Ok(true) => match secret::seal_old(&pool).await {
            Ok(0) => {}
            Ok(n) => tracing::info!("encrypted {n} two-step secrets with SECRET_KEY"),
            Err(e) => tracing::error!("could not encrypt the two-step secrets: {e}"),
        },
        Ok(false) => tracing::warn!("SECRET_KEY is not set: two-step secrets stay in plain text in the database"),
        Err(e) => panic!("{e}"),
    }
    ai::load(&pool).await;
    stt::load(&pool).await;

    // The website's database login (phase 4a).
    let _ = routes::portal::INTERNAL_KEY.set(db::portal_internal_key(env::var("PORTAL_DB_PASSWORD").ok().as_deref()));
    match db::portal_role(&pool, env::var("PORTAL_DB_PASSWORD").ok().as_deref()).await {
        Ok(true) => tracing::info!("website database login ready"),
        Ok(false) => tracing::warn!("PORTAL_DB_PASSWORD is not set: the website can't sign in to the database"),
        Err(e) => tracing::error!("could not set up the website's database login: {e}"),
    }

    let archive = PathBuf::from(env::var("ARCHIVE_DIR").unwrap_or_else(|_| "/data/PhysioPulse".into()));
    if let Err(e) = ensure_archive(&archive).await {
        tracing::warn!("could not create archive folders at {}: {e}", archive.display());
    }

    // Session recordings: a folder of its own, outside the archive, that the backups never include.
    let audio = PathBuf::from(env::var("AUDIO_DIR").unwrap_or_else(|_| "/data/audio".into()));
    if let Err(e) = tokio::fs::create_dir_all(&audio).await {
        tracing::warn!("could not create the recordings folder at {}: {e}", audio.display());
    }

    let idle_minutes = env::var("IDLE_MINUTES").ok().and_then(|v| v.parse().ok()).unwrap_or(15);
    let state = auth::AppState { pool: pool.clone(), idle_minutes, archive: archive.clone(), audio };
    routes::recordings::spawn_worker(state.clone());

    // Housekeeping: remove expired sessions and challenges every 10 minutes.
    tokio::spawn(async move {
        loop {
            let _ = sqlx::query("DELETE FROM sessions WHERE expires_at < now() OR (NOT phone AND last_seen < now() - make_interval(mins => $1::int))").bind(idle_minutes as i32).execute(&pool).await;
            let _ = sqlx::query("DELETE FROM challenges WHERE expires_at < now()").execute(&pool).await;
            let _ = sqlx::query("DELETE FROM pending_signins WHERE expires_at < now()").execute(&pool).await;
            let _ = sqlx::query("DELETE FROM rate_limits WHERE window_start < now() - interval '1 day'").execute(&pool).await;
            // Website requests from people who never became patients are kept 90 days (privacy notice on the site).
            let _ = sqlx::query("DELETE FROM booking_requests WHERE source = 'website' AND patient_id IS NULL AND status <> 'accepted' AND created_at < now() - interval '90 days'").execute(&pool).await;
            tokio::time::sleep(Duration::from_secs(600)).await;
        }
    });

    let addr: SocketAddr = env::var("BIND").unwrap_or_else(|_| "0.0.0.0:8080".into()).parse().expect("BIND must be host:port");
    tracing::info!("PhysioPulse server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    axum::serve(listener, app(state).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; })
        .await
        .expect("server");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_client_address() {
        let _ = PROXIES.set(parse_cidrs("127.0.0.1/32,172.16.0.0/12"));
        let npm: IpAddr = "172.18.0.5".parse().unwrap();
        let lan: IpAddr = "192.168.1.20".parse().unwrap();
        assert_eq!(real_ip(Some(npm), Some("203.0.113.9")), "203.0.113.9", "behind the proxy: the address it saw");
        assert_eq!(real_ip(Some(npm), Some("1.2.3.4, 203.0.113.9")), "203.0.113.9", "the client can't put a fake one first");
        assert_eq!(real_ip(Some(lan), Some("203.0.113.9")), "192.168.1.20", "not the proxy: its own address, header ignored");
        assert_eq!(real_ip(Some(npm), None), "172.18.0.5");
        assert!(in_cidr("10.1.2.3".parse().unwrap(), ("10.0.0.0".parse().unwrap(), 8)));
        assert!(!in_cidr("11.1.2.3".parse().unwrap(), ("10.0.0.0".parse().unwrap(), 8)));
    }
}
