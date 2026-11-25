-- Phase 2: appointment reminders sent by hand from the therapist's own phone or computer (owner decision
-- 7 Nov 2026: no SMS provider for now). The app prepares the message and opens Messages, WhatsApp or email;
-- the person presses Send and the app records that the reminder went out. No message text is stored.

CREATE TABLE reminders_sent (
    appointment_id  UUID NOT NULL REFERENCES appointments(id) ON DELETE CASCADE,
    patient_id      UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    channel         TEXT NOT NULL CHECK (channel IN ('sms', 'whatsapp', 'email', 'call', 'other')),
    sent_by         UUID REFERENCES staff(id),
    sent_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (appointment_id, patient_id)
);

-- The message, one per language. {nome}/{name}, {dia}/{day}, {hora}/{time}, {clinica}/{clinic},
-- {terapeuta}/{therapist} are filled in by the app. Never anything clinical.
ALTER TABLE clinic ADD COLUMN reminder_pt TEXT NOT NULL DEFAULT
    'Olá {nome}! Lembramos a sua sessão na {clinica} {dia} às {hora}. Se não puder vir, avise-nos, por favor. Obrigado!';
ALTER TABLE clinic ADD COLUMN reminder_en TEXT NOT NULL DEFAULT
    'Hi {name}! A reminder of your session at {clinic} {day} at {time}. If you can’t come, please let us know. Thank you!';
