# Architecture

```
 Staff computers (Mac, Windows; later iPhone)       Router (home)            ZimaOS home server (Docker)
 ┌──────────────────────────────┐   HTTPS 443    ┌──────────────┐  443/80  ┌──────────────────────────────────┐
 │ PhysioPulse app (Tauri 2)    │ ─────────────► │ forwards only│ ───────► │ Nginx Proxy Manager (TLS, Let's  │
 │  React UI  ── invoke("api") ─┤  clinic's      │ 443 (+80 for │          │  Encrypt) → server :8095         │
 │  Rust core: server.rs        │  address       │ certificates)│          │ server   Rust/Axum  :8095        │
 │   token in memory only       │ (duckdns now,  └──────────────┘          │ db       PostgreSQL 16 (no port) │
 │   device key in device.json  │  .pt later)                              │ backup   nightly pg_dump | age   │
 └──────────────────────────────┘                                          └──────────────┬───────────────────┘
                                                                                          │ home network only
                                                       AI PC (Windows, 192.168.1.66) ◄────┘ Ollama :11434, whisper :8178
```

- **One source of truth:** the server's PostgreSQL. Computers keep no patient data at rest.
- **Phase 3 (11 Nov 2026): the staff API faces the internet** through Nginx Proxy Manager (HTTPS). What protects it: password + 6-digit code + **owner-approved device** (`routes/devices.rs`), rate limits in the database (per email and per internet address), two-step secrets encrypted with `SECRET_KEY`. The AI PC is never public: only the server reaches it, on the home network. Tailscale is no longer needed (it keeps working during the move: plain http is allowed only to private/Tailscale addresses, https everywhere else).

- **Phase 4a (11 Nov 2026): the public website** is a second service on the same server, `server/portal` (container `site`, port 8096 on the server, `physiopulse.duckdns.org` through NPM; later `physiopulse.pt`). It makes plain HTML pages from the database (no JavaScript, no cookies) and signs in as `physiopulse_portal`, a database login that sees only the `portal` views and writes only through two functions (migration 0018). Requests from its form become rows in `booking_requests` (source `website`) for the staff app. The staff API stays at `physiopulse-clinica.duckdns.org` (port 8095).

```
 Visitor's browser ──HTTPS──► NPM (physiopulse.duckdns.org) ──► site :8096 ──(physiopulse_portal: portal views + 2 functions)──► db
 Staff app ─────────HTTPS──► NPM (physiopulse-clinica.duckdns.org) ──► server :8095 ──(owner of everything)──────────────────► db
```

## Repository

```
CLAUDE.md               rules for AI assistants (read first)
claude/                 project memory: status, spec, decisions, design, security, dev guide
src/                    React UI (same code for desktop app and browser preview)
  App.tsx               window frame, menus, shortcuts, command table, sheets, alerts, toasts, idle lock
  pages/                Login, Today, Schedule, Patients, Finance, Team, Settings
  components/           ui.tsx (controls), Sheets.tsx (forms), MoneySheets.tsx (payment/package/expense/product/salary sheets), Assistant.tsx, Layout.tsx, menus.tsx (context menus)
  lib/api.ts            the only way to call the server (Tauri → Rust; preview → stand-in)
  lib/ctx.tsx           app context: L(), role, me, toast, sheets, confirm, menuAt
  lib/platform.ts       isTauri / isMac / shortcut labels / native menu events
  lib/data.ts           phase 2 server shapes, useApi hook, labels
  lib/preview.ts        browser-preview stand-in for the phase 2 API
  lib/preview3.ts       browser-preview stand-in for the phase 3 (money) API
  lib/money.ts          phase 3 shapes, labels, cents helpers
  data/mock.ts          sample data for Team hours
  styles/app.css        all tokens and styles
src-tauri/              desktop shell (Rust)
  src/lib.rs            builder: state, menu, close → sign out, vibrancy/Mica, commands
  src/server.rs         HTTP client, server address, token handling
  src/menu.rs           native menu bar (ids must match App.tsx run())
  capabilities/         what the web view may do (only window drag + menu events)
  tauri.conf.json       window, CSP, bundle
server/                 clinic server (Rust)
  src/main.rs           router, config, housekeeping
  src/auth.rs           passwords, TOTP, sessions, Auth extractor, roles, rate limit, access log
  src/routes/auth.rs    status, setup, signup, login, verify, me, logout
  src/routes/devices.rs device approval: pending sign-ins, owner's device list (approve/decline/remove/rename), recovery code, server console
  src/secret.rs         SECRET_KEY: encrypts the two-step (TOTP) secrets in the database (ChaCha20-Poly1305)
  src/routes/staff.rs   staff list/update, invites, end sessions, access log
  src/routes/patients.rs  patient list/file (open_file = the one logged read), anamnesis, notes, tests, prescriptions, consents (+ approval of texts)
  src/routes/reminders.rs reminders sent by hand: the day's list, mark sent, message templates
  src/routes/schedule.rs  appointments, series, overlaps, classes, waitlist, booking requests, Today alerts
  src/routes/money.rs   prices, packages/plans, what a done visit costs, payments (void, never delete), owed, expenses, files, cash close, salaries, month close, accountant export
  src/export.rs         dependency-free ZIP, CSV and PDF writers for the export
  src/routes/util.rs    role helpers, safe partial updates (whitelisted columns, values cast by PostgreSQL)
  src/db.rs             migration runner (embedded .sql files, applied once, in order)
  migrations/           SQL migrations
  backup/               backup container: backup.sh, restore.sh, nightly loop
  tests/                e2e.py (phase 1 API), e2e_phase2.py (phase 2 API), e2e_phase3.py (money), test-backup-restore.sh
  docker-compose.yml    server + db + backup + ollama (ollama only with --profile ai)
  portal/               the public website (phase 4a): its own crate and Dockerfile (`physiopulse-site`)
    src/main.rs         routes (/, /servicos, /equipa, /marcar, /privacidade, /termos, /conta and /en/…), form token, rate limits, visitor address
    src/html.rs         every page as escaped HTML, legal texts (PT/EN), the downtime page with the axolotl
    static/             site.css, icon.svg
  src/routes/website.rs Settings › Website (owner): switches, texts, team page, prices shown
  make-installer.sh     builds dist/install-physiopulse.sh: one-file install/update for ZimaOS (and the one-time move from /DATA/AppData/clinica)
```

## Sign-in flow

Every request from the app carries `X-Device-Key` (64 hex, made once by the Rust core, kept in `device.json` in the app's config folder). The server keeps only its SHA-256.

```
App start → getServer()
  none → "Connect to the server" → set_server(url) checks GET /api/status
        (no scheme: https for internet names, http for private/Tailscale/LAN addresses; http to the internet is refused)
  setup_needed → "Set up your clinic" → POST /api/setup → enrolment   (this device is approved automatically: how=setup)
  else → "Sign in" → POST /api/auth/login
        → {next:"code", challenge} → 6 digits → POST /api/auth/verify
             approved device → {next:"session", token…} → session
             new device      → {next:"pending", ticket, device:{code,name}, owner} → waiting screen
                               POST /api/auth/pending every 5 s (Rust adds the ticket) → pending | session | 403 device_revoked
                               owner only: POST /api/auth/pending/recover {recovery_code} → session
  remembered person on a trusted, approved computer (≤ 30 days since the code) → "Olá de novo, <nome>"
        → password only → POST /api/auth/login (+ device_token added by Rust) → {next:"session"} → session
        (a token from before phase 3 is "carried over": it approves its device once, how=carried_over)
        → {next:"enrol", enrol}   (account without two-step yet) → QR → verify → session or pending
"Create one" → invite code + details → POST /api/auth/signup → enrolment → verify → session or pending
A device the owner removed: login/verify answer 403 {error:"device_revoked"} (after the password).
An app without a device key (≤ 0.7.0): 403 {error:"app_update"}.
```

- `verify` returns `{token, staff, idle_minutes}`; Rust keeps `token` (and a pending `ticket`) and passes only `staff` and `idle_minutes` to the UI.
- Every session belongs to one device (`sessions.device_id`); the request must come with that device's key and the device must still be approved, or it's 401 (removing a device ends its sessions at once).
- The UI locks itself after `idle_minutes` without input; the server also refuses sessions idle that long.
- Any 401 on a normal call fires `physiopulse:signed-out` → back to sign-in with "Your session ended".
- Closing the window calls `/auth/logout` (2 s max) and the token is dropped from memory.

## API (phase 1)

All under `/api`. Auth = `Authorization: Bearer <token>` (added by Rust). `X-Device` = computer name.

| Method | Path | Who | Notes |
| --- | --- | --- | --- |
| GET | /status | public | `{setup_needed, clinic, version}` |
| POST | /setup | public, first run only | creates clinic + owner → enrolment |
| POST | /auth/signup | public + invite code | → enrolment |
| POST | /auth/login | public | `{email, password, device_token?}` → `{next:"code",challenge}`, `{next:"enrol",enrol}`, or on a trusted computer `{next:"session", token, staff, idle_minutes}` |
| POST | /auth/verify | challenge | `{challenge, code, trust_device?}` → `{token, staff, idle_minutes, device_token?}` (token valid 30 days; Rust keeps it) |
| POST | /auth/forget-device | public | `{device_token}` → `{ok}` (always) |
| POST | /auth/logout | any signed in | deletes the session |
| GET | /me | any | the signed-in person |
| GET | /staff | owner | with last activity |
| PATCH | /staff/:id | owner | `{active?, role?}`; can't change yourself |
| DELETE | /staff/:id/sessions | owner | sign out everywhere |
| POST | /invites | owner | `{role, email?}` → `{code}` (shown once) |
| GET | /access-log | owner | `?subject_type&subject_id&limit` |
| * | /ai/* | owner | see phase 2 (status, settings, letter draft) and phase 4 (rewrite, receipts, reports, assistant) |

## API (phase 2)

Who: **all** = owner, therapist, reception. **clin** = owner + therapist (403 for reception, checked on the server).

| Method | Path | Who | Notes |
| --- | --- | --- | --- |
| GET | /patients | all | `?q&include_inactive&limit`; contacts only; logs `patients.listed` (never the search text) |
| POST | /patients | all | `{name, birth_date?, sex?, nif?, sns_number?, phone?, email?, address?, guardian_*?, lang?, reminders?, therapist_id?, admin_note?}` → `{id, number}` |
| GET | /patients/:id | all | **the** patient file, via `open_file()`: logs `patient.viewed` (`full`/`reduced`). Reception gets `patient, consents, appointments, prescriptions (used/booked/remaining), documents (kinds `prescription` and `consent` only), view:"reduced"` (prescriptions: owner decision 4 Nov); clin also gets `anamnesis, notes, tests` and every document |
| PATCH | /patients/:id | all | any patient field; log detail = changed field names only |
| PUT | /patients/:id/anamnesis | clin | `{main_complaint, history, medication, medical_history, goals, regions:[{region, symptoms[], notes}], red_flags:[]}` |
| POST | /patients/:id/notes | clin | `{appointment_id?, template, blocks:[…], text}` → draft |
| PATCH/DELETE | /notes/:id | clin | draft only; author or owner |
| POST | /notes/:id/sign | clin | author or owner; afterwards locked by a database trigger. Also writes the note as PDF to `Utentes/<n.º>-<nome>/Notas/` and indexes it as a `note` document → `{signed_at, document_id}` (`null` if the archive write failed; the note is still signed) |
| POST | /notes/:id/pdf | clin | save the PDF of a signed note again (idempotent: one PDF per note) → `{document_id}` |
| POST | /patients/:id/tests | clin | `{test_code, score, max_score?, higher_is_worse?, answers?, taken_on?}` → `{previous, worse}` |
| DELETE | /tests/:id | clin | |
| POST | /patients/:id/prescriptions | clin | `{doctor, specialty, issued_on, sessions_total, valid_until?, payer?, document_id?}` (document must be this patient's, kind `prescription`) |
| PATCH | /prescriptions/:id | clin | |
| GET | /consent-texts | all | every version; `draft` = not approved yet; `current` = the newest **approved** version per kind+language |
| POST | /consent-texts | owner | `{kind, lang, title, body, valid_months?}` → new version, approved at once (the owner wrote it) |
| POST | /consent-texts/:id/approve | owner | approve a draft as it is (409 if already approved). Logs `consent_text.approved` |
| POST | /patients/:id/consents | all | `{text_id, granted, signed_by_name, signature_png (data:image/png)}`; approved texts only (400 for a draft); expiry from `valid_months`; saves the PDF in `Consentimentos/` → `{id, expires_on, document_id \| null}` (null = PDF failed, consent still recorded) |
| GET | /consents/:id | all | with signature + text; logs `patient.viewed` detail `consent` |
| POST | /consents/:id/revoke | all | the signed record stays; saves a withdrawal PDF → `{ok, document_id \| null}`; 409 if already withdrawn |
| POST | /consents/:id/pdf | all | save the PDF(s) again (idempotent: one per consent and event) → `{document_id, revoked_document_id}` |
| GET | /reminders | all | `?day=YYYY-MM-DD` (default tomorrow) → `{day, clinic, template_pt, template_en, items[]}`: booked sessions/assessments/home visits + each class attendee, with phone (or guardian's), email, lang, preferred channel, `sent_at/sent_channel/sent_by_name`. Logs `reminders.listed` |
| POST | /reminders/sent | all | `{appointment_id, patient_id, channel: sms/whatsapp/email/call/other, sent}`; 404 if the patient isn't on that appointment. Logs `appointment.reminded` / `appointment.reminder_undone`. The message text is never sent to or stored on the server |
| PUT | /reminders/template | owner | `{pt, en}` (≤ 600 chars each). Logs `reminder_text.updated` |
| GET | /appointments | all | `?from&to` (RFC 3339, ≤ 62 days) `&therapist_id&include_cancelled`; classes include attendees |
| POST | /appointments | all | `{kind: session/assessment/class/home/block, starts_at, ends_at, therapist_id, patient_id?, prescription_id?, title, color, address, travel_minutes, capacity, admin_note, repeat?: {weekdays:[1–7], until? \| count?}, allow_overlap?}` → `{ids, series_id, overlaps}`; **409 `overlap`** with `details` unless `allow_overlap` (blocks never need it) |
| PATCH | /appointments/:id | all | `{scope: one/following/all, starts_at+ends_at?, status?, therapist_id?, …, allow_overlap?}`; moving a series keeps each occurrence on its own day; status other than `cancelled` only with `one`; cancelling one returns `waitlist_matches` |
| POST | /appointments/:id/attendees | all | classes; 409 when full |
| PATCH/DELETE | /appointments/:id/attendees/:pid | all | `{attended}` |
| GET/POST | /waitlist | all | `{patient_id? or name+phone, therapist_id?, weekdays[], from_time?, to_time?, note}` |
| PATCH | /waitlist/:id | all | e.g. `{status:"removed"}` |
| GET/POST | /booking-requests | all | `?status=pending`; POST = entered by hand until the portal exists |
| POST | /booking-requests/:id/accept | all | body = a new appointment → books it |
| POST | /booking-requests/:id/decline, /reopen | all | reopen = Undo (removes the appointment it made if unused) |
| GET | /alerts | all | `booking_requests, consents_missing (next 7 days), consents_expiring (30 days), prescriptions (≤ 2 left or ≤ 14 days)`; clin also `worse_scores (last 30 days)`, `null` for reception |
| GET | /therapists | all | active owner + therapists: `{id, name, role}` only (for booking) |
| POST | /patients/:id/documents | clin; reception for `prescription` | `{kind: photo/report/imaging/prescription/letter/other, filename, data (base64)}`; 15 MB body limit on this route, 10 MB file; type from the first bytes (PDF/JPEG/PNG/HEIC/WebP), extension corrected; saved as `Utentes/<n.º>-<nome>/Documentos/<date>-<id8>-<name>`; logs `document.uploaded` |
| GET | /documents/:id | clin; reception for `prescription` | `{…, data (base64)}` to show in the app; logs `document.opened`. Reception gets 404 for clinical documents |
| GET | /documents/:id/raw | same | the file (attachment) for Save a Copy…; logs `document.saved` |
| PATCH | /documents/:id | same | `{kind?, filename?}`; not for `note` PDFs; logs `document.updated` |
| DELETE | /documents/:id | owner, or the uploader on the same day | not for `note` PDFs (409 + trigger); clears a prescription's link; deletes the file; logs `document.removed` |
| GET | /patients/:id/export/:kind | clin | `kind` = `full` or `summary` → a zip (`Processo <n.º> <date>/`: `Processo clínico.pdf`, `LEIA-ME.txt`; full adds `Documentos/` and `Notas/` originals). Signed notes only (drafts counted, not exported). Logs `patient.exported` (`full`/`summary`). The desktop `download` command allows this path |
| GET | /patients/:id/legal | owner | what anonymise/delete would touch: counts of notes (signed/draft), documents, tests, consents, upcoming/done appointments, payments, active plans, `owed_cents`, `last_contact` (YYYY-MM-DD of the last visit done/missed, class attended, note or test; the app warns before deleting within 20 years of it) |
| POST | /patients/:id/anonymise | owner | `{confirm_name}` (must match the current name; spaces/case ignored, accents not) → removes name (→ "Utente anonimizado"), contacts, NIF/SNS, guardian, admin note, consent signatures and signer names, every document and the archive folder, waitlist/requests; cancels future bookings; birth date keeps the year. Clinical content stays. Then the file is read-only (PATCH 404, no documents). Logs `patient.anonymised` |
| POST | /patients/:id/delete | owner | `{confirm_name}` → no payments: row deleted (cascade). With payments: everything is removed except the payments and plans, and the row stays as "Utente apagado" with `deleted_at` (hidden from lists and the file, 404). Archive folder removed. Logs `patient.deleted` (`removed` / `N payments kept without a name`) |
| GET | /patients/:id/reimbursement | clin | `?prescription_id&from&to` (period defaults to the prescription date → today, else 3 months) → `{from, to, prescriptions, prescription_id, payer, sessions (done in the period), payments (this patient's, not voided, with invoice number/file), report (pre-filled), signer, professional_id}`. Logs `patient.viewed` (`reimbursement`) |
| POST | /patients/:id/reimbursement | clin | `{prescription_id?, from, to, payment_ids[], payer, report, professional_id}` → **one PDF**: identity, prescription, sessions, invoices + total, the clinical report signed by the caller (with their cédula), "Anexos"; then the prescription scan and each invoice file as pages (PDFs joined with `export::join`, JPEG/PNG on their own page). Protected/unsupported files and payments without a file are listed "junte à parte", not attached. Saved in `Documentos/` as a `report` document → `{document_id, filename, pages, attached[], skipped[]}`. Logs `patient.reimbursement_pack`. Remembers `staff.professional_id` |
| GET | /patients/:id/letters | clin | `{letters: [{id, lang, addressee, body, ai_drafted, created_by(_name), approved_by_name, approved_at, document_id}], professional_id}` |
| GET | /patients/:id/letters/template | clin | `?lang=pt|en` → `{lang, addressee, body}` pre-filled from the file (no AI), with `[a completar]` / `[to complete]` where a person must write. Logs `patient.viewed` (`letter`) |
| POST | /patients/:id/letters/ai-draft | owner | `{lang}` → `{text, model}` from the **local** AI (Ollama, `ai.rs`). Nothing is saved. 409 `ai_offline` / `ai_no_model` / `ai_failed`. Logs `ai.letter_drafted` (ok/failed; never the text) |
| POST | /patients/:id/letters | clin | `{lang, addressee, body, ai_drafted}` → `{id}` (a draft). Logs `letter.created` |
| PATCH / DELETE | /letters/:id | clin | drafts only; the owner or whoever started it. Logs `letter.updated` / `letter.draft_deleted` |
| POST | /letters/:id/approve | owner | `{discharge, professional_id?}`; refuses a body with `[a completar]`. Locks the letter (trigger), saves the PDF in `Cartas/` (document kind `letter`, `letter_id`, locked like note PDFs) and with `discharge` sets the patient inactive → `{document_id | null, discharged}`. Logs `letter.approved` |
| POST | /letters/:id/pdf | clin | save the approved letter's PDF again (idempotent) |
| GET | /ai/status | owner | `{available, url, model, reason?: offline/no_model/not_local/error, models}` (models = what that Ollama has) |
| GET | /ai/settings | owner | `{url, model, url_in_app, model_in_app, server_url, server_model}` — the app's choice (`clinic.ai_url/ai_model`) or else OLLAMA_URL/OLLAMA_MODEL |
| PUT | /ai/settings | owner | `{url, model}`; "100.86.97.21" → `http://100.86.97.21:11434`; only loopback/private/Tailscale addresses (400 otherwise); empty = back to the server's; used at once, logged `ai.settings_changed` (address · model). Returns the new status |
| POST | /ai/test | owner | `{url, model}` → status for that address/model, nothing saved |
| GET | /complaints | owner | every complaint, newest first: `{id, received_on, source: paper/electronic/other, book_ref, patient_id, patient_name, complainant, summary, reply, replied_on, sent_to_ers_on, due_on, …}` |
| POST | /complaints | owner | `{received_on, source, book_ref?, patient_id?, complainant?, summary?}` → `{id, due_on}`. `due_on` = 10 working days after receipt (`workdays.rs`: Mon–Fri without national holidays). Logs `complaint.created` |
| PATCH / DELETE | /complaints/:id | owner | any field; a new `received_on` moves `due_on`. Logs `complaint.updated` (field names) / `complaint.deleted` |
| GET | /days/:day | owner | what deleting that day touches: `{appointments, blocks, attendees, notes_signed, notes_draft, tests, documents, payments_kept, patients}` |
| POST | /days/:day/delete | owner | `{confirm: day}` → deletes that day's appointments (class lists, reminders), notes written that day (signed ones and their PDFs too, via `physiopulse.legal_tool`), tests taken that day and documents added that day (not consent/letter PDFs). Keeps payments (unlinked from the visit), invoices, expenses, consents, letters, patients. Files removed after commit. Logs `day.deleted` (counts only) |

Times: the series generator and the waitlist matcher work in `CLINIC_TZ` (default `Europe/Lisbon`), so a weekly 18:00 stays at 18:00 across summer-time changes.

Errors are `{error, message}` (+ `details` for 409 `overlap`) with human-readable, non-technical messages. The desktop shell passes `status, message, code, details` to the UI; `api()` throws them as `ApiErr`.

## API (phase 3: money)

**all** = owner, therapist, reception. **owner** = 403 for everyone else. Amounts are integer cents.

| Method | Path | Who | Notes |
| --- | --- | --- | --- |
| GET | /products | all | `?all=true` includes retired. Each has `vat_exempt` |
| POST, PATCH | /products, /products/:id | owner | kind visit/pack/monthly; one active visit price per appointment kind (409); `vat_exempt` (default true; class items seeded false). Copied to `patient_plans.vat_exempt` at sale; the accountant export's `entradas.csv` has an IVA column from it |
| GET | /patients/:id/money | all | `{plans, payments, owed, due_cents}`; logs `patient.viewed` detail `money` |
| POST | /patients/:id/plans | all | `{product_id, starts_on?, shared_with?}` → `{id, attached}` (unpaid done visits since `starts_on` move onto it) |
| PATCH | /plans/:id | all | `{status: paused/active/ended/cancelled, shared_with?, note?}`; `expires_on` owner only; cancel only if unused and unpaid (409) |
| GET | /payments | owner | `?from&to&method&uninvoiced=true` |
| POST | /payments | all | `{patient_id?, method: cash/mbway/transfer/card, received_on?, invoice_number?, items:[{amount_cents, appointment_id? \| plan_id (+covers_month for monthly) \| description}]}` → `{ids, numbers}`; no future dates; Stripe refused |
| PATCH | /payments/:id | all | `{invoice_number?, invoice_file_id?, note?}` |
| POST | /payments/:id/void | owner, or the recorder the same day | `{reason}` required |
| GET | /finance/overview | owner | `?month=YYYY-MM`: totals, by method, previous month, 6-month series, owed, closed |
| GET | /finance/owed | owner | every unpaid item |
| GET/POST, PATCH/DELETE | /expenses, /expenses/:id | owner | categories in `CATEGORIES` (money.rs / lib/money.ts) |
| POST | /finance/files | all for `invoice`, owner for `expense` | `{kind, filename, data (base64), day?}`; PDF/JPEG/PNG/HEIC/WebP by magic bytes; ≤ 10 MB; 15 MB body limit on this route only |
| GET | /finance/files/:id/raw | all for invoices, owner otherwise | the file (Content-Disposition attachment); the desktop shell's `download` command saves it to Downloads |
| GET/POST | /finance/cash | owner | `?day` → expected by method; POST `{day, counted:{method: cents}, note}` (redo replaces) |
| GET/POST | /finance/salaries | owner | `?month`; POST `{staff_id, month, gross_cents, paid_on?, method}` |
| DELETE | /finance/salaries/:id | owner | |
| PUT | /staff/:id/pay | owner | `{model: "fixed", fixed_cents}` (commission/rental later) |
| GET | /finance/months | owner | closed months with totals |
| POST | /finance/months/:YYYY-MM/close, /reopen | owner | only ended months; locks payments, expenses, salaries (trigger) |
| POST | /finance/export | owner | `{from, to (YYYY-MM, inclusive, ≤ 24 months), include_files, include_pdf}` → `{id, filename, path, provisional}` |

`PATCH /appointments/:id` also takes `plan_id` (choose the package by hand) and `charge_waived` (owner). Marking done/attended sets `plan_id`/`charge_cents` (see `apply_done` in money.rs). `GET /appointments` returns `plan_id, charge_cents, charge_waived, paid_cents`. `GET /alerts` adds `plans_low` (all) and `money` (owner, else null).

## API (phase 4: local AI, hours and closures)

All AI routes are **owner only** (403 for everyone else, nothing reaches the AI). The desktop shell waits up to 330 s for `/ai/*`. AI failures answer 409 with `error`: `ai_offline` (`details.url`), `ai_no_model`, `ai_no_vision`, `ai_failed`, and for receipts `ai_scan_pdf` / `ai_heic`. Prompts and answers are never logged. Code: `server/src/routes/assist.rs`, `assistant.rs`; `ai::generate_json` asks Ollama for a JSON schema (`format`) and can send images.

| Method | Path | Who | Notes |
| --- | --- | --- | --- |
| POST | /ai/rewrite-note | owner | `{patient_id, template, blocks, text, lang}` → `{text, model}`: a SOAP draft from only what's filled in (no name sent). Nothing saved. Logs `ai.note_rewritten` on the patient. Notes take `ai_assisted` (shown as an "AI" badge) |
| POST | /ai/read-receipt | owner | `{file_id}` (a `/finance/files` upload of kind `expense`) → `{fields: {supplier, supplier_nif, spent_on, amount_cents, vat_cents, category, document_ref, description, method}, missing, model}`. Photos go as images (needs a vision model); PDFs as their text (scans → `ai_scan_pdf`). Every field is checked (`tidy_receipt`): bad dates, totals, NIFs, categories are dropped, never guessed. Expenses take `ai_read` |
| GET | /finance/reports/figures?month&lang | owner | the month's figures and the previous month's, as rows `[label, this, previous]` (`month_facts`, `fact_rows`) |
| POST | /ai/report | owner | `{month, lang}` → `{month, facts, rows, text, model}`: server figures + AI commentary. Nothing saved |
| GET / POST | /finance/reports | owner | list kept reports / keep one `{month, lang, text, model}` (figures recomputed by the server) → PDF in `Financas/<ano>/Relatorios/` (finance file kind `report`) |
| DELETE | /finance/reports/:id | owner | removes the report and its PDF |
| POST | /ai/assistant | owner | `{message, history: [{who, text}] (last 6), lang}` → `{reply, change: {id, skill, rows, warnings, status} | null, rows: [label, this, prev][] | null, model}`. The AI gets the price list, staff names and hours (never patients) and answers a skill in JSON; the server checks it and stores a `pending` change |
| POST | /assistant/changes/:id/apply | owner | refused if not pending, more than a day old, or the thing changed since the preview (409). One transaction; logs `assistant.applied` (skill · what · before → after) |
| POST | /assistant/changes/:id/cancel | owner | pending → cancelled |
| POST | /assistant/changes/:id/undo | owner | applied → undone, only if still as the change left it; logs `assistant.undone` |
| GET | /clinic/hours | everyone | `{hours: {mon…sun: ["08:00","20:00"] | null}, closures: [{id, starts_on, ends_on, label, days}]}` (upcoming) |
| PUT | /clinic/hours | owner | all seven days; open < close |
| GET | /closures/check?starts_on&ends_on | owner | `{open_days, booked}` |
| POST | /closures | owner | `{starts_on, ends_on, label}` (today or later, ≤ 3 months) → one blocked-time appointment per open day (no therapist, `closure_id`), `{id, days, booked}`. Booked appointments stay booked |
| DELETE | /closures/:id | owner | removes the closure and its blocked time |

Assistant skills: `price`, `product_active` (prices and packages), `hours`, `closure` (hours and closures), `staff_pay` (fixed monthly pay), `staff_active` (never the owner), `report` (figures + 2–3 sentence summary). Anything else gets a plain answer.

## API (audit phase A: accounts, clinic details, server and backups — 10 Nov 2026)

Code: `server/src/routes/account.rs`, `clinic.rs`, `system.rs`; migration `0013_accounts_clinic.sql`. Tests: `server/tests/e2e_account.py`.

| Method | Path | Who | Notes |
| --- | --- | --- | --- |
| PATCH | /me | everyone | `{name}` → the person (spaces tidied). Logs `account.name_changed` |
| POST | /me/password | everyone | `{current, new}` (12+, different) → `{ok, other_sessions_ended}`. Ends the person's *other* sessions. Wrong current → 400 (not 401: the session stays) and counts toward the 5-per-15-min limit. Logs `account.password_changed` / `…_change_failed` |
| GET | /me/recovery-codes | owner | `{left, total, created_at}` |
| POST | /me/recovery-codes | owner | `{password}` → `{codes: [10 × "XXXX-XXXX-XXXX"]}`; old ones stop working. Logs `account.recovery_codes_made` |
| POST | /auth/verify | — | (changed) an owner confirming a **two-step setup** with no unused codes left also gets `recovery_codes` (shown once) |
| POST | /auth/recover | — | `{challenge (the login one), recovery_code}` → `{next: "enrol", enrol, codes_left}`: the code is used up, two-step gets a new secret, trusted computers are forgotten; finishing the enrol signs in. Logs `login.recovery_code_used` / `login.bad_recovery_code` |
| POST | /auth/reset | — | `{email, code, password}`: an owner-made reset code **or** one of the person's recovery codes → new password, signed out everywhere (next sign-in still asks the 6 digits). Codes typed with spaces/lower case match. 5 failures / 15 min per email. Logs `account.password_reset` (detail `reset_code`/`recovery_code`) / `…_failed` |
| POST | /staff/:id/reset-code | owner | → `{code, email, expires_in_hours: 24}`, shown once; a newer code replaces the older. Not for yourself. Logs `staff.reset_code_made` |
| POST | /staff/:id/reset-two-step | owner | new secret, `totp_enabled = false`, sessions + trusted computers + challenges gone; next sign-in shows a new QR. Logs `staff.two_step_reset` |
| GET | /invites | owner | unused, unexpired, not withdrawn: `[{id, role, email, created_at, expires_at, created_by_name}]` (never the code) |
| DELETE | /invites/:id | owner | withdraw (`revoked_at`); the code stops working. 409 if used/withdrawn. Logs `invite.revoked` |
| GET | /clinic | everyone | `{name, nif, ers_number, address, phone, email, logo: data URL | null, updated_at}` |
| PUT | /clinic | owner | same fields; NIF checked (9 digits, mod-11), email/phone sanity; `logo`: absent = keep, null/"" = remove, else a JPEG (≤ 512 KB, checked). Logs `clinic.updated` |
| GET | /system/status | owner | `{version, address (PUBLIC_ADDRESS), database, archive, backup, stale_hours: 36}`; `backup` = `{state: ok|old|failed|running|unknown, requested, last_ok, last_error, db_file, files_file, bytes, kept_days, offsite: off|ok|failed, next_at}` read from `$BACKUP_CONTROL/status.json` (never a made-up time) |
| POST | /system/backup | owner | writes `$BACKUP_CONTROL/request`; the backup container picks it up within 30 s. 409 when there's no status file (service not running). Logs `system.backup_requested` |
| GET | /alerts | everyone | (changed) `backup`: owner only, `{state, last_ok}` when the backup is old/failed/unknown, else null |

**PDF header.** Every document query adds `'clinic_info'`; `export::clinic_header()` draws the logo (≤ 140 × 48 pt), name, `NIF … · ERS …`, address and contacts at the top of signed notes, consents, record exports, reimbursement packs and discharge letters (and the logo on monthly reports).

**Signed notes.** `POST /notes/:id/sign` drops scale blocks whose `value` isn't a number in the same statement that locks the note.

**Backups and the server.** The backup container (`server/backup/run-nightly.sh`) runs at start, nightly at `BACKUP_HOUR`, and whenever `$BACKUP_CONTROL/request` appears (checked every 30 s); `status.sh` keeps `$BACKUP_CONTROL/status.json` (`last_ok`, `last_error`, file names, size, off-site result, `running`, `next_at`). The folder (`$DATA_DIR/backup-control`, owned by uid 10001) is the only thing the two containers share; the server never sees the backups or the key.

## API (audit phase B: picker, duplicates, language — 10 Nov 2026)

Code: `server/src/routes/patients.rs` (`tidy_nif`, `check_nif`, `duplicates`, list paging), `server/src/i18n.rs`. Tests: `server/tests/e2e_phaseb.py`.

| Method | Path | Who | Notes |
| --- | --- | --- | --- |
| GET | /patients | everyone | (changed) also `offset` (paging, same name order) and `id` (just that patient, active or not — the search picker opened from a file). NIF search ignores spaces |
| POST, PATCH | /patients, /patients/:id | everyone | (changed) `nif` tidied (spaces, dots, "PT" dropped; foreign numbers in capitals) and checked: a Portuguese NIF needs a valid check digit → 400; a NIF another patient has → 409 `duplicate_nif` with `details: {id, number, name}` |
| GET | /patients/duplicates | everyone | `?nif=&phone=&name=&birth_date=&exclude=` → up to 5 `[{id, number, name, birth_date, phone, reason: nif|phone|name_birth}]`. Phone compares the last 9 digits; name is case/space-insensitive and needs the birth date. Anonymised/deleted records never match. Logs `patients.duplicates_checked` (count only) when something matched |

**Language.** Every answer passes through `i18n::localise`: when the request says `Accept-Language: pt…`, an error's `message` is replaced by its pt-PT row in `i18n::PT` (code and `details` unchanged). Messages stay English in the code and the log. The desktop shell sends the app's language on every call (`server.rs` `api`, new `lang` argument). A unit test reads every `AppError` message in `src/` and fails if one has no Portuguese row — add the row when you add a message. Apps older than 0.4.0 send no language and keep translating with the table in `src/lib/api.ts`.

**Desktop shell.** New command `window_action(action)` (`minimize`, `toggle_maximize`, `toggle_fullscreen`) for the in-window Windows menus.

**Schedule UI.** `src/components/TimeGrid.tsx`: the day view (one column per therapist) and the week view (7 days) share it. Click a free time → New Appointment with date, time and that column's therapist; drag → `PATCH /appointments/:id {scope: "one", starts_at, ends_at, therapist_id, allow_overlap}`; the bottom edge changes the end. Overlaps are laid out in lanes (chains included). Blocked time sits behind sessions. `src/components/PatientPicker.tsx` replaces every patient dropdown.

## API (audit phase C: access log, test forms, Social Security — 10 Nov 2026)

Code: `server/src/routes/staff.rs` (`access_log`, `access_log_export`), `server/src/forms.rs` (scoring), `server/src/routes/patients.rs` (`add_test`), `server/src/routes/money.rs` (`totals`, `salaries`, `set_ss_rate`, `pay_salary`, `employer_ss`). Tests: `server/tests/e2e_phasec.py`.

| Method | Path | Who | Notes |
| --- | --- | --- | --- |
| GET | /access-log | owner | (changed) filters `from`, `to` (YYYY-MM-DD, clinic time zone, both included), `staff_id`, `actions` (comma list: exact `login.failed` or prefix `patient.`), `q` (words in person, patient name/number, computer, action, detail; `%`/`_` literal), `before` (id, for "Show more"), plus `subject_type`, `subject_id`, `limit` (≤ 1000). Newest first by id. Rows gain `subject_label` ("0042 Ana Silva" for a patient; empty once deleted or anonymised). Bad date / from after to / odd action → 400 |
| GET | /access-log/export | owner | Same filters, up to 100 000 rows → `text/csv` (semicolons, UTF-8 BOM): Data e hora (clinic local time) · Quem · Ação · Utente ou assunto · Computador · Detalhe. Logs `access_log.exported` with the filters (not the search words). The desktop shell's `download` allows this path with a plain query string |
| POST | /patients/:id/tests | owner, therapist | (changed) `answers: {items: {id: n}, activities?: [..]}` for `nprs`, `spadi`, `lefs`, `psfs`, `koos` → the server checks every answer and works out the score (`forms.rs`), ignoring `score`; stored as `{form: 1, items, sub?, activities?}`. Without `items` (or for other codes) it's the typed total as before; a total above `max_score` → 400. Answer adds `score` |
| GET | /finance/salaries | owner | (changed) adds `employer_ss_bp` (clinic rate, basis points); each staff row `employer_ss_bp` (own rate or null) and `effective_ss_bp`; each payment `employer_ss_cents` |
| PUT | /finance/salaries/rate | owner | `{employer_ss_bp: 0..10000}`; applies to salaries recorded from then on. Logs `salary.rate_changed` |
| PUT | /staff/:id/pay | owner | (changed) optional `employer_ss_bp` (null/absent = clinic rate) |
| POST | /finance/salaries | owner | (changed) optional `employer_ss_cents`; absent → gross × rate, rounded half up to the cent. Answer adds `employer_ss_cents` |

Month totals (`totals`, overview, month close, AI report, accountant export) add `employer_ss_cents`; `result_cents` = revenue − expenses − salaries − employer Social Security. Accountant export: `salarios.csv` and `resumo.csv` gain a "Segurança Social, entidade patronal (€)" column; `resumo.pdf` a line when non-zero. Clinical-record export prints SPADI/KOOS subscales and PSFS activities under each result.

**Test forms UI.** `src/lib/forms.ts` (items, answer labels, the same scoring for the live total), `src/components/TestForms.tsx` (`RecordTest`, `TestHistory`, `TestResultSheet`). Item ids must match `server/src/forms.rs`.

## API (PhysioPulse plan, phase 2: session recording — 11 Nov 2026)
"AI" below = the owner, or a therapist whose AI switch is on (`staff.ai_enabled`, Team; `Auth::require_ai`). Reception never.
Each recording is visible to the person who made it and to the owner. Code: `server/src/routes/recordings.rs`, `server/src/stt.rs`.

| Method | Path | Who | What |
| --- | --- | --- | --- |
| POST | /patients/:id/recordings | AI | `{template, blocks, appointment_id?, lang}` → `{id}`. `blocks` = the note template's blocks (labels, ranges, options) the AI may fill. Logs `recording.started` |
| PUT | /recordings/:id/chunks/:n | author | `{data}` base64 of 16 kHz mono s16le PCM, ≤ 4 MB raw (6 MB body limit). In order from 0; a repeat of an earlier piece is accepted once; a gap → 409 `chunk_order` `{expected}`. Max 2 h 30 |
| POST | /recordings/:id/finish | author | Joins the pieces into `audio.wav`, status → `queued`. < 10 s → 400. Logs `recording.uploaded` (seconds only) |
| GET | /patients/:id/recordings | AI | `{recordings: [{id, status, seconds, template, error, note_id, created_by_name, created_at, delete_after…}], speech}` — no content; `speech` = last known whisper status |
| GET | /recordings/:id | AI (author/owner) | Once `ready`: `{report, plan, values, transcript, template, lang, model}`. Logs `recording.opened` |
| GET | /recordings/:id/audio | AI (author/owner) | The WAV (desktop: `view_file`, played in the app, nothing saved). Logs `recording.played` |
| POST | /recordings/:id/retry | AI (author/owner) | A `failed` one back to `queued` |
| DELETE | /recordings/:id | author/owner | Folder removed, row marked `deleted` (`by_hand`). Logs `recording.deleted` |
| POST | /ai/speech/test | owner | `{url}` → `{available, url, reason?}` (nothing saved) |

Also: `POST/PATCH /patients/:id/notes` accept `recording_id` (links a **ready** recording of the same patient, author/owner); `POST /notes/:id/sign` then deletes it (`signed`). `GET /ai/status` (AI) adds `speech`; `GET/PUT /ai/settings` (owner) add `speech_url` (absent = unchanged; only `speech_url` sent = the Ollama choice is left alone). `PATCH /staff/:id {ai_enabled}` (owner; therapists only, 400 otherwise; logs `staff.ai_on/off`). `/me` and `/staff` include `ai_enabled`. AI routes opened to therapists with AI on: `/ai/rewrite-note`, `/patients/:id/letters/ai-draft`, `/ai/status`. Receipts, reports, the assistant and AI settings stay owner-only.

**Statuses:** recording → queued → transcribing → drafting → ready (or failed: `stt_failed`, `no_speech`, `ai_failed`, `ai_no_model`). **Worker** (`spawn_worker`, every `RECORDING_POLL_SECS`, default 15 s): one at a time, oldest first; waits (stays `queued`) while whisper or Ollama don't answer; a transcript already made isn't redone; three failed tries → `failed`; a restart puts in-flight work back in the queue. Also deletes recordings past `delete_after` (30 days, `expired`) and folders with no live row.

**The draft:** (1) whisper.cpp `POST /inference` (multipart WAV, `language`, `response_format=verbose_json`) → one line per segment, silence marks dropped. (2) Ollama, plain report via `style::generate_plain_ctx` (context 8k–32k from the transcript length, 10 min): the prompt explains the unlabelled two-person conversation, no invention, no plan. (3) Ollama JSON (schema built from the blocks + `plan`): values kept only if they fit (`check_values`: range, step, options), plan tidied and style-checked. If (3) fails the report still comes back.

## API (PhysioPulse plan, phase 3: device approval — 11 Nov 2026)

| Method | Path | Who | What |
| --- | --- | --- | --- |
| POST | `/auth/pending` | waiting device (ticket + its key) | `{next:"pending", device}` · session when approved · 403 `device_revoked` when declined · 401 when the 30 min ran out |
| POST | `/auth/pending/recover` | waiting owner | `{recovery_code}` approves this device (how=recovery_code) and signs in; `codes_left` |
| GET | `/devices` | owner | all devices, pending first: name, code, status, how, who asked, who approved, last seen, last IP, `this_device`, open sessions and who |
| POST | `/devices/:id/approve` | owner | pending or removed → approved |
| POST | `/devices/:id/revoke` | owner | removes (or declines): sessions end, trusted tokens go; not the device you're using |
| PATCH | `/devices/:id` | owner | `{name}` |
| GET | `/alerts` | everyone | adds `devices` (owner: waiting devices; null for others) |
| GET | `/system/status` | owner | adds `secrets_encrypted` |

Server console (inside the container): `physiopulse-server devices` lists them; `physiopulse-server approve K7Q-M2X` approves a waiting device (how=console).
Access log: `device.requested`, `device.approved`, `device.declined`, `device.revoked`, `device.renamed`, `device.bad_recovery_code`, `login.device_refused`; `login.failed` now has the internet address in `detail`.

## API (PhysioPulse plan, phase 4a: website — 11 Nov 2026)

| Method | Path | Who | What |
| --- | --- | --- | --- |
| GET | `/website` | owner | `{site, team, products, clinic, requests_30d, site_url}` |
| PUT | `/website` | owner | any of `published, requests_open, home_visits, opening_soon, show_prices, tagline_pt/en (≤90), intro_pt/en, home_text_pt/en (≤700)`; plain text (no `<`/`>`); publishing needs clinic name, NIF and a phone or email |
| PUT | `/website/team/:id` | owner | `web_visible, web_title_pt/en (≤60), web_bio_pt/en (≤500)`; owner/therapists only (404 for reception) |
| PUT | `/website/products/:id` | owner | `{web_visible}` |
| GET | `/booking-requests` | everyone | rows now carry `source` (`staff`/`website`), `day_part`, `address`, `lang` |

Access log: `website.published`, `website.unpublished`, `website.changed`, `website.team_changed`, `website.prices_changed`, and `booking_request.received` (no staff, device `site`, detail = request id).

**The website service** (`server/portal`, no API of its own): `GET /`, `/servicos`, `/equipa`, `/marcar` (+ `POST`), `/privacidade`, `/termos`, `/conta` (coming soon, 4b) and the same under `/en/…` (`/en/services`, `/en/team`, `/en/book`, `/en/privacy`, `/en/terms`, `/en/account`); `/site.css`, `/icon.svg`, `/robots.txt`, `/downtime.html`, `/health`. The form: `service` (assessment/session/home), `name`, `phone`, `email`, `day` (tomorrow … +62 days), `part` (morning/afternoon/any → 09:00/14:00/10:00 Lisbon), `address` (home), `message`, `privacy`, `t`+`s` (signed time), `website` (honeypot). Success → 303 to `/marcar?enviado=1`.

## API (assistant schedule and one-press booking — 11 Nov 2026)

| Method | Path | Who | What |
| --- | --- | --- | --- |
| GET | `/booking-requests/:id/suggest` | everyone | `{kind, minutes, slot: {starts_at, ends_at, therapist_id, therapist_name, day, time} | null, alternatives: [slot] (≤3), patient: {id, name, number} | null, lookalikes: [{id, number, name, reason: phone/email/name}]}`; 409 if already answered |
| POST | `/booking-requests/:id/book` | everyone | `{starts_at, ends_at?, therapist_id, patient_id?, allow_overlap?}` → `{appointment_id, patient_id, patient_created, overlaps}`. Makes the file from the request when there's no `patient_id` (name, phone, e-mail, address), books (message → admin note; home visit: address + 15 min travel), accepts — one transaction. Logs `patient.created` + `booking_request.accepted` |
| POST | `/booking-requests/:id/reopen` | everyone | now also `{patient_removed}`: a file the press made goes if nothing else uses it |
| POST | `/ai/assistant` | owner, or anyone with `ai_enabled` | answer now also has `list: [{id?, when, title, sub, status?, patient_id?}] | null` and `choices: [{label, send}] | null`. Non-owners get only the schedule jobs (the owner's skills aren't offered to the model and are refused by the server). |
| POST | `/assistant/changes/:id/apply|cancel|undo` | owner, or whoever asked (schedule jobs) | owner jobs stay owner-only |
| GET | `/ai/status` | owner, or anyone with `ai_enabled` | |
| PATCH | `/staff/:id {ai_enabled}` | owner | therapists **and reception** (400 for the owner) |

Schedule skills (`routes/agenda.rs`): `book` (patient, date, time, kind, minutes, therapist, repeat_weekdays + repeat_count, phone, address), `move` (appointment ref or patient + date; date/time/therapist), `cancel` and `mark` (refs; status done/no_show), `accept_request` / `decline_request` (request ref or name; optional date/time/therapist), `waitlist_add`, `free_slots`, `agenda` (reply + refs). Refs (A1…, R1…) are rebuilt on every message from the same ordered query, so they only mean something within that message. Previews are built in rolled-back transactions to find clashes; Apply refuses (409) if new clashes appear or the appointment/status changed. `assistant_changes.after` keeps what Undo needs (`ids`, `patient_id`, `created_patient`). Access-log detail = skill, count, time (never names); subject = the patient.

## API (PhysioPulse plan, phase 5: the staff iPhone app — 12 Nov 2026)

| Method | Path | Who | What |
| --- | --- | --- | --- |
| any | every route | — | The iPhone app's Rust core adds `X-Device-Kind: phone`. It only matters when a device is first saved (`devices.kind`); after that the stored kind decides. |
| GET | `/devices`, `/alerts` | owner | rows now have `kind` (computer / phone) |
| GET | `/staff/:id/phones` | owner (anyone), others (themselves) | `[{id (session), device_id, name, code, signed_in, last_seen, expires_at, this_device}]` — the person's phone sessions |
| DELETE | `/staff/:id/phones/:sid` | owner | ends that phone session (the phone stays approved). Logs `staff.phone_signed_out` |

Sessions (`auth::create_session`): on a `kind = 'phone'` device, `phone = true`, `expires_at = now + 30 days`, no idle check (`Auth` and housekeeping skip `last_seen` for phone sessions), and older phone sessions of the same person on that phone are deleted first. Computers unchanged.

iPhone app (`src-tauri`, same crate): `run()` → `desktop()` or, on iOS, `phone_app()` with commands `app_info` (`form: "phone"`), `get_server`, `set_server`, `api`, `view_file`, `remembered_account`, `forget_account`, `phone_user`, `phone_unlock {prompt}`, `phone_lock`, `phone_sign_out`. `phone.rs` keeps the token in the keychain (service `app.physiopulse.session`) with user-presence access control and `phone.json` (name, email, server) in the app's folder. UI: `src/phone/` (`PhoneApp.tsx` shell, lock, tabs; `schedule.tsx`, `patients.tsx`, `record.tsx`, `account.tsx`, `ui.tsx`, `phone.css`), chosen in `main.tsx` by `isPhone` (`src/lib/phone.ts`: an iPhone user agent inside Tauri, or `?phone` in the browser preview).

## UI data layer (phase 2)

- `src/lib/data.ts`: server shapes, `useApi(path)` (GET + reload on `physiopulse:changed`, which `api()` fires after every successful write), date helpers (local time = clinic time), labels shared by pages.
- `src/lib/preview.ts`: browser-preview stand-in for all phase 2 endpoints, same role rules, seeded relative to today. Not used in the desktop app. Phase 4 stand-ins: `src/lib/previewAi.ts` (canned AI answers).
- `src/components/Ai.tsx`: `useAiStatus()`, `aiOffText()`, `AiCard` (labelled card with specific progress text, then Use/Retry/Discard).

## Database (migration 0001)

- `clinic` (single row: name)
- `staff` (email unique, role ∈ owner/therapist/reception, argon2 hash, TOTP secret, enabled flag, last used step, active)
- `challenges` (login/enrol step, 5 attempts, 5–15 min expiry)
- `sessions` (token SHA-256, device, last_seen, expires_at)
- `invites` (code SHA-256, role, optional email, expiry, used_at)
- `access_log` (append-only; trigger blocks UPDATE/DELETE)

## Database (migration 0002)

- `patients` (n.º = identity column; contacts, guardian, language, reminder channel, therapist, `admin_note` non-clinical, `anonymised_at`)
- `anamnesis` (one per patient; `regions` and `red_flags` JSON)
- `notes` (draft → signed; trigger refuses UPDATE/DELETE of a signed note unless the session sets `physiopulse.legal_tool = on`, reserved for the owner's anonymise/delete tools)
- `test_results`, `prescriptions` (sessions used = appointments with that prescription marked done)
- `consent_texts` (versioned per kind+language) and `consents` (signature PNG, expiry, revoked)
- `documents` (index of files in the archive folder; see migration 0004)

## Database (migration 0003)

- `products` (price list), `patient_plans` (sold packages/plans, prices copied at sale), `appointments` + `class_attendees` gain `plan_id`, `charge_cents`, `charge_waived`
- `payments` (identity `number`; trigger: never deleted, only invoice fields/note/void can change, nothing in a closed month), `finance_files`
- `expenses`, `cash_closes`, `staff_pay`, `salary_payments`, `month_closes` (trigger locks expenses and salaries of a closed month)
- `appointments` (+ `series`, `class_attendees`), `waitlist`, `booking_requests`

## Database (migration 0004)

- `documents` gains `note_id` (one PDF per signed note, unique) and a `kind` check; trigger refuses UPDATE/DELETE of a `note` document unless `physiopulse.legal_tool = on` (same rule as signed notes)

## Database (migration 0005)

- `patients.deleted_at`: a deleted patient who has payments stays as a nameless placeholder (payments are never deleted). Lists and the file read skip rows with `deleted_at`. The legal tools (`server/src/routes/legal.rs`) run in one transaction with `SET LOCAL physiopulse.legal_tool = 'on'`; the archive folder is removed after commit.

## Database (migration 0006)

- `consent_texts.approved_at/approved_by` (texts that existed before are approved; the six first drafts written by Claude arrive unapproved).
- `documents.consent_id` + `consent_event` (`signed`/`revoked`, unique per consent and event); kind `consent`; the lock trigger covers `consent` like `note`.

## Database (migration 0007)

- `reminders_sent (appointment_id, patient_id, channel, sent_by, sent_at)`; `clinic.reminder_pt/reminder_en` (message templates).

## Database (migration 0008)

- `staff.professional_id` (cédula profissional, printed under signatures on packs and letters).
- `discharge_letters (patient_id, lang, addressee, body, ai_drafted, created_by, approved_by, approved_at)`: approved rows locked by trigger (only `physiopulse.legal_tool` gets past). Anonymise/delete remove them.
- `documents.letter_id` (unique); the documents lock trigger now also covers approved-letter PDFs.

## Database (migration 0011)

- `notes.ai_assisted`, `expenses.ai_read` (labels: the text/details started from the local AI).
- `clinic.opening_hours` (JSON per weekday); `closures (starts_on, ends_on, label)` and `appointments.closure_id` (cascade).
- `finance_files.kind` gains `report`; `ai_reports (month, lang, facts, text, model, file_id)`.
- `assistant_changes (skill, target, params, before, after, rows, status pending/applied/cancelled/undone)`.

Add new tables with a new numbered file in `server/migrations/` **and** list it in `MIGRATIONS` in `src/db.rs`.

## Database (migration 0013)
`recovery_codes` (owner's 10 single-use codes, SHA-256 only), `reset_codes` (owner-made, 24 h, SHA-256 only), `invites.created_at` + `revoked_at`, and `clinic.nif`, `ers_number`, `address`, `phone`, `email`, `logo` (JPEG bytes), `updated_at`.

## Database (migration 0014)
`clinic.employer_ss_bp` (default 2375 = 23,75 %), `staff_pay.employer_ss_bp` (null = clinic rate), `salary_payments.employer_ss_cents` (kept per payment; 0 for older ones).

## Database (migration 0016)
`staff.ai_enabled` (trigger `staff_ai_role`: always true for owners, false for reception). `clinic.stt_url`. `recordings` (patient, appointment, template, lang, status, chunks, bytes, seconds, attempts, error code, note_id, model, created_by/at, finished_at, ready_at, delete_after = +30 days, deleted_at, deleted_why ∈ signed/by_hand/expired/patient/day). **No clinical text in the database:** audio, transcript and draft are files in `AUDIO_DIR/<id>/` (`audio.wav`, `transcript.txt`, `draft.json`, `blocks.json`), a folder the backup container doesn't mount — so pg_dump backups never hold a recording.

## Database (migration 0017)
`devices` (key_hash, name, code, status pending/approved/revoked, how setup/carried_over/owner/recovery_code/console, requested_by, approved_by, dates, last_seen, last_ip). `sessions.device_id` NOT NULL (existing sessions were deleted: everyone signs in once after the update). `trusted_devices.device_id` + `carried_over` (true for every token made before 0017). `pending_signins` (ticket hash, staff, device, trust, enrol, 30 min). `rate_limits` (key, fails, window_start; 15-minute window). `staff.totp_secret` is `v1:`+base64(nonce‖ChaCha20-Poly1305) when `SECRET_KEY` is set (plain values are encrypted at start-up).

## Database (migration 0018)
`website` (one row: switches and texts). `staff.web_visible/web_title_*/web_bio_*`. `products.web_visible`. `booking_requests.source/day_part/address/lang`. Schema `portal`: views `site`, `services`, `team`, `closures`; table `hits` (rate limits by hashed key); functions `hit()` and `request_booking()` (SECURITY DEFINER). Role `physiopulse_portal` (NOLOGIN until the server sets `PORTAL_DB_PASSWORD`; grants re-applied at every start by `db::portal_role`).

## Database (migration 0019)
`staff_ai_by_role()` keeps the owner on, starts reception off, and turns the switch off on a change of role (reception may now have it). `assistant_changes.skill` also allows `book, move, cancel, mark, accept_request, decline_request, waitlist_add`. `booking_requests.created_patient`.

## Database (migration 0020)
`devices.kind` (computer | phone, default computer). `sessions.phone` (bool) + partial index on `staff_id` where phone.

## Archive folders (for the owner and the local AI)

Under `ARCHIVE_DIR` (default `/data/PhysioPulse`, mapped to `$DATA_DIR/archive`):

```
Clinica/Utentes/<n.º>-<nome>/   Documentos/ (uploads, reimbursement packs), Notas/ (signed notes as PDF), Consentimentos/ (signed consents with the signature, and withdrawals, as PDF), Cartas/ (approved discharge letters)   (phase 2)
                                (n.º is 4 digits; the folder is found by number, so a renamed patient keeps it)
Clinica/Financas/<ano>/<mês>/   Entradas/ (invoice PDFs), Saidas/ (receipts)                   (phase 3)
Clinica/Financas/Exportacoes/   accountant export zips                                         (phase 3)
Clinica/Agenda/<ano>/<mês>.csv                                                                  (phase 2)
Clinica/Financas/<ano>/Relatorios/  monthly reports (server figures + AI commentary) as PDF          (phase 4)
Clinica/Sistema/                access log exports, complaints                                  (phase 1+)
```

The database stays the source of truth; the archive is a readable mirror written by the server.

## Configuration (server env)

`DATABASE_URL`, `BIND` (default `0.0.0.0:8080` inside the container), `ARCHIVE_DIR`, `IDLE_MINUTES` (15), `CLINIC_TZ` (Europe/Lisbon), `OLLAMA_URL` (must be the `ollama` container, localhost or a private/Tailscale address over plain HTTP — anything else is refused), `OLLAMA_MODEL` (default `gemma3:12b`), `RUST_LOG`, `AUDIO_DIR` (default `/data/audio`; compose mounts `${DATA_DIR}/audio`, never into the backup container), `WHISPER_URL` (whisper-server on the AI PC, e.g. `http://192.168.1.66:8178`; local addresses only; the app's Settings › Local AI wins), `RECORDING_POLL_SECS` (15), `PUBLIC_ADDRESS` (shown in Settings › Server; compose sets it from `PUBLIC_URL`), `BACKUP_CONTROL` (default `/backup-control`, the folder shared with the backup container), **`SECRET_KEY`** (64 hex; encrypts the two-step secrets; required by compose; never in backups), **`TRUSTED_PROXIES`** (CIDRs allowed to send `X-Forwarded-For`; default `127.0.0.1/32,::1/128,172.16.0.0/12` = Docker, where NPM runs). **`PORTAL_DB_PASSWORD`** (phase 4a: 16+ letters/digits; the server sets it on the `physiopulse_portal` login at start-up; empty = that login can't sign in), **`SITE_URL`** (the website's public address, shown in Settings › Website). The website service reads `DATABASE_URL` (the `physiopulse_portal` login), `BIND` (default `0.0.0.0:8090`), `FORM_KEY` (64 hex; signs the form and hashes visitor addresses; random if empty) and `TRUSTED_PROXIES`. Compose adds `BIND_IP` (default `0.0.0.0`), `SITE_PORT` (8096), `SERVER_PORT` (8095 on the owner's server because Nginx Proxy Manager has 8080), `PUBLIC_URL`, `POSTGRES_PASSWORD`, `DATA_DIR`, `BACKUP_DIR`, `AGE_RECIPIENT`, `RCLONE_REMOTE`. `TAILSCALE_IP` is no longer used.

Live server: ZimaOS, `/DATA/AppData/physiopulse` (until the 0.15.0 move: `/DATA/AppData/clinica`), API at `http://100.68.180.126:8095` (Tailscale) until phase 3 is deployed; then the clinic's HTTPS address through Nginx Proxy Manager. The server runs as uid 10001, which owns `$DATA_DIR/archive`.

## API and database (PhysioPulse plan, phase 4b: patient accounts — 23 Nov 2026)
Server 0.24.0, website 0.2.0, migration **0024_patient_accounts**.
- **Split of duties.** The website (`physiopulse_portal` login) only signs patients in: it calls `portal.account_by_email`, `invite_check`, `account_create`, `account_link`, `account_reset`, `session_start`, `session_get`, `session_two_step_ok`, `session_end`, `account_update` (SECURITY DEFINER; all refuse while `website.published AND website.accounts_open` is false). It still can't read any table. What a signed-in patient sees or does comes from the staff server's **internal API**: `GET /api/internal/portal/me`, `POST …/cancel`, `POST …/request`, `GET …/invoice/:id`. Each call needs `x-portal-key` (SHA-256 of `physiopulse-portal-internal:` + PORTAL_DB_PASSWORD, so nothing new to set up; wrong or missing → 404) and `x-portal-session` (the patient's cookie; only a SHA-256 is stored). Answers cover only the patients linked to that account; no notes, tests, prescriptions or documents. The website reaches the server at `SERVER_URL` (default `http://server:8080`, the compose network).
- **Tables:** `patient_accounts` (email, Argon2id hash made by the website, two-step secret sealed by the website with ChaCha20-Poly1305 under a key derived from FORM_KEY, lang, last sign-in), `patient_account_links` (account ↔ patient, `self` or `guardian`), `patient_codes` (invite 14 days / reset 24 h, SHA-256 of the code only, one use), `patient_sessions` (30 min idle, 12 h max, `pending_2fa`). `booking_requests` gains source `portal`, `kind` (`book`, `move`, `cancelled`) and `about_appointment_id`. `website.accounts_open` (off by default; `portal.site` shows it). A trigger removes links, codes and orphan accounts when a patient is anonymised or deleted.
- **Staff API:** `GET /patients/:id/portal` (everyone: accounts, unused invites, open, minor, can_manage, site_url, clinic, patient name/phone/lang), `POST …/portal/invite {relation}` (everyone; under-16 → guardian only; anonymised → refused), `DELETE …/portal/invites/:cid`, `POST …/portal/reset {account_id}` and `DELETE …/portal/accounts/:aid` (reception, the patient's therapist or the owner). `GET /booking-requests` adds `about_starts_at`. Booking a `move` request cancels the old session; Undo books it back. A `cancelled` request can only be marked seen (decline); booking it is refused.
- **Website pages** (`server/portal/src/account.rs`): `/conta` (+ `/en/account`), `/conta/entrar`, `/conta/codigo`, `/conta/sair`, `/conta/convite`, `/conta/repor`, `/conta/pedido`, `/conta/cancelar`, `/conta/seguranca`, `/conta/fatura/:id`. One cookie `pp_conta` (HttpOnly, Secure, SameSite=Strict, 12 h), set only by these pages; forms on signed-in pages carry an HMAC of the session. Rate limits with `portal.hit`: sign-in 10/15 min per address and per email, two-step 10/15 min, invite/reset 10/hour, settings 15/15 min; requests 10/day per account (server).
