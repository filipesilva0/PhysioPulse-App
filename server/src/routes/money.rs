//! Phase 3: money — price list, packages and plans, payments in, invoice records, what's owed,
//! expenses, cash close, salaries, month close and the accountant export.
//!
//! Roles (claude/SECURITY.md, SPEC.md › Pages):
//! - Everyone: the price list, a patient's packages/payments/what they owe (reception takes payments
//!   at the desk), selling a package, recording a payment, adding the invoice number.
//! - Owner only: Finance (all payments, owed list, overview, expenses, cash close, salaries,
//!   month close, export), prices, voiding someone else's payment, waiving a charge.
//!
//! Payments are fiscal records: never deleted (database trigger), only voided with a reason, and
//! nothing in a closed month can change. Amounts are whole cents.

use super::schedule::tz;
use super::util::{changed, sets, update_row, K};
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use crate::export::{self, Line, Zip};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

fn owner(a: &Auth) -> ApiResult<()> {
    a.require(&["owner"])
}
fn subject(id: Uuid) -> Option<(&'static str, String)> {
    Some(("patient", id.to_string()))
}

pub const METHODS: &[&str] = &["cash", "mbway", "transfer", "card", "stripe"];
pub const EXPENSE_METHODS: &[&str] = &["cash", "mbway", "transfer", "card", "debit"];
pub const CATEGORIES: &[(&str, &str)] = &[
    ("rent", "Renda"), ("utilities", "Eletricidade, água e gás"), ("telecom", "Telecomunicações"), ("supplies", "Consumíveis clínicos"),
    ("equipment", "Equipamento"), ("software", "Software e subscrições"), ("insurance", "Seguros"), ("training", "Formação"),
    ("marketing", "Marketing"), ("accounting", "Contabilidade e serviços"), ("office", "Material de escritório"), ("bank", "Comissões bancárias"),
    ("taxes", "Impostos e taxas"), ("other", "Outras"),
];
const CLOSED: &str = "That month is closed. Reopen it in Finance › Month Close to change it.";
const BAD_MONTH: &str = "Choose a month like 2026-10.";
pub(crate) const MAX_FILE: usize = 10 * 1024 * 1024;

pub(crate) fn method_pt(m: &str) -> &'static str {
    match m { "cash" => "Numerário", "mbway" => "MB WAY", "transfer" => "Transferência", "card" => "Cartão (TPA)", "stripe" => "Stripe", "debit" => "Débito direto", _ => "Outro" }
}
pub(crate) fn category_pt(c: &str) -> &'static str {
    CATEGORIES.iter().find(|x| x.0 == c).map(|x| x.1).unwrap_or("Outras")
}
pub(crate) fn month_pt(m: u32) -> &'static str {
    ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"][(m.clamp(1, 12) - 1) as usize]
}
fn kind_pt(k: &str) -> &'static str {
    match k { "session" => "Sessão", "assessment" => "Avaliação inicial", "home" => "Domicílio", "class" => "Aula de grupo", _ => "Marcação" }
}

/// The VAT column of the accountant export. Unknown (walk-ins described in words) stays empty for the accountant to fill.
fn vat_pt(v: &Value) -> &'static str {
    match v.as_bool() { Some(true) => "Isento — artigo 9.º, n.º 1 do CIVA", Some(false) => "Sujeito a IVA (23 %)", None => "" }
}

/// "2026-10" → ("2026-10-01", year, month).
pub(crate) fn parse_month(s: &str) -> ApiResult<(String, i32, u32)> {
    let mut it = s.trim().splitn(2, '-');
    let y: i32 = it.next().and_then(|v| v.parse().ok()).ok_or(AppError::BadRequest(BAD_MONTH))?;
    let m: u32 = it.next().and_then(|v| v.parse().ok()).ok_or(AppError::BadRequest(BAD_MONTH))?;
    if !(2000..=2100).contains(&y) || !(1..=12).contains(&m) {
        return Err(AppError::BadRequest(BAD_MONTH));
    }
    Ok((format!("{y}-{m:02}-01"), y, m))
}

pub(crate) async fn today(s: &AppState) -> ApiResult<String> {
    let (d,): (String,) = sqlx::query_as("SELECT ((now() AT TIME ZONE $1)::date)::text").bind(tz()).fetch_one(&s.pool).await?;
    Ok(d)
}

async fn ensure_open<'e, E: sqlx::Executor<'e, Database = Postgres>>(ex: E, day: &str) -> ApiResult<()> {
    let (closed,): (bool,) = sqlx::query_as("SELECT month_is_closed($1::date)").bind(day).fetch_one(ex).await?;
    if closed { Err(AppError::Conflict(CLOSED)) } else { Ok(()) }
}

// ───────── price list ─────────

#[derive(Deserialize)]
pub struct ProductsQuery { all: Option<bool> }

/// Everyone: the price list (reception sells packages). `?all=true` also lists retired products.
pub async fn products(State(s): State<AppState>, _a: Auth, Query(q): Query<ProductsQuery>) -> ApiResult<Json<Value>> {
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(to_jsonb(p) - 'updated_by' ORDER BY p.active DESC, p.sort, p.name_pt), '[]') FROM products p WHERE p.active OR $1")
        .bind(q.all.unwrap_or(false)).fetch_one(&s.pool).await?;
    Ok(Json(v))
}

const PRODUCT_FIELDS: &[(&str, K)] = &[
    ("name_pt", K::Req("text")), ("name_en", K::Req("text")), ("price_cents", K::Req("int")), ("sessions", K::Opt("int")),
    ("valid_months", K::Opt("int")), ("shareable", K::Req("boolean")), ("instalments", K::Req("int")),
    ("includes_assessment", K::Req("boolean")), ("active", K::Req("boolean")), ("sort", K::Req("int")), ("applies_to", K::Req("text")),
    // VAT: exempt under art. 9.º, n.º 1 CIVA when therapeutic and given by a physiotherapist (LEGAL.md).
    ("vat_exempt", K::Req("boolean")),
];

/// Owner: add a product (package, plan or a visit price).
pub async fn create_product(State(s): State<AppState>, a: Auth, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let kind = body.get("kind").and_then(Value::as_str).unwrap_or("");
    if !["visit", "pack", "monthly"].contains(&kind) {
        return Err(AppError::BadRequest("Choose the type: visit, package or monthly plan."));
    }
    let name = |k: &str| body.get(k).and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
    if name("name_pt").is_empty() || name("name_en").is_empty() {
        return Err(AppError::BadRequest("Enter the name in Portuguese and in English."));
    }
    if !body.get("price_cents").is_some_and(|v| v.as_i64().is_some_and(|c| c >= 0)) {
        return Err(AppError::BadRequest("Enter the price."));
    }
    let visit_kind = body.get("visit_kind").and_then(Value::as_str);
    if kind == "visit" {
        let vk = visit_kind.filter(|v| ["session", "assessment", "home", "class"].contains(v)).ok_or(AppError::BadRequest("Choose which appointment this price is for."))?;
        let (taken,): (bool,) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM products WHERE kind = 'visit' AND active AND visit_kind = $1)").bind(vk).fetch_one(&s.pool).await?;
        if taken {
            return Err(AppError::Conflict("There’s already a price for this kind of appointment. Change that one instead."));
        }
    }
    let id = Uuid::new_v4();
    let code = format!("p-{}", &id.simple().to_string()[..8]);
    let mut tx = s.pool.begin().await?;
    // A package needs its number of sessions at insert (table rule); the rest is set by update_row.
    let sessions = body.get("sessions").and_then(Value::as_i64).map(|n| n as i32);
    sqlx::query("INSERT INTO products (id, code, kind, visit_kind, applies_to, name_pt, name_en, price_cents, sessions, updated_by, sort)
                 VALUES ($1, $2, $3, $4, CASE WHEN $4 = 'class' THEN 'class' ELSE 'session' END, '', '', 0, $6, $5, 1000)")
        .bind(id).bind(&code).bind(kind).bind(if kind == "visit" { visit_kind } else { None }).bind(a.staff.id).bind(sessions).execute(&mut *tx).await?;
    update_row(&mut *tx, "products", id, sets(&body, PRODUCT_FIELDS, 2)?, "", "").await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "product.created", None, &a.device, &code).await;
    Ok(Json(json!({ "id": id, "code": code })))
}

/// Owner: change a price, name, validity…, or retire a product (`active: false`). Packages already sold keep their price.
pub async fn update_product(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    if body.get("price_cents").is_some_and(|v| !v.as_i64().is_some_and(|c| c >= 0)) {
        return Err(AppError::BadRequest("Enter the price."));
    }
    let found = update_row(&s.pool, "products", id, sets(&body, PRODUCT_FIELDS, 2)?, &format!("updated_at = now(), updated_by = '{}'", a.staff.id), "").await?;
    if !found {
        return Err(AppError::NotFound);
    }
    auth::log(&s.pool, Some(a.staff.id), "product.updated", None, &a.device, &changed(&body, PRODUCT_FIELDS)).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── what an appointment uses and costs ─────────

/// The plan a done appointment (or class attendance) should use: active, in date, right kind, with a credit left.
/// `$1` patient, `$2` applies_to, `$3` local day, `$4` appointment kind, `$5` a specific plan or null,
/// `$6` this appointment (left out of the counts), `$7` time zone.
const PICK_PLAN: &str = "
    SELECT pp.id FROM patient_plans pp
    WHERE (pp.patient_id = $1 OR pp.shared_with = $1) AND pp.applies_to = $2 AND pp.status = 'active'
      AND pp.starts_on <= $3::date AND (pp.expires_on IS NULL OR pp.expires_on >= $3::date)
      AND ($4 <> 'assessment' OR (pp.includes_assessment AND NOT EXISTS (
            SELECT 1 FROM appointments x WHERE x.plan_id = pp.id AND x.kind = 'assessment' AND x.status = 'done' AND x.id <> $6)))
      AND ($4 <> 'home')
      AND ($5::uuid IS NULL OR pp.id = $5)
      AND (pp.sessions IS NULL OR pp.sessions > (
            (SELECT count(*) FROM appointments x WHERE x.plan_id = pp.id AND x.status = 'done' AND x.id <> $6
               AND (pp.kind = 'pack' OR to_char(x.starts_at AT TIME ZONE $7, 'YYYY-MM') = to_char($3::date, 'YYYY-MM')))
          + (SELECT count(*) FROM class_attendees ca JOIN appointments ap ON ap.id = ca.appointment_id
             WHERE ca.plan_id = pp.id AND ca.attended AND (ca.appointment_id, ca.patient_id) IS DISTINCT FROM ($6, $1)
               AND (pp.kind = 'pack' OR to_char(ap.starts_at AT TIME ZONE $7, 'YYYY-MM') = to_char($3::date, 'YYYY-MM')))))
    -- Monthly plans first (their credits lapse at the end of the month), then the package that expires soonest.
    ORDER BY (pp.kind = 'monthly') DESC, pp.expires_on NULLS LAST, pp.starts_on, pp.created_at LIMIT 1";

async fn pick_plan(tx: &mut Transaction<'_, Postgres>, patient: Uuid, applies: &str, day: &str, kind: &str, plan: Option<Uuid>, appt: Uuid) -> ApiResult<Option<Uuid>> {
    Ok(sqlx::query_scalar(PICK_PLAN)
        .bind(patient).bind(applies).bind(day).bind(kind).bind(plan).bind(appt).bind(tz())
        .fetch_optional(&mut **tx).await?)
}

async fn visit_price(tx: &mut Transaction<'_, Postgres>, kind: &str) -> ApiResult<Option<i32>> {
    Ok(sqlx::query_scalar("SELECT price_cents FROM products WHERE kind = 'visit' AND active AND visit_kind = $1")
        .bind(kind).fetch_optional(&mut **tx).await?)
}

/// A session/assessment/home visit became done (`auto`), or its package was changed by hand (`!auto`):
/// use a credit from a package or plan, else charge the visit price.
pub async fn apply_done(tx: &mut Transaction<'_, Postgres>, id: Uuid, auto: bool) -> ApiResult<()> {
    let row: Option<(String, Option<Uuid>, String, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT kind, patient_id, ((starts_at AT TIME ZONE $2)::date)::text, plan_id, status FROM appointments WHERE id = $1")
        .bind(id).bind(tz()).fetch_optional(&mut **tx).await?;
    let Some((kind, Some(patient), day, chosen, status)) = row else { return Ok(()) };
    if status != "done" || !matches!(kind.as_str(), "session" | "assessment" | "home") {
        return Ok(());
    }
    let mut plan = None;
    if let Some(p) = chosen {
        plan = pick_plan(tx, patient, "session", &day, &kind, Some(p), id).await?;
        if plan.is_none() && !auto {
            return Err(AppError::BadRequest("That package can’t be used for this appointment (used up, expired, or another kind)."));
        }
    }
    if plan.is_none() && auto {
        plan = pick_plan(tx, patient, "session", &day, &kind, None, id).await?;
    }
    let charge = if plan.is_some() { None } else { visit_price(tx, &kind).await? };
    sqlx::query("UPDATE appointments SET plan_id = $2, charge_cents = $3 WHERE id = $1").bind(id).bind(plan).bind(charge).execute(&mut **tx).await?;
    Ok(())
}

/// Status changed on these appointments: done → work out the charge; anything else → nothing is owed for them.
pub async fn on_status(tx: &mut Transaction<'_, Postgres>, ids: &[Uuid], status: &str) -> ApiResult<()> {
    if status == "done" {
        for id in ids {
            apply_done(tx, *id, true).await?;
        }
    } else {
        sqlx::query("UPDATE appointments SET charge_cents = NULL WHERE id = ANY($1)").bind(ids).execute(&mut **tx).await?;
    }
    Ok(())
}

/// Class attendance marked: attended → a class credit or the drop-in price; otherwise nothing owed.
pub async fn on_attended(tx: &mut Transaction<'_, Postgres>, appt: Uuid, patient: Uuid, attended: Option<bool>) -> ApiResult<()> {
    if attended != Some(true) {
        sqlx::query("UPDATE class_attendees SET charge_cents = NULL WHERE appointment_id = $1 AND patient_id = $2").bind(appt).bind(patient).execute(&mut **tx).await?;
        return Ok(());
    }
    let day: String = sqlx::query_scalar("SELECT ((starts_at AT TIME ZONE $2)::date)::text FROM appointments WHERE id = $1").bind(appt).bind(tz()).fetch_one(&mut **tx).await?;
    let chosen: Option<Uuid> = sqlx::query_scalar("SELECT plan_id FROM class_attendees WHERE appointment_id = $1 AND patient_id = $2").bind(appt).bind(patient).fetch_one(&mut **tx).await?;
    let mut plan = None;
    if let Some(p) = chosen {
        plan = pick_plan(tx, patient, "class", &day, "class", Some(p), appt).await?;
    }
    if plan.is_none() {
        plan = pick_plan(tx, patient, "class", &day, "class", None, appt).await?;
    }
    let charge = if plan.is_some() { None } else { visit_price(tx, "class").await? };
    sqlx::query("UPDATE class_attendees SET plan_id = $3, charge_cents = $4 WHERE appointment_id = $1 AND patient_id = $2")
        .bind(appt).bind(patient).bind(plan).bind(charge).execute(&mut **tx).await?;
    Ok(())
}

// ───────── what's owed ─────────

/// Every unpaid item: done visits with a charge, packages not fully paid, monthly plan months not paid.
/// `$1` one patient or null for everyone, `$2` time zone.
const OWED_SQL: &str = "
    WITH paid_ap AS (
        SELECT appointment_id, patient_id, sum(amount_cents) AS s FROM payments
        WHERE voided_at IS NULL AND appointment_id IS NOT NULL GROUP BY 1, 2),
    cur AS (SELECT date_trunc('month', now() AT TIME ZONE $2)::date AS cm),
    items AS (
        SELECT 'appointment' AS type, ap.patient_id, ap.id AS appointment_id, NULL::uuid AS plan_id, NULL::date AS month,
               (ap.starts_at AT TIME ZONE $2)::date AS day, ap.kind AS what, NULL::text AS name_pt, NULL::text AS name_en,
               ap.charge_cents::bigint AS price_cents, ap.charge_cents - COALESCE(pa.s, 0) AS due_cents
        FROM appointments ap LEFT JOIN paid_ap pa ON pa.appointment_id = ap.id AND pa.patient_id = ap.patient_id
        WHERE ap.status = 'done' AND ap.patient_id IS NOT NULL AND ap.charge_cents > 0 AND NOT ap.charge_waived
        UNION ALL
        SELECT 'appointment', ca.patient_id, ap.id, NULL, NULL, (ap.starts_at AT TIME ZONE $2)::date, 'class', ap.title, ap.title,
               ca.charge_cents, ca.charge_cents - COALESCE(pa.s, 0)
        FROM class_attendees ca JOIN appointments ap ON ap.id = ca.appointment_id
        LEFT JOIN paid_ap pa ON pa.appointment_id = ap.id AND pa.patient_id = ca.patient_id
        WHERE ca.attended AND ca.charge_cents > 0 AND NOT ca.charge_waived AND ap.status <> 'cancelled'
        UNION ALL
        SELECT 'plan', pp.patient_id, NULL, pp.id, NULL, pp.starts_on, pp.kind, pp.name_pt, pp.name_en, pp.price_cents,
               pp.price_cents - COALESCE((SELECT sum(amount_cents) FROM payments WHERE plan_id = pp.id AND voided_at IS NULL), 0)
        FROM patient_plans pp WHERE pp.kind = 'pack' AND pp.status <> 'cancelled'
        UNION ALL
        SELECT 'month', pp.patient_id, NULL, pp.id, g.mo::date, g.mo::date, pp.kind, pp.name_pt, pp.name_en, pp.price_cents,
               pp.price_cents - COALESCE((SELECT sum(amount_cents) FROM payments WHERE plan_id = pp.id AND covers_month = g.mo::date AND voided_at IS NULL), 0)
        FROM patient_plans pp CROSS JOIN cur
        CROSS JOIN LATERAL generate_series(date_trunc('month', pp.starts_on)::date,
                                           LEAST(COALESCE(date_trunc('month', pp.ended_on)::date, cur.cm), cur.cm), interval '1 month') AS g(mo)
        WHERE pp.kind = 'monthly' AND pp.status <> 'cancelled' AND NOT (g.mo::date = ANY(pp.skipped_months))
          AND NOT (pp.status = 'paused' AND g.mo::date = cur.cm))
    SELECT COALESCE(json_agg(json_build_object('type', i.type, 'patient_id', i.patient_id, 'patient_name', p.name, 'patient_number', p.number,
               'appointment_id', i.appointment_id, 'plan_id', i.plan_id, 'month', i.month, 'day', i.day, 'what', i.what,
               'name_pt', i.name_pt, 'name_en', i.name_en, 'price_cents', i.price_cents, 'due_cents', i.due_cents) ORDER BY i.day, p.name), '[]')
    FROM items i JOIN patients p ON p.id = i.patient_id
    WHERE i.due_cents > 0 AND ($1::uuid IS NULL OR i.patient_id = $1)";

pub(crate) async fn owed(s: &AppState, patient: Option<Uuid>) -> ApiResult<Value> {
    let (v,): (Value,) = sqlx::query_as(OWED_SQL).bind(patient).bind(tz()).fetch_one(&s.pool).await?;
    Ok(v)
}

/// What one patient owes, in cents (for the legal tools' confirmation).
pub(crate) async fn owed_for(s: &AppState, patient: Uuid) -> ApiResult<i64> {
    let v = owed(s, Some(patient)).await?;
    Ok(v.as_array().map(|v| v.iter().filter_map(|x| x["due_cents"].as_i64()).sum()).unwrap_or(0))
}

/// Owner: everything owed, by everyone.
pub async fn owed_all(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    owner(&a)?;
    Ok(Json(owed(&s, None).await?))
}

// ───────── a patient's money ─────────

/// `$1` one patient (owned or shared with them) or null, `$2` time zone.
const PLANS_SQL: &str = "
    SELECT COALESCE(json_agg(x ORDER BY (x.status IN ('active', 'paused')) DESC, x.starts_on DESC), '[]') FROM (
      SELECT pp.id, pp.patient_id, op.name AS patient_name, pp.product_id, pp.kind, pp.applies_to, pp.name_pt, pp.name_en, pp.price_cents,
             pp.sessions, pp.instalments, pp.shareable, pp.includes_assessment, pp.shared_with, sp.name AS shared_with_name,
             pp.starts_on, pp.expires_on, pp.status, pp.ended_on, pp.skipped_months, pp.note, pp.created_at,
             u.used, u.used_month, CASE WHEN pp.kind = 'pack' THEN pp.sessions - u.used END AS remaining,
             COALESCE(pd.paid, 0) AS paid_cents
      FROM patient_plans pp
      JOIN patients op ON op.id = pp.patient_id
      LEFT JOIN patients sp ON sp.id = pp.shared_with
      LEFT JOIN LATERAL (SELECT sum(amount_cents)::bigint AS paid FROM payments WHERE plan_id = pp.id AND voided_at IS NULL) pd ON TRUE
      CROSS JOIN LATERAL (
        SELECT count(*) AS used,
               count(*) FILTER (WHERE to_char(z.day, 'YYYY-MM') = to_char(now() AT TIME ZONE $2, 'YYYY-MM')) AS used_month
        FROM (SELECT (ap.starts_at AT TIME ZONE $2)::date AS day FROM appointments ap WHERE ap.plan_id = pp.id AND ap.status = 'done'
              UNION ALL
              SELECT (ap.starts_at AT TIME ZONE $2)::date FROM class_attendees ca JOIN appointments ap ON ap.id = ca.appointment_id
              WHERE ca.plan_id = pp.id AND ca.attended) z) u
      WHERE ($1::uuid IS NULL OR pp.patient_id = $1 OR pp.shared_with = $1)) x";

const PAYMENT_JSON: &str = "json_build_object('id', py.id, 'number', py.number, 'received_on', py.received_on, 'patient_id', py.patient_id,
      'patient_name', p.name, 'patient_number', p.number, 'patient_nif', p.nif, 'amount_cents', py.amount_cents, 'method', py.method,
      'description', py.description, 'appointment_id', py.appointment_id, 'plan_id', py.plan_id, 'covers_month', py.covers_month,
      'invoice_number', py.invoice_number, 'invoice_file_id', py.invoice_file_id, 'invoiced_at', py.invoiced_at, 'note', py.note,
      'created_by', py.created_by, 'created_by_name', st.name, 'created_at', py.created_at, 'voided_at', py.voided_at, 'void_reason', py.void_reason)";

/// Everyone: a patient's packages and plans, payments and what they owe (the Payments tab).
/// Not clinical, but it's that person's file, so the read is logged.
pub async fn patient_money(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>) -> ApiResult<Json<Value>> {
    let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM patients WHERE id = $1").bind(pid).fetch_optional(&s.pool).await?;
    exists.ok_or(AppError::NotFound)?;
    let (plans,): (Value,) = sqlx::query_as(PLANS_SQL).bind(pid).bind(tz()).fetch_one(&s.pool).await?;
    let (payments,): (Value,) = sqlx::query_as(&format!(
        "SELECT COALESCE(json_agg({PAYMENT_JSON} ORDER BY py.received_on DESC, py.number DESC), '[]')
         FROM payments py LEFT JOIN patients p ON p.id = py.patient_id LEFT JOIN staff st ON st.id = py.created_by
         WHERE py.patient_id = $1")).bind(pid).fetch_one(&s.pool).await?;
    let owed = owed(&s, Some(pid)).await?;
    let due: i64 = owed.as_array().map(|v| v.iter().filter_map(|x| x["due_cents"].as_i64()).sum()).unwrap_or(0);
    auth::log(&s.pool, Some(a.staff.id), "patient.viewed", subject(pid), &a.device, "money").await;
    Ok(Json(json!({ "plans": plans, "payments": payments, "owed": owed, "due_cents": due })))
}

#[derive(Deserialize)]
pub struct SellPlan { product_id: Uuid, starts_on: Option<String>, shared_with: Option<Uuid>, #[serde(default)] note: String }

/// Everyone: sell a package or monthly plan to a patient (payment is recorded separately, in full or in instalments).
pub async fn sell_plan(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Json(r): Json<SellPlan>) -> ApiResult<Json<Value>> {
    let ok: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM patients WHERE id = $1 AND anonymised_at IS NULL AND active").bind(pid).fetch_optional(&s.pool).await?;
    ok.ok_or(AppError::NotFound)?;
    let p: Option<(String, bool, bool)> = sqlx::query_as("SELECT kind, active, shareable FROM products WHERE id = $1").bind(r.product_id).fetch_optional(&s.pool).await?;
    let Some((kind, active, shareable)) = p else { return Err(AppError::BadRequest("Choose a package or monthly plan.")) };
    if !active || kind == "visit" {
        return Err(AppError::BadRequest("Choose a package or monthly plan."));
    }
    if let Some(sw) = r.shared_with {
        if !shareable {
            return Err(AppError::BadRequest("This package can’t be shared."));
        }
        if sw == pid {
            return Err(AppError::BadRequest("Choose someone else to share it with."));
        }
    }
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO patient_plans (id, patient_id, product_id, kind, applies_to, name_pt, name_en, price_cents, sessions, instalments,
                                    shareable, includes_assessment, shared_with, starts_on, expires_on, note, created_by, vat_exempt)
         SELECT $1, $2, pr.id, pr.kind, pr.applies_to, pr.name_pt, pr.name_en, pr.price_cents, pr.sessions, pr.instalments,
                pr.shareable, pr.includes_assessment, $4, d.s,
                CASE WHEN pr.kind = 'pack' AND pr.valid_months IS NOT NULL THEN (d.s + make_interval(months => pr.valid_months))::date - 1 END,
                $5, $6, pr.vat_exempt
         FROM products pr, (SELECT COALESCE(NULLIF($3, '')::date, (now() AT TIME ZONE $7)::date) AS s) d WHERE pr.id = $8")
        .bind(id).bind(pid).bind(r.starts_on.unwrap_or_default()).bind(r.shared_with).bind(r.note.trim()).bind(a.staff.id).bind(tz()).bind(r.product_id)
        .execute(&mut *tx).await?;
    // Backdated sale: unpaid done visits since the start date use the new credits (oldest first).
    let past: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT ap.id FROM appointments ap, patient_plans pp
         WHERE pp.id = $2 AND ap.patient_id = $1 AND ap.status = 'done' AND ap.plan_id IS NULL AND ap.charge_cents > 0 AND NOT ap.charge_waived
           AND ap.kind IN ('session', 'assessment') AND (ap.starts_at AT TIME ZONE $3)::date >= pp.starts_on
           AND NOT EXISTS (SELECT 1 FROM payments py WHERE py.appointment_id = ap.id AND py.voided_at IS NULL)
         ORDER BY ap.starts_at")
        .bind(pid).bind(id).bind(tz()).fetch_all(&mut *tx).await?;
    let mut attached = 0;
    for (ap,) in past {
        let (kind, day): (String, String) = sqlx::query_as("SELECT kind, ((starts_at AT TIME ZONE $2)::date)::text FROM appointments WHERE id = $1")
            .bind(ap).bind(tz()).fetch_one(&mut *tx).await?;
        if pick_plan(&mut tx, pid, "session", &day, &kind, Some(id), ap).await?.is_some() {
            sqlx::query("UPDATE appointments SET plan_id = $2, charge_cents = NULL WHERE id = $1").bind(ap).bind(id).execute(&mut *tx).await?;
            attached += 1;
        }
    }
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "plan.sold", subject(pid), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "id": id, "attached": attached })))
}

/// Everyone: pause/resume a monthly plan, end a plan, cancel one sold by mistake, share it, add a note.
/// Owner only: change the expiry date.
pub async fn update_plan(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    let row: Option<(Uuid, String, String, bool)> = sqlx::query_as("SELECT patient_id, kind, status, shareable FROM patient_plans WHERE id = $1 FOR UPDATE")
        .bind(id).fetch_optional(&mut *tx).await?;
    let Some((pid, kind, status, shareable)) = row else { return Err(AppError::NotFound) };
    if status == "cancelled" {
        return Err(AppError::Conflict("This package was cancelled."));
    }
    let cur = "date_trunc('month', now() AT TIME ZONE $2)::date";
    if let Some(st) = body.get("status").and_then(Value::as_str) {
        match st {
            "paused" => {
                if kind != "monthly" {
                    return Err(AppError::BadRequest("Only monthly plans can be paused. End a package instead."));
                }
                // This month isn't charged, unless it's already paid.
                sqlx::query(&format!(
                    "UPDATE patient_plans SET status = 'paused',
                       skipped_months = CASE WHEN {cur} = ANY(skipped_months) OR EXISTS (SELECT 1 FROM payments WHERE plan_id = $1 AND covers_month = {cur} AND voided_at IS NULL)
                                             THEN skipped_months ELSE array_append(skipped_months, {cur}) END
                     WHERE id = $1")).bind(id).bind(tz()).execute(&mut *tx).await?;
            }
            "active" => {
                sqlx::query(&format!("UPDATE patient_plans SET status = 'active', ended_on = NULL, skipped_months = array_remove(skipped_months, {cur}) WHERE id = $1"))
                    .bind(id).bind(tz()).execute(&mut *tx).await?;
            }
            "ended" => {
                sqlx::query("UPDATE patient_plans SET status = 'ended', ended_on = (now() AT TIME ZONE $2)::date WHERE id = $1").bind(id).bind(tz()).execute(&mut *tx).await?;
            }
            "cancelled" => {
                let (used,): (bool,) = sqlx::query_as(
                    "SELECT EXISTS (SELECT 1 FROM payments WHERE plan_id = $1 AND voided_at IS NULL)
                         OR EXISTS (SELECT 1 FROM appointments WHERE plan_id = $1 AND status = 'done')
                         OR EXISTS (SELECT 1 FROM class_attendees WHERE plan_id = $1 AND attended)").bind(id).fetch_one(&mut *tx).await?;
                if used {
                    return Err(AppError::Conflict("This package has been used or paid, so it can’t be cancelled. End it instead."));
                }
                sqlx::query("UPDATE patient_plans SET status = 'cancelled' WHERE id = $1").bind(id).execute(&mut *tx).await?;
                sqlx::query("UPDATE appointments SET plan_id = NULL WHERE plan_id = $1").bind(id).execute(&mut *tx).await?;
            }
            _ => return Err(AppError::BadRequest("Choose a status.")),
        }
    }
    if let Some(sw) = body.get("shared_with") {
        let sw: Option<Uuid> = if sw.is_null() || sw.as_str() == Some("") { None } else { Some(serde_json::from_value(sw.clone()).map_err(|_| AppError::BadRequest("Choose who to share it with."))?) };
        if sw.is_some() && !shareable {
            return Err(AppError::BadRequest("This package can’t be shared."));
        }
        if sw == Some(pid) {
            return Err(AppError::BadRequest("Choose someone else to share it with."));
        }
        sqlx::query("UPDATE patient_plans SET shared_with = $2 WHERE id = $1").bind(id).bind(sw).execute(&mut *tx).await?;
    }
    if body.contains_key("expires_on") {
        owner(&a)?;
    }
    update_row(&mut *tx, "patient_plans", id, sets(&body, &[("note", K::Text), ("expires_on", K::Opt("date"))], 2)?, "", "").await?;
    tx.commit().await?;
    let fields: Vec<&str> = ["status", "shared_with", "note", "expires_on"].into_iter().filter(|k| body.contains_key(*k)).collect();
    auth::log(&s.pool, Some(a.staff.id), "plan.updated", subject(pid), &a.device, &fields.join(",")).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── payments ─────────

#[derive(Deserialize)]
pub struct PayItem { amount_cents: i64, #[serde(default)] description: String, appointment_id: Option<Uuid>, plan_id: Option<Uuid>, covers_month: Option<String> }

#[derive(Deserialize)]
pub struct NewPayment {
    patient_id: Option<Uuid>,
    received_on: Option<String>,
    method: String,
    items: Vec<PayItem>,
    #[serde(default)] invoice_number: String,
    #[serde(default)] note: String,
}

/// Everyone: record money received (one row per thing paid for). Stripe payments will record themselves later.
pub async fn record_payment(State(s): State<AppState>, a: Auth, Json(r): Json<NewPayment>) -> ApiResult<Json<Value>> {
    if r.items.is_empty() || r.items.len() > 30 {
        return Err(AppError::BadRequest("Add what is being paid."));
    }
    if !METHODS.contains(&r.method.as_str()) || r.method == "stripe" {
        return Err(AppError::BadRequest("Choose how it was paid: cash, MB WAY, transfer or card."));
    }
    let today = today(&s).await?;
    let day = r.received_on.clone().filter(|d| !d.trim().is_empty()).unwrap_or_else(|| today.clone());
    let (future, too_old): (bool, bool) = sqlx::query_as("SELECT $1::date > $2::date, $1::date < $2::date - 400").bind(&day).bind(&today).fetch_one(&s.pool).await?;
    if future {
        return Err(AppError::BadRequest("The payment date can’t be in the future."));
    }
    if too_old {
        return Err(AppError::BadRequest("Check the payment date."));
    }
    if r.patient_id.is_some() {
        let ok: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM patients WHERE id = $1 AND anonymised_at IS NULL").bind(r.patient_id).fetch_optional(&s.pool).await?;
        ok.ok_or(AppError::BadRequest("Choose the patient."))?;
    }
    let mut tx = s.pool.begin().await?;
    ensure_open(&mut *tx, &day).await?;
    let mut ids = Vec::new();
    let mut numbers = Vec::new();
    for it in &r.items {
        if it.amount_cents <= 0 || it.amount_cents > 1_000_000 {
            return Err(AppError::BadRequest("Check the amount."));
        }
        let mut description = it.description.trim().to_string();
        let mut covers: Option<String> = None;
        if let Some(ap) = it.appointment_id {
            let pid = r.patient_id.ok_or(AppError::BadRequest("Choose the patient."))?;
            let row: Option<(String, String)> = sqlx::query_as(
                "SELECT kind, to_char(starts_at AT TIME ZONE $3, 'DD/MM/YYYY') FROM appointments
                 WHERE id = $1 AND (patient_id = $2 OR EXISTS (SELECT 1 FROM class_attendees WHERE appointment_id = $1 AND patient_id = $2))")
                .bind(ap).bind(pid).bind(tz()).fetch_optional(&mut *tx).await?;
            let (kind, date) = row.ok_or(AppError::BadRequest("That appointment isn’t this patient’s."))?;
            if description.is_empty() {
                description = format!("{} {date}", kind_pt(&kind));
            }
        }
        if let Some(pl) = it.plan_id {
            let pid = r.patient_id.ok_or(AppError::BadRequest("Choose the patient."))?;
            let row: Option<(Uuid, String, String, String)> = sqlx::query_as("SELECT patient_id, kind, name_pt, status FROM patient_plans WHERE id = $1")
                .bind(pl).fetch_optional(&mut *tx).await?;
            let (owner_id, kind, name, status) = row.ok_or(AppError::BadRequest("Choose the package."))?;
            if owner_id != pid {
                return Err(AppError::BadRequest("That package belongs to another patient."));
            }
            if status == "cancelled" {
                return Err(AppError::BadRequest("That package was cancelled."));
            }
            if kind == "monthly" {
                let m = it.covers_month.clone().unwrap_or_else(|| format!("{}-01", &today[..7]));
                let (first, y, mo) = parse_month(&m[..7.min(m.len())])?;
                if description.is_empty() {
                    description = format!("{name} — {} de {y}", month_pt(mo));
                }
                covers = Some(first);
            } else if description.is_empty() {
                description = name;
            }
        }
        if description.is_empty() {
            return Err(AppError::BadRequest("Describe what is being paid."));
        }
        let (id, number): (Uuid, i32) = sqlx::query_as(
            "INSERT INTO payments (id, received_on, patient_id, amount_cents, method, description, appointment_id, plan_id, covers_month,
                                   invoice_number, invoiced_at, note, created_by)
             VALUES ($1, $2::date, $3, $4, $5, $6, $7, $8, $9::date, $10, CASE WHEN $10 <> '' THEN now() END, $11, $12) RETURNING id, number")
            .bind(Uuid::new_v4()).bind(&day).bind(r.patient_id).bind(it.amount_cents as i32).bind(&r.method).bind(&description)
            .bind(it.appointment_id).bind(it.plan_id).bind(covers).bind(r.invoice_number.trim()).bind(r.note.trim()).bind(a.staff.id)
            .fetch_one(&mut *tx).await?;
        ids.push(id);
        numbers.push(number);
    }
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "payment.recorded", r.patient_id.and_then(subject), &a.device, &format!("{} x{}", r.method, ids.len())).await;
    Ok(Json(json!({ "ids": ids, "numbers": numbers })))
}

#[derive(Deserialize)]
pub struct PaymentsQuery { from: Option<String>, to: Option<String>, method: Option<String>, uninvoiced: Option<bool> }

/// Owner: payments between two dates (default: this month).
pub async fn list_payments(State(s): State<AppState>, a: Auth, Query(q): Query<PaymentsQuery>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (v,): (Value,) = sqlx::query_as(&format!(
        "WITH b AS (SELECT COALESCE(NULLIF($1, '')::date, date_trunc('month', now() AT TIME ZONE $5)::date) AS f,
                           COALESCE(NULLIF($2, '')::date, (now() AT TIME ZONE $5)::date) AS t)
         SELECT COALESCE(json_agg({PAYMENT_JSON} ORDER BY py.received_on DESC, py.number DESC), '[]')
         FROM payments py CROSS JOIN b LEFT JOIN patients p ON p.id = py.patient_id LEFT JOIN staff st ON st.id = py.created_by
         WHERE py.received_on BETWEEN b.f AND b.t AND (NULLIF($3, '') IS NULL OR py.method = $3)
           AND (NOT $4 OR (py.invoice_number = '' AND py.voided_at IS NULL))"))
        .bind(q.from.unwrap_or_default()).bind(q.to.unwrap_or_default()).bind(q.method.unwrap_or_default()).bind(q.uninvoiced.unwrap_or(false)).bind(tz())
        .fetch_one(&s.pool).await?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct PaymentPatch { invoice_number: Option<String>, invoice_file_id: Option<Uuid>, note: Option<String> }

/// Everyone: add the invoice number (and PDF) from the certified invoicing software, or a note.
pub async fn update_payment(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(r): Json<PaymentPatch>) -> ApiResult<Json<Value>> {
    let row: Option<(Option<Uuid>, String, bool)> = sqlx::query_as("SELECT patient_id, received_on::text, voided_at IS NOT NULL FROM payments WHERE id = $1")
        .bind(id).fetch_optional(&s.pool).await?;
    let Some((pid, day, voided)) = row else { return Err(AppError::NotFound) };
    if voided {
        return Err(AppError::Conflict("This payment was voided."));
    }
    ensure_open(&s.pool, &day).await?;
    if let Some(f) = r.invoice_file_id {
        let k: Option<(String,)> = sqlx::query_as("SELECT kind FROM finance_files WHERE id = $1").bind(f).fetch_optional(&s.pool).await?;
        if k.map(|x| x.0) != Some("invoice".into()) {
            return Err(AppError::BadRequest("Attach the invoice PDF again."));
        }
    }
    sqlx::query(
        "UPDATE payments SET invoice_number = COALESCE(btrim($2), invoice_number), invoice_file_id = COALESCE($3, invoice_file_id), note = COALESCE(btrim($4), note),
                invoiced_at = CASE WHEN COALESCE(btrim($2), invoice_number) <> '' THEN COALESCE(invoiced_at, now()) END
         WHERE id = $1")
        .bind(id).bind(r.invoice_number).bind(r.invoice_file_id).bind(r.note).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "payment.invoiced", pid.and_then(subject), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct VoidReq { #[serde(default)] reason: String }

/// Owner, or whoever recorded it on the same day: void a payment entered by mistake. It stays in the records.
pub async fn void_payment(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(r): Json<VoidReq>) -> ApiResult<Json<Value>> {
    if r.reason.trim().is_empty() {
        return Err(AppError::BadRequest("Say why it’s being voided."));
    }
    let row: Option<(Option<Uuid>, String, Option<Uuid>, bool, bool)> = sqlx::query_as(
        "SELECT patient_id, received_on::text, created_by, voided_at IS NOT NULL, (created_at AT TIME ZONE $2)::date = (now() AT TIME ZONE $2)::date
         FROM payments WHERE id = $1").bind(id).bind(tz()).fetch_optional(&s.pool).await?;
    let Some((pid, day, by, voided, same_day)) = row else { return Err(AppError::NotFound) };
    if a.staff.role != "owner" && !(by == Some(a.staff.id) && same_day) {
        return Err(AppError::Forbidden);
    }
    if voided {
        return Err(AppError::Conflict("This payment was already voided."));
    }
    ensure_open(&s.pool, &day).await?;
    sqlx::query("UPDATE payments SET voided_at = now(), voided_by = $2, void_reason = $3 WHERE id = $1")
        .bind(id).bind(a.staff.id).bind(r.reason.trim()).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "payment.voided", pid.and_then(subject), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── overview ─────────

#[derive(Deserialize)]
pub struct MonthQuery { month: Option<String> }

async fn month_or_current(s: &AppState, m: Option<String>) -> ApiResult<(String, i32, u32)> {
    match m.filter(|x| !x.is_empty()) {
        Some(m) => parse_month(&m),
        None => parse_month(&today(s).await?[..7]),
    }
}

/// Totals for one month: revenue, expenses, salaries, result, by method, uninvoiced.
pub(crate) async fn totals(s: &AppState, first: &str) -> ApiResult<Value> {
    let (v,): (Value,) = sqlx::query_as(
        "WITH b AS (SELECT $1::date AS f, ($1::date + interval '1 month')::date AS t)
         SELECT json_build_object(
           'revenue_cents', (SELECT COALESCE(sum(amount_cents), 0) FROM payments, b WHERE voided_at IS NULL AND received_on >= b.f AND received_on < b.t),
           'payments', (SELECT count(*) FROM payments, b WHERE voided_at IS NULL AND received_on >= b.f AND received_on < b.t),
           'expenses_cents', (SELECT COALESCE(sum(amount_cents), 0) FROM expenses, b WHERE spent_on >= b.f AND spent_on < b.t),
           'salaries_cents', (SELECT COALESCE(sum(gross_cents), 0) FROM salary_payments, b WHERE month = b.f),
           'employer_ss_cents', (SELECT COALESCE(sum(employer_ss_cents), 0) FROM salary_payments, b WHERE month = b.f),
           'by_method', (SELECT COALESCE(json_object_agg(method, c), '{}') FROM (
               SELECT method, sum(amount_cents) AS c FROM payments, b WHERE voided_at IS NULL AND received_on >= b.f AND received_on < b.t GROUP BY method) x),
           'expenses_by_category', (SELECT COALESCE(json_object_agg(category, c), '{}') FROM (
               SELECT category, sum(amount_cents) AS c FROM expenses, b WHERE spent_on >= b.f AND spent_on < b.t GROUP BY category) x),
           'uninvoiced', (SELECT count(*) FROM payments, b WHERE voided_at IS NULL AND invoice_number = '' AND received_on >= b.f AND received_on < b.t),
           'uninvoiced_cents', (SELECT COALESCE(sum(amount_cents), 0) FROM payments, b WHERE voided_at IS NULL AND invoice_number = '' AND received_on >= b.f AND received_on < b.t))")
        .bind(first).fetch_one(&s.pool).await?;
    let mut v = v;
    let r = v["revenue_cents"].as_i64().unwrap_or(0);
    // Salaries cost the gross pay plus the employer's Social Security (audit phase C).
    let out = v["expenses_cents"].as_i64().unwrap_or(0) + v["salaries_cents"].as_i64().unwrap_or(0) + v["employer_ss_cents"].as_i64().unwrap_or(0);
    v["result_cents"] = json!(r - out);
    Ok(v)
}

/// Owner: the Finance overview for a month (default this month), with the previous five for the chart.
pub async fn overview(State(s): State<AppState>, a: Auth, Query(q): Query<MonthQuery>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (first, _, _) = month_or_current(&s, q.month).await?;
    let mut t = totals(&s, &first).await?;
    let (prev,): (String,) = sqlx::query_as("SELECT (($1::date - interval '1 month')::date)::text").bind(&first).fetch_one(&s.pool).await?;
    let p = totals(&s, &prev).await?;
    let (series,): (Value,) = sqlx::query_as(
        "SELECT json_agg(json_build_object('month', to_char(m, 'YYYY-MM'),
            'revenue_cents', (SELECT COALESCE(sum(amount_cents), 0) FROM payments WHERE voided_at IS NULL AND received_on >= m AND received_on < m + interval '1 month'),
            'out_cents', (SELECT COALESCE(sum(amount_cents), 0) FROM expenses WHERE spent_on >= m AND spent_on < m + interval '1 month')
                       + (SELECT COALESCE(sum(gross_cents + employer_ss_cents), 0) FROM salary_payments WHERE month = m::date)) ORDER BY m)
         FROM generate_series($1::date - interval '5 months', $1::date, interval '1 month') m")
        .bind(&first).fetch_one(&s.pool).await?;
    let owed = owed(&s, None).await?;
    let list = owed.as_array().cloned().unwrap_or_default();
    let owed_total: i64 = list.iter().filter_map(|x| x["due_cents"].as_i64()).sum();
    let owed_patients = list.iter().filter_map(|x| x["patient_id"].as_str()).collect::<std::collections::HashSet<_>>().len();
    let closed: Option<(Value,)> = sqlx::query_as("SELECT json_build_object('closed_at', closed_at, 'closed_by_name', st.name) FROM month_closes mc LEFT JOIN staff st ON st.id = mc.closed_by WHERE month = $1::date")
        .bind(&first).fetch_optional(&s.pool).await?;
    t["month"] = json!(&first[..7]);
    t["previous"] = p;
    t["series"] = series;
    t["owed_cents"] = json!(owed_total);
    t["owed_patients"] = json!(owed_patients);
    t["closed"] = closed.map(|c| c.0).unwrap_or(Value::Null);
    Ok(Json(t))
}

/// Extra Today alerts: packages running out (everyone) and, for the owner, money owed and today's takings.
pub async fn alerts_extra(s: &AppState, a: &Auth) -> ApiResult<Value> {
    let (low,): (Value,) = sqlx::query_as(&format!(
        "SELECT COALESCE(json_agg(json_build_object('patient_id', x.patient_id, 'name', x.patient_name, 'plan_id', x.id, 'name_pt', x.name_pt,
                                                    'name_en', x.name_en, 'remaining', x.remaining, 'expires_on', x.expires_on)), '[]')
         FROM ({PLANS_SQL}) q, json_to_recordset(q.coalesce) AS x(id uuid, patient_id uuid, patient_name text, name_pt text, name_en text,
                                                                 kind text, status text, remaining int, expires_on date)
         WHERE x.kind = 'pack' AND x.status = 'active' AND x.remaining > 0
           AND (x.remaining <= 1 OR x.expires_on <= (now() AT TIME ZONE $2)::date + 14)
           AND EXISTS (SELECT 1 FROM patients p WHERE p.id = x.patient_id AND p.active)"))
        .bind(None::<Uuid>).bind(tz()).fetch_one(&s.pool).await?;
    let mut out = json!({ "plans_low": low, "money": null });
    if a.staff.role == "owner" {
        let owed = owed(s, None).await?;
        let list = owed.as_array().cloned().unwrap_or_default();
        let mut by: Vec<(String, String, i64)> = Vec::new();
        for x in &list {
            let pid = x["patient_id"].as_str().unwrap_or("").to_string();
            let d = x["due_cents"].as_i64().unwrap_or(0);
            match by.iter_mut().find(|b| b.0 == pid) {
                Some(b) => b.2 += d,
                None => by.push((pid, x["patient_name"].as_str().unwrap_or("").to_string(), d)),
            }
        }
        by.sort_by(|a, b| b.2.cmp(&a.2));
        let (today_cents,): (i64,) = sqlx::query_as("SELECT COALESCE(sum(amount_cents), 0)::bigint FROM payments WHERE voided_at IS NULL AND received_on = (now() AT TIME ZONE $1)::date")
            .bind(tz()).fetch_one(&s.pool).await?;
        out["money"] = json!({
            "owed_cents": by.iter().map(|b| b.2).sum::<i64>(),
            "owed": by.iter().take(5).map(|b| json!({ "patient_id": b.0, "name": b.1, "due_cents": b.2 })).collect::<Vec<_>>(),
            "owed_patients": by.len(),
            "today_cents": today_cents,
        });
    }
    Ok(out)
}

// ───────── expenses ─────────

const EXPENSE_FIELDS: &[(&str, K)] = &[
    ("spent_on", K::Req("date")), ("supplier", K::Text), ("supplier_nif", K::Text), ("category", K::Req("text")), ("description", K::Text),
    ("amount_cents", K::Req("int")), ("vat_cents", K::Opt("int")), ("method", K::Req("text")), ("document_ref", K::Text), ("file_id", K::Opt("uuid")), ("ai_read", K::Req("boolean")),
];

fn check_expense(body: &Map<String, Value>) -> ApiResult<()> {
    if let Some(c) = body.get("category") {
        if !c.as_str().is_some_and(|c| CATEGORIES.iter().any(|x| x.0 == c)) {
            return Err(AppError::BadRequest("Choose a category."));
        }
    }
    if let Some(m) = body.get("method") {
        if !m.as_str().is_some_and(|m| EXPENSE_METHODS.contains(&m)) {
            return Err(AppError::BadRequest("Choose how it was paid."));
        }
    }
    if body.get("amount_cents").is_some_and(|v| !v.as_i64().is_some_and(|c| c > 0 && c <= 10_000_000)) {
        return Err(AppError::BadRequest("Check the amount."));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct RangeQuery { from: Option<String>, to: Option<String> }

/// Owner: expenses between two dates (default: this month).
pub async fn list_expenses(State(s): State<AppState>, a: Auth, Query(q): Query<RangeQuery>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (v,): (Value,) = sqlx::query_as(
        "WITH b AS (SELECT COALESCE(NULLIF($1, '')::date, date_trunc('month', now() AT TIME ZONE $3)::date) AS f,
                           COALESCE(NULLIF($2, '')::date, (date_trunc('month', now() AT TIME ZONE $3) + interval '1 month - 1 day')::date) AS t)
         SELECT COALESCE(json_agg(json_build_object('id', e.id, 'spent_on', e.spent_on, 'supplier', e.supplier, 'supplier_nif', e.supplier_nif,
                  'category', e.category, 'description', e.description, 'amount_cents', e.amount_cents, 'vat_cents', e.vat_cents, 'method', e.method, 'ai_read', e.ai_read,
                  'document_ref', e.document_ref, 'file_id', e.file_id, 'filename', f.filename, 'created_at', e.created_at,
                  'closed', month_is_closed(e.spent_on)) ORDER BY e.spent_on DESC, e.created_at DESC), '[]')
         FROM expenses e CROSS JOIN b LEFT JOIN finance_files f ON f.id = e.file_id
         WHERE e.spent_on BETWEEN b.f AND b.t")
        .bind(q.from.unwrap_or_default()).bind(q.to.unwrap_or_default()).bind(tz()).fetch_one(&s.pool).await?;
    Ok(Json(v))
}

/// Owner: add an expense (receipt photo/PDF optional: upload it first with /finance/files).
pub async fn create_expense(State(s): State<AppState>, a: Auth, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    check_expense(&body)?;
    let day = body.get("spent_on").and_then(Value::as_str).unwrap_or("").to_string();
    let amount = body.get("amount_cents").and_then(Value::as_i64).unwrap_or(0);
    if day.is_empty() || amount <= 0 {
        return Err(AppError::BadRequest("Enter the date and the amount."));
    }
    let mut tx = s.pool.begin().await?;
    ensure_open(&mut *tx, &day).await?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO expenses (id, spent_on, amount_cents, created_by) VALUES ($1, $2::date, $3, $4)")
        .bind(id).bind(&day).bind(amount as i32).bind(a.staff.id).execute(&mut *tx).await?;
    update_row(&mut *tx, "expenses", id, sets(&body, EXPENSE_FIELDS, 2)?, "", "").await?;
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "expense.created", None, &a.device, "").await;
    Ok(Json(json!({ "id": id })))
}

pub async fn update_expense(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(body): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    check_expense(&body)?;
    let old: Option<(String,)> = sqlx::query_as("SELECT spent_on::text FROM expenses WHERE id = $1").bind(id).fetch_optional(&s.pool).await?;
    let (old,) = old.ok_or(AppError::NotFound)?;
    ensure_open(&s.pool, &old).await?;
    if let Some(d) = body.get("spent_on").and_then(Value::as_str) {
        ensure_open(&s.pool, d).await?;
    }
    update_row(&s.pool, "expenses", id, sets(&body, EXPENSE_FIELDS, 2)?, "", "").await?;
    auth::log(&s.pool, Some(a.staff.id), "expense.updated", None, &a.device, &changed(&body, EXPENSE_FIELDS)).await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_expense(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let old: Option<(String,)> = sqlx::query_as("SELECT spent_on::text FROM expenses WHERE id = $1").bind(id).fetch_optional(&s.pool).await?;
    let (old,) = old.ok_or(AppError::NotFound)?;
    ensure_open(&s.pool, &old).await?;
    sqlx::query("DELETE FROM expenses WHERE id = $1").bind(id).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "expense.deleted", None, &a.device, "").await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── files (invoice PDFs, receipts, exports) ─────────

#[derive(Deserialize)]
pub struct Upload { kind: String, filename: String, data: String, day: Option<String> }

pub(crate) fn sniff(b: &[u8]) -> Option<(&'static str, &'static str)> {
    if b.starts_with(b"%PDF") {
        Some(("application/pdf", "pdf"))
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("image/jpeg", "jpg"))
    } else if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(("image/png", "png"))
    } else if b.len() > 12 && &b[4..8] == b"ftyp" && (&b[8..12] == b"heic" || &b[8..12] == b"heix" || &b[8..12] == b"mif1") {
        Some(("image/heic", "heic"))
    } else if b.len() > 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

pub(crate) async fn save_file(s: &AppState, a: &Auth, kind: &str, filename: &str, day: &str, bytes: &[u8], mime: &str) -> ApiResult<(Uuid, String)> {
    let id = Uuid::new_v4();
    let (y, m) = (&day[..4], &day[5..7]);
    let sub = match kind { "invoice" => format!("Financas/{y}/{m}/Entradas"), "expense" => format!("Financas/{y}/{m}/Saidas"), "report" => format!("Financas/{y}/Relatorios"), _ => "Financas/Exportacoes".into() };
    let name = if kind == "export" { export::safe_name(filename) } else { format!("{}-{}", &id.simple().to_string()[..8], export::safe_name(filename)) };
    let rel = format!("{sub}/{name}");
    let full = s.archive.join(&rel);
    if let Some(dir) = full.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| AppError::Internal(format!("archive folder: {e}")))?;
    }
    tokio::fs::write(&full, bytes).await.map_err(|e| AppError::Internal(format!("archive write: {e}")))?;
    let sha = hex::encode(Sha256::digest(bytes));
    sqlx::query("INSERT INTO finance_files (id, kind, filename, mime, bytes, path, sha256, uploaded_by) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(id).bind(kind).bind(export::safe_name(filename)).bind(mime).bind(bytes.len() as i64).bind(&rel).bind(sha).bind(a.staff.id)
        .execute(&s.pool).await?;
    Ok((id, rel))
}

/// Upload an invoice PDF (everyone, from the certified software) or an expense receipt (owner).
/// Body is JSON with the file as base64 (`data`), at most 10 MB; PDF, JPEG, PNG, HEIC or WebP only.
pub async fn upload(State(s): State<AppState>, a: Auth, Json(u): Json<Upload>) -> ApiResult<Json<Value>> {
    match u.kind.as_str() {
        "invoice" => {}
        "expense" => owner(&a)?,
        _ => return Err(AppError::BadRequest("Choose what this file is.")),
    }
    let data = u.data.split_once("base64,").map(|x| x.1).unwrap_or(&u.data);
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|_| AppError::BadRequest("That file couldn’t be read. Try again."))?;
    if bytes.is_empty() || bytes.len() > MAX_FILE {
        return Err(AppError::BadRequest("Files must be under 10 MB."));
    }
    let (mime, _) = sniff(&bytes).ok_or(AppError::BadRequest("Use a PDF or a photo (JPEG, PNG, HEIC)."))?;
    let day = match u.day.filter(|d| d.len() >= 7) { Some(d) => d, None => today(&s).await? };
    parse_month(&day[..7])?;
    let (id, _) = save_file(&s, &a, &u.kind, &u.filename, &day, &bytes, mime).await?;
    auth::log(&s.pool, Some(a.staff.id), "finance.file_uploaded", None, &a.device, &u.kind).await;
    Ok(Json(json!({ "id": id, "filename": export::safe_name(&u.filename), "bytes": bytes.len(), "mime": mime })))
}

pub(crate) fn pct_encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The file itself (invoices: everyone; receipts and exports: owner). The desktop app saves it to Downloads.
pub async fn file_raw(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let row: Option<(String, String, String, String)> = sqlx::query_as("SELECT kind, filename, mime, path FROM finance_files WHERE id = $1")
        .bind(id).fetch_optional(&s.pool).await?;
    let (kind, filename, mime, path) = row.ok_or(AppError::NotFound)?;
    if kind != "invoice" {
        owner(&a)?;
    }
    if path.contains("..") {
        return Err(AppError::NotFound);
    }
    let bytes = tokio::fs::read(s.archive.join(&path)).await.map_err(|_| AppError::NotFound)?;
    auth::log(&s.pool, Some(a.staff.id), "finance.file_opened", None, &a.device, &kind).await;
    let mut res = bytes.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{}", pct_encode(&filename))) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(res)
}

// ───────── cash close ─────────

#[derive(Deserialize)]
pub struct DayQuery { day: Option<String> }

async fn expected(s: &AppState, day: &str) -> ApiResult<Value> {
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_object_agg(method, c), '{}') FROM (
           SELECT method, sum(amount_cents) AS c FROM payments WHERE voided_at IS NULL AND received_on = $1::date GROUP BY method) x")
        .bind(day).fetch_one(&s.pool).await?;
    Ok(v)
}

/// Owner: what the day's payments add up to, by method, and the close if it was done.
pub async fn cash_day(State(s): State<AppState>, a: Auth, Query(q): Query<DayQuery>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let day = match q.day.filter(|d| !d.is_empty()) { Some(d) => d, None => today(&s).await? };
    let exp = expected(&s, &day).await?;
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM payments WHERE voided_at IS NULL AND received_on = $1::date").bind(&day).fetch_one(&s.pool).await?;
    let close: Option<(Value,)> = sqlx::query_as(
        "SELECT json_build_object('counted', c.counted, 'expected', c.expected, 'difference_cents', c.difference_cents, 'note', c.note,
                                  'closed_at', c.closed_at, 'closed_by_name', st.name)
         FROM cash_closes c LEFT JOIN staff st ON st.id = c.closed_by WHERE c.day = $1::date").bind(&day).fetch_optional(&s.pool).await?;
    let (recent,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('day', day, 'difference_cents', difference_cents) ORDER BY day DESC), '[]')
         FROM (SELECT * FROM cash_closes ORDER BY day DESC LIMIT 10) c").fetch_one(&s.pool).await?;
    Ok(Json(json!({ "day": day, "expected": exp, "payments": count, "close": close.map(|c| c.0), "recent": recent })))
}

#[derive(Deserialize)]
pub struct CashClose { day: String, counted: Map<String, Value>, #[serde(default)] note: String }

/// Owner: close the day. Redoing it replaces the earlier close (until the month is closed).
pub async fn close_day(State(s): State<AppState>, a: Auth, Json(r): Json<CashClose>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    ensure_open(&s.pool, &r.day).await?;
    let mut counted = Map::new();
    for (k, v) in &r.counted {
        if !METHODS.contains(&k.as_str()) {
            continue;
        }
        let c = v.as_i64().filter(|c| *c >= 0).ok_or(AppError::BadRequest("Check the counted amounts."))?;
        counted.insert(k.clone(), json!(c));
    }
    let exp = expected(&s, &r.day).await?;
    let sum = |m: &Map<String, Value>| m.values().filter_map(Value::as_i64).sum::<i64>();
    let diff = sum(&counted) - exp.as_object().map(sum).unwrap_or(0);
    sqlx::query(
        "INSERT INTO cash_closes (day, expected, counted, difference_cents, note, closed_by) VALUES ($1::date, $2, $3, $4, $5, $6)
         ON CONFLICT (day) DO UPDATE SET expected = $2, counted = $3, difference_cents = $4, note = $5, closed_by = $6, closed_at = now()")
        .bind(&r.day).bind(&exp).bind(Value::Object(counted)).bind(diff as i32).bind(r.note.trim()).bind(a.staff.id)
        .execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "cash.closed", None, &a.device, &r.day).await;
    Ok(Json(json!({ "ok": true, "difference_cents": diff })))
}

// ───────── salaries ─────────

/// Owner: staff with their pay settings and what was paid in a month (default this month).
pub async fn salaries(State(s): State<AppState>, a: Auth, Query(q): Query<MonthQuery>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (first, _, _) = month_or_current(&s, q.month).await?;
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('staff_id', st.id, 'name', st.name, 'role', st.role, 'active', st.active,
                  'model', COALESCE(sp.model, 'fixed'), 'fixed_cents', COALESCE(sp.fixed_cents, 0),
                  'employer_ss_bp', sp.employer_ss_bp, 'effective_ss_bp', COALESCE(sp.employer_ss_bp, (SELECT employer_ss_bp FROM clinic WHERE id = 1), 2375),
                  'paid', (SELECT COALESCE(json_agg(json_build_object('id', x.id, 'gross_cents', x.gross_cents, 'employer_ss_cents', x.employer_ss_cents, 'paid_on', x.paid_on, 'method', x.method, 'note', x.note) ORDER BY x.paid_on), '[]')
                           FROM salary_payments x WHERE x.staff_id = st.id AND x.month = $1::date))
                ORDER BY st.role = 'owner', st.name), '[]')
         FROM staff st LEFT JOIN staff_pay sp ON sp.staff_id = st.id
         WHERE st.active OR EXISTS (SELECT 1 FROM salary_payments x WHERE x.staff_id = st.id AND x.month = $1::date)")
        .bind(&first).fetch_one(&s.pool).await?;
    let (closed,): (bool,) = sqlx::query_as("SELECT month_is_closed($1::date)").bind(&first).fetch_one(&s.pool).await?;
    let rate = clinic_ss_bp(&s).await?;
    Ok(Json(json!({ "month": &first[..7], "closed": closed, "employer_ss_bp": rate, "staff": v })))
}

async fn clinic_ss_bp(s: &AppState) -> ApiResult<i32> {
    let r: Option<(i32,)> = sqlx::query_as("SELECT employer_ss_bp FROM clinic WHERE id = 1").fetch_optional(&s.pool).await?;
    Ok(r.map(|x| x.0).unwrap_or(2375))
}

#[derive(Deserialize)]
pub struct SsRate { employer_ss_bp: i64 }

/// Owner: the clinic's employer Social Security rate, in basis points (2375 = 23,75 %). Applies to salaries
/// recorded from now on; amounts already recorded don't change.
pub async fn set_ss_rate(State(s): State<AppState>, a: Auth, Json(r): Json<SsRate>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    if !(0..=10_000).contains(&r.employer_ss_bp) {
        return Err(AppError::BadRequest("Use a rate between 0 and 100 %."));
    }
    let n = sqlx::query("UPDATE clinic SET employer_ss_bp = $1 WHERE id = 1").bind(r.employer_ss_bp as i32).execute(&s.pool).await?.rows_affected();
    if n == 0 { return Err(AppError::NotFound); }
    auth::log(&s.pool, Some(a.staff.id), "salary.rate_changed", None, &a.device, &r.employer_ss_bp.to_string()).await;
    Ok(Json(json!({ "ok": true, "employer_ss_bp": r.employer_ss_bp })))
}

#[derive(Deserialize)]
pub struct PayModel {
    model: String, fixed_cents: i64,
    /// Their own employer Social Security rate in basis points; absent or null = the clinic's rate.
    #[serde(default)] employer_ss_bp: Option<i64>,
}

/// Owner: someone's pay model. Fixed monthly pay for now; commission and room rental come later.
pub async fn set_pay(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(r): Json<PayModel>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    if r.model != "fixed" {
        return Err(AppError::BadRequest("Commission and room rental aren’t available yet. Use a fixed amount."));
    }
    if !(0..=10_000_000).contains(&r.fixed_cents) {
        return Err(AppError::BadRequest("Check the amount."));
    }
    if r.employer_ss_bp.is_some_and(|b| !(0..=10_000).contains(&b)) {
        return Err(AppError::BadRequest("Use a rate between 0 and 100 %."));
    }
    let n = sqlx::query(
        "INSERT INTO staff_pay (staff_id, model, fixed_cents, employer_ss_bp) SELECT id, $2, $3, $4 FROM staff WHERE id = $1
         ON CONFLICT (staff_id) DO UPDATE SET model = $2, fixed_cents = $3, employer_ss_bp = $4, updated_at = now()")
        .bind(id).bind(&r.model).bind(r.fixed_cents as i32).bind(r.employer_ss_bp.map(|b| b as i32)).execute(&s.pool).await?.rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    auth::log(&s.pool, Some(a.staff.id), "staff.pay_changed", Some(("staff", id.to_string())), &a.device, &r.model).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct SalaryPay {
    staff_id: Uuid, month: String, gross_cents: i64, paid_on: Option<String>, #[serde(default = "transfer")] method: String, #[serde(default)] note: String,
    /// The employer's Social Security for this payment. Absent = worked out from their rate (or the clinic's).
    #[serde(default)] employer_ss_cents: Option<i64>,
}
fn transfer() -> String { "transfer".into() }

/// Owner: record a salary paid for a month.
pub async fn pay_salary(State(s): State<AppState>, a: Auth, Json(r): Json<SalaryPay>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (first, _, _) = parse_month(&r.month)?;
    if r.gross_cents <= 0 || r.gross_cents > 10_000_000 {
        return Err(AppError::BadRequest("Check the amount."));
    }
    if !["cash", "transfer"].contains(&r.method.as_str()) {
        return Err(AppError::BadRequest("Choose how it was paid."));
    }
    ensure_open(&s.pool, &first).await?;
    let ss = match r.employer_ss_cents {
        Some(c) if !(0..=10_000_000).contains(&c) => return Err(AppError::BadRequest("Check the Social Security amount.")),
        Some(c) => c,
        None => {
            let own: Option<(Option<i32>,)> = sqlx::query_as("SELECT employer_ss_bp FROM staff_pay WHERE staff_id = $1").bind(r.staff_id).fetch_optional(&s.pool).await?;
            let bp = match own.and_then(|x| x.0) { Some(b) => b, None => clinic_ss_bp(&s).await? };
            employer_ss(r.gross_cents, bp as i64)
        }
    };
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO salary_payments (id, staff_id, month, gross_cents, paid_on, method, note, created_by, employer_ss_cents)
                 VALUES ($1, $2, $3::date, $4, COALESCE(NULLIF($5, '')::date, (now() AT TIME ZONE $9)::date), $6, $7, $8, $10)")
        .bind(id).bind(r.staff_id).bind(&first).bind(r.gross_cents as i32).bind(r.paid_on.unwrap_or_default()).bind(&r.method).bind(r.note.trim()).bind(a.staff.id).bind(tz()).bind(ss as i32)
        .execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "salary.paid", Some(("staff", r.staff_id.to_string())), &a.device, &first[..7]).await;
    Ok(Json(json!({ "id": id, "employer_ss_cents": ss })))
}

/// The employer's contribution on a gross salary: `bp` basis points, rounded half up to the cent.
pub fn employer_ss(gross_cents: i64, bp: i64) -> i64 {
    (gross_cents * bp + 5_000) / 10_000
}

pub async fn delete_salary(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let row: Option<(String, Uuid)> = sqlx::query_as("SELECT month::text, staff_id FROM salary_payments WHERE id = $1").bind(id).fetch_optional(&s.pool).await?;
    let (m, sid) = row.ok_or(AppError::NotFound)?;
    ensure_open(&s.pool, &m).await?;
    sqlx::query("DELETE FROM salary_payments WHERE id = $1").bind(id).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "salary.deleted", Some(("staff", sid.to_string())), &a.device, &m[..7]).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── month close ─────────

/// Owner: closed months, newest first.
pub async fn months(State(s): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (v,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('month', to_char(mc.month, 'YYYY-MM'), 'totals', mc.totals, 'closed_at', mc.closed_at, 'closed_by_name', st.name)
                 ORDER BY mc.month DESC), '[]') FROM month_closes mc LEFT JOIN staff st ON st.id = mc.closed_by").fetch_one(&s.pool).await?;
    Ok(Json(v))
}

/// Owner: close a finished month. Its payments, expenses and salaries are then locked.
pub async fn close_month(State(s): State<AppState>, a: Auth, Path(month): Path<String>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (first, _, _) = parse_month(&month)?;
    let (ended,): (bool,) = sqlx::query_as("SELECT $1::date < date_trunc('month', now() AT TIME ZONE $2)::date").bind(&first).bind(tz()).fetch_one(&s.pool).await?;
    if !ended {
        return Err(AppError::BadRequest("A month can be closed once it has ended."));
    }
    let t = totals(&s, &first).await?;
    let n = sqlx::query("INSERT INTO month_closes (month, totals, closed_by) VALUES ($1::date, $2, $3) ON CONFLICT DO NOTHING")
        .bind(&first).bind(&t).bind(a.staff.id).execute(&s.pool).await?.rows_affected();
    if n == 0 {
        return Err(AppError::Conflict("That month is already closed."));
    }
    auth::log(&s.pool, Some(a.staff.id), "month.closed", None, &a.device, &first[..7]).await;
    Ok(Json(json!({ "ok": true, "totals": t })))
}

/// Owner: reopen a closed month to fix something (logged).
pub async fn reopen_month(State(s): State<AppState>, a: Auth, Path(month): Path<String>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (first, _, _) = parse_month(&month)?;
    let n = sqlx::query("DELETE FROM month_closes WHERE month = $1::date").bind(&first).execute(&s.pool).await?.rows_affected();
    if n == 0 {
        return Err(AppError::Conflict("That month isn’t closed."));
    }
    auth::log(&s.pool, Some(a.staff.id), "month.reopened", None, &a.device, &first[..7]).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── accountant export ─────────

#[derive(Deserialize)]
pub struct ExportReq { from: String, to: String, #[serde(default = "yes")] include_files: bool, #[serde(default = "yes")] include_pdf: bool }
fn yes() -> bool { true }

fn s_(v: &Value) -> String {
    match v { Value::Null => String::new(), Value::String(s) => s.clone(), other => other.to_string() }
}
fn i_(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}
fn date_pt(v: &Value) -> String {
    let s = s_(v);
    if s.len() >= 10 { format!("{}/{}/{}", &s[8..10], &s[5..7], &s[..4]) } else { s }
}

/// Owner: one zip with everything in and out for a month or a range of months (both inclusive), sorted into folders.
/// Saved in the archive (PhysioPulse/Financas/Exportacoes) and returned as a file id for download.
pub async fn export_zip(State(s): State<AppState>, a: Auth, Json(r): Json<ExportReq>) -> ApiResult<Json<Value>> {
    owner(&a)?;
    let (f, fy, fm) = parse_month(&r.from)?;
    let (t, ty, tm) = parse_month(&r.to)?;
    if (ty, tm) < (fy, fm) || (ty - fy) * 12 + tm as i32 - fm as i32 > 23 {
        return Err(AppError::BadRequest("Choose up to two years, with the start before the end."));
    }
    let label = if f == t { f[..7].to_string() } else { format!("{}_a_{}", &f[..7], &t[..7]) };
    let root = format!("Exportacao_{label}");
    let (end,): (String,) = sqlx::query_as("SELECT (($1::date + interval '1 month')::date)::text").bind(&t).fetch_one(&s.pool).await?;

    // Data
    let (pays,): (Value,) = sqlx::query_as(&format!(
        "SELECT COALESCE(json_agg({PAYMENT_JSON} || jsonb_build_object('invoice_path', fi.path, 'invoice_filename', fi.filename,
                 -- VAT as the price list says: the plan's (copied at sale), else the visit price for that kind of appointment.
                 'vat_exempt', COALESCE((SELECT pp.vat_exempt FROM patient_plans pp WHERE pp.id = py.plan_id),
                                        (SELECT pr.vat_exempt FROM appointments ap JOIN products pr ON pr.kind = 'visit' AND pr.active AND pr.visit_kind = ap.kind
                                         WHERE ap.id = py.appointment_id)))::jsonb ORDER BY py.received_on, py.number), '[]')
         FROM payments py LEFT JOIN patients p ON p.id = py.patient_id LEFT JOIN staff st ON st.id = py.created_by
         LEFT JOIN finance_files fi ON fi.id = py.invoice_file_id
         WHERE py.received_on >= $1::date AND py.received_on < $2::date").replace("json_build_object('id', py.id", "jsonb_build_object('id', py.id"))
        .bind(&f).bind(&end).fetch_one(&s.pool).await?;
    let (exps,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('spent_on', e.spent_on, 'supplier', e.supplier, 'supplier_nif', e.supplier_nif, 'category', e.category,
                 'description', e.description, 'amount_cents', e.amount_cents, 'vat_cents', e.vat_cents, 'method', e.method, 'document_ref', e.document_ref,
                 'path', fi.path, 'filename', fi.filename) ORDER BY e.spent_on, e.created_at), '[]')
         FROM expenses e LEFT JOIN finance_files fi ON fi.id = e.file_id WHERE e.spent_on >= $1::date AND e.spent_on < $2::date")
        .bind(&f).bind(&end).fetch_one(&s.pool).await?;
    let (sals,): (Value,) = sqlx::query_as(
        "SELECT COALESCE(json_agg(json_build_object('month', to_char(x.month, 'YYYY-MM'), 'name', st.name, 'role', st.role, 'gross_cents', x.gross_cents, 'employer_ss_cents', x.employer_ss_cents,
                 'paid_on', x.paid_on, 'method', x.method, 'note', x.note) ORDER BY x.month, st.name), '[]')
         FROM salary_payments x JOIN staff st ON st.id = x.staff_id WHERE x.month >= $1::date AND x.month < $2::date")
        .bind(&f).bind(&end).fetch_one(&s.pool).await?;
    let (clinic,): (String,) = sqlx::query_as("SELECT COALESCE((SELECT name FROM clinic LIMIT 1), 'PhysioPulse')").fetch_one(&s.pool).await?;
    let (now_pt,): (String,) = sqlx::query_as("SELECT to_char(now() AT TIME ZONE $1, 'DD/MM/YYYY HH24:MI')").bind(tz()).fetch_one(&s.pool).await?;
    let month_list: Vec<(String,)> = sqlx::query_as("SELECT to_char(m, 'YYYY-MM-DD') FROM generate_series($1::date, $2::date, interval '1 month') m").bind(&f).bind(&t).fetch_all(&s.pool).await?;
    let mut per_month = Vec::new();
    for (m,) in &month_list {
        let tot = totals(&s, m).await?;
        let (closed,): (bool,) = sqlx::query_as("SELECT month_is_closed($1::date)").bind(m).fetch_one(&s.pool).await?;
        per_month.push((m.clone(), tot, closed));
    }
    let provisional = per_month.iter().any(|x| !x.2);

    let (y, mo, d): (i32, u8, u8) = { let today = today(&s).await?; (today[..4].parse().unwrap_or(2026), today[5..7].parse().unwrap_or(1), today[8..10].parse().unwrap_or(1)) };
    let mut zip = Zip::new(y, mo, d);
    let empty = vec![];
    let pays = pays.as_array().unwrap_or(&empty);
    let exps = exps.as_array().unwrap_or(&empty);
    let sals = sals.as_array().unwrap_or(&empty);

    // 01 Entradas
    let rows: Vec<Vec<String>> = pays.iter().map(|p| vec![
        s_(&p["number"]), date_pt(&p["received_on"]), s_(&p["patient_name"]), s_(&p["patient_number"]), s_(&p["patient_nif"]),
        s_(&p["description"]), method_pt(p["method"].as_str().unwrap_or("")).into(), export::cents(i_(&p["amount_cents"])),
        vat_pt(&p["vat_exempt"]).into(),
        s_(&p["invoice_number"]), if p["voided_at"].is_null() { String::new() } else { "Sim".into() }, s_(&p["void_reason"]),
    ]).collect();
    zip.add(&format!("{root}/01 Entradas/entradas.csv"), &export::csv(
        &["N.º recibo interno", "Data", "Utente", "N.º utente", "NIF", "Descrição", "Método", "Valor (€)", "IVA", "Fatura n.º", "Anulado", "Motivo da anulação"], &rows));
    let mut by_method: Vec<(String, i64, i64)> = Vec::new();
    for p in pays.iter().filter(|p| p["voided_at"].is_null()) {
        let m = method_pt(p["method"].as_str().unwrap_or("")).to_string();
        match by_method.iter_mut().find(|x| x.0 == m) { Some(x) => { x.1 += 1; x.2 += i_(&p["amount_cents"]) } None => by_method.push((m, 1, i_(&p["amount_cents"]))) }
    }
    zip.add(&format!("{root}/01 Entradas/por-metodo.csv"), &export::csv(&["Método", "N.º de pagamentos", "Total (€)"],
        &by_method.iter().map(|x| vec![x.0.clone(), x.1.to_string(), export::cents(x.2)]).collect::<Vec<_>>()));
    let mut missing_files = 0;
    if r.include_files {
        let mut seen = std::collections::HashSet::new();
        for p in pays {
            if let (Some(path), Some(name)) = (p["invoice_path"].as_str(), p["invoice_filename"].as_str()) {
                if !seen.insert(path.to_string()) || path.contains("..") { continue; }
                match tokio::fs::read(s.archive.join(path)).await {
                    Ok(b) => {
                        // Named after the invoice number (e.g. "FR 2026-14.pdf"), falling back to the uploaded name.
                        let ext = name.rsplit_once('.').map(|x| x.1).unwrap_or("pdf");
                        let num = export::safe_name(&s_(&p["invoice_number"]).replace('/', "-"));
                        let file = if s_(&p["invoice_number"]).trim().is_empty() { export::safe_name(name) } else { format!("{num}.{ext}") };
                        zip.add(&format!("{root}/01 Entradas/Faturas/{file}"), &b)
                    }
                    Err(_) => missing_files += 1,
                }
            }
        }
    }

    // 02 Saídas
    let rows: Vec<Vec<String>> = exps.iter().map(|e| vec![
        date_pt(&e["spent_on"]), s_(&e["supplier"]), s_(&e["supplier_nif"]), category_pt(e["category"].as_str().unwrap_or("")).into(),
        s_(&e["description"]), s_(&e["document_ref"]), method_pt(e["method"].as_str().unwrap_or("")).into(),
        export::cents(i_(&e["amount_cents"])), if e["vat_cents"].is_null() { String::new() } else { export::cents(i_(&e["vat_cents"])) },
        s_(&e["filename"]),
    ]).collect();
    zip.add(&format!("{root}/02 Saídas/saidas.csv"), &export::csv(
        &["Data", "Fornecedor", "NIF do fornecedor", "Categoria", "Descrição", "Documento n.º", "Método", "Valor (€)", "IVA (€)", "Ficheiro"], &rows));
    if r.include_files {
        for e in exps {
            if let (Some(path), Some(name)) = (e["path"].as_str(), e["filename"].as_str()) {
                if path.contains("..") { continue; }
                match tokio::fs::read(s.archive.join(path)).await {
                    Ok(b) => zip.add(&format!("{root}/02 Saídas/{}/{} {}", category_pt(e["category"].as_str().unwrap_or("")), &s_(&e["spent_on"])[..10.min(s_(&e["spent_on"]).len())], export::safe_name(name)), &b),
                    Err(_) => missing_files += 1,
                }
            }
        }
    }

    // 03 Salários
    let rows: Vec<Vec<String>> = sals.iter().map(|x| vec![
        s_(&x["month"]), s_(&x["name"]), match x["role"].as_str() { Some("owner") => "Proprietário", Some("therapist") => "Fisioterapeuta", _ => "Receção" }.into(),
        export::cents(i_(&x["gross_cents"])), export::cents(i_(&x["employer_ss_cents"])), date_pt(&x["paid_on"]), method_pt(x["method"].as_str().unwrap_or("")).into(), s_(&x["note"]),
    ]).collect();
    zip.add(&format!("{root}/03 Salários/salarios.csv"), &export::csv(&["Mês", "Nome", "Função", "Valor bruto (€)", "Segurança Social, entidade patronal (€)", "Pago em", "Método", "Nota"], &rows));

    // 04 Resumo
    let rows: Vec<Vec<String>> = per_month.iter().map(|(m, tt, closed)| vec![
        m[..7].to_string(), export::cents(i_(&tt["revenue_cents"])), export::cents(i_(&tt["expenses_cents"])), export::cents(i_(&tt["salaries_cents"])),
        export::cents(i_(&tt["employer_ss_cents"])), export::cents(i_(&tt["result_cents"])), tt["payments"].to_string(), tt["uninvoiced"].to_string(), if *closed { "Sim".into() } else { "Não (provisório)".into() },
    ]).collect();
    zip.add(&format!("{root}/04 Resumo/resumo.csv"), &export::csv(
        &["Mês", "Receita (€)", "Despesas (€)", "Salários (€)", "Segurança Social, entidade patronal (€)", "Resultado (€)", "N.º de pagamentos", "Pagamentos sem fatura", "Mês fechado"], &rows));
    if r.include_pdf {
        let mut l = vec![Line::Title(format!("{clinic} — resumo financeiro")), Line::Text(format!("Período: {} · gerado a {now_pt}", label.replace("_a_", " a "))), Line::Gap];
        if provisional {
            l.push(Line::Text("Atenção: inclui meses ainda não fechados; os valores podem mudar.".into()));
            l.push(Line::Gap);
        }
        let (mut rv, mut ex, mut sa, mut ss) = (0i64, 0i64, 0i64, 0i64);
        for (m, tt, closed) in &per_month {
            let (yy, mm): (i32, u32) = (m[..4].parse().unwrap_or(0), m[5..7].parse().unwrap_or(1));
            let name = month_pt(mm);
            let name = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
            l.push(Line::Heading(format!("{name} de {yy}{}", if *closed { "" } else { " (provisório)" })));
            l.push(Line::Row("Receita".into(), export::euros(i_(&tt["revenue_cents"]))));
            if let Some(bm) = tt["by_method"].as_object() {
                for (k, v) in bm { l.push(Line::Row(format!("    {}", method_pt(k)), export::euros(i_(v)))); }
            }
            l.push(Line::Row("Despesas".into(), export::euros(i_(&tt["expenses_cents"]))));
            if let Some(bc) = tt["expenses_by_category"].as_object() {
                for (k, v) in bc { l.push(Line::Row(format!("    {}", category_pt(k)), export::euros(i_(v)))); }
            }
            l.push(Line::Row("Salários".into(), export::euros(i_(&tt["salaries_cents"]))));
            if i_(&tt["employer_ss_cents"]) > 0 { l.push(Line::Row("Segurança Social (entidade patronal)".into(), export::euros(i_(&tt["employer_ss_cents"])))); }
            l.push(Line::Row("Resultado".into(), export::euros(i_(&tt["result_cents"]))));
            if i_(&tt["uninvoiced"]) > 0 {
                l.push(Line::Text(format!("{} pagamento(s) sem número de fatura registado ({}).", tt["uninvoiced"], export::euros(i_(&tt["uninvoiced_cents"])))));
            }
            rv += i_(&tt["revenue_cents"]);
            ex += i_(&tt["expenses_cents"]);
            sa += i_(&tt["salaries_cents"]);
            ss += i_(&tt["employer_ss_cents"]);
        }
        if per_month.len() > 1 {
            l.push(Line::Heading("Total do período".into()));
            l.push(Line::Row("Receita".into(), export::euros(rv)));
            l.push(Line::Row("Despesas".into(), export::euros(ex)));
            l.push(Line::Row("Salários".into(), export::euros(sa)));
            if ss > 0 { l.push(Line::Row("Segurança Social (entidade patronal)".into(), export::euros(ss))); }
            l.push(Line::Row("Resultado".into(), export::euros(rv - ex - sa - ss)));
        }
        zip.add(&format!("{root}/04 Resumo/resumo.pdf"), &export::pdf(&l));
    }

    // LEIA-ME
    let readme = format!(
"{clinic}
Exportação para a contabilidade — período {period}
Gerada a {now_pt} pela aplicação PhysioPulse.
{prov}
Conteúdo
  01 Entradas   entradas.csv: todos os pagamentos recebidos (os anulados ficam marcados, com o motivo).
                por-metodo.csv: totais por método de pagamento.
                Faturas/: PDF das faturas emitidas no programa de faturação certificado, quando anexados.
  02 Saídas     saidas.csv: todas as despesas; os talões e faturas de fornecedores estão em pastas por categoria.
  03 Salários   salarios.csv: salários pagos, valor bruto.
  04 Resumo     resumo.csv e resumo.pdf: receita, despesas, salários e resultado por mês.

Notas
  - Valores em euros, com vírgula decimal. Os ficheiros CSV usam ponto e vírgula (abrem diretamente no Excel).
  - As faturas oficiais são emitidas no programa de faturação certificado pela AT; aqui fica o número e o PDF.
  - O n.º de recibo interno é só para referência da clínica, não é um documento fiscal.
  - A coluna IVA segue a tabela de preços da aplicação (isento: fisioterapia com fim terapêutico, art. 9.º, n.º 1 do CIVA).
    Vazia quando o pagamento não está ligado a uma marcação ou pacote. Confirme no programa de faturação.
{missing}",
        period = label.replace("_a_", " a "),
        prov = if provisional { "\nATENÇÃO: o período inclui meses ainda não fechados na aplicação; os valores são provisórios.\n" } else { "" },
        missing = if missing_files > 0 { format!("  - {missing_files} ficheiro(s) anexo(s) não foram encontrados no servidor.\n") } else { String::new() },
    );
    zip.add(&format!("{root}/LEIA-ME.txt"), readme.replace('\n', "\r\n").as_bytes());

    let bytes = zip.finish();
    let filename = format!("{root}.zip");
    let today = today(&s).await?;
    let (id, path) = save_file(&s, &a, "export", &filename, &today, &bytes, "application/zip").await?;
    auth::log(&s.pool, Some(a.staff.id), "finance.exported", None, &a.device, &label).await;
    Ok(Json(json!({ "id": id, "filename": filename, "bytes": bytes.len(), "path": format!("PhysioPulse/{path}"), "provisional": provisional,
                    "payments": pays.len(), "expenses": exps.len(), "salaries": sals.len(), "missing_files": missing_files })))
}

#[cfg(test)]
mod ss_tests {
    use super::employer_ss;
    #[test]
    fn employer_social_security_rounds_to_the_cent() {
        assert_eq!(employer_ss(100_000, 2375), 23_750); // 1000,00 € → 237,50 €
        assert_eq!(employer_ss(87_050, 2375), 20_674);  // 870,50 € × 23,75 % = 206,74375 → 206,74 €
        assert_eq!(employer_ss(1, 5000), 1);            // half a cent rounds up
        assert_eq!(employer_ss(123_456, 0), 0);
    }
}
