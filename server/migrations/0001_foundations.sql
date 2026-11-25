-- Phase 1: staff accounts, two-step sign-in, sessions, invites, access log.

CREATE TABLE clinic (
    id          SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE staff (
    id              UUID PRIMARY KEY,
    email           TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    role            TEXT NOT NULL CHECK (role IN ('owner', 'therapist', 'reception')),
    password_hash   TEXT NOT NULL,
    totp_secret     TEXT NOT NULL,
    totp_enabled    BOOLEAN NOT NULL DEFAULT FALSE,
    totp_last_step  BIGINT NOT NULL DEFAULT 0,        -- blocks reuse of the same code
    active          BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Short-lived step between password and code (or code setup).
CREATE TABLE challenges (
    id          UUID PRIMARY KEY,
    staff_id    UUID NOT NULL REFERENCES staff(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('login', 'enrol')),
    attempts    INT NOT NULL DEFAULT 0,
    device      TEXT NOT NULL DEFAULT '',
    expires_at  TIMESTAMPTZ NOT NULL
);

-- Only a hash of the session token is stored.
CREATE TABLE sessions (
    id           UUID PRIMARY KEY,
    token_hash   TEXT NOT NULL UNIQUE,
    staff_id     UUID NOT NULL REFERENCES staff(id) ON DELETE CASCADE,
    device       TEXT NOT NULL DEFAULT '',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL
);
CREATE INDEX sessions_staff ON sessions(staff_id);

CREATE TABLE invites (
    id          UUID PRIMARY KEY,
    code_hash   TEXT NOT NULL UNIQUE,
    role        TEXT NOT NULL CHECK (role IN ('owner', 'therapist', 'reception')),
    email       TEXT,
    created_by  UUID NOT NULL REFERENCES staff(id),
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ
);

-- Append-only access log. Rows are never updated or deleted by the app.
CREATE TABLE access_log (
    id            BIGSERIAL PRIMARY KEY,
    at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    staff_id      UUID REFERENCES staff(id),
    action        TEXT NOT NULL,
    subject_type  TEXT,
    subject_id    TEXT,
    device        TEXT NOT NULL DEFAULT '',
    detail        TEXT NOT NULL DEFAULT ''
);
CREATE INDEX access_log_subject ON access_log(subject_type, subject_id);
CREATE INDEX access_log_at ON access_log(at DESC);

CREATE FUNCTION access_log_append_only() RETURNS trigger AS $$
BEGIN RAISE EXCEPTION 'access_log is append-only'; END; $$ LANGUAGE plpgsql;
CREATE TRIGGER access_log_no_update BEFORE UPDATE OR DELETE ON access_log
    FOR EACH ROW EXECUTE FUNCTION access_log_append_only();
