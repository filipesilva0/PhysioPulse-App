-- The assistant can have its own local-AI model (13 Nov 2026): it needs one that uses tools (e.g. qwen3:8b),
-- while letters, notes and receipts can stay on the main model. NULL = use the main model.
ALTER TABLE clinic ADD COLUMN ai_assistant_model TEXT;

-- What the assistant with tools can prepare besides the schedule and the owner's jobs: patient files (new, details),
-- the anamnesis and progress-note drafts. Still previews that wait for Apply and can be undone.
ALTER TABLE assistant_changes DROP CONSTRAINT IF EXISTS assistant_changes_skill_check;
ALTER TABLE assistant_changes ADD CONSTRAINT assistant_changes_skill_check CHECK (skill IN (
    'price', 'product_active', 'hours', 'closure', 'staff_pay', 'staff_active',
    'book', 'move', 'cancel', 'mark', 'accept_request', 'decline_request', 'waitlist_add',
    'patient_create', 'patient_update', 'anamnesis', 'note_draft'));
