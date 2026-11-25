# Security

Health data in Portugal (RGPD, Lei 12/2005, Lei 58/2019). These are **release gates**: no real patient data goes in until every item is checked.

Status key: ✅ done and tested · 🟡 built, needs the owner's machines · ⬜ not started

## Desktop app
- ✅ Only the Rust core talks to the server; the web view has no filesystem, shell or network permissions (`src-tauri/capabilities/default.json`).
- ✅ Strict CSP in `tauri.conf.json`; DevTools are off in release builds by default.
- ✅ Session token held in Rust memory only, never given to the UI, dropped on close (stronger than storing it in the Keychain: closing the app always signs out).
- 🟡 Signed and notarised builds (Apple Developer ID; Windows code-signing certificate): not bought yet (DECISIONS 10 Nov).
- 🟡 Signed auto-updates (`tauri-plugin-updater`, the owner's minisign key, signed version, HTTPS from GitHub; `RELEASING.md`): built 25 Nov 2026; checked once the first release (0.15.0) is installed and updated.
- 🟡 FileVault (Mac) / BitLocker (Windows) switched on for every clinic computer.
- ✅ No patient data in server logs: request spans record method + path only (never the query string); database errors are turned into plain messages and only their code is logged; access-log details hold ids and field names, never values (checked in `e2e_phase2.py`). Keep it that way in new code.

## Sign-in and access
- ✅ Personal accounts only; invite codes for staff; first-run setup locks itself after the owner exists.
- ✅ Argon2id passwords, 12+ characters; same message and timing for unknown email vs wrong password.
- ✅ Two-step verification (TOTP) for everyone; codes can't be reused.
- ✅ Trusted computers (10 Nov 2026, owner decision): after the code, that person on that computer needs only the password for 30 days. Random 32-byte token, only its SHA-256 on the server, bound to one person; kept by the Rust core in `account.json` (config folder, 0600), never given to the UI. Revoked by "Esquecer neste computador", sign-out-everywhere and deactivation; logged (`login.computer_trusted`, `login.ok` + `trusted_computer`). Risk accepted: someone with the computer's files *and* the password skips the code — hence FileVault/BitLocker stays a gate. Checked in `e2e_trusted.py`.
- ✅ Account recovery (audit C4, 10 Nov 2026): change own password (ends other sessions; current password needed, rate-limited); owner-made one-time reset codes (24 h, newest only, SHA-256 only, rate-limited per email); owner can reset someone's two-step (signs them out, forgets trusted computers); owners get 10 single-use recovery codes at two-step setup (SHA-256 only; a code either re-enrols two-step or resets the password, never both). All logged; no codes in the server log (checked in `e2e_account.py`).
- ✅ 15-minute idle timeout on server and client; 12-hour absolute session limit; sign-out on close.
- ✅ Rate limiting: 5 failures / 15 min per email (sign-in) and per device (invite codes).
- ✅ Roles enforced on the server for every request.
- ✅ Access log, append-only by database trigger, survives backup/restore.
- ✅ Every read of a patient file written to the access log: `open_file()` in `server/src/routes/patients.rs` is the only file read (`patient.viewed`, full/reduced); single consents log too; the list logs `patients.listed`.
- ✅ Reception gets a server-side reduced view of patients (no anamnesis, notes, tests; documents only of kind `prescription`; prescriptions read-only by owner decision) and 403 on clinical writes; Today alerts leave out clinical ones for reception.
- ✅ Patient documents: type checked by the file's first bytes, 10 MB max, names stripped of path characters; every open (`document.opened`) and saved copy (`document.saved`) is logged; reception gets 404 (not 403) for clinical documents. Files are opened inside the app; only Save a Copy… puts one on a computer (Downloads).
- ✅ Signed progress notes can't be changed or deleted (database trigger), and neither can their PDF copy in the archive (trigger on `documents`).
- ✅ AI features callable by the owner role only, checked on the server (letter draft 8 Nov; note rewrite, receipt reading, monthly report, assistant 9 Nov 2026 — `e2e_phase4.py` checks therapist and reception get 403 and nothing reaches the AI). Only what each job needs is sent: no patient name for a note rewrite; aggregate figures for reports; the assistant sees prices, staff names and hours, and since 11 Nov 2026 the agenda (patient names, times, therapists; never contacts, notes or money). AI-made text and expenses are labelled. Assistant changes need Apply, are refused if stale, logged, and undoable.
- ✅ The AI is local only: `ai.rs` refuses any `OLLAMA_URL` that isn't the `ollama` container, localhost or a private/Tailscale address over plain HTTP. Prompts and answers are never logged; the access log says only that a draft was asked for. The AI never saves: the owner uses, retries or discards the draft, then edits and approves.
- ✅ Approved discharge letters and their PDFs can't be changed or removed (triggers); reception gets 403 on packs and letters and 404 on their documents.

## Server and data
- ✅ API published only on the Tailscale IP; database and Ollama have no published ports. **Phase 3 (built 11 Nov 2026, not deployed):** the API is published on the server's own address (`BIND_IP`) for Nginx Proxy Manager, which gives it HTTPS at the clinic's public address; the router forwards only 443 (+80 for Let's Encrypt) to NPM, never 8095. Database and Ollama still have no published ports; the AI PC is reached only by the server on the home network.
- ✅ Nightly backups encrypted to an age **public** key (the server can't read its own backups); 30 days kept; optional off-site copy via rclone.
- ✅ Restore tested (here): backup restored into an empty database with identical row counts.
- ✅ Backup state is real (audit C1, 10 Nov 2026): the backup container writes a status file the server reads; Settings › Server shows the newest backup's time and files, failures and the off-site result; Today warns the owner when the backup is over 36 hours old, failed or can't be seen. "Back Up Now" works through a request file. The server can't read the backups (root-only folder, age public key) — the shared folder holds only the status and the request.
- 🟡 Tailscale access rules limiting `server:8095` (the PhysioPulse port) to clinic devices (admin console).
- ⬜ `DATA_DIR` on an encrypted disk — **not for now, owner's decision (7 Nov 2026), confirmed 11 Nov 2026 for phase 3** (asked again with the API going public: "keep plain disk for now"). Remaining risk: someone taking the server or its disk, or a break-in through the public API reading the files in the clear. What limits the second: device approval, rate limits, encrypted two-step secrets, encrypted backups.
- ✅ Restore test on the real server passed 4 Nov 2026 (empty database). Repeat with real accounts, then once a quarter.
- 🟡 age private key made off the server (owner's PC, 4 Nov). Owner to confirm it's in a password manager + printed, and deleted from Documents.
- ⬜ Optional: HTTPS inside the tailnet with `tailscale serve` (WireGuard already encrypts traffic).

## Legal / organisational (owner + adviser)
- 🟡 Data-protection impact assessment (DPIA): draft in `DPIA.md` (9 Nov 2026) — owner to fill in, review and sign.
- 🟡 Privacy notice for patients: draft in `PRIVACY_NOTICE.md` (PT + EN). ⬜ Consent texts reviewed.
- 🟡 Staff training: `STAFF_GUIDE.md` with a sign-off list.
- ✅ Complaints log with the ERS deadline (10 working days), owner only (8 Nov). ✅ ERS registration number, NIF and address in Settings › Clinic, printed on every PDF (10 Nov). ⬜ Owner to fill them in.
- ⬜ Retention of clinical and tax records confirmed with the accountant.

## PhysioPulse plan gates (11 Nov 2026) — must be ✅ before a real patient uses each part
Plan: https://claude.ai/artifact/LS7Lg4hYTn7JQ2uDwJLym8 (phases: 1 rename · 2 recording · 3 public foundations · 4 website/portal · 5 iPhone).
- ✅ Phase 1 (rename): the record locks answer only to `physiopulse.legal_tool` (migration 0015; tested: the old name unlocks nothing). The move keeps the old Clínica folders untouched, checks every table's row count, and rolls back by itself.
- ✅ Phase 2 (recording) — built and tested (`e2e_recording.py`, 73 checks), **not deployed**: audio and transcripts stay on the clinic server (`AUDIO_DIR`) and the AI PC only (local addresses enforced for whisper as for Ollama); deleted when the note is signed, by hand, or after 30 days unsigned (owner decision); the audio folder isn't mounted in the backup container and the database holds no transcript or draft (checked with pg_dump); every start, upload, open, play and deletion in the access log, never content; one AI switch per person (owner, 11 Nov: umbrella switch), off by default, server-enforced, and a database trigger keeps it on for owners and off for reception. Accepted risk (owner): no record kept of the patient's agreement to recording. **Known limits:** while a session is being recorded the app doesn't lock itself after the idle time (locking signs out and would end the recording; it locks as usual 15 min after recording stops, and Lock asks to stop first); audio crosses Tailscale from the room PC to the server and from the server to the AI PC (encrypted by WireGuard; plain HTTP inside it, like Ollama); the transcript and draft can be read by the owner and the author while they exist.
- ⬜ Phase 2 gate still open: the owner judges transcription on the invented two-person session (`claude/RECORDING_TEST.md`) before a real patient is recorded.
- ✅ Phase 3 (public staff API) — built and tested 11 Nov 2026 (`e2e_devices.py`, 63 checks; all 17 suites green; upgrade from 0.16 rehearsed on a populated database), **not deployed**:
  - ✅ Device approval for every new device: after password + code a new device gets only a ticket and waits (`pending`); the owner approves it from an approved device (Settings › Devices, Today alert); backups: a printed recovery code on the waiting screen (owner only), then the server console (`physiopulse-server approve CODE`). One approval per device, for everyone who uses it. Sessions are bound to the device key; a session from another device, or without the key, is refused.
  - ✅ Device list with rename, remove/decline (sessions end at once, trusted tokens forgotten, sign-in refused after the password), approve again. Can't remove the device you're on.
  - ✅ Computers trusted before phase 3 approve their device on their first sign-in, once (owner decision 11 Nov). A removed device can't be re-approved by an old token.
  - ✅ Rate limits in the database (survive restarts): 5 failures per email/account/key in 15 min, 30 per internet address (sign-in, reset, invites, waiting tickets). The real address comes from `X-Forwarded-For` only when the connection is from Nginx Proxy Manager (`TRUSTED_PROXIES`); a client can't fake it.
  - ✅ TOTP secrets encrypted with `SECRET_KEY` (ChaCha20-Poly1305), encrypted at start-up when the key is first set; the key is in `.env` only (installer makes it, prints it once), never in backups. Settings › Server shows it.
  - ✅ The desktop app refuses plain http to internet addresses (https://), allows it only to private/Tailscale/LAN addresses during the move.
  - ⬜ HTTPS: Nginx Proxy Manager proxy host with a Let's Encrypt certificate, "Force SSL" and HSTS on (owner, with DEVELOPMENT.md › Public address).
  - ⬜ Router: only 443 and 80 forwarded to NPM. Check from outside that 8095 is closed.
  - ⬜ Only then: Tailscale off on the clinic computers, the server and the AI PC (AI PC address reserved in the router).
  - **Known limits:** the clinic's address is public, so anyone can try the sign-in (rate limits + device approval are what stop them); `/api/status` and `/health` answer without sign-in (clinic name and version); the device key sits in a file in the user's own app folder (copying that file and the password and the phone code would be needed to impersonate the device); a lost recovery-code sheet plus the owner's password and phone would let someone approve a device.
- ✅ Assistant schedule jobs and one-press booking (11 Nov 2026, `e2e_agenda.py` 84 checks): the assistant opens for anyone with the AI switch, but the owner's jobs stay owner-only on the server whatever the AI answers; reception with the switch still can't record or draft clinical text; a schedule change is applied/undone only by whoever asked or the owner; previews are rechecked on Apply; the server log has no names or prompts; the access log has the patient as subject and no names in the detail.
- ✅ Phase 4a (public website + appointment requests) — built and tested 11 Nov 2026 (`e2e_website.py`, 94 checks; all 18 suites green), **not deployed**:
  - ✅ Separate service and container (`server/portal`, `physiopulse-site`) with its own database login `physiopulse_portal`: not a superuser, can read only the `portal` views (clinic details, website texts, the price list, the team page, coming closures) and write only through `portal.request_booking()` and `portal.hit()` (SECURITY DEFINER, `search_path` fixed). Tested: it can't read patients, staff, sessions, payments, booking requests, the access log, the clinic or website tables, can't insert requests directly, can't change the settings or create tables. The team view has no email column.
  - ✅ The database checks every request again (published, requests open, service allowed, name, a contact, address for home visits, day from tomorrow to two months, lengths) and caps the website at 100 requests a day in total. Each request is in the access log (`booking_request.received`, device "site") without the person's details.
  - ✅ No JavaScript, no cookies (tested: no `Set-Cookie`), no analytics or outside resources; strict CSP (`default-src 'none'`, `form-action 'self'`, `frame-ancestors 'none'`), `no-referrer`, `nosniff`, Permissions-Policy; everything from the database and the visitor is HTML-escaped (tested with a script tag); owner texts can't contain `<` or `>`.
  - ✅ Spam: a signed form token (3 s – 2 h, HMAC with `FORM_KEY`), a hidden field robots fill in (answered as sent, nothing stored), 5 requests an hour and 10 a day per internet address (`X-Forwarded-For` only from Nginx Proxy Manager), where only a keyed hash of the address is kept, for two days.
  - ✅ Unpublished: every page says "being prepared", `noindex`, robots.txt disallows everything, and the database refuses requests. Publishing needs the clinic's name, NIF and a contact; owner only (server-checked); published/unpublished/changes in the access log.
  - ✅ Website requests from people who never became patients are deleted after 90 days (server housekeeping; privacy policy says so).
  - ✅ After a restore into a new database server the website's login and grants are re-created at start-up; `restore.sh` restores with `--no-privileges` so it never fails on that login.
  - ⬜ Owner: review and approve the privacy policy and terms on the site (drafts written by Claude; not legal advice) before publishing. Fill in NIF, ERS registration and contacts in Settings › Clinic.
  - ⬜ HTTPS: NPM proxy host `physiopulse.duckdns.org` → port 8096 with Let's Encrypt, Force SSL, HSTS (DEVELOPMENT.md › Website). Never forward 8096 on the router.
  - **Known limits:** anyone can send a request (the form is public by design: rate limits and the daily cap stop floods, not one determined person); the request reaches reception, who also see its message (the form asks people not to write health details there); the site shows the clinic's phone, email and NIF (required); the date field uses the visitor's browser format.
- ✅ Phase 4b (patient accounts) — built and tested 23 Nov 2026 (`e2e_portal.py`, 105 checks), **closed until the owner opens it**:
  - ✅ Same separate service and limited login; the website still can't read any table (tested for patients, appointments, payments, notes and the new account tables). Patient data reaches it only through the staff server's internal API, which needs the website key and a live, fully signed-in session, and answers only for the linked patients.
  - ✅ Argon2id (10–200 characters); the same answer and the same work for an unknown email and a wrong password; sessions stored as SHA-256, 30 min idle / 12 h; cookie HttpOnly + Secure + SameSite=Strict, only on /conta pages (public pages still set no cookie — tested); per-session form token.
  - ✅ Codes: 12 characters without look-alikes, SHA-256 only, one use, invite 14 days, reset 24 h (an older reset code stops working); reset codes only from reception, the patient's therapist or the owner (owner, 23 Nov: no date-of-birth check); a reset turns two-step off and ends every session.
  - ✅ Two-step optional (TOTP, secret sealed with a key from FORM_KEY, set up with a QR and the current password; a half-signed-in session gets nothing).
  - ✅ Rate limits in the database by hashed address and by email; 10 requests a day per account on the server.
  - ✅ Under-16s only through a guardian; anonymising or deleting a patient removes access; staff can remove an account's access.
  - ✅ What a patient does is in the access log with device "site" (account made/linked, password reset, cancellations, requests, invoice downloads); staff invites, resets and removals too.
  - ⬜ Owner: approve privacy policy version 2 (adds the patient area and its one cookie), sign the DPIA addendum, try it with a test patient, then open it in Settings › Site e portal.
  - **Known limits:** a bug in the website could start a session for any account (it holds the sign-in logic); that reaches only sessions, requests and invoices of patients who have accounts, never clinical data. Patients choose their own password (no breach-list check). Document requests by zip are not built (clinical documents stay "ask the clinic").
- ✅ Phase 5 (iPhone) — built and tested 12 Nov 2026 on the server side (`e2e_phone.py`, 38 checks), **not built on a Mac or tried on a phone yet**:
  - ✅ Phone sessions only on a device the owner approved **as a phone** (`devices.kind`, fixed when it first asks; the owner sees "iPhone" before approving). A computer sending `X-Device-Kind: phone` later still gets a computer session (tested).
  - ✅ 30-day absolute expiry, no idle timeout, one per person per phone; bound to the device key like every session (refused from another device or without the key — tested); removing the device ends it.
  - ✅ Owner signs a phone out from Team (`DELETE /staff/:id/phones/:sid`, owner only, logged); people see only their own phones.
  - ✅ Token in the iOS keychain with `kSecAccessControlUserPresence` + `kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly` (Face ID or passcode to read; not synced, not in backups, deleted by iOS if the passcode is removed); read back only after the system prompt; dropped from memory whenever the app leaves the screen (not while recording); deleted on sign-out and on any 401. The web layer never has it (same rule as the desktop). The phone never gets a trusted-computer token.
  - ✅ Role rules unchanged on the phone (server-side; tested: a therapist's phone has no finances or device list). Reception gets the reduced patient file. Every file opened is in the access log.
  - ⬜ On a real iPhone: Face ID prompt on each reopen, cancel stays locked, deleting the keychain after the owner signs the phone out, recording audio still goes only to the clinic server.
  - **Accepted risks (owner, 11 Nov):** no idle lock while the app is open and on screen; the free Apple account means a weekly rebuild. **Known limits:** the passcode is accepted instead of Face ID (iOS standard); while a session is being recorded the app doesn't lock when it leaves the screen (locking would stop it) and keeps the screen on; patient data shown on screen can be seen by anyone holding the unlocked phone while the app is open.
- ✅ All AI output for the clinical record is plain text (no titles, lists or SOAP), checked in code (`server/src/style.rs`) before the owner sees it.

## Known limits to revisit
- TOTP secrets are stored in plain text in the database (the disk isn't encrypted, owner decision 7 Nov — so this matters more). Phase 2+: encrypt them with a server key from the environment.
- Rate limiting is in memory (resets on server restart). Fine for one clinic; move to the database if it becomes a concern.

## Reminders (7 Nov 2026)
- Reminders leave the clinic from the staff member's own WhatsApp/Messages/email: the desktop shell only opens `sms:`, `mailto:` and `https://wa.me/` links (`src-tauri/src/messages.rs`), and the message template must never contain clinical information (said in Settings). The server stores only that a reminder was sent (who, when, channel), never the text.
