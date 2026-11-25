-- Audit fixes, phase A (10 Nov 2026).
-- 1. Account recovery: the owner's printed recovery codes, and one-time reset codes the owner gives a staff member.
-- 2. Clinic details printed on every PDF (NIF, ERS registration, address, contacts, logo).

-- Ten single-use codes for an owner, shown once (only their hashes are kept). One code either replaces a lost
-- phone at sign-in (two-step is set up again) or resets a forgotten password.
CREATE TABLE recovery_codes (
    id          UUID PRIMARY KEY,
    staff_id    UUID NOT NULL REFERENCES staff(id) ON DELETE CASCADE,
    code_hash   TEXT NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    used_at     TIMESTAMPTZ
);
CREATE INDEX recovery_codes_staff ON recovery_codes(staff_id);

-- A one-time code the owner gives someone who forgot their password. Valid 24 hours; only its hash is kept.
CREATE TABLE reset_codes (
    id          UUID PRIMARY KEY,
    staff_id    UUID NOT NULL REFERENCES staff(id) ON DELETE CASCADE,
    code_hash   TEXT NOT NULL UNIQUE,
    created_by  UUID NOT NULL REFERENCES staff(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ
);
CREATE INDEX reset_codes_staff ON reset_codes(staff_id);

-- Invites can be withdrawn before they are used.
ALTER TABLE invites ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT now(), ADD COLUMN revoked_at TIMESTAMPTZ;

ALTER TABLE clinic
    ADD COLUMN nif        TEXT NOT NULL DEFAULT '',
    ADD COLUMN ers_number TEXT NOT NULL DEFAULT '',
    ADD COLUMN address    TEXT NOT NULL DEFAULT '',
    ADD COLUMN phone      TEXT NOT NULL DEFAULT '',
    ADD COLUMN email      TEXT NOT NULL DEFAULT '',
    ADD COLUMN logo       BYTEA,             -- JPEG, at most 512 KB, drawn at the top of every PDF
    ADD COLUMN updated_at TIMESTAMPTZ;
