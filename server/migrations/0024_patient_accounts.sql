-- PhysioPulse plan, phase 4b (23 Nov 2026): patient accounts on the website (/conta).
--
-- Owner's answers (DECISIONS.md, 23 Nov): an account starts from a code a staff member makes in the patient's file
-- (no date-of-birth check); a forgotten password needs a one-time code from reception, the patient's therapist or the
-- owner; two-step codes are optional; under-16s have no login (a guardian's account shows them); patients see
-- sessions, make requests, cancel up to 24 h before, see invoices and what they owe — never clinical notes or tests.
-- The whole area stays closed (`website.accounts_open`) until the owner opens it.
--
-- Split of duties, so a bug in the website can't reach more than it must:
--   * the website (`physiopulse_portal` login) only signs people in: it calls the functions below, which see
--     accounts, codes and sessions — never a patient's file, appointments or money;
--   * what a signed-in patient sees or does goes through the staff server's internal API (routes/portal.rs),
--     which checks the session here and answers only for the patients linked to that account.

ALTER TABLE website ADD COLUMN accounts_open BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE patient_accounts (
    id             UUID PRIMARY KEY,
    email          TEXT NOT NULL UNIQUE CHECK (email = lower(btrim(email)) AND length(email) BETWEEN 3 AND 200),
    password_hash  TEXT NOT NULL,                  -- Argon2id, made by the website
    totp_secret    TEXT,                           -- two-step secret, encrypted by the website; NULL = off
    lang           TEXT NOT NULL DEFAULT 'pt' CHECK (lang IN ('pt', 'en')),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_login_at  TIMESTAMPTZ
);

-- Whose sessions and invoices an account shows: the person themself, or a child/dependant as guardian.
CREATE TABLE patient_account_links (
    account_id  UUID NOT NULL REFERENCES patient_accounts(id) ON DELETE CASCADE,
    patient_id  UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    relation    TEXT NOT NULL CHECK (relation IN ('self', 'guardian')),
    created_by  UUID REFERENCES staff(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, patient_id)
);
CREATE INDEX patient_account_links_patient ON patient_account_links (patient_id);

-- One-time codes made by staff: an invite (opens or links an account to a patient) or a password reset.
-- Only a SHA-256 of the code is kept; the staff member sees the code once.
CREATE TABLE patient_codes (
    id          UUID PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('invite', 'reset')),
    code_hash   TEXT NOT NULL UNIQUE,
    patient_id  UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    account_id  UUID REFERENCES patient_accounts(id) ON DELETE CASCADE,   -- resets only
    relation    TEXT NOT NULL DEFAULT 'self' CHECK (relation IN ('self', 'guardian')),
    created_by  UUID REFERENCES staff(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ,
    CHECK ((kind = 'reset') = (account_id IS NOT NULL))
);
CREATE INDEX patient_codes_patient ON patient_codes (patient_id, created_at);

-- Website sessions. Only a SHA-256 of the cookie is kept. 30 minutes idle, 12 hours at most.
CREATE TABLE patient_sessions (
    token_hash   TEXT PRIMARY KEY,
    account_id   UUID NOT NULL REFERENCES patient_accounts(id) ON DELETE CASCADE,
    pending_2fa  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL DEFAULT now() + interval '12 hours'
);
CREATE INDEX patient_sessions_account ON patient_sessions (account_id);

-- Requests from signed-in patients land in Schedule › Booking Requests like the others, with what they are.
ALTER TABLE booking_requests DROP CONSTRAINT IF EXISTS booking_requests_source_check;
ALTER TABLE booking_requests ADD CONSTRAINT booking_requests_source_check CHECK (source IN ('staff', 'website', 'portal'));
ALTER TABLE booking_requests ADD COLUMN kind TEXT NOT NULL DEFAULT 'book' CHECK (kind IN ('book', 'move', 'cancelled'));
-- The appointment a "move" or "cancelled" refers to (the existing appointment_id is the one staff book from a request).
ALTER TABLE booking_requests ADD COLUMN about_appointment_id UUID REFERENCES appointments(id) ON DELETE SET NULL;

-- Anonymising or deleting a patient ends their portal access (links and codes go; an account left with nobody is removed).
CREATE FUNCTION patient_portal_cleanup() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.anonymised_at IS NOT NULL AND OLD.anonymised_at IS NULL) OR (NEW.deleted_at IS NOT NULL AND OLD.deleted_at IS NULL) THEN
        DELETE FROM patient_account_links WHERE patient_id = NEW.id;
        DELETE FROM patient_codes WHERE patient_id = NEW.id;
        DELETE FROM patient_accounts a WHERE NOT EXISTS (SELECT 1 FROM patient_account_links l WHERE l.account_id = a.id);
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER patients_portal_cleanup AFTER UPDATE OF anonymised_at, deleted_at ON patients
    FOR EACH ROW EXECUTE FUNCTION patient_portal_cleanup();

-- The website now also needs to know whether the patient area is open.
CREATE OR REPLACE VIEW portal.site AS
SELECT w.published, w.requests_open, w.home_visits, w.opening_soon, w.show_prices,
       w.tagline_pt, w.tagline_en, w.intro_pt, w.intro_en, w.home_text_pt, w.home_text_en,
       c.name AS clinic_name, c.nif, c.ers_number, c.address, c.phone, c.email, c.opening_hours,
       GREATEST(w.updated_at, COALESCE(c.updated_at, c.created_at)) AS updated_at,
       w.accounts_open
FROM website w CROSS JOIN clinic c;

-- ───────── what the website may do with accounts ─────────
-- Codes are compared as SHA-256 of the code in capitals without spaces or dashes (the same as routes/portal.rs).

CREATE FUNCTION portal.code_hash(p_code TEXT) RETURNS TEXT LANGUAGE sql IMMUTABLE AS $$
    SELECT encode(sha256(convert_to(upper(regexp_replace(coalesce(p_code, ''), '[^A-Za-z0-9]', '', 'g')), 'UTF8')), 'hex')
$$;

CREATE FUNCTION portal.accounts_open() RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS $$
    SELECT published AND accounts_open FROM website WHERE id = 1
$$;

-- For signing in: the stored hash (checked by the website with Argon2) and the two-step secret, by email.
CREATE FUNCTION portal.account_by_email(p_email TEXT) RETURNS TABLE (id UUID, password_hash TEXT, totp_secret TEXT)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS $$
BEGIN
    IF NOT portal.accounts_open() THEN RAISE EXCEPTION 'closed'; END IF;
    RETURN QUERY SELECT a.id, a.password_hash, a.totp_secret FROM patient_accounts a
        WHERE a.email = lower(btrim(p_email)) AND EXISTS (SELECT 1 FROM patient_account_links l WHERE l.account_id = a.id);
END $$;

-- What an invite code is for, before the person chooses an email and password (NULL when not valid).
CREATE FUNCTION portal.invite_check(p_code TEXT) RETURNS TEXT
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS $$
DECLARE r TEXT;
BEGIN
    IF NOT portal.accounts_open() THEN RAISE EXCEPTION 'closed'; END IF;
    SELECT c.relation INTO r FROM patient_codes c JOIN patients p ON p.id = c.patient_id
     WHERE c.code_hash = portal.code_hash(p_code) AND c.kind = 'invite' AND c.used_at IS NULL AND c.expires_at > now()
       AND p.anonymised_at IS NULL AND p.deleted_at IS NULL;
    RETURN r;
END $$;

-- An invite used to open a new account. Errors: closed, bad_code, email_taken.
CREATE FUNCTION portal.account_create(p_code TEXT, p_email TEXT, p_hash TEXT, p_lang TEXT) RETURNS UUID
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
DECLARE c RECORD; v_id UUID := gen_random_uuid(); v_email TEXT := lower(btrim(p_email));
BEGIN
    IF NOT portal.accounts_open() THEN RAISE EXCEPTION 'closed'; END IF;
    SELECT pc.* INTO c FROM patient_codes pc JOIN patients p ON p.id = pc.patient_id
     WHERE pc.code_hash = portal.code_hash(p_code) AND pc.kind = 'invite' AND pc.used_at IS NULL AND pc.expires_at > now()
       AND p.anonymised_at IS NULL AND p.deleted_at IS NULL FOR UPDATE OF pc;
    IF NOT FOUND THEN RAISE EXCEPTION 'bad_code'; END IF;
    IF v_email !~ '^[^@\s]+@[^@\s]+\.[^@\s]+$' OR length(v_email) > 200 THEN RAISE EXCEPTION 'bad_email'; END IF;
    IF p_hash !~ '^\$argon2id\$' THEN RAISE EXCEPTION 'bad_input'; END IF;
    IF EXISTS (SELECT 1 FROM patient_accounts WHERE email = v_email) THEN RAISE EXCEPTION 'email_taken'; END IF;
    INSERT INTO patient_accounts (id, email, password_hash, lang) VALUES (v_id, v_email, p_hash, CASE WHEN p_lang = 'en' THEN 'en' ELSE 'pt' END);
    INSERT INTO patient_account_links (account_id, patient_id, relation, created_by) VALUES (v_id, c.patient_id, c.relation, c.created_by);
    UPDATE patient_codes SET used_at = now() WHERE id = c.id;
    INSERT INTO access_log (staff_id, action, subject_type, subject_id, device, detail)
    VALUES (NULL, 'portal.account_created', 'patient', c.patient_id, 'site', c.relation);
    RETURN v_id;
END $$;

-- An invite used by someone who already has an account (the website has checked their password): adds the patient.
CREATE FUNCTION portal.account_link(p_code TEXT, p_account UUID) RETURNS VOID
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
DECLARE c RECORD;
BEGIN
    IF NOT portal.accounts_open() THEN RAISE EXCEPTION 'closed'; END IF;
    SELECT pc.* INTO c FROM patient_codes pc JOIN patients p ON p.id = pc.patient_id
     WHERE pc.code_hash = portal.code_hash(p_code) AND pc.kind = 'invite' AND pc.used_at IS NULL AND pc.expires_at > now()
       AND p.anonymised_at IS NULL AND p.deleted_at IS NULL FOR UPDATE OF pc;
    IF NOT FOUND THEN RAISE EXCEPTION 'bad_code'; END IF;
    INSERT INTO patient_account_links (account_id, patient_id, relation, created_by) VALUES (p_account, c.patient_id, c.relation, c.created_by)
        ON CONFLICT (account_id, patient_id) DO UPDATE SET relation = EXCLUDED.relation;
    UPDATE patient_codes SET used_at = now() WHERE id = c.id;
    INSERT INTO access_log (staff_id, action, subject_type, subject_id, device, detail)
    VALUES (NULL, 'portal.account_linked', 'patient', c.patient_id, 'site', c.relation);
END $$;

-- A reset code from staff: new password; two-step off (staff knew who they were talking to); every session ends.
CREATE FUNCTION portal.account_reset(p_code TEXT, p_hash TEXT) RETURNS UUID
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
DECLARE c RECORD;
BEGIN
    IF NOT portal.accounts_open() THEN RAISE EXCEPTION 'closed'; END IF;
    IF p_hash !~ '^\$argon2id\$' THEN RAISE EXCEPTION 'bad_input'; END IF;
    SELECT * INTO c FROM patient_codes WHERE code_hash = portal.code_hash(p_code) AND kind = 'reset' AND used_at IS NULL AND expires_at > now() FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'bad_code'; END IF;
    UPDATE patient_accounts SET password_hash = p_hash, totp_secret = NULL WHERE id = c.account_id;
    DELETE FROM patient_sessions WHERE account_id = c.account_id;
    UPDATE patient_codes SET used_at = now() WHERE id = c.id;
    INSERT INTO access_log (staff_id, action, subject_type, subject_id, device, detail)
    VALUES (NULL, 'portal.password_reset', 'patient', c.patient_id, 'site', '');
    RETURN c.account_id;
END $$;

-- A new session after the password (and, when on, before the two-step code). The website keeps the cookie.
CREATE FUNCTION portal.session_start(p_account UUID, p_token_hash TEXT, p_pending BOOLEAN) RETURNS VOID
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
BEGIN
    IF NOT portal.accounts_open() THEN RAISE EXCEPTION 'closed'; END IF;
    IF length(p_token_hash) <> 64 THEN RAISE EXCEPTION 'bad_input'; END IF;
    DELETE FROM patient_sessions WHERE expires_at < now() OR last_seen < now() - interval '30 minutes';
    INSERT INTO patient_sessions (token_hash, account_id, pending_2fa) VALUES (p_token_hash, p_account, p_pending);
    IF NOT p_pending THEN UPDATE patient_accounts SET last_login_at = now() WHERE id = p_account; END IF;
END $$;

-- The session behind a cookie (and keeps it alive), or nothing. Ends after 30 minutes idle or 12 hours.
CREATE FUNCTION portal.session_get(p_token_hash TEXT)
RETURNS TABLE (account_id UUID, email TEXT, pending_2fa BOOLEAN, totp_secret TEXT, lang TEXT)
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
BEGIN
    IF NOT portal.accounts_open() THEN RETURN; END IF;
    RETURN QUERY UPDATE patient_sessions s SET last_seen = now() FROM patient_accounts a
        WHERE s.token_hash = p_token_hash AND a.id = s.account_id AND s.expires_at > now() AND s.last_seen > now() - interval '30 minutes'
        RETURNING a.id, a.email, s.pending_2fa, a.totp_secret, a.lang;
END $$;

CREATE FUNCTION portal.session_two_step_ok(p_token_hash TEXT) RETURNS VOID
LANGUAGE sql SECURITY DEFINER SET search_path = public, pg_temp AS $$
    UPDATE patient_accounts SET last_login_at = now() WHERE id = (SELECT account_id FROM patient_sessions WHERE token_hash = p_token_hash);
    UPDATE patient_sessions SET pending_2fa = FALSE WHERE token_hash = p_token_hash;
$$;

CREATE FUNCTION portal.session_end(p_token_hash TEXT) RETURNS VOID
LANGUAGE sql SECURITY DEFINER SET search_path = public, pg_temp AS $$
    DELETE FROM patient_sessions WHERE token_hash = p_token_hash;
$$;

-- Changes on the signed-in account (the website has checked the current password first).
CREATE FUNCTION portal.account_update(p_token_hash TEXT, p_hash TEXT, p_totp TEXT, p_set_totp BOOLEAN, p_lang TEXT) RETURNS VOID
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
DECLARE v_account UUID;
BEGIN
    SELECT account_id INTO v_account FROM patient_sessions
     WHERE token_hash = p_token_hash AND NOT pending_2fa AND expires_at > now() AND last_seen > now() - interval '30 minutes';
    IF v_account IS NULL THEN RAISE EXCEPTION 'no_session'; END IF;
    IF p_hash IS NOT NULL THEN
        IF p_hash !~ '^\$argon2id\$' THEN RAISE EXCEPTION 'bad_input'; END IF;
        UPDATE patient_accounts SET password_hash = p_hash WHERE id = v_account;
        DELETE FROM patient_sessions WHERE account_id = v_account AND token_hash <> p_token_hash;
    END IF;
    IF p_set_totp THEN UPDATE patient_accounts SET totp_secret = p_totp WHERE id = v_account; END IF;
    IF p_lang IN ('pt', 'en') THEN UPDATE patient_accounts SET lang = p_lang WHERE id = v_account; END IF;
END $$;

REVOKE ALL ON FUNCTION portal.accounts_open(), portal.account_by_email(TEXT), portal.invite_check(TEXT),
    portal.account_create(TEXT, TEXT, TEXT, TEXT), portal.account_link(TEXT, UUID), portal.account_reset(TEXT, TEXT),
    portal.session_start(UUID, TEXT, BOOLEAN), portal.session_get(TEXT), portal.session_two_step_ok(TEXT), portal.session_end(TEXT),
    portal.account_update(TEXT, TEXT, TEXT, BOOLEAN, TEXT) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION portal.accounts_open(), portal.account_by_email(TEXT), portal.invite_check(TEXT),
    portal.account_create(TEXT, TEXT, TEXT, TEXT), portal.account_link(TEXT, UUID), portal.account_reset(TEXT, TEXT),
    portal.session_start(UUID, TEXT, BOOLEAN), portal.session_get(TEXT), portal.session_two_step_ok(TEXT), portal.session_end(TEXT),
    portal.account_update(TEXT, TEXT, TEXT, BOOLEAN, TEXT) TO physiopulse_portal;
GRANT SELECT ON portal.site TO physiopulse_portal;
