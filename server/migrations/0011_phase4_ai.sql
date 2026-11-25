-- Phase 4: the local AI's work (owner only) — note rewrite, receipt reading, monthly reports, the admin
-- assistant — plus real opening hours and closures (the assistant's "hours and closures" skill).

-- A progress note whose text started from a local-AI rewrite (shown as a label; the AI never saves).
ALTER TABLE notes ADD COLUMN ai_assisted BOOLEAN NOT NULL DEFAULT FALSE;

-- An expense whose details were pre-filled by the AI from the receipt (the owner confirmed them).
ALTER TABLE expenses ADD COLUMN ai_read BOOLEAN NOT NULL DEFAULT FALSE;

-- Opening hours per weekday: ["08:00","20:00"] or null (closed).
ALTER TABLE clinic ADD COLUMN opening_hours JSONB NOT NULL DEFAULT
  '{"mon":["08:00","20:00"],"tue":["08:00","20:00"],"wed":["08:00","20:00"],"thu":["08:00","20:00"],"fri":["08:00","20:00"],"sat":["09:00","13:00"],"sun":null}';

-- The clinic closed for one or more days (holidays, training, works). Each open day gets a blocked-time
-- appointment with no therapist, so the schedule shows it; removing the closure removes those blocks.
CREATE TABLE closures (
    id          UUID PRIMARY KEY,
    starts_on   DATE NOT NULL,
    ends_on     DATE NOT NULL,
    label       TEXT NOT NULL DEFAULT '',
    created_by  UUID REFERENCES staff(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (ends_on >= starts_on AND ends_on - starts_on <= 92)
);
ALTER TABLE appointments ADD COLUMN closure_id UUID REFERENCES closures(id) ON DELETE CASCADE;
CREATE INDEX appointments_closure ON appointments(closure_id);

-- Monthly reports the owner kept: the figures are worked out by the server; the text is the AI's draft as
-- the owner accepted (and maybe corrected) it. Also saved as a PDF in Financas/<ano>/Relatorios.
ALTER TABLE finance_files DROP CONSTRAINT finance_files_kind_check;
ALTER TABLE finance_files ADD CONSTRAINT finance_files_kind_check CHECK (kind IN ('invoice', 'expense', 'export', 'report'));
CREATE TABLE ai_reports (
    id          UUID PRIMARY KEY,
    month       DATE NOT NULL CHECK (month = date_trunc('month', month)::date),
    lang        TEXT NOT NULL CHECK (lang IN ('pt', 'en')),
    facts       JSONB NOT NULL,
    text        TEXT NOT NULL,
    model       TEXT NOT NULL DEFAULT '',
    file_id     UUID REFERENCES finance_files(id) ON DELETE SET NULL,
    created_by  UUID REFERENCES staff(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ai_reports_month ON ai_reports(month DESC);

-- Changes the admin assistant proposed. Nothing changes until the owner presses Apply; every applied
-- change can be undone while what it changed is still as it left it.
CREATE TABLE assistant_changes (
    id          UUID PRIMARY KEY,
    skill       TEXT NOT NULL CHECK (skill IN ('price', 'product_active', 'hours', 'closure', 'staff_pay', 'staff_active')),
    target      UUID,                    -- product, staff member or (after applying) closure
    params      JSONB NOT NULL,          -- what to do, as the server understood it
    before      JSONB NOT NULL,
    after       JSONB NOT NULL,
    rows        JSONB NOT NULL,          -- [[what, before, after], …] as shown to the owner
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'applied', 'cancelled', 'undone')),
    created_by  UUID REFERENCES staff(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at  TIMESTAMPTZ
);
