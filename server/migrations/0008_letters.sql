-- Reimbursement pack and discharge letters (owner decisions, 8 Nov 2026).
--
-- Reimbursement pack: one PDF (short clinical report, sessions, invoices, prescription scan) made by the
-- owner or a therapist and saved in the patient's Documentos folder as a `report` document. No table needed.
--
-- Discharge letters: the local AI (Ollama, on this server) can draft the text; only the owner approves it.
-- Therapists may write a draft by hand. An approved letter is saved as PDF in `Utentes/<n.º>-<nome>/Cartas/`
-- and, like a signed note, can't be changed or removed (except by the owner's legal tools).

-- The professional licence number (cédula profissional, Ordem dos Fisioterapeutas) printed under a signature.
ALTER TABLE staff ADD COLUMN professional_id TEXT NOT NULL DEFAULT '';

CREATE TABLE discharge_letters (
    id           UUID PRIMARY KEY,
    patient_id   UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    lang         TEXT NOT NULL DEFAULT 'pt' CHECK (lang IN ('pt', 'en')),
    addressee    TEXT NOT NULL DEFAULT '',      -- e.g. "Ao(À) médico(a) assistente"
    body         TEXT NOT NULL DEFAULT '',
    ai_drafted   BOOLEAN NOT NULL DEFAULT FALSE, -- the text started from a local-AI draft
    created_by   UUID REFERENCES staff(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    approved_by  UUID REFERENCES staff(id),
    approved_at  TIMESTAMPTZ
);
CREATE INDEX discharge_letters_patient ON discharge_letters(patient_id, created_at DESC);

-- An approved letter is part of the clinical record.
CREATE FUNCTION discharge_letters_locked() RETURNS trigger AS $$
BEGIN
    IF OLD.approved_at IS NOT NULL AND current_setting('clinica.legal_tool', true) IS DISTINCT FROM 'on' THEN
        RAISE EXCEPTION 'an approved discharge letter can''t be changed';
    END IF;
    RETURN COALESCE(NEW, OLD);
END; $$ LANGUAGE plpgsql;
CREATE TRIGGER discharge_letters_lock BEFORE UPDATE OR DELETE ON discharge_letters
    FOR EACH ROW EXECUTE FUNCTION discharge_letters_locked();

ALTER TABLE documents ADD COLUMN letter_id UUID REFERENCES discharge_letters(id) ON DELETE CASCADE;
CREATE UNIQUE INDEX documents_one_per_letter ON documents(letter_id) WHERE letter_id IS NOT NULL;

-- The PDF of an approved letter is locked like the PDFs of signed notes and consents.
CREATE OR REPLACE FUNCTION documents_note_locked() RETURNS trigger AS $$
BEGIN
    IF (OLD.kind IN ('note', 'consent') OR OLD.letter_id IS NOT NULL)
       AND current_setting('clinica.legal_tool', true) IS DISTINCT FROM 'on' THEN
        RAISE EXCEPTION 'the PDF of a signed note, consent or letter can''t be removed';
    END IF;
    RETURN COALESCE(NEW, OLD);
END; $$ LANGUAGE plpgsql;
