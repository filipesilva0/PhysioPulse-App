//! The clinic's own details (audit C5, 10 Nov 2026): name, NIF, ERS registration, address, contacts and logo.
//! Everyone signed in can read them (they appear on documents); only the owner changes them.
//! They're printed at the top of every PDF the server makes (`export::clinic_header`).

use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{extract::State, Json};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};

/// Portuguese NIF/NIPC: 9 digits, first digit allowed, mod-11 check digit.
pub fn nif_ok(nif: &str) -> bool {
    let d: Vec<u32> = nif.chars().filter_map(|c| c.to_digit(10)).collect();
    if d.len() != 9 || nif.chars().any(|c| !c.is_ascii_digit()) {
        return false;
    }
    // Individuals 1–3; companies 5; public bodies 6; others 7–9 (45 = non-resident individuals, 70/71/... = special).
    if d[0] == 0 || d[0] == 4 {
        return false;
    }
    let sum: u32 = d[..8].iter().enumerate().map(|(i, x)| x * (9 - i as u32)).sum();
    let check = match 11 - sum % 11 { 10 | 11 => 0, c => c };
    check == d[8]
}

/// The clinic's details as JSON (the logo as a data URL, or null).
pub async fn read(pool: &sqlx::PgPool) -> ApiResult<Value> {
    let row: Option<(Value,)> = sqlx::query_as(
        "SELECT json_build_object('name', name, 'nif', nif, 'ers_number', ers_number, 'address', address, 'phone', phone,
                'email', email, 'logo', CASE WHEN logo IS NULL THEN NULL ELSE 'data:image/jpeg;base64,' || encode(logo, 'base64') END,
                'updated_at', updated_at)
         FROM clinic WHERE id = 1").fetch_optional(pool).await?;
    Ok(row.map(|r| r.0).unwrap_or_else(|| json!({ "name": "", "nif": "", "ers_number": "", "address": "", "phone": "", "email": "", "logo": null })))
}

/// Everyone signed in.
pub async fn get(State(s): State<AppState>, _a: Auth) -> ApiResult<Json<Value>> {
    let mut v = read(&s.pool).await?;
    if let Some(l) = v["logo"].as_str() {
        // base64 from PostgreSQL wraps lines; the app wants one line.
        v["logo"] = json!(l.replace(['\n', '\r'], ""));
    }
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct Put {
    name: String,
    #[serde(default)] nif: String,
    #[serde(default)] ers_number: String,
    #[serde(default)] address: String,
    #[serde(default)] phone: String,
    #[serde(default)] email: String,
    /// Absent = keep the logo; null or "" = remove it; otherwise a JPEG as base64 or a data URL.
    #[serde(default, deserialize_with = "some")]
    logo: Option<Option<String>>,
}
fn some<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Ok(Some(Option::deserialize(d)?))
}

const MAX_LOGO: usize = 512 * 1024;

/// Owner: change the clinic's details.
pub async fn put(State(s): State<AppState>, a: Auth, Json(r): Json<Put>) -> ApiResult<Json<Value>> {
    a.require(&["owner"])?;
    let name = r.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(AppError::BadRequest("Write the clinic’s name."));
    }
    let nif: String = r.nif.chars().filter(|c| !c.is_whitespace()).collect();
    if !nif.is_empty() && !nif_ok(&nif) {
        return Err(AppError::BadRequest("That NIF isn’t valid. Check the 9 digits."));
    }
    let email = r.email.trim().to_lowercase();
    if !email.is_empty() && (!email.contains('@') || email.contains(' ') || email.len() > 120) {
        return Err(AppError::BadRequest("Check the email address."));
    }
    let phone = r.phone.trim();
    if phone.chars().filter(|c| c.is_ascii_digit()).count() > 0 && phone.chars().filter(|c| c.is_ascii_digit()).count() < 9 {
        return Err(AppError::BadRequest("Check the phone number."));
    }
    let (ers, address) = (r.ers_number.trim(), r.address.trim());
    if ers.chars().count() > 30 || address.chars().count() > 300 || phone.chars().count() > 30 {
        return Err(AppError::BadRequest("Some details are too long."));
    }
    let logo: Option<Option<Vec<u8>>> = match r.logo {
        None => None,
        Some(None) => Some(None),
        Some(Some(b)) if b.trim().is_empty() => Some(None),
        Some(Some(b)) => {
            let b64 = b.split_once(";base64,").map(|x| x.1).unwrap_or(&b);
            let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim())
                .map_err(|_| AppError::BadRequest("The logo must be a JPEG or PNG picture."))?;
            if bytes.len() > MAX_LOGO {
                return Err(AppError::BadRequest("The logo is too big. Use a picture under 512 KB."));
            }
            if crate::export::picture_from_jpeg(&bytes).is_none() {
                return Err(AppError::BadRequest("The logo must be a JPEG or PNG picture."));
            }
            Some(Some(bytes))
        }
    };
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE clinic SET name = $1, nif = $2, ers_number = $3, address = $4, phone = $5, email = $6, updated_at = now() WHERE id = 1")
        .bind(name).bind(&nif).bind(ers).bind(address).bind(phone).bind(&email).execute(&mut *tx).await?;
    if let Some(l) = &logo {
        sqlx::query("UPDATE clinic SET logo = $1 WHERE id = 1").bind(l).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    auth::log(&s.pool, Some(a.staff.id), "clinic.updated", None, &a.device, if logo.is_some() { "logo" } else { "" }).await;
    get(State(s), a).await
}

#[cfg(test)]
mod tests {
    use super::nif_ok;
    #[test]
    fn nif_check_digit() {
        assert!(nif_ok("501442600")); // a well-known company NIPC
        assert!(nif_ok("123456789"));
        assert!(!nif_ok("123456780"));
        assert!(!nif_ok("12345678"));
        assert!(!nif_ok("412345678"));
        assert!(!nif_ok("12345678a"));
    }
}
