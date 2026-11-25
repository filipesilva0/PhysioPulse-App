-- Phase 2: patients, clinical record, consents, prescriptions, schedule, waitlist, booking requests.
-- Clinical fields live in their own tables so the server can leave them out for reception.

CREATE TABLE patients (
    id               UUID PRIMARY KEY,
    number           INT GENERATED ALWAYS AS IDENTITY UNIQUE,   -- n.º de utente da clínica
    name             TEXT NOT NULL,
    birth_date       DATE,
    sex              TEXT CHECK (sex IN ('f', 'm', 'x')),
    nif              TEXT,
    sns_number       TEXT,
    phone            TEXT,
    email            TEXT,
    address          TEXT,
    guardian_name    TEXT,                -- for minors
    guardian_phone   TEXT,
    lang             TEXT NOT NULL DEFAULT 'pt' CHECK (lang IN ('pt', 'en')),
    reminders        TEXT NOT NULL DEFAULT 'sms' CHECK (reminders IN ('sms', 'email', 'none')),
    therapist_id     UUID REFERENCES staff(id),
    admin_note       TEXT NOT NULL DEFAULT '',   -- non-clinical (billing, contacts); reception can see it
    active           BOOLEAN NOT NULL DEFAULT TRUE,
    anonymised_at    TIMESTAMPTZ,
    created_by       UUID REFERENCES staff(id),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX patients_name ON patients (lower(name));

-- Anamnesis: one current version per patient (changes are recorded in the access log).
CREATE TABLE anamnesis (
    patient_id       UUID PRIMARY KEY REFERENCES patients(id) ON DELETE CASCADE,
    main_complaint   TEXT NOT NULL DEFAULT '',
    history          TEXT NOT NULL DEFAULT '',
    medication       TEXT NOT NULL DEFAULT '',
    medical_history  TEXT NOT NULL DEFAULT '',
    goals            TEXT NOT NULL DEFAULT '',
    regions          JSONB NOT NULL DEFAULT '[]',   -- [{region, side?, symptoms: [..], notes}]
    red_flags        JSONB NOT NULL DEFAULT '[]',   -- ["night_pain", ...]
    updated_by       UUID REFERENCES staff(id),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Schedule ---------------------------------------------------------------

CREATE TABLE series (
    id          UUID PRIMARY KEY,
    weekdays    INT[] NOT NULL,          -- ISO 1 = Monday … 7 = Sunday
    until_date  DATE,
    count       INT,
    created_by  UUID REFERENCES staff(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE prescriptions (
    id              UUID PRIMARY KEY,
    patient_id      UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    doctor          TEXT NOT NULL DEFAULT '',
    specialty       TEXT NOT NULL DEFAULT '',
    issued_on       DATE NOT NULL,
    sessions_total  INT NOT NULL CHECK (sessions_total > 0),
    valid_until     DATE,
    payer           TEXT NOT NULL DEFAULT '',    -- e.g. ADSE, insurer name
    document_id     UUID,                         -- scan, set once documents exist
    created_by      UUID REFERENCES staff(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX prescriptions_patient ON prescriptions(patient_id);

CREATE TABLE appointments (
    id               UUID PRIMARY KEY,
    kind             TEXT NOT NULL CHECK (kind IN ('session', 'assessment', 'class', 'home', 'block')),
    starts_at        TIMESTAMPTZ NOT NULL,
    ends_at          TIMESTAMPTZ NOT NULL,
    therapist_id     UUID REFERENCES staff(id),
    patient_id       UUID REFERENCES patients(id) ON DELETE CASCADE,   -- sessions, assessments, home visits
    series_id        UUID REFERENCES series(id) ON DELETE SET NULL,
    prescription_id  UUID REFERENCES prescriptions(id) ON DELETE SET NULL,
    status           TEXT NOT NULL DEFAULT 'booked' CHECK (status IN ('booked', 'done', 'cancelled', 'no_show')),
    title            TEXT NOT NULL DEFAULT '',   -- class name or blocked-time label
    color            TEXT NOT NULL DEFAULT '',   -- blocked time
    address          TEXT NOT NULL DEFAULT '',   -- home visits
    travel_minutes   INT NOT NULL DEFAULT 0,     -- home visits: buffer before and after
    capacity         INT,                        -- classes
    admin_note       TEXT NOT NULL DEFAULT '',
    created_by       UUID REFERENCES staff(id),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (ends_at > starts_at),
    CHECK (kind IN ('class', 'block') OR patient_id IS NOT NULL),
    CHECK (kind <> 'class' OR capacity > 0)
);
CREATE INDEX appointments_time ON appointments(starts_at);
CREATE INDEX appointments_patient ON appointments(patient_id);
CREATE INDEX appointments_series ON appointments(series_id);

CREATE TABLE class_attendees (
    appointment_id  UUID NOT NULL REFERENCES appointments(id) ON DELETE CASCADE,
    patient_id      UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    attended        BOOLEAN,
    added_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (appointment_id, patient_id)
);

CREATE TABLE waitlist (
    id            UUID PRIMARY KEY,
    patient_id    UUID REFERENCES patients(id) ON DELETE CASCADE,
    name          TEXT NOT NULL DEFAULT '',     -- when not yet a patient
    phone         TEXT NOT NULL DEFAULT '',
    email         TEXT NOT NULL DEFAULT '',
    therapist_id  UUID REFERENCES staff(id),     -- preferred therapist (optional)
    weekdays      INT[] NOT NULL DEFAULT '{}',   -- empty = any day
    from_time     TIME,                          -- optional preferred window
    to_time       TIME,
    note          TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL DEFAULT 'waiting' CHECK (status IN ('waiting', 'offered', 'booked', 'removed')),
    offered_appointment UUID REFERENCES appointments(id) ON DELETE SET NULL,
    offer_expires_at    TIMESTAMPTZ,
    created_by    UUID REFERENCES staff(id),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (patient_id IS NOT NULL OR name <> '')
);

-- From the patient portal later; entered by hand until then.
CREATE TABLE booking_requests (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL,
    phone           TEXT NOT NULL DEFAULT '',
    email           TEXT NOT NULL DEFAULT '',
    patient_id      UUID REFERENCES patients(id) ON DELETE SET NULL,
    service         TEXT NOT NULL DEFAULT 'session',
    preferred_at    TIMESTAMPTZ NOT NULL,
    message         TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'accepted', 'declined')),
    appointment_id  UUID REFERENCES appointments(id) ON DELETE SET NULL,
    decided_by      UUID REFERENCES staff(id),
    decided_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Clinical record ---------------------------------------------------------

-- Progress notes: draft until signed; a signed note can't be changed or deleted.
CREATE TABLE notes (
    id              UUID PRIMARY KEY,
    patient_id      UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    appointment_id  UUID REFERENCES appointments(id) ON DELETE SET NULL,
    template        TEXT NOT NULL DEFAULT '',
    blocks          JSONB NOT NULL DEFAULT '[]',    -- [{type: slider|choice|checklist|text, label, value}]
    text            TEXT NOT NULL DEFAULT '',
    created_by      UUID REFERENCES staff(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    signed_by       UUID REFERENCES staff(id),
    signed_at       TIMESTAMPTZ
);
CREATE INDEX notes_patient ON notes(patient_id, created_at DESC);

CREATE FUNCTION notes_signed_locked() RETURNS trigger AS $$
BEGIN
    IF OLD.signed_at IS NOT NULL THEN
        -- Allowed only: whole-patient deletion (cascade) and anonymisation run by the owner's legal tools,
        -- which set this flag inside their own transaction.
        IF current_setting('clinica.legal_tool', true) = 'on' THEN
            RETURN COALESCE(NEW, OLD);
        END IF;
        RAISE EXCEPTION 'a signed note can''t be changed';
    END IF;
    RETURN COALESCE(NEW, OLD);
END; $$ LANGUAGE plpgsql;
CREATE TRIGGER notes_lock BEFORE UPDATE OR DELETE ON notes
    FOR EACH ROW EXECUTE FUNCTION notes_signed_locked();

-- Tests and questionnaires (the PhysioPulse library is rebuilt in the app; the server stores results).
CREATE TABLE test_results (
    id              UUID PRIMARY KEY,
    patient_id      UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    test_code       TEXT NOT NULL,          -- e.g. 'ndi', 'odi', 'quickdash', 'eva'
    answers         JSONB NOT NULL DEFAULT '{}',
    score           NUMERIC NOT NULL,
    max_score       NUMERIC,
    higher_is_worse BOOLEAN NOT NULL DEFAULT TRUE,
    taken_on        DATE NOT NULL DEFAULT CURRENT_DATE,
    recorded_by     UUID REFERENCES staff(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX test_results_patient ON test_results(patient_id, test_code, taken_on);

-- Consents ----------------------------------------------------------------

CREATE TABLE consent_texts (
    id          UUID PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('treatment', 'data', 'media', 'marketing', 'guardian', 'invasive')),
    version     INT NOT NULL,
    lang        TEXT NOT NULL CHECK (lang IN ('pt', 'en')),
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,
    valid_months INT,                        -- null = doesn't expire
    created_by  UUID REFERENCES staff(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (kind, version, lang)
);

CREATE TABLE consents (
    id              UUID PRIMARY KEY,
    patient_id      UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    text_id         UUID NOT NULL REFERENCES consent_texts(id),
    kind            TEXT NOT NULL,
    granted         BOOLEAN NOT NULL,        -- data/media/marketing can be refused
    signed_by_name  TEXT NOT NULL,           -- patient or guardian
    signature_png   TEXT NOT NULL,           -- data: URL of the on-screen signature
    signed_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_on      DATE,
    revoked_at      TIMESTAMPTZ,
    recorded_by     UUID REFERENCES staff(id)
);
CREATE INDEX consents_patient ON consents(patient_id, kind);

-- Documents (files live in the archive folder; this is the index).
CREATE TABLE documents (
    id           UUID PRIMARY KEY,
    patient_id   UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL DEFAULT 'other',   -- photo, report, prescription, letter, other
    filename     TEXT NOT NULL,
    mime         TEXT NOT NULL,
    bytes        BIGINT NOT NULL,
    path         TEXT NOT NULL,                   -- relative to ARCHIVE_DIR
    sha256       TEXT NOT NULL,
    uploaded_by  UUID REFERENCES staff(id),
    uploaded_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX documents_patient ON documents(patient_id);
ALTER TABLE prescriptions ADD FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE SET NULL;
