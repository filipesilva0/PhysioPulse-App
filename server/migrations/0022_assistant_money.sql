-- The assistant, stage 2 (13 Nov 2026): money and staff. Payments, packages, invoice numbers and voids (as in the
-- app: everyone at the desk), and the owner's expenses, salaries, waived charges and month close. Still previews
-- that wait for Apply; a void can't be undone (payments are fiscal records), everything else can while unchanged.
ALTER TABLE assistant_changes DROP CONSTRAINT IF EXISTS assistant_changes_skill_check;
ALTER TABLE assistant_changes ADD CONSTRAINT assistant_changes_skill_check CHECK (skill IN (
    'price', 'product_active', 'hours', 'closure', 'staff_pay', 'staff_active',
    'book', 'move', 'cancel', 'mark', 'accept_request', 'decline_request', 'waitlist_add',
    'patient_create', 'patient_update', 'anamnesis', 'note_draft',
    'payment', 'payment_void', 'invoice_number', 'plan_sell', 'plan_status',
    'expense', 'salary', 'waive', 'month_close', 'month_reopen'));
