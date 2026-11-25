//! Small helpers shared by the phase 2 routes.

use crate::auth::Auth;
use crate::error::{ApiResult, AppError};
use serde_json::{Map, Value};

/// Owner and therapists see clinical information; reception never does.
pub fn clinical(a: &Auth) -> bool {
    matches!(a.staff.role.as_str(), "owner" | "therapist")
}
pub fn require_clinical(a: &Auth) -> ApiResult<()> {
    a.require(&["owner", "therapist"])
}

/// Column kinds for [`sets`]. Values always travel as text and are cast by PostgreSQL,
/// so bad input becomes a friendly 400 (see `From<sqlx::Error>`).
#[derive(Clone, Copy)]
pub enum K {
    /// NOT NULL text: null or missing becomes ''.
    Text,
    /// Nullable value cast to this type; '' or null clears it.
    Opt(&'static str),
    /// NOT NULL value cast to this type.
    Req(&'static str),
    Json,
    IntArray,
}

/// Builds `col = $n` pieces for the keys present in `body` (others are ignored).
/// `first` is the number of the first free placeholder.
pub fn sets(body: &Map<String, Value>, fields: &[(&'static str, K)], first: usize) -> ApiResult<(Vec<String>, Vec<Option<String>>)> {
    let mut parts = Vec::new();
    let mut binds = Vec::new();
    for (name, kind) in fields {
        let Some(v) = body.get(*name) else { continue };
        let n = first + binds.len();
        let (sql, val) = match kind {
            K::Text => (format!("{name} = COALESCE(${n}, '')"), scalar(v)?),
            K::Opt(t) => (format!("{name} = NULLIF(${n}, '')::{t}"), scalar(v)?),
            K::Req(t) => (format!("{name} = ${n}::{t}"), scalar(v)?),
            K::Json => (format!("{name} = ${n}::jsonb"), if v.is_null() { None } else { Some(v.to_string()) }),
            K::IntArray => (format!("{name} = COALESCE(${n}::int[], '{{}}')"), int_array(v)?),
        };
        parts.push(sql);
        binds.push(val);
    }
    Ok((parts, binds))
}

fn scalar(v: &Value) -> ApiResult<Option<String>> {
    Ok(match v {
        Value::Null => None,
        Value::String(s) => Some(s.trim().to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => return Err(AppError::BadRequest("Some details aren’t in the right format. Check dates, times and numbers.")),
    })
}

pub fn int_array(v: &Value) -> ApiResult<Option<String>> {
    match v {
        Value::Null => Ok(None),
        Value::Array(a) => {
            let nums: Option<Vec<String>> = a.iter().map(|x| x.as_i64().map(|n| n.to_string())).collect();
            nums.map(|n| Some(format!("{{{}}}", n.join(","))))
                .ok_or(AppError::BadRequest("Some details aren’t in the right format. Check dates, times and numbers."))
        }
        _ => Err(AppError::BadRequest("Some details aren’t in the right format. Check dates, times and numbers.")),
    }
}

/// Runs `UPDATE table SET <sets>, <extra> WHERE id = $1 [AND <cond>]` and says whether a row matched.
pub async fn update_row<'e, E>(ex: E, table: &str, id: uuid::Uuid, sets_: (Vec<String>, Vec<Option<String>>), extra: &str, cond: &str) -> ApiResult<bool>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let (mut parts, binds) = sets_;
    if !extra.is_empty() {
        parts.push(extra.to_string());
    }
    if parts.is_empty() {
        return Ok(true);
    }
    let sql = format!("UPDATE {table} SET {} WHERE id = $1{}", parts.join(", "), if cond.is_empty() { String::new() } else { format!(" AND {cond}") });
    let mut q = sqlx::query(&sql).bind(id);
    for b in binds {
        q = q.bind(b);
    }
    Ok(q.execute(ex).await?.rows_affected() > 0)
}

/// Field names present in a request body, for the access log (names only, never values).
pub fn changed(body: &Map<String, Value>, fields: &[(&'static str, K)]) -> String {
    fields.iter().filter(|(n, _)| body.contains_key(*n)).map(|(n, _)| *n).collect::<Vec<_>>().join(",")
}

/// Runs a query that returns one JSON value (`SELECT json_build_object(...)` etc.).
pub async fn one_json<'e, E>(ex: E, sql: &str, id: uuid::Uuid) -> ApiResult<Value>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<(Value,)> = sqlx::query_as(sql).bind(id).fetch_optional(ex).await?;
    row.map(|r| r.0).ok_or(AppError::NotFound)
}
