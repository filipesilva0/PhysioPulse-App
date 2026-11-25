-- Audit phase C (10 Nov 2026): the employer's Social Security contribution on salaries.
-- The rate is a clinic setting (basis points: 2375 = 23,75 %, the general employer TSU rate in 2026), so a change
-- in the law is one edit in Finance › Salaries. A staff member can have their own rate (a reduced rate, or 0 for
-- someone not on payroll). Each salary payment keeps the amount worked out when it was recorded, so a later change
-- of rate never alters past months. Payments recorded before this migration keep 0.

ALTER TABLE clinic ADD COLUMN employer_ss_bp INT NOT NULL DEFAULT 2375 CHECK (employer_ss_bp BETWEEN 0 AND 10000);
ALTER TABLE staff_pay ADD COLUMN employer_ss_bp INT CHECK (employer_ss_bp BETWEEN 0 AND 10000);
ALTER TABLE salary_payments ADD COLUMN employer_ss_cents INT NOT NULL DEFAULT 0 CHECK (employer_ss_cents >= 0);
