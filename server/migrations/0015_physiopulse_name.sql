-- 11 Nov 2026: the app is now PhysioPulse, internal names included (DECISIONS.md, 11 Nov, rename).
-- The owner's legal tools pass the record locks with a transaction-local setting. It was called
-- clinica.legal_tool; from here on it is physiopulse.legal_tool. Same rules, new name: these are the
-- latest bodies of each lock (0002, 0008), with only the setting's name changed.

CREATE OR REPLACE FUNCTION notes_signed_locked() RETURNS trigger AS $$
BEGIN
    IF OLD.signed_at IS NOT NULL THEN
        -- Allowed only: whole-patient deletion (cascade) and anonymisation run by the owner's legal tools,
        -- which set this flag inside their own transaction.
        IF current_setting('physiopulse.legal_tool', true) = 'on' THEN
            RETURN COALESCE(NEW, OLD);
        END IF;
        RAISE EXCEPTION 'a signed note can''t be changed';
    END IF;
    RETURN COALESCE(NEW, OLD);
END; $$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION discharge_letters_locked() RETURNS trigger AS $$
BEGIN
    IF OLD.approved_at IS NOT NULL AND current_setting('physiopulse.legal_tool', true) IS DISTINCT FROM 'on' THEN
        RAISE EXCEPTION 'an approved discharge letter can''t be changed';
    END IF;
    RETURN COALESCE(NEW, OLD);
END; $$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION documents_note_locked() RETURNS trigger AS $$
BEGIN
    IF (OLD.kind IN ('note', 'consent') OR OLD.letter_id IS NOT NULL)
       AND current_setting('physiopulse.legal_tool', true) IS DISTINCT FROM 'on' THEN
        RAISE EXCEPTION 'the PDF of a signed note, consent or letter can''t be removed';
    END IF;
    RETURN COALESCE(NEW, OLD);
END; $$ LANGUAGE plpgsql;
