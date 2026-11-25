-- The assistant, stage 3 (13 Nov 2026): documents, letters, legal tools, settings. Renaming a document, recording a
-- consent withdrawal (can't be undone), marking reminders sent, discharge-letter drafts and edits, the reimbursement
-- pack; the owner's complaints log, clinic details, reminder message, AI switches and website. Still previews that
-- wait for Apply. The assistant never approves letters, signs, deletes, anonymises or exports.
ALTER TABLE assistant_changes DROP CONSTRAINT IF EXISTS assistant_changes_skill_check;
ALTER TABLE assistant_changes ADD CONSTRAINT assistant_changes_skill_check CHECK (skill IN (
    'price', 'product_active', 'hours', 'closure', 'staff_pay', 'staff_active',
    'book', 'move', 'cancel', 'mark', 'accept_request', 'decline_request', 'waitlist_add',
    'patient_create', 'patient_update', 'anamnesis', 'note_draft',
    'payment', 'payment_void', 'invoice_number', 'plan_sell', 'plan_status',
    'expense', 'salary', 'waive', 'month_close', 'month_reopen',
    'document_update', 'letter_draft', 'letter_update', 'pack', 'consent_revoke', 'reminder_mark',
    'complaint_create', 'complaint_update', 'clinic_details', 'reminder_text', 'staff_ai', 'website'));
