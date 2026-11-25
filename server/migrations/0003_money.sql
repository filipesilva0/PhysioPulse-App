-- Phase 3: money. Prices, packages and plans sold to patients, payments in, invoice records,
-- expenses, cash close, salaries, month close. Amounts are whole cents (INT), never floats.
-- Manual route first (owner decision 4 Nov 2026): no Stripe yet; invoices are issued in the
-- company's certified invoicing software and their number + PDF are recorded here.

-- Files the server keeps for money (invoice PDFs, expense receipts, accountant exports).
-- They live under ARCHIVE_DIR/Financas/…; this is the index.
CREATE TABLE finance_files (
    id           UUID PRIMARY KEY,
    kind         TEXT NOT NULL CHECK (kind IN ('invoice', 'expense', 'export')),
    filename     TEXT NOT NULL,
    mime         TEXT NOT NULL,
    bytes        BIGINT NOT NULL,
    path         TEXT NOT NULL,                -- relative to ARCHIVE_DIR
    sha256       TEXT NOT NULL,
    uploaded_by  UUID REFERENCES staff(id),
    uploaded_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Price list. `kind`:
--   visit   one appointment of `visit_kind` (session, assessment, home, class drop-in)
--   pack    `sessions` credits for `applies_to`, valid `valid_months`, optionally shareable, paid in `instalments`
--   monthly `sessions` per calendar month for `applies_to` (null = no limit), charged every month
CREATE TABLE products (
    id            UUID PRIMARY KEY,
    code          TEXT NOT NULL UNIQUE,
    kind          TEXT NOT NULL CHECK (kind IN ('visit', 'pack', 'monthly')),
    visit_kind    TEXT CHECK (visit_kind IN ('session', 'assessment', 'home', 'class')),
    applies_to    TEXT NOT NULL DEFAULT 'session' CHECK (applies_to IN ('session', 'class')),
    name_pt       TEXT NOT NULL,
    name_en       TEXT NOT NULL,
    price_cents   INT NOT NULL CHECK (price_cents >= 0),
    sessions      INT CHECK (sessions > 0),
    valid_months  INT CHECK (valid_months > 0),
    shareable     BOOLEAN NOT NULL DEFAULT FALSE,
    instalments   INT NOT NULL DEFAULT 1 CHECK (instalments BETWEEN 1 AND 12),
    includes_assessment BOOLEAN NOT NULL DEFAULT FALSE,   -- the starter pack: first credit covers the assessment
    active        BOOLEAN NOT NULL DEFAULT TRUE,
    sort          INT NOT NULL DEFAULT 0,
    updated_by    UUID REFERENCES staff(id),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (kind <> 'visit' OR visit_kind IS NOT NULL),
    CHECK (kind <> 'pack' OR sessions IS NOT NULL)
);
-- Only one active visit price per appointment kind (it's what a done appointment costs).
CREATE UNIQUE INDEX products_one_visit_price ON products (visit_kind) WHERE kind = 'visit' AND active;

-- The owner's price list (SPEC.md › Money), with the suggested ladder for the pack of 10
-- and the monthly plans (owner decision 4 Nov 2026). All editable in Finance.
INSERT INTO products (id, code, kind, visit_kind, applies_to, name_pt, name_en, price_cents, sessions, valid_months, shareable, instalments, includes_assessment, sort) VALUES
 (gen_random_uuid(), 'assessment', 'visit',   'assessment', 'session', 'Avaliação inicial (60 min)',        'First assessment (60 min)',         3000, NULL, NULL, FALSE, 1, FALSE, 10),
 (gen_random_uuid(), 'single',     'visit',   'session',    'session', 'Sessão (45 min)',                   'Single session (45 min)',           2000, NULL, NULL, FALSE, 1, FALSE, 20),
 (gen_random_uuid(), 'home',       'visit',   'home',       'session', 'Domicílio (60 min)',                'Home visit (60 min)',               4000, NULL, NULL, FALSE, 1, FALSE, 30),
 (gen_random_uuid(), 'class1',     'visit',   'class',      'class',   'Aula de grupo',                     'Group class, drop-in',              1000, NULL, NULL, FALSE, 1, FALSE, 40),
 (gen_random_uuid(), 'starter',    'pack',    NULL,         'session', 'Início: avaliação + 4 sessões',     'Starter: assessment + 4 sessions',  10000,   5,    2, FALSE, 1, TRUE,  50),
 (gen_random_uuid(), 'pack5',      'pack',    NULL,         'session', 'Pacote de 5',                       'Pack of 5',                          8000,   5,    3, FALSE, 1, FALSE, 60),
 (gen_random_uuid(), 'pack10',     'pack',    NULL,         'session', 'Pacote de 10',                      'Pack of 10',                        17000,  10,    6, TRUE,  1, FALSE, 70),
 (gen_random_uuid(), 'rehab20',    'pack',    NULL,         'session', 'Reabilitação, 20 sessões',          'Rehab pack of 20',                  30000,  20,    9, TRUE,  2, FALSE, 80),
 (gen_random_uuid(), 'cc4',        'monthly', NULL,         'session', 'Acompanhamento, 4 por mês',         'Continuous care, 4 a month',         7200,   4, NULL, FALSE, 1, FALSE, 90),
 (gen_random_uuid(), 'cc8',        'monthly', NULL,         'session', 'Acompanhamento, 8 por mês',         'Continuous care, 8 a month',        13600,   8, NULL, FALSE, 1, FALSE, 100),
 (gen_random_uuid(), 'class10',    'pack',    NULL,         'class',   'Cartão de 10 aulas',                'Class card, 10 classes',             8000,  10,    3, FALSE, 1, FALSE, 110),
 (gen_random_uuid(), 'class2w',    'monthly', NULL,         'class',   'Aulas, 2 por semana',               'Group classes, 2 a week',            7500, NULL, NULL, FALSE, 1, FALSE, 120);

-- A package or plan sold to a patient. Name, price and credits are copied from the product at sale,
-- so later price changes don't rewrite what someone already bought.
CREATE TABLE patient_plans (
    id              UUID PRIMARY KEY,
    patient_id      UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    product_id      UUID REFERENCES products(id) ON DELETE SET NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('pack', 'monthly')),
    applies_to      TEXT NOT NULL CHECK (applies_to IN ('session', 'class')),
    name_pt         TEXT NOT NULL,
    name_en         TEXT NOT NULL,
    price_cents     INT NOT NULL CHECK (price_cents >= 0),     -- whole pack, or per month
    sessions        INT,                                        -- pack: credits; monthly: per month (null = no limit)
    instalments     INT NOT NULL DEFAULT 1,
    shareable       BOOLEAN NOT NULL DEFAULT FALSE,
    includes_assessment BOOLEAN NOT NULL DEFAULT FALSE,
    shared_with     UUID REFERENCES patients(id) ON DELETE SET NULL,   -- one family member may use it too
    starts_on       DATE NOT NULL DEFAULT CURRENT_DATE,
    expires_on      DATE,                                       -- packs
    -- cancelled = sold by mistake (only while nothing was used or paid): no credits, nothing owed
    status          TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused', 'ended', 'cancelled')),
    ended_on        DATE,                                       -- monthly: last month charged is this one's month
    skipped_months  DATE[] NOT NULL DEFAULT '{}',               -- monthly: first-of-month dates not charged (paused)
    note            TEXT NOT NULL DEFAULT '',
    created_by      UUID REFERENCES staff(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (shared_with IS NULL OR shared_with <> patient_id)
);
CREATE INDEX patient_plans_patient ON patient_plans(patient_id);
CREATE INDEX patient_plans_shared ON patient_plans(shared_with);

-- What a done appointment used and cost. Set by the server when the appointment is marked done
-- (a credit from an active plan, or the visit price), cleared if it goes back to booked/missed.
ALTER TABLE appointments
    ADD COLUMN plan_id       UUID REFERENCES patient_plans(id) ON DELETE SET NULL,
    ADD COLUMN charge_cents  INT CHECK (charge_cents >= 0),
    ADD COLUMN charge_waived BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX appointments_plan ON appointments(plan_id);
ALTER TABLE class_attendees
    ADD COLUMN plan_id       UUID REFERENCES patient_plans(id) ON DELETE SET NULL,
    ADD COLUMN charge_cents  INT CHECK (charge_cents >= 0),
    ADD COLUMN charge_waived BOOLEAN NOT NULL DEFAULT FALSE;

-- Payments in. A fiscal record: never deleted; a mistake is voided with a reason.
-- One row per thing paid for: an appointment, a plan (pack price or instalment; with `covers_month`
-- for a monthly plan), or something else described in words.
CREATE TABLE payments (
    id               UUID PRIMARY KEY,
    number           INT GENERATED ALWAYS AS IDENTITY UNIQUE,   -- internal receipt number
    received_on      DATE NOT NULL DEFAULT CURRENT_DATE,
    patient_id       UUID REFERENCES patients(id) ON DELETE RESTRICT,
    amount_cents     INT NOT NULL CHECK (amount_cents > 0),
    method           TEXT NOT NULL CHECK (method IN ('cash', 'mbway', 'transfer', 'card', 'stripe')),
    description      TEXT NOT NULL DEFAULT '',
    appointment_id   UUID REFERENCES appointments(id) ON DELETE SET NULL,
    plan_id          UUID REFERENCES patient_plans(id) ON DELETE SET NULL,
    covers_month     DATE CHECK (covers_month = date_trunc('month', covers_month)::date),
    invoice_number   TEXT NOT NULL DEFAULT '',
    invoice_file_id  UUID REFERENCES finance_files(id) ON DELETE SET NULL,
    invoiced_at      TIMESTAMPTZ,
    note             TEXT NOT NULL DEFAULT '',
    created_by       UUID REFERENCES staff(id),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    voided_at        TIMESTAMPTZ,
    voided_by        UUID REFERENCES staff(id),
    void_reason      TEXT NOT NULL DEFAULT ''
);
CREATE INDEX payments_day ON payments(received_on);
CREATE INDEX payments_patient ON payments(patient_id);
CREATE INDEX payments_appointment ON payments(appointment_id);
CREATE INDEX payments_plan ON payments(plan_id);

-- Expenses (the clinic's own costs). Receipt photo/PDF optional; AI reading arrives in phase 4.
CREATE TABLE expenses (
    id            UUID PRIMARY KEY,
    spent_on      DATE NOT NULL,
    supplier      TEXT NOT NULL DEFAULT '',
    supplier_nif  TEXT NOT NULL DEFAULT '',
    category      TEXT NOT NULL DEFAULT 'other',
    description   TEXT NOT NULL DEFAULT '',
    amount_cents  INT NOT NULL CHECK (amount_cents > 0),
    vat_cents     INT CHECK (vat_cents >= 0),
    method        TEXT NOT NULL DEFAULT 'transfer' CHECK (method IN ('cash', 'mbway', 'transfer', 'card', 'debit')),
    document_ref  TEXT NOT NULL DEFAULT '',            -- supplier's invoice number
    file_id       UUID REFERENCES finance_files(id) ON DELETE SET NULL,
    created_by    UUID REFERENCES staff(id),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX expenses_day ON expenses(spent_on);

-- End-of-day cash close: what the payments say vs what was counted, per method.
CREATE TABLE cash_closes (
    day          DATE PRIMARY KEY,
    expected     JSONB NOT NULL,     -- {"cash": 5000, "mbway": 16000, ...} in cents
    counted      JSONB NOT NULL,
    difference_cents INT NOT NULL,   -- counted − expected, all methods
    note         TEXT NOT NULL DEFAULT '',
    closed_by    UUID REFERENCES staff(id),
    closed_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Pay settings per person (fixed now; commission and room rental later).
CREATE TABLE staff_pay (
    staff_id        UUID PRIMARY KEY REFERENCES staff(id) ON DELETE CASCADE,
    model           TEXT NOT NULL DEFAULT 'fixed' CHECK (model IN ('fixed', 'commission', 'rental')),
    fixed_cents     INT NOT NULL DEFAULT 0 CHECK (fixed_cents >= 0),
    commission_pct  NUMERIC(5,2),
    rental_cents    INT,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE salary_payments (
    id           UUID PRIMARY KEY,
    staff_id     UUID NOT NULL REFERENCES staff(id),
    month        DATE NOT NULL CHECK (month = date_trunc('month', month)::date),
    gross_cents  INT NOT NULL CHECK (gross_cents > 0),
    paid_on      DATE NOT NULL DEFAULT CURRENT_DATE,
    method       TEXT NOT NULL DEFAULT 'transfer' CHECK (method IN ('cash', 'transfer')),
    note         TEXT NOT NULL DEFAULT '',
    created_by   UUID REFERENCES staff(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX salary_payments_month ON salary_payments(month);

-- A closed month: its payments, expenses and salaries can't change until the owner reopens it.
CREATE TABLE month_closes (
    month      DATE PRIMARY KEY CHECK (month = date_trunc('month', month)::date),
    totals     JSONB NOT NULL,
    closed_by  UUID REFERENCES staff(id),
    closed_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Locks ---------------------------------------------------------------------

CREATE FUNCTION month_is_closed(d DATE) RETURNS BOOLEAN AS $$
    SELECT EXISTS (SELECT 1 FROM month_closes WHERE month = date_trunc('month', d)::date);
$$ LANGUAGE sql STABLE;

-- Payments: no deletes ever; only invoice details, the note and voiding can change; nothing in a closed month.
CREATE FUNCTION payments_guard() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'payments are never deleted; void them';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF month_is_closed(NEW.received_on) THEN RAISE EXCEPTION 'month closed'; END IF;
        RETURN NEW;
    END IF;
    -- The appointment or plan it pointed to was deleted (ON DELETE SET NULL): always allowed.
    IF (to_jsonb(NEW) - 'appointment_id' - 'plan_id' - 'invoice_file_id') = (to_jsonb(OLD) - 'appointment_id' - 'plan_id' - 'invoice_file_id')
       AND (NEW.appointment_id IS NULL OR NEW.appointment_id = OLD.appointment_id)
       AND (NEW.plan_id IS NULL OR NEW.plan_id = OLD.plan_id)
       AND (NEW.invoice_file_id IS NULL OR NEW.invoice_file_id IS NOT DISTINCT FROM OLD.invoice_file_id) THEN
        RETURN NEW;
    END IF;
    IF month_is_closed(OLD.received_on) THEN RAISE EXCEPTION 'month closed'; END IF;
    IF OLD.voided_at IS NOT NULL THEN RAISE EXCEPTION 'voided payment'; END IF;
    -- Only the invoice details, the note and voiding may change.
    IF (to_jsonb(NEW) - 'invoice_number' - 'invoice_file_id' - 'invoiced_at' - 'note' - 'voided_at' - 'voided_by' - 'void_reason')
       IS DISTINCT FROM
       (to_jsonb(OLD) - 'invoice_number' - 'invoice_file_id' - 'invoiced_at' - 'note' - 'voided_at' - 'voided_by' - 'void_reason') THEN
        RAISE EXCEPTION 'a payment can''t be changed; void it and record it again';
    END IF;
    RETURN NEW;
END; $$ LANGUAGE plpgsql;
CREATE TRIGGER payments_lock BEFORE INSERT OR UPDATE OR DELETE ON payments
    FOR EACH ROW EXECUTE FUNCTION payments_guard();

CREATE FUNCTION closed_month_guard() RETURNS trigger AS $$
DECLARE d_old DATE; d_new DATE;
BEGIN
    IF TG_TABLE_NAME = 'expenses' THEN
        IF TG_OP <> 'INSERT' THEN d_old := OLD.spent_on; END IF;
        IF TG_OP <> 'DELETE' THEN d_new := NEW.spent_on; END IF;
    ELSE
        IF TG_OP <> 'INSERT' THEN d_old := OLD.month; END IF;
        IF TG_OP <> 'DELETE' THEN d_new := NEW.month; END IF;
    END IF;
    IF (d_old IS NOT NULL AND month_is_closed(d_old)) OR (d_new IS NOT NULL AND month_is_closed(d_new)) THEN
        RAISE EXCEPTION 'month closed';
    END IF;
    RETURN COALESCE(NEW, OLD);
END; $$ LANGUAGE plpgsql;
CREATE TRIGGER expenses_lock BEFORE INSERT OR UPDATE OR DELETE ON expenses
    FOR EACH ROW EXECUTE FUNCTION closed_month_guard();
CREATE TRIGGER salary_payments_lock BEFORE INSERT OR UPDATE OR DELETE ON salary_payments
    FOR EACH ROW EXECUTE FUNCTION closed_month_guard();
