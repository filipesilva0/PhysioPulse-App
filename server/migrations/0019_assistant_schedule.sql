-- The assistant books and changes appointments; requests can be booked with one press (owner, 11 Nov 2026).

-- The AI switch now also opens the assistant's schedule jobs, so reception can have it too (it only ever unlocks
-- the schedule for them: recording, note write-up and letters stay clinical, `auth::ai_allowed`). The owner is
-- always on. Someone who becomes reception starts with it off; the owner turns it on in Team.
CREATE OR REPLACE FUNCTION staff_ai_by_role() RETURNS trigger AS $$
BEGIN
    IF NEW.role = 'owner' THEN NEW.ai_enabled := TRUE;
    ELSIF TG_OP = 'INSERT' AND NEW.role = 'reception' THEN NEW.ai_enabled := FALSE;
    ELSIF TG_OP = 'UPDATE' AND NEW.role IS DISTINCT FROM OLD.role THEN NEW.ai_enabled := FALSE;
    END IF;
    RETURN NEW;
END; $$ LANGUAGE plpgsql;

-- The new jobs the assistant can prepare. Each is still a preview that waits for Apply and can be undone.
ALTER TABLE assistant_changes DROP CONSTRAINT IF EXISTS assistant_changes_skill_check;
ALTER TABLE assistant_changes ADD CONSTRAINT assistant_changes_skill_check CHECK (skill IN (
    'price', 'product_active', 'hours', 'closure', 'staff_pay', 'staff_active',
    'book', 'move', 'cancel', 'mark', 'accept_request', 'decline_request', 'waitlist_add'));
CREATE INDEX IF NOT EXISTS assistant_changes_by ON assistant_changes (created_by, created_at);

-- A request booked with one press can make the patient's file; Undo removes that file again if nothing else uses it.
ALTER TABLE booking_requests ADD COLUMN created_patient BOOLEAN NOT NULL DEFAULT FALSE;
