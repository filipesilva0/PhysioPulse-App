-- Phase 2 of the PhysioPulse plan (11 Nov 2026): session recording, speech-to-text on the AI PC, and a
-- plain-text AI report for the therapist to review. Plus one AI switch per person (owner, 11 Nov:
-- "umbrella switch for all ai features").

-- One switch per person for every AI feature their role can reach. The owner always has it; reception
-- never does (no clinical data). Enforced here so no code path can get it wrong.
ALTER TABLE staff ADD COLUMN ai_enabled BOOLEAN NOT NULL DEFAULT FALSE;
CREATE FUNCTION staff_ai_by_role() RETURNS trigger AS $$
BEGIN
    IF NEW.role = 'owner' THEN NEW.ai_enabled := TRUE;
    ELSIF NEW.role = 'reception' THEN NEW.ai_enabled := FALSE;
    END IF;
    RETURN NEW;
END; $$ LANGUAGE plpgsql;
CREATE TRIGGER staff_ai_role BEFORE INSERT OR UPDATE ON staff FOR EACH ROW EXECUTE FUNCTION staff_ai_by_role();
UPDATE staff SET ai_enabled = TRUE WHERE role = 'owner';

-- Where the speech-to-text program (whisper.cpp server) runs: chosen in Settings › Local AI, else WHISPER_URL.
ALTER TABLE clinic ADD COLUMN stt_url TEXT;

-- A recorded session. The audio, the transcript and the AI draft are FILES in AUDIO_DIR/<id>/ — a folder
-- the backups never include — so none of them outlives its deletion. This row holds no clinical text.
-- Deleted (files removed, row marked) when the note made from it is signed, by hand, or after 30 days.
CREATE TABLE recordings (
    id              UUID PRIMARY KEY,
    patient_id      UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    appointment_id  UUID REFERENCES appointments(id) ON DELETE SET NULL,
    template        TEXT NOT NULL DEFAULT '',
    lang            TEXT NOT NULL DEFAULT 'pt' CHECK (lang IN ('pt', 'en')),
    -- recording: chunks arriving · queued: waiting for the AI PC · transcribing · drafting · ready · failed · deleted
    status          TEXT NOT NULL DEFAULT 'recording'
                    CHECK (status IN ('recording', 'queued', 'transcribing', 'drafting', 'ready', 'failed', 'deleted')),
    chunks          INT NOT NULL DEFAULT 0,
    bytes           BIGINT NOT NULL DEFAULT 0,
    seconds         INT NOT NULL DEFAULT 0,
    attempts        INT NOT NULL DEFAULT 0,
    error           TEXT NOT NULL DEFAULT '',     -- a code (stt_failed, ai_failed, too_short…), never content
    note_id         UUID REFERENCES notes(id) ON DELETE SET NULL,
    model           TEXT NOT NULL DEFAULT '',
    created_by      UUID REFERENCES staff(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at     TIMESTAMPTZ,
    ready_at        TIMESTAMPTZ,
    delete_after    TIMESTAMPTZ NOT NULL DEFAULT now() + interval '30 days',
    deleted_at      TIMESTAMPTZ,
    deleted_why     TEXT NOT NULL DEFAULT '' CHECK (deleted_why IN ('', 'signed', 'by_hand', 'expired', 'patient', 'day'))
);
CREATE INDEX recordings_patient ON recordings(patient_id, created_at DESC);
CREATE INDEX recordings_queue ON recordings(status) WHERE status IN ('queued', 'transcribing', 'drafting');
CREATE INDEX recordings_note ON recordings(note_id);
