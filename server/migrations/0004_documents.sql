-- Phase 2: patient documents in the archive, and signed notes saved there as PDF.
-- The `documents` table (0002) is the index; the files live under ARCHIVE_DIR/Utentes/<n.º>-<nome>/.

ALTER TABLE documents ADD COLUMN note_id UUID REFERENCES notes(id) ON DELETE CASCADE;
ALTER TABLE documents ADD CONSTRAINT documents_kind
    CHECK (kind IN ('photo', 'report', 'imaging', 'prescription', 'letter', 'note', 'other'));
CREATE UNIQUE INDEX documents_one_per_note ON documents(note_id) WHERE note_id IS NOT NULL;

-- The PDF of a signed note is part of the clinical record: like the note itself it can't be removed,
-- except by the owner's legal tools (whole-patient deletion), which set clinica.legal_tool in their transaction.
CREATE FUNCTION documents_note_locked() RETURNS trigger AS $$
BEGIN
    IF OLD.kind = 'note' AND current_setting('clinica.legal_tool', true) IS DISTINCT FROM 'on' THEN
        RAISE EXCEPTION 'the PDF of a signed note can''t be removed';
    END IF;
    RETURN COALESCE(NEW, OLD);
END; $$ LANGUAGE plpgsql;
CREATE TRIGGER documents_note_lock BEFORE UPDATE OR DELETE ON documents
    FOR EACH ROW EXECUTE FUNCTION documents_note_locked();
