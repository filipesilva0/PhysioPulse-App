-- PhysioPulse plan, phase 3: the staff server faces the internet, so every device needs the owner's approval.
--
-- A device is one copy of the app on one computer (later: one phone). The app's Rust core makes a random
-- key the first time it starts and sends it with every request (X-Device-Key); only its SHA-256 is kept here.
-- A device is approved once, for everyone who uses it (a shared reception PC is approved once).
-- New devices wait as 'pending' until the owner approves them from an approved device, with a printed
-- recovery code, or from the server console. 'revoked' = removed or declined by the owner.

CREATE TABLE devices (
    id            UUID PRIMARY KEY,
    key_hash      TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL DEFAULT '',            -- computer name the app sends; the owner can rename it
    code          TEXT NOT NULL,                        -- short code shown on the waiting screen, to match it in the list
    status        TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'revoked')),
    how           TEXT NOT NULL DEFAULT '',             -- setup | carried_over | owner | recovery_code | console
    requested_by  UUID REFERENCES staff(id) ON DELETE SET NULL,
    approved_by   UUID REFERENCES staff(id) ON DELETE SET NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    approved_at   TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ,
    last_seen     TIMESTAMPTZ,
    last_ip       TEXT NOT NULL DEFAULT ''
);
CREATE INDEX devices_status ON devices(status);

-- Every session belongs to one device; removing the device ends its sessions at once.
-- Sessions made before devices existed have none: everyone signs in again once after the update.
DELETE FROM sessions;
ALTER TABLE sessions ADD COLUMN device_id UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE;

-- Trusted-computer tokens (password only for 30 days) now belong to a device too.
-- Tokens made before this update are "carried over": the first sign-in with one approves the
-- device it comes from, once (owner decision, 11 Nov 2026: computers already trusted keep working).
ALTER TABLE trusted_devices ADD COLUMN device_id UUID REFERENCES devices(id) ON DELETE CASCADE;
ALTER TABLE trusted_devices ADD COLUMN carried_over BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE trusted_devices SET carried_over = TRUE;

-- A sign-in that passed password and code on a device still waiting for approval. The app keeps the
-- ticket (Rust core only) and swaps it for a session once the owner approves; 30 minutes.
CREATE TABLE pending_signins (
    id           UUID PRIMARY KEY,
    ticket_hash  TEXT NOT NULL UNIQUE,
    staff_id     UUID NOT NULL REFERENCES staff(id) ON DELETE CASCADE,
    device_id    UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    trust        BOOLEAN NOT NULL DEFAULT FALSE,
    enrol        BOOLEAN NOT NULL DEFAULT FALSE,     -- the person had just set up two-step (owner recovery codes)
    expires_at   TIMESTAMPTZ NOT NULL
);

-- Failed attempts (sign-in, codes, invites, resets), kept in the database instead of the server's memory,
-- so a restart doesn't reset them.
CREATE TABLE rate_limits (
    key           TEXT PRIMARY KEY,
    fails         INT NOT NULL DEFAULT 0,
    window_start  TIMESTAMPTZ NOT NULL DEFAULT now()
);
