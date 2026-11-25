# Spec

Copy of record of the owner's feature spec (originally a shared document, see README.md). If they disagree, ask the owner; DECISIONS.md has the reasoning.

## Pages

Six pages in the left rail, each with its own color. Every page has a list sidebar, a main area and an inspector on the right (hideable).

| Page | What it does | Who sees it |
| --- | --- | --- |
| Today | Day strip, stats, today's appointments, alerts (money owed, worsening scores, unsigned consents, prescriptions running out, booking requests, complaint deadlines) | Everyone; money tiles owner only |
| Schedule | Day (default), week and month views; sessions, group classes, home visits, blocked time; recurring series; overlapping sessions; online booking requests; waitlist | Everyone |
| Patients | List and file: anamnesis, progress notes, tests, prescriptions, consents, documents, payments; legal tools on a ⋯ button | Therapists and owner see everything; reception sees contacts, plan, consents and payments only |
| Finance | Overview, payments in, packages and plans, expenses (with AI receipt reading), cash close, salaries, AI reports, accountant export | Owner only |
| Team | Staff, roles, invites, hours, pay model, permissions table | Owner; others see their own profile |
| Settings | Grouped overview, then clinic, schedule and waitlist, reminders, consents library, patient portal, security (with access log), server and backups, local AI, invoicing, payment methods | Owner; others see account items only |

Common to every page: full menu bar with shortcuts (every toolbar action is also a menu command), search as you type, Undo for reversible actions, context menus on appointments, an access-log entry each time someone opens a patient file.

## Clinical record

- **Anamnesis:** main complaint, history, medication, medical history, goals; **regions and symptoms** (Add region → body region list → symptom chips + notes per region; collapsible, removable); red-flag checkboxes.
- **Progress note builder** (one per session): template per condition; blocks = sliders (pain 0–10, range of motion, function), multiple choice, technique checklist, free text; add/remove blocks. "Rewrite with AI" (owner only) drafts a SOAP note from the blocks; the therapist uses, retries or discards it, then signs. The AI never saves.
- **Tests and questionnaires:** PhysioPulse library rebuilt natively, automatic scoring, history chart, "worse" badge and an alert on Today when a score worsens. Can be sent to the patient to fill at home (via the portal, later).
- **Prescriptions (credenciais):** doctor, specialty, date, sessions, validity, scan. Sessions count down; alert at 2 left and 14 days before expiry. **Reimbursement pack**: one PDF for insurers and ADSE — short clinical report, sessions and invoices, then the prescription scan and the invoice files (owner and therapists; 8 Nov 2026).
- **Consents:** treatment; data processing (RGPD); photos/videos; marketing messages; guardian (minors); invasive techniques (dry needling). Versioned text, on-screen signature, signed date, optional expiry; missing/expired consents show as alerts.
- **Documents:** drag photos/PDFs; **discharge letter**: pre-filled from the file, or drafted by the local AI for the owner; only the owner approves (then it's a locked PDF in `Cartas/`, optionally marking the patient discharged). Decided 8 Nov 2026: PDF, not a Word template.
- **Legal tools (⋯ on the patient card):** export clinical record (full or summary, PDF + originals in a zip, no reason asked); reimbursement pack; discharge letter; that patient's access log; anonymise; delete. Clinic-wide in the File menu: complaints log (ERS submission due 10 working days after receipt), delete everything from one day. Tax documents are always kept as the law requires.

## Schedule

| Type | Length | Notes |
| --- | --- | --- |
| Individual session | 30, 45 or 60 min | May overlap another session for the same therapist (warn, allow) |
| First assessment | 30–60 min | Own service and price |
| Group class | 30–60 min | Capacity, attendees, attendance; uses class packs |
| Home visit | 30–60 min + travel | Address, travel buffer, own price |
| Blocked time | any | Label (holiday, training, illness, admin) and colour |

- Recurring series (weekly or twice weekly, until a date or N sessions; edit one or all).
- Reminders by email/SMS/push per patient, 24 h before by default, templates in PT and EN. No confirm-by-reply.
- Online booking (patient portal, later): anyone can request; requests appear in the Schedule sidebar to accept/decline (with Undo). Slots shown one month ahead.
- Waitlist: optional preferred window; when a slot opens, the first match gets an SMS/email with a claim link; 2 hours, then the next person.
- No-shows: a status only for now.

## Money

Owner's prices (all editable). Savings vs €20 single sessions and €10 drop-in classes:

| Product | Price | Per session | Saving | Valid for |
| --- | --- | --- | --- | --- |
| First assessment (60 min) | €30 | — | — | — |
| Single session (45 min) | €20 | €20 | — | — |
| Starter: assessment + 4 sessions | €100 | €25 | €10 | 2 months |
| Pack of 5 | €80 | €16 | 20% | 3 months |
| Pack of 10 | €200 | €20 | 0% | 6 months, shareable with one family member |
| Rehab pack of 20 (post-surgery) | €300 | €15 | 25% | 9 months, shareable, 2 instalments |
| Continuous care, 4/month | €160/month | €40 | none | monthly, pause anytime |
| Continuous care, 8/month | €300/month | €37.50 | none | monthly |
| Home visit (60 min) | €40 | — | — | travel beyond 10 km extra |
| Group class drop-in | €10 | — | — | — |
| Class card, 10 classes | €80 | €8 | 20% | 3 months |
| Group classes, 2 a week | €75/month | ~€8.65 | 13% | monthly |

Decided 5 Nov 2026: the suggested ladder replaces the three prices that didn't reward buying ahead — pack of 10 **€170**, continuous care 4/month **€72**, 8/month **€136**.

- **Payments in:** cash, MB WAY to the clinic phone, bank transfer (recorded by hand); Stripe QR on screen or link by SMS (card, MB WAY, Multibanco; records itself). Stripe: no monthly fee.
- **Automatic monthly payments:** continuous care, class plans and instalments via Stripe (card or SEPA debit). Stripe's MB WAY can't recur, so MB WAY patients get a monthly SMS request. Failed payment → alert on Today and the patient card; plan pauses after a second failure.
- **Invoicing**: the clinic is a company (5 Nov) → certified provider API (InvoiceXpress, Vendus, Moloni) later; manual route now: the app stores the invoice number + PDF per payment.
- **Expenses:** drop a photo/PDF → local AI reads supplier, date, amount, category → owner confirms.
- **Cash close, salaries** (fixed now; commission and room rental later), **AI monthly report**.
- **Accountant export** (month/quarter/year zip): `01 Entradas` (invoices, receipts, CSV by method), `02 Saídas` (expenses by category + CSV), `03 Salários`, `04 Resumo` (CSV + PDF), `LEIA-ME.txt`.

## AI (owner only)

- Runs on the clinic server with Ollama. Nothing leaves the clinic.
- Note rewrite, discharge letter, receipt reading, monthly report.
- **Assistant:** chat panel (⌥⌘A), for the owner and anyone whose AI switch is on. Since 13 Nov 2026 it works with **tools** and has the person's own reach in the app: looks up patients, files (anamnesis, notes, tests — clinical roles only), the agenda, free times, requests, the team; prepares bookings, moves, cancellations, marks, request answers, waitlist, new files, patient details, anamnesis changes and note drafts (unsigned), and the owner's prices, hours, closures, staff and month summaries. Stage 2 (13 Nov): payments, packages, invoice numbers, voids; the owner's owed list, expenses, salaries, waived charges, month close. Stage 3 (13 Nov): a patient's documents and consents (reception: prescriptions and consent PDFs only), renaming a document, recording a consent withdrawal, reminders of a day and marking them sent; discharge-letter drafts and edits and the reimbursement pack (clinical roles); the owner's complaints log, legal check, clinic details, reminder message, AI switches and website. Shows what it looked up. Every change shows a before/after preview, waits for "Apply", is logged and can be undone (except a void or a consent withdrawal). It never approves letters, signs, anonymises, deletes or exports.
- Always label AI output, show specific progress text, and offer Use / Retry / Discard (or Apply / Cancel).

- **One-press booking of requests** (Schedule › Booking Requests, everyone): each request shows the first free time that fits; **Book** makes the patient file if needed and books; Undo removes both. A request that looks like a known patient asks which file.

## Patient portal (later, replaces PhysioPulse)

Book, cancel/reschedule, pay online, download invoices, intake form before the first session, message the therapist. Each feature can be switched off in Settings. Patient-facing iOS/Android app eventually.

## Staff iPhone app (phase 5, 12 Nov 2026)

Staff only (no patient app). Tabs: **Hoje** (your sessions, the team's, owner: devices to approve and booking requests to book or decline), **Agenda** (mine/everyone, week strip, day timeline), **Utentes** (search, read-only file: contacts, plan, prescriptions, consents, main complaint, latest scores, signed notes; reception: reduced file), **Gravar** (owner and therapists with the AI switch: record a session for the local AI), **Conta** (this iPhone, language, sign out). On an appointment: mark done or missed (Undo), open the file, record it. Writing notes, assessments and tests, booking, finance, team, settings, legal tools and the assistant stay on the computer. Face ID every time the app reopens, no idle lock, password and code again after 30 days; the owner can sign a phone out from Team.

## Build plan

| Phase | Content | Gate to pass |
| --- | --- | --- |
| 0 | Clickable prototype | Owner signs off screens |
| 1 | Tauri shell, signing, Rust server + PostgreSQL + Ollama in Docker, Tailscale, accounts, two-step, roles, access log, encrypted backups | App and server security items done; a backup restore tested |
| 2 | Patient file, note builder, tests, consents, prescriptions; schedule with series, overlaps, classes, home visits; reminders | One week of real use on the owner's own sessions |
| 3 | Payments, Stripe QR, auto-payments, packages, expenses with photo import, cash close, salaries, invoice records, accountant export | One month closed and handed to the accountant from the app |
| 4 | Archive folders, AI note rewrite, AI reports, admin assistant | DPIA and privacy notice reviewed; staff trained |
| 5 | Go live; then the patient portal and the invoicing API | — |
