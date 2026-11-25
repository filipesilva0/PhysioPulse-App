//! Tiny migration runner: applies each embedded .sql file once, in order, in a transaction.

use sqlx::PgPool;

const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_foundations", include_str!("../migrations/0001_foundations.sql")),
    ("0002_patients", include_str!("../migrations/0002_patients.sql")),
    ("0003_money", include_str!("../migrations/0003_money.sql")),
    ("0004_documents", include_str!("../migrations/0004_documents.sql")),
    ("0005_legal", include_str!("../migrations/0005_legal.sql")),
    ("0006_consents", include_str!("../migrations/0006_consents.sql")),
    ("0007_reminders", include_str!("../migrations/0007_reminders.sql")),
    ("0008_letters", include_str!("../migrations/0008_letters.sql")),
    ("0009_complaints_vat", include_str!("../migrations/0009_complaints_vat.sql")),
    ("0010_ai_settings", include_str!("../migrations/0010_ai_settings.sql")),
    ("0011_phase4_ai", include_str!("../migrations/0011_phase4_ai.sql")),
    ("0012_trusted_computers", include_str!("../migrations/0012_trusted_computers.sql")),
    ("0013_accounts_clinic", include_str!("../migrations/0013_accounts_clinic.sql")),
    ("0014_employer_ss", include_str!("../migrations/0014_employer_ss.sql")),
    ("0015_physiopulse_name", include_str!("../migrations/0015_physiopulse_name.sql")),
    ("0016_recordings", include_str!("../migrations/0016_recordings.sql")),
    ("0017_devices", include_str!("../migrations/0017_devices.sql")),
    ("0018_website", include_str!("../migrations/0018_website.sql")),
    ("0019_assistant_schedule", include_str!("../migrations/0019_assistant_schedule.sql")),
    ("0020_phones", include_str!("../migrations/0020_phones.sql")),
    ("0021_assistant_tools", include_str!("../migrations/0021_assistant_tools.sql")),
    ("0022_assistant_money", include_str!("../migrations/0022_assistant_money.sql")),
    ("0023_assistant_documents", include_str!("../migrations/0023_assistant_documents.sql")),
    ("0024_patient_accounts", include_str!("../migrations/0024_patient_accounts.sql")),
];

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("CREATE TABLE IF NOT EXISTS schema_migrations (name TEXT PRIMARY KEY, applied_at TIMESTAMPTZ NOT NULL DEFAULT now())")
        .execute(pool)
        .await?;
    for (name, sql) in MIGRATIONS {
        let done: Option<(String,)> = sqlx::query_as("SELECT name FROM schema_migrations WHERE name = $1").bind(name).fetch_optional(pool).await?;
        if done.is_some() {
            continue;
        }
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(sql).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO schema_migrations (name) VALUES ($1)").bind(name).execute(&mut *tx).await?;
        tx.commit().await?;
        tracing::info!("applied migration {name}");
    }
    Ok(())
}

/// The website's own database login (phase 4a). Created by migration 0018; this makes sure it exists after a
/// restore into a new database server (roles aren't in backups), re-applies its grants, and sets its password
/// from `PORTAL_DB_PASSWORD` — or takes its login away when that is empty, so the website can't connect.
pub async fn portal_role(pool: &PgPool, password: Option<&str>) -> Result<bool, sqlx::Error> {
    // Spliced into SQL (ALTER ROLE takes no parameters), so only plain letters and digits are accepted.
    let pw = password.map(str::trim).filter(|p| !p.is_empty()).filter(|p| {
        let ok = p.len() >= 16 && p.chars().all(|c| c.is_ascii_alphanumeric());
        if !ok { tracing::error!("PORTAL_DB_PASSWORD must be at least 16 letters and digits: the website stays signed out"); }
        ok
    });
    sqlx::raw_sql(
        "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'physiopulse_portal') THEN CREATE ROLE physiopulse_portal NOLOGIN; END IF; END $$;
         GRANT USAGE ON SCHEMA portal TO physiopulse_portal;
         GRANT SELECT ON portal.site, portal.services, portal.team, portal.closures TO physiopulse_portal;
         GRANT EXECUTE ON FUNCTION portal.hit(TEXT, INT, INT), portal.request_booking(TEXT, TEXT, TEXT, TEXT, DATE, TEXT, TEXT, TEXT, TEXT) TO physiopulse_portal;
         GRANT EXECUTE ON FUNCTION portal.accounts_open(), portal.account_by_email(TEXT), portal.invite_check(TEXT),
             portal.account_create(TEXT, TEXT, TEXT, TEXT), portal.account_link(TEXT, UUID), portal.account_reset(TEXT, TEXT),
             portal.session_start(UUID, TEXT, BOOLEAN), portal.session_get(TEXT), portal.session_two_step_ok(TEXT), portal.session_end(TEXT),
             portal.account_update(TEXT, TEXT, TEXT, BOOLEAN, TEXT) TO physiopulse_portal;",
    ).execute(pool).await?;
    match pw {
        Some(p) => { sqlx::raw_sql(&format!("ALTER ROLE physiopulse_portal WITH LOGIN PASSWORD '{p}'")).execute(pool).await?; Ok(true) }
        None => { sqlx::raw_sql("ALTER ROLE physiopulse_portal WITH NOLOGIN").execute(pool).await?; Ok(false) }
    }
}

/// The key the website and the staff server share for the internal patient-area API (routes/portal.rs): derived
/// from the website's database password, so nothing new has to be set up. None when the website has no login.
pub fn portal_internal_key(password: Option<&str>) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let p = password.map(str::trim).filter(|p| p.len() >= 16 && p.chars().all(|c| c.is_ascii_alphanumeric()))?;
    Some(Sha256::digest(format!("physiopulse-portal-internal:{p}").as_bytes()).into())
}
