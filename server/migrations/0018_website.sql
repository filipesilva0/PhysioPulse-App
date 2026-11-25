-- PhysioPulse plan, phase 4a (11 Nov 2026): the public website and appointment requests from it.
--
-- The website is served by a SEPARATE service (server/portal) that signs in to the database as its own role,
-- `physiopulse_portal`. That role can read only the views in the `portal` schema below (no patient, staff
-- e-mail, money or clinical data) and can write only through `portal.request_booking()` and `portal.hit()`.
-- A bug in a public page therefore cannot reach the staff data. The role's password is set by the staff
-- server at start-up from PORTAL_DB_PASSWORD (db.rs), which also re-applies the grants below.

-- What the owner edits in Settings › Website. One row.
CREATE TABLE website (
    id              SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    published       BOOLEAN NOT NULL DEFAULT FALSE,   -- off: every page says "coming soon" and search engines are told to stay away
    requests_open   BOOLEAN NOT NULL DEFAULT TRUE,    -- the "request an appointment" form
    home_visits     BOOLEAN NOT NULL DEFAULT TRUE,    -- home visits offered (on the site and in the form)
    opening_soon    BOOLEAN NOT NULL DEFAULT TRUE,    -- the clinic's space isn't open yet: "Abertura em breve" instead of the address
    show_prices     BOOLEAN NOT NULL DEFAULT TRUE,
    tagline_pt      TEXT NOT NULL DEFAULT '',
    tagline_en      TEXT NOT NULL DEFAULT '',
    intro_pt        TEXT NOT NULL DEFAULT '',
    intro_en        TEXT NOT NULL DEFAULT '',
    home_text_pt    TEXT NOT NULL DEFAULT '',
    home_text_en    TEXT NOT NULL DEFAULT '',
    updated_by      UUID REFERENCES staff(id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO website (tagline_pt, tagline_en, intro_pt, intro_en, home_text_pt, home_text_en) VALUES (
  'Fisioterapia com tempo para si',
  'Physiotherapy with time for you',
  'Avaliamos com calma, explicamos o que se passa e combinamos consigo um plano de tratamento. Cada sessão é individual e acompanhada pelo mesmo fisioterapeuta.',
  'We take the time to assess you, explain what is going on and agree a treatment plan with you. Every session is one-to-one, with the same physiotherapist.',
  'Se não puder vir à clínica, vamos a sua casa, com o mesmo cuidado e o material necessário.',
  'If you can''t come to the clinic, we come to your home, with the same care and the equipment we need.'
);

-- The team page: who shows, with a title and a short bio. No photos, no e-mail.
ALTER TABLE staff
    ADD COLUMN web_visible  BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN web_title_pt TEXT NOT NULL DEFAULT '',
    ADD COLUMN web_title_en TEXT NOT NULL DEFAULT '',
    ADD COLUMN web_bio_pt   TEXT NOT NULL DEFAULT '',
    ADD COLUMN web_bio_en   TEXT NOT NULL DEFAULT '';

-- The price list on the site is the app's own; an item can be left off the site.
ALTER TABLE products ADD COLUMN web_visible BOOLEAN NOT NULL DEFAULT TRUE;

-- Requests from the website land in the same list as phone/e-mail requests (Schedule › Booking Requests).
ALTER TABLE booking_requests
    ADD COLUMN source      TEXT NOT NULL DEFAULT 'staff' CHECK (source IN ('staff', 'website')),
    ADD COLUMN day_part    TEXT NOT NULL DEFAULT '' CHECK (day_part IN ('', 'morning', 'afternoon', 'any')),
    ADD COLUMN address     TEXT NOT NULL DEFAULT '',     -- home visits only
    ADD COLUMN lang        TEXT NOT NULL DEFAULT 'pt' CHECK (lang IN ('pt', 'en'));
CREATE INDEX booking_requests_source ON booking_requests (source, created_at);

-- ───────── what the website may see ─────────
CREATE SCHEMA portal;

CREATE VIEW portal.site AS
SELECT w.published, w.requests_open, w.home_visits, w.opening_soon, w.show_prices,
       w.tagline_pt, w.tagline_en, w.intro_pt, w.intro_en, w.home_text_pt, w.home_text_en,
       c.name AS clinic_name, c.nif, c.ers_number, c.address, c.phone, c.email, c.opening_hours,
       GREATEST(w.updated_at, COALESCE(c.updated_at, c.created_at)) AS updated_at
FROM website w CROSS JOIN clinic c;

CREATE VIEW portal.services AS
SELECT code, kind, visit_kind, applies_to, name_pt, name_en, price_cents, sessions, valid_months, vat_exempt, sort
FROM products WHERE active AND web_visible;

CREATE VIEW portal.team AS
SELECT name, role = 'owner' AS is_owner, web_title_pt AS title_pt, web_title_en AS title_en, web_bio_pt AS bio_pt, web_bio_en AS bio_en
FROM staff WHERE active AND web_visible AND role IN ('owner', 'therapist');

-- Closures from today on (holidays, training), so the site can say when the clinic is closed.
CREATE VIEW portal.closures AS
SELECT starts_on, ends_on, label FROM closures WHERE ends_on >= CURRENT_DATE;

-- Rate limits for the public form, by a hash of the visitor's address (the address itself is never stored).
CREATE TABLE portal.hits (
    key           TEXT NOT NULL,
    window_start  TIMESTAMPTZ NOT NULL,
    hits          INT NOT NULL DEFAULT 0,
    PRIMARY KEY (key, window_start)
);

-- Counts one hit for `p_key` in the current window; FALSE once the limit is reached.
CREATE FUNCTION portal.hit(p_key TEXT, p_max INT, p_minutes INT) RETURNS BOOLEAN
LANGUAGE plpgsql SECURITY DEFINER SET search_path = portal, pg_temp AS $$
DECLARE w TIMESTAMPTZ := to_timestamp(floor(extract(epoch FROM now()) / (p_minutes * 60)) * (p_minutes * 60)); n INT;
BEGIN
    IF length(p_key) > 200 OR p_minutes < 1 OR p_minutes > 1440 THEN RAISE EXCEPTION 'bad_input'; END IF;
    DELETE FROM portal.hits WHERE window_start < now() - interval '2 days';
    INSERT INTO portal.hits (key, window_start, hits) VALUES (p_key, w, 1)
        ON CONFLICT (key, window_start) DO UPDATE SET hits = portal.hits.hits + 1
        RETURNING hits INTO n;
    RETURN n <= p_max;
END $$;

-- The only way the website writes clinic data: one appointment request, checked here again whatever the
-- website checked. Errors are short codes the website turns into a sentence.
CREATE FUNCTION portal.request_booking(p_service TEXT, p_name TEXT, p_phone TEXT, p_email TEXT, p_day DATE,
                                       p_day_part TEXT, p_address TEXT, p_message TEXT, p_lang TEXT) RETURNS UUID
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
DECLARE s RECORD; v_id UUID := gen_random_uuid(); v_at TIMESTAMPTZ; v_today DATE := (now() AT TIME ZONE 'Europe/Lisbon')::date;
BEGIN
    SELECT published, requests_open, home_visits INTO s FROM website WHERE id = 1;
    IF NOT FOUND OR NOT s.published OR NOT s.requests_open THEN RAISE EXCEPTION 'closed'; END IF;
    IF p_service NOT IN ('assessment', 'session', 'home') THEN RAISE EXCEPTION 'bad_service'; END IF;
    IF p_service = 'home' AND NOT s.home_visits THEN RAISE EXCEPTION 'bad_service'; END IF;
    IF p_day_part NOT IN ('morning', 'afternoon', 'any') OR p_lang NOT IN ('pt', 'en') THEN RAISE EXCEPTION 'bad_input'; END IF;
    IF length(btrim(p_name)) < 2 OR length(p_name) > 120 THEN RAISE EXCEPTION 'bad_name'; END IF;
    IF btrim(p_phone) = '' AND btrim(p_email) = '' THEN RAISE EXCEPTION 'no_contact'; END IF;
    IF length(p_phone) > 40 OR length(p_email) > 200 OR length(p_address) > 300 OR length(p_message) > 1000 THEN RAISE EXCEPTION 'too_long'; END IF;
    IF p_service = 'home' AND length(btrim(p_address)) < 5 THEN RAISE EXCEPTION 'no_address'; END IF;
    IF p_day IS NULL OR p_day <= v_today OR p_day > v_today + 62 THEN RAISE EXCEPTION 'bad_day'; END IF;
    -- A ceiling on everything the website can add in a day, whatever addresses it comes from.
    IF (SELECT count(*) FROM booking_requests WHERE source = 'website' AND created_at > now() - interval '1 day') >= 100 THEN
        RAISE EXCEPTION 'busy';
    END IF;
    v_at := (p_day + CASE p_day_part WHEN 'morning' THEN time '09:00' WHEN 'afternoon' THEN time '14:00' ELSE time '10:00' END)
            AT TIME ZONE 'Europe/Lisbon';
    INSERT INTO booking_requests (id, name, phone, email, service, preferred_at, message, source, day_part, address, lang)
    VALUES (v_id, btrim(p_name), btrim(p_phone), lower(btrim(p_email)), p_service, v_at, btrim(p_message), 'website', p_day_part,
            CASE WHEN p_service = 'home' THEN btrim(p_address) ELSE '' END, p_lang);
    -- Logged without any of the person's details.
    INSERT INTO access_log (staff_id, action, subject_type, subject_id, device, detail)
    VALUES (NULL, 'booking_request.received', NULL, NULL, 'site', v_id::text);
    RETURN v_id;
END $$;

REVOKE ALL ON FUNCTION portal.hit(TEXT, INT, INT) FROM PUBLIC;
REVOKE ALL ON FUNCTION portal.request_booking(TEXT, TEXT, TEXT, TEXT, DATE, TEXT, TEXT, TEXT, TEXT) FROM PUBLIC;

DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'physiopulse_portal') THEN
        CREATE ROLE physiopulse_portal NOLOGIN;
    END IF;
END $$;
GRANT USAGE ON SCHEMA portal TO physiopulse_portal;
GRANT SELECT ON portal.site, portal.services, portal.team, portal.closures TO physiopulse_portal;
GRANT EXECUTE ON FUNCTION portal.hit(TEXT, INT, INT), portal.request_booking(TEXT, TEXT, TEXT, TEXT, DATE, TEXT, TEXT, TEXT, TEXT) TO physiopulse_portal;
