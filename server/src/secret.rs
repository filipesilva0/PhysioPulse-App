//! Secrets kept encrypted in the database (PhysioPulse plan, phase 3): the two-step (TOTP) secrets.
//!
//! The key is `SECRET_KEY` (64 hex characters) from the server's `.env`, made by the installer. It is never
//! in the database or in the backups, so a stolen backup or database copy can't make anyone's 6-digit codes.
//! Keep a copy with the backup key: restoring a backup on a new server needs it (without it, every person
//! sets up two-step again — owners with a recovery code).
//!
//! Format: `v1:` + base64(12-byte nonce ‖ ChaCha20-Poly1305 ciphertext). Values without the prefix are
//! plain (from before phase 3) and are encrypted at start-up (`seal_old`).

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chacha20poly1305::{aead::{Aead, KeyInit}, ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use std::sync::OnceLock;

static KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();
const PREFIX: &str = "v1:";

/// Reads `SECRET_KEY`. Returns false (and the server keeps secrets in plain text, as before) when it's missing.
pub fn init(hex_key: Option<&str>) -> Result<bool, String> {
    let key = match hex_key.map(str::trim).filter(|k| !k.is_empty()) {
        None => None,
        Some(k) => {
            let b = hex::decode(k).map_err(|_| "SECRET_KEY must be 64 hexadecimal characters (openssl rand -hex 32)".to_string())?;
            let a: [u8; 32] = b.try_into().map_err(|_| "SECRET_KEY must be 64 hexadecimal characters (openssl rand -hex 32)".to_string())?;
            Some(a)
        }
    };
    let on = key.is_some();
    let _ = KEY.set(key);
    Ok(on)
}

fn key() -> Option<&'static [u8; 32]> {
    KEY.get().and_then(|k| k.as_ref())
}

pub fn enabled() -> bool {
    key().is_some()
}

/// Encrypts with the server key (or returns the text unchanged when there is no key).
pub fn seal(plain: &str) -> String {
    let Some(k) = key() else { return plain.to_string() };
    let cipher = ChaCha20Poly1305::new(Key::from_slice(k));
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher.encrypt(Nonce::from_slice(&nonce), plain.as_bytes()).expect("encryption cannot fail");
    let mut out = nonce.to_vec();
    out.extend(ct);
    format!("{PREFIX}{}", B64.encode(out))
}

/// Decrypts a sealed value; plain values (from before phase 3) come back as they are.
pub fn open(stored: &str) -> Result<String, String> {
    let Some(b64) = stored.strip_prefix(PREFIX) else { return Ok(stored.to_string()) };
    let k = key().ok_or("a secret is encrypted but SECRET_KEY is not set")?;
    let raw = B64.decode(b64).map_err(|_| "damaged secret")?;
    if raw.len() < 13 {
        return Err("damaged secret".into());
    }
    let (nonce, ct) = raw.split_at(12);
    let plain = ChaCha20Poly1305::new(Key::from_slice(k)).decrypt(Nonce::from_slice(nonce), ct).map_err(|_| "secret doesn't match SECRET_KEY")?;
    String::from_utf8(plain).map_err(|_| "damaged secret".into())
}

pub fn is_sealed(stored: &str) -> bool {
    stored.starts_with(PREFIX)
}

/// At start-up: encrypt the two-step secrets still in plain text. Returns how many were encrypted.
pub async fn seal_old(pool: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    if !enabled() {
        return Ok(0);
    }
    let rows: Vec<(uuid::Uuid, String)> = sqlx::query_as("SELECT id, totp_secret FROM staff WHERE totp_secret NOT LIKE 'v1:%'").fetch_all(pool).await?;
    let mut n = 0;
    for (id, plain) in rows {
        n += sqlx::query("UPDATE staff SET totp_secret = $2 WHERE id = $1 AND totp_secret = $3")
            .bind(id).bind(seal(&plain)).bind(&plain).execute(pool).await?.rows_affected();
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_plain_values() {
        assert!(init(Some("zz")).is_err());
        init(Some(&"ab".repeat(32))).unwrap();
        let s = seal("JBSWY3DPEHPK3PXP");
        assert!(is_sealed(&s) && !s.contains("JBSWY3DP"));
        assert_ne!(s, seal("JBSWY3DPEHPK3PXP"), "a fresh nonce every time");
        assert_eq!(open(&s).unwrap(), "JBSWY3DPEHPK3PXP");
        assert_eq!(open("JBSWY3DPEHPK3PXP").unwrap(), "JBSWY3DPEHPK3PXP", "old plain values still read");
        let mut bad: Vec<char> = s.chars().collect();
        bad[20] = if bad[20] == 'A' { 'B' } else { 'A' };
        let bad: String = bad.into_iter().collect();
        assert!(open(&bad).is_err(), "a changed value is refused");
    }
}
