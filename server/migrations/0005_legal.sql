-- Phase 2: legal tools on the patient file (owner decision, 7 Nov 2026).
-- Export: owner and therapists. Anonymise and delete: owner only, after typing the patient's name.
--
-- A deleted patient who has payments can't disappear completely: payments are fiscal records and are
-- never deleted (payments_guard). Their row stays as a nameless placeholder ("Utente apagado") with
-- `deleted_at` set; everything clinical and every contact detail is removed. A deleted patient with no
-- payments is removed outright.

ALTER TABLE patients ADD COLUMN deleted_at TIMESTAMPTZ;
CREATE INDEX patients_live ON patients (active) WHERE deleted_at IS NULL;
