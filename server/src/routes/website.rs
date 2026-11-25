//! Settings › Website (PhysioPulse plan, phase 4a): what the public site shows. Owner only.
//! The site itself is a separate service (server/portal) that reads these through the `portal` views.

use super::util::{changed, sets, update_row, K};
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{extract::{Path, State}, Json};
use serde_json::{json, Map, Value};
use uuid::Uuid;

const SITE: &[(&str, K)] = &[
    ("published", K::Req("boolean")), ("requests_open", K::Req("boolean")), ("home_visits", K::Req("boolean")),
    ("opening_soon", K::Req("boolean")), ("show_prices", K::Req("boolean")), ("accounts_open", K::Req("boolean")),
    ("tagline_pt", K::Text), ("tagline_en", K::Text), ("intro_pt", K::Text), ("intro_en", K::Text),
    ("home_text_pt", K::Text), ("home_text_en", K::Text),
];
const PERSON: &[(&str, K)] = &[
    ("web_visible", K::Req("boolean")), ("web_title_pt", K::Text), ("web_title_en", K::Text), ("web_bio_pt", K::Text), ("web_bio_en", K::Text),
];

/// Longest text allowed per field (characters).
fn limit(name: &str) -> usize {
    match name {
        "tagline_pt" | "tagline_en" => 90,
        "web_title_pt" | "web_title_en" => 60,
        "web_bio_pt" | "web_bio_en" => 500,
        _ => 700,
    }
}

fn check_lengths(body: &Map<String, Value>, fields: &[(&str, K)]) -> ApiResult<()> {
    for (n, k) in fields {
        if let (K::Text, Some(Value::String(v))) = (k, body.get(*n)) {
            if v.trim().chars().count() > limit(n) {
                return Err(AppError::BadRequest("Some texts are too long. Shorten them and try again."));
            }
            // Plain text only: the site shows it as written, and angle brackets would only look broken.
            if v.contains('<') || v.contains('>') {
                return Err(AppError::BadRequest("Write plain text, without < or >."));
            }
        }
    }
    Ok(())
}

/// Where the site is published (`SITE_URL`), for the app's "Open Website" button.
pub(crate) fn site_url() -> String {
    std::env::var("SITE_URL").unwrap_or_default().trim().trim_end_matches('/').to_string()
}

/// Owner: everything Settings › Website shows.
pub async fn get(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let (v,): (Value,) = sqlx::query_as(
        "SELECT json_build_object(
            'site', (SELECT to_jsonb(w) - 'id' - 'updated_by' FROM website w WHERE id = 1),
            'team', (SELECT COALESCE(json_agg(json_build_object('id', id, 'name', name, 'role', role, 'web_visible', web_visible,
                        'web_title_pt', web_title_pt, 'web_title_en', web_title_en, 'web_bio_pt', web_bio_pt, 'web_bio_en', web_bio_en)
                        ORDER BY role <> 'owner', name), '[]')
                     FROM staff WHERE active AND role IN ('owner', 'therapist')),
            'products', (SELECT COALESCE(json_agg(json_build_object('id', id, 'name_pt', name_pt, 'name_en', name_en, 'kind', kind,
                        'price_cents', price_cents, 'web_visible', web_visible) ORDER BY sort), '[]')
                     FROM products WHERE active),
            'clinic', (SELECT json_build_object('name', name, 'address', address, 'phone', phone, 'email', email, 'nif', nif, 'ers_number', ers_number) FROM clinic WHERE id = 1),
            'requests_30d', (SELECT count(*) FROM booking_requests WHERE source = 'website' AND created_at > now() - interval '30 days'))")
        .fetch_one(&s.pool).await?;
    let mut v = v;
    v["site_url"] = json!(site_url());
    Ok(Json(v))
}

/// Owner: change the site's switches and texts.
pub async fn put(State(s): State<AppState>, a: Auth, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    check_lengths(&body, SITE)?;
    if body.get("published") == Some(&Value::Bool(true)) {
        // Publishing needs what the legal pages and the footer must show (DL 74/2017, Lei 58/2019): who the clinic is and how to reach it.
        let (name, nif, contact): (String, String, bool) = sqlx::query_as("SELECT name, nif, (phone <> '' OR email <> '') FROM clinic WHERE id = 1").fetch_one(&s.pool).await?;
        if name.trim().is_empty() || nif.trim().is_empty() || !contact {
            return Err(AppError::BadRequest("Before publishing, fill in the clinic’s name, NIF and a phone or email in Settings › Clinic."));
        }
    }
    let fields = changed(&body, SITE);
    let mut tx = s.pool.begin().await?;
    let (parts, binds) = sets(&body, SITE, 1)?;
    if !parts.is_empty() {
        let sql = format!("UPDATE website SET {}, updated_by = ${}, updated_at = now() WHERE id = 1", parts.join(", "), binds.len() + 1);
        let mut q = sqlx::query(&sql);
        for b in binds { q = q.bind(b); }
        q.bind(a.staff.id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    if !fields.is_empty() {
        let action = match body.get("published") { Some(Value::Bool(true)) => "website.published", Some(Value::Bool(false)) => "website.unpublished", _ => "website.changed" };
        auth::log(&s.pool, Some(a.staff.id), action, None, &a.device, &fields).await;
    }
    get(State(s), a).await
}

/// Owner: how one person appears on the team page.
pub async fn put_person(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    check_lengths(&body, PERSON)?;
    let ok = update_row(&s.pool, "staff", id, sets(&body, PERSON, 2)?, "", "role IN ('owner', 'therapist')").await?;
    if !ok {
        return Err(AppError::NotFound);
    }
    auth::log(&s.pool, Some(a.staff.id), "website.team_changed", Some(("staff", id.to_string())), &a.device, &changed(&body, PERSON)).await;
    Ok(Json(json!({ "ok": true })))
}

/// Owner: show or hide one price-list item on the site.
pub async fn put_product(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let fields = [("web_visible", K::Req("boolean"))];
    if !update_row(&s.pool, "products", id, sets(&body, &fields, 2)?, "", "").await? {
        return Err(AppError::NotFound);
    }
    auth::log(&s.pool, Some(a.staff.id), "website.prices_changed", None, &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}
