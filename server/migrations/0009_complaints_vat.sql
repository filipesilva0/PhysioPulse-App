-- Complaints log (ERS), VAT-exempt price-list items, 8 Nov 2026. See claude/LEGAL.md.

-- Complaints from the Livro de Reclamações (paper or electronic) or made another way.
-- Paper complaints must reach the ERS within 10 working days (Regulamento ERS 65/2015, art. 9.º);
-- `due_on` is that day, worked out by the server (weekends and national holidays don't count).
-- Kept at least 3 years (LEGAL.md). Owner only (finances, staff and the clinic's legal side are the owner's).
CREATE TABLE complaints (
    id              UUID PRIMARY KEY,
    received_on     DATE NOT NULL,
    source          TEXT NOT NULL CHECK (source IN ('paper', 'electronic', 'other')),
    book_ref        TEXT NOT NULL DEFAULT '',     -- sheet number in the paper book, or the electronic book's reference
    patient_id      UUID REFERENCES patients(id) ON DELETE SET NULL,
    complainant     TEXT NOT NULL DEFAULT '',
    summary         TEXT NOT NULL DEFAULT '',
    reply           TEXT NOT NULL DEFAULT '',     -- what the clinic answered: accepts the facts or not, and why
    replied_on      DATE,
    sent_to_ers_on  DATE,
    due_on          DATE NOT NULL,
    created_by      UUID REFERENCES staff(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX complaints_received ON complaints (received_on DESC);

-- VAT. Physiotherapy with a therapeutic purpose is exempt (art. 9.º, n.º 1 CIVA); fitness-style classes
-- are taxed at 23 %. Classes start as "not exempt": the owner switches one on when it's therapeutic and
-- given by a physiotherapist (e.g. clinical Pilates).
ALTER TABLE products ADD COLUMN vat_exempt BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE products SET vat_exempt = FALSE WHERE applies_to = 'class';

-- Copied at sale, like the name and price, so a later change doesn't rewrite what was sold.
ALTER TABLE patient_plans ADD COLUMN vat_exempt BOOLEAN;
UPDATE patient_plans pp SET vat_exempt = pr.vat_exempt FROM products pr WHERE pr.id = pp.product_id;
