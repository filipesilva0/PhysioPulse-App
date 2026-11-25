-- Trusted computers: after the 6-digit code once, the same person on the same computer
-- only needs their password for 30 days. Only a hash of the token is stored; the token
-- itself is kept by the desktop app's Rust core, never by the web view.

CREATE TABLE trusted_devices (
    id          UUID PRIMARY KEY,
    staff_id    UUID NOT NULL REFERENCES staff(id) ON DELETE CASCADE,
    token_hash  TEXT NOT NULL UNIQUE,
    device      TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used   TIMESTAMPTZ,
    expires_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX trusted_devices_staff ON trusted_devices(staff_id);
