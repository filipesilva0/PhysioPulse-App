# Decisions

Every decision the owner made, newest last. Don't reverse one without asking them.

## 3 Nov 2026 — scope and platform
- Fully separate product from PhysioPulse. Internal use for one clinic for now; more therapists later.
- Mac and Windows desktop app first. A patient-facing website later; a patient-facing mobile app eventually.
- Tauri + Rust is acceptable "if safe enough" for health data → yes, with the gates in SECURITY.md.
- Roles: owner, therapists, receptionist. Only the owner sees finances.
- Bilingual PT/EN, switchable. Lotus layout and frosted effect, but its own colors. Follow the Apple HIG.
- Data: everything on the ZimaOS home server, reached over Tailscale, neatly organised in folders so a local AI can read it. No offline mode needed.
- Ollama runs on the server, not on each computer.

## 3 Nov 2026 — clinical
- Records kept forever, with export, anonymise and delete per person or per day.
- Legal tools: full-record export, deletion/anonymise flow, consent expiry tracking, complaints log (ERS 10-working-day deadline). All six consent types.
- These legal tools live on a button on each patient card, not in Settings (owner comment on the spec). Clinic-wide ones (complaints log, delete a day) are in the File menu.
- Progress notes: templates with sliders, multiple-choice questions, add/remove blocks, free text, plus a local-AI "Rewrite" button. The AI never saves on its own.
- No body chart (takes too much space): instead, an "Add region" button in the anamnesis, then a symptom picker per region.
- Discharge letter: AI drafts into a Word template.
- Tests/questionnaires: the PhysioPulse library, rebuilt natively. Alert when a score worsens.
- Clinical record export: full or summary, chosen at export time.
- No home exercises and no extra clinical tools for now.

## 3 Nov 2026 — schedule
- Session length customisable (30/45/60). Overlapping sessions sometimes. Group classes and home visits.
- No no-show policy for now.
- Online booking open to anyone (later, on the patient portal). Slots shown one month ahead.
- Waitlist: optional preferred window; cancelled slots offered first-come-first-served by SMS/email; 2 hours to respond.
- Reminders only (no confirm-by-reply). Blocked time with a label and colour; admin can change it.

## 3 Nov 2026 — money
- Payments: cash, MB WAY (to the clinic phone, recorded by hand), bank transfer, and Stripe QR/link (card, MB WAY, Multibanco).
- Monthly plans and instalments charge automatically (card/SEPA via Stripe); MB WAY patients get a monthly SMS request.
- Packages: owner set the prices (see SPEC.md › Money). Claude flagged that the pack of 10 and the monthly plans don't save money vs singles; owner hasn't decided.
- Accountant export: one zip of ins and outs, sorted into folders.
- Expense receipts: photo → local AI pre-fills → owner confirms.
- Business form: "probably company, maybe sole trader" → support both; invoicing route undecided (manual first).
- Overdue reminders: the owner handles them personally. No gift vouchers or promo codes for now. Finance overview is enough for reports.

## 3 Nov 2026 — AI
- All AI features are owner-only for now. Therapist access to note rewriting may be unlocked later per person.
- Admin assistant: the owner can say "set the assessment price to €35" and it does it, after a preview and confirmation. One "skill" per area. Changes also publish to the patient portal once it exists.

## 4 Nov 2026 — look and sign-in
- Reference images in `references/` set the look: warm light-grey canvas, white rounded cards, capsule buttons, a day strip with dots, appointment cards with avatar and "time • therapist", one dark highlight card for money owed, grouped settings rows.
- Sign-in opens first; "Create account" is secondary. Staff join with an invite code.
- The app follows the system's light/dark setting (no in-app theme switch, per the HIG).

## 4 Nov 2026 — patient portal
- PhysioPulse will be deprecated; the new website becomes the patient portal (book, cancel/reschedule, pay, download invoices, intake form, message the therapist).

## 4 Nov 2026 — deployment
- The owner has a Windows PC and a Mac: both builds are needed (Apple Developer ID too).
- Deploy on ZimaOS now with data on the normal `/DATA` disk for testing; move to an encrypted disk before any real patient data.
- Backup key pair made on the owner's Windows PC; private key kept off the server.
- Clínica uses port 8095 on the Tailscale address (8080 stays with Nginx Proxy Manager).

## 4 Nov 2026 — phase 2 UI
- Next step chosen: the phase 2 UI (Patients, then Schedule, then Today on real data) before the remaining phase 2 server work.
- Reception **sees prescriptions (credenciais)** — doctor, payer, sessions used/left, validity — in the patient file and in Today's alerts, so they can book against ADSE/insurer prescriptions. Creating and editing prescriptions stays with therapists and the owner.

## 5 Nov 2026 — phase 3 (money)
- Start phase 3 now, overlapping phase 2 (document upload and legal tools wait).
- The clinic is a **company**: invoices will come from a certified invoicing provider's API (InvoiceXpress, Vendus or Moloni — not chosen). Until then the manual route: issue the invoice in that software and record its number + PDF in the app.
- Prices start with the **suggested ladder**: pack of 10 €170, continuous care 4/month €72, 8/month €136 (everything else as in SPEC.md; all editable).
- **Manual payments first, Stripe later** (cash, MB WAY, transfer, card terminal recorded by hand).
- Defaults chosen by Claude while building (ask if any is wrong): reception can sell packages and record payments (matches the Team permissions table), but Finance pages stay owner-only; payments are voided, never deleted, by the owner or by whoever recorded them the same day; a done session uses a monthly plan first, then the package that expires soonest; a closed month can be reopened by the owner (logged).

## 7 Nov 2026 — keep building, deploy last
- Keep building the app; **update the server on ZimaOS last** (one redeploy at the end, not after each feature).
- Defaults chosen by Claude for patient documents (ask if any is wrong): reception can add and open **scanned prescriptions only** (it never sees that other documents exist); therapists and the owner see everything. A document can be removed by the owner, or by whoever added it on the same day. The PDF of a signed note can't be removed or changed, like the note. Documents open inside the app; "Save a Copy…" is the only way to put one on a computer, and both are logged.

## 2026-11-07 — Calmer pages, same frame
- Owner kept the frame: frosted title bar and icon rail, separate content panel (an attempt at a single-surface window with a page sidebar was rejected).
- Inside the panel: no inspector column, no patient/staff list column. Pages with sections (Schedule, Finance, Settings) keep a slim sections column; everything else is one content area.
- Patients and Team open as lists; choosing someone opens their file, with ‹ Back (title bar, Go menu, ⌘[ / ⌘]).
- Summary figures are plain numbers, not cards; ⋯ buttons in rows appear on hover.

## 7 Nov 2026 — legal tools on the patient file
- **Export the clinical record:** owner and therapists. **Anonymise and Delete:** owner only (enforced on the server).
- **Delete is allowed even with signed notes**, with strong confirmation: the owner types the patient's name. Everything about the person goes, including signed notes and the archive folder. Payments stay for tax, without a name ("Utente apagado"). Claude suggested checking clinical-record keep periods with an adviser.
- This round: per-patient tools only. Complaints log and "delete everything from one day" come next.
- Confirmations look like the app's own alert (app mark, one sentence, name field, Delete/Cancel stacked). The owner rejected a first version with two boxed lists as ugly.
- Defaults chosen by Claude (ask if any is wrong): anonymising keeps the birth year, removes all documents (they carry names and faces) and consent signatures, cancels future bookings, and makes the file read-only. The text of signed notes stays as written. Export filenames carry the patient number, not the name. Summary export = PDF only; full = PDF + original documents + note PDFs.

## 7 Nov 2026 — encryption, consents, reminders
- **No encrypted disk for now.** The owner accepts the risk: the server is in their home and reachable only over Tailscale. `DATA_DIR` stays on plain `/DATA`; this is no longer a phase 1 gate item. Backups stay encrypted. Claude noted the remaining risk is physical (the server or its disk being stolen) and that BitLocker/FileVault on the clinic computers is still advised.
- **Consent texts:** Claude writes the six drafts (PT and EN) from Portuguese law; the owner reviews and approves them in Settings › Consents. A draft can't be used for signatures until approved.
- **Legal questions:** Claude researches them itself (see `claude/LEGAL.md`) instead of waiting for an accountant/adviser. Findings are a starting point, not legal advice.
- **Reminders:** no SMS provider for now. Reminders are sent from the therapist's own phone/computer: the app prepares the message and opens Messages (SMS) or WhatsApp with it filled in; the person presses Send. Automatic reminders are "coming soon".
- **Signed consents are saved as PDF** in `Utentes/<n.º>-<nome>/Consentimentos/` (like signed notes) and go into the full export. A withdrawal is saved as its own PDF in the same folder.

## 8 Nov 2026 — reimbursement pack and discharge letters
- **Reimbursement pack = one PDF** (to upload to ADSE Direta or an insurer's site): identity, prescription, sessions done, invoices and a short clinical report first; then the prescription scan and each invoice file.
- **Pack: owner and therapists only** (it holds a clinical report). Reception can't make one (server 403).
- **Discharge letter: the AI drafts it and only the owner approves and corrects it.** Owner's words: "the ai should be able to draft it and only i to approve it and correct it if need be". So the local AI (Ollama on the server) is switched on for this one feature ahead of phase 4. The letter is a PDF (not the Word template from 3 Nov).
- **Discharge on approval: asked each time** ("Dar alta ao utente" ticked by default). It makes the patient inactive; future bookings stay.
- Defaults chosen by Claude (ask if any is wrong): therapists may write a letter draft by hand (pre-filled from the file) for the owner to approve, but can't use the AI; the pre-filled text marks gaps with "[a completar]" and a letter can't be approved until they're filled; the cédula profissional is asked once and remembered per person; the pack is saved in the patient's Documentos (kind "report") and the letter in Cartas/; anonymising removes letters (they name the person); AI model `gemma3:12b` by default (change `OLLAMA_MODEL` in `.env`; `gemma3:4b` if the server is slow).
- Same day, owner: remake the two sheets "more aesthetically pleasing and easier to read with no clutter". Claude's choices: grouped rows, disclosure for the session list, letters save themselves (no Save Draft button), the discharge tick and cédula live in the Approve alert, no extra buttons under Documents.

## 8 Nov 2026 — complaints, delete a day, VAT, hold-to-delete
- Owner: build the complaints log, "delete everything from one day" and the VAT marks next.
- **Every delete is a button held for 5 seconds** (owner's words: "everything thats deletable should be a hold button delete for 5 sec"). Applies to every action that deletes data, including the patient delete (after typing the name) and anonymise.
- Defaults chosen by Claude (ask if any is wrong): complaints are owner only, like the File menu entries; the deadline is 10 working days after receipt for every complaint (Carnival and the municipal holiday count as working days, so it errs early); a paper complaint is done once entered with the ERS, others once answered; deleting a patient keeps the complaint without the link. "Delete a day" removes appointments, notes (signed ones too, with a warning), tests and documents added that day, and keeps payments, invoices, consents, letters and expenses. The 20-year keep period is a warning on Delete Patient, not a block. Group classes start "subject to VAT (23 %)" until the owner marks a class therapeutic. Removals that already offer Undo (waitlist, class list, a test just recorded) keep Undo instead of the hold.

## 8–9 Nov 2026 — deploy and where the AI runs
- The local AI (Ollama) runs on **another clinic machine on the Tailscale network, 100.86.97.21** (a Windows PC with a GPU-capable setup), not on ZimaOS. Replaces "Ollama runs on the server" (3 Nov). Patient text for letter drafts travels only inside the clinic's Tailscale network; the server still refuses any public address (`ai.rs`).
- Model: **gemma4:12b** (already on that machine; gemma3:12b isn't). Set in `/DATA/AppData/clinica/.env` (`OLLAMA_URL`, `OLLAMA_MODEL`).
- Delete the Ollama container and the 8 GB model that were briefly installed on ZimaOS (done 9 Nov).

## 9 Nov 2026 — after the first Windows test
- "Fix it all": every problem found in the Windows test gets fixed (see STATUS.md, 9 Nov).
- Light and dark mode can be chosen by hand: Settings › Overview › Appearance — Automatic (follows the system, the default), Light or Dark. Kept per computer. This replaces "follow the system, no in-app switch" in DESIGN.md.

## 9 Nov 2026 — phase 4
- Owner: "do phase 4" (after the Windows retest is planned). Built ahead of the phase 2/3 gates, like phase 3 was; no real data goes in until phase 1's gate is met.
- Defaults chosen by Claude (ask if any is wrong):
  - **Everything AI stays owner only** (3 Nov decision), on the AI PC (100.86.97.21). Only what a job needs is sent: a note rewrite gets the note's filled-in fields and free text, never the patient's name; a monthly report gets aggregate figures; the assistant gets the price list, staff names and opening hours, never patients.
  - **Note rewrite** puts a SOAP draft (S/O/A/P) in a labelled card; "Use This Text" replaces Free notes (with Undo), scales and choices stay. The note is then marked "AI" in the list. Signing works as before.
  - **Receipt reading** is a button after attaching the receipt (not automatic). Photos (JPEG/PNG/WebP) need a model that reads pictures — if gemma4:12b can't, use gemma3:12b or qwen2.5vl:7b for this; PDFs from invoicing software are read as text; scanned PDFs and HEIC photos are refused with a plain reason. The fields the AI filled get a violet outline; "Undo Fill" puts back what was there. Expenses read this way show an AI mark.
  - **Monthly report**: the server works out every figure; the AI only writes 3–5 paragraphs of commentary, which the owner can correct before "Keep Report". Kept reports are PDFs in `Financas/<ano>/Relatorios/` and can be deleted (hold).
  - **Assistant skills**: prices (amount, on sale or not), opening hours per weekday, closures, staff fixed pay, deactivate/reactivate staff (never the owner), and month summaries. A proposal can be applied within a day; it's refused if the thing changed since; Undo works while the thing is still as the change left it.
  - **Opening hours and closures are now real** (they were a mock-up in Settings › Schedule): default Mon–Fri 08:00–20:00, Sat 09:00–13:00, Sun closed. A closure blocks each open day in the schedule (no therapist); appointments already booked stay booked and the app says how many to reschedule. Public holidays aren't closed automatically yet (the switch was removed rather than left as a mock-up).
  - Phase 4 gate papers drafted for the owner: `DPIA.md`, `PRIVACY_NOTICE.md` (PT + EN), `STAFF_GUIDE.md`.

## 10 Nov 2026 — sign-in and assistant
- **Two-step code once per computer, for 30 days.** After the 6-digit code on a computer, the same person there only types their password for 30 days (counted from that code); then the code once more. Trust is dropped on "Esquecer neste computador", "sign out everywhere", or deactivation. Owner chose 30 days over "until revoked" (health data).
- **Welcome-back screen:** "Bem-vindo de volta, <primeiro nome>" with the password only. The owner first asked for a round profile picture, then dropped it ("just the welcome and password").
- **Assistant chat:** one conversation for the whole app (same on every page), cleared once a day; no chat history kept anywhere.

## 10 Nov 2026 — audit fixes, phase A
- After the full app audit (`AUDIT-2026-11-10.md`), the owner chose to fix **Phase A only** first (trust and safety, before any real patient data); Phase B and C come after.
- **Account recovery: "owner resets + codes".** Everyone can change their own password and name. The owner can give a staff member a one-time password reset code (24 hours) and reset their two-step (lost phone). Owners get **10 printed recovery codes** when two-step is first set up (and can make new ones in Settings › Account); one code either signs in without the phone (two-step is then set up again) or resets a forgotten password. Staff don't get codes: the owner resets them.
- **Settings › Server and backups shows the real state and "Back Up Now" really backs up.** The server reports its version, address and the newest backup (from the backup container's status file); Today warns the owner when the last backup is over 36 hours old, failed, or can't be seen.
- Defaults chosen by Claude (ask if any is wrong):
  - A setting is either wired or absent: fixed policies (two-step always on, sign-out on close, idle lock from the server) are shown as facts; unbuilt features (patient portal, Stripe, automatic monthly charges, invoicing software link, automatic waitlist offers) show one "Em breve" row each.
  - Invoicing page now says what was decided on 5 Nov: company, AT-certified software, number + PDF recorded by hand for now.
  - Progress-note scales start **unset ("—")**; only scales someone touched are part of the note. Unset ones are dropped from the signed note, its PDF, the export, letters and the AI prompt. Clicking the number clears it again.
  - Unsaved typing is never thrown away silently: the note being written and unsaved anamnesis are kept in memory per patient while you look elsewhere (dot on the tab), the note is also saved as a server draft 8 seconds after typing stops, Esc/Cancel on a sheet with typing asks "Discard changes?", and app shortcuts are off while a sheet or alert is open. Kept only in memory (never on disk), dropped when someone else signs in or the app closes; a lock by the same person keeps it.
  - Clinic details (name, NIF with check digit, ERS registration, address, phone, email, logo) are printed at the top of every PDF. Logo is turned into a JPEG of at most 600 × 240 px by the app.
  - Team: per-person "hours" removed (there was no such data); pay shows the real fixed amount with a link to Finance › Salaries; role can be changed; unused invites are listed and can be withdrawn. `src/data/mock.ts` deleted.
  - The plain sign-in screen says "Entrar" (it greeted first-time users with "Bem-vindo de volta"); the welcome-back screen keeps the owner's wording.
  - Versions: app 0.3.0, server 0.12.0; About shows both, from the build and the server.


## 10 Nov 2026 — audit fixes, phase B (daily-use friction)
- Owner: "let's do phase B of the audit". Chose **all three schedule steps** (click an empty slot to book, one column per therapist, drag to move/resize), **duplicate patients: warn and allow, same NIF refused**, and **server messages answered in the app's language** (the server translates; one table).
- Defaults chosen by Claude (ask if any is wrong):
  - Only **booked** sessions can be dragged; done, missed and cancelled ones keep their time (they carry charges). A drag moves **only that one** of a series (the toast says so) and has Undo. Dropping on a busy time asks "Mover mesmo assim?".
  - Day view: one column per therapist shown (the colour chips hide/show columns); whole-clinic blocked time appears in every column; with one therapist it's a single column. Week view always shows all 7 days. Hours shown = the opening hours of the days on screen plus anything booked outside them; closed hours are shaded.
  - A clinic-closed day with nothing booked shows "Clínica fechada" with New Appointment (you can still book).
  - **NIF:** a Portuguese NIF must have a valid check digit (spaces, dots and a "PT" prefix are tidied away); a foreign tax number is accepted when it starts with its two-letter country code (e.g. ES12345678Z), since it can't be checked. Duplicate warnings: same phone (last 9 digits), or same name and birth date; anonymised and deleted records don't count.
  - **Finance and Team are hidden** (rail, Go menu, ⌘4/⌘5) for therapists and reception; the server already refused them. File menu shows only what the role can do.
  - **Windows menus:** Minimise, Maximise and Full Screen (F11) work; "Clínica Help", "Send Feedback", "Redo" and "Bring All to Front" were removed until they exist (also from the Mac Help menu).
  - Row actions (⋯) are always visible, in a quieter grey at rest. The "Registar…" invoice badge is now a real "Registar fatura…" button.
  - Patient list loads 100 at a time with "Mostrar mais"; the patient fields in every sheet are search fields (name, number, phone, email or NIF).
  - Versions: app 0.4.0, server 0.13.0.

## 10 Nov 2026 — audit fixes, phase C (polish)
- Owner: "start audit phase C" → chose **Minor fixes + access-log filters/export + question-by-question tests** for this run; signed builds and auto-update later.
- **Test forms: free questionnaires only.** NPRS, SPADI, LEFS, PSFS and KOOS get full forms with automatic scoring; **ODI, NDI and QuickDASH keep the typed total** (they are licensed: their questions can't go inside software without a licence).
- **Updates (for when that part is built): auto-update from the clinic server** over Tailscale, signed with an updater key the owner keeps. No Apple/Windows code-signing purchase for now (Windows will show "unknown publisher" on first install).
- **Employer Social Security on salaries:** owner: "whatever's best, as we don't even know if that 23,75 is going to change". So: the rate is a **clinic setting** (23,75 % to start), changed in Finance › Salários; a person can have their **own rate** (reduced, or 0 for someone not on payroll); each salary payment **keeps the amount worked out when it was recorded**, so a change of rate never alters past months; the amount can also be typed (e.g. from the accountant). Payments recorded before migration 0014 keep 0.
- Defaults chosen by Claude (ask if any is wrong):
  - **KOOS:** each of the five subscales is kept and shown (0 = extreme problems, 100 = none); the single number on the card is **the mean of the five** (KOOS has no official total). Up to 2 blank items per subscale (KOOS user guide). SPADI: up to 2 blank in total (the maximum shrinks), shown as %, with pain and disability. LEFS: all 20 needed. PSFS: 1–5 activities named by the patient, mean score; the next PSFS starts with last time's activities.
  - The server **scores the answers itself** and ignores any total the app sends with them. A form test can still be recorded as a total ("Só o total", for a paper questionnaire).
  - The Portuguese wording of the forms is Claude's translation of the English originals (see OPEN_QUESTIONS).
  - **Access log:** filters by days, person, kind (patient files opened, clinical changes, sign-ins, warnings, accounts, money, schedule, settings/AI/system) and words (person, patient, computer); 100 rows at a time with "Mostrar mais"; patient rows show the patient's number and name; **CSV export** (semicolons, Excel-friendly) of what's filtered, logged as `access_log.exported` with the filters (never the search words).
  - **Money is always shown with two decimals** ("472,00 €").
  - "Bem-vindo de volta" → **"Olá de novo"** (neutral). "Sessões hoje" counts one-to-one sessions only (classes have their own figure). The profit ratio is hidden when the month is a loss. Month Close shows plain figures, like everywhere else. The Finance sidebar says "Exportar".
  - Find: a session opens its details sheet (it used to open Today). A `#page` link counts as a move, so Back returns from it. A click hides the rail tooltip until the pointer leaves.
  - When the server doesn't answer at launch, sign-in offers **Tentar de novo** and **Mudar de servidor…**.
  - Versions: app 0.5.0, server 0.14.0, migration 0014.

## 11 Nov 2026 — PhysioPulse: one product, rename first
Owner's answers to the phone app and website plan (https://claude.ai/artifact/LS7Lg4hYTn7JQ2uDwJLym8, the plan of record):
- **Name: everything is PhysioPulse** — desktop app, iPhone app, patient website — **internal identifiers included**. The old PhysioPulse website becomes **PhysioPlus** at physioplus.duckdns.org and keeps running until retired (that rename is not part of phase 1: owner, "skip old site for now").
- **App id: `app.physiopulse`** (owner chose it over `physiopulse.app` when asked, 11 Nov). Updater key stays when updates are built.
- **Build order:** 1 rename (+ the pending 0.14.0 deploy) · 2 session recording · 3 public foundations (domain, device approval; Tailscale removed) · 4 website and portal · 5 iPhone app. No deadline; recurring budget ≈ €1/month (the .pt domain only).
- **Phone app:** staff only, iPhone, installed by the owner with Xcode on a free Apple account (weekly rebuild); no patient app; assessments, notes and tests are written on the PC only. Face ID on every reopen, no idle lock, password again after 30 days.
- **Recording:** whole session, the patient is asked first; no consent record kept (owner's accepted risk); audio deleted when the note is signed, or after 30 days if never signed. AI features have a **switch per person and per feature**, off by default except for the owner.
- **AI writing:** notes and AI text are **plain flowing text — no titles, bold, bullets or SOAP — anywhere**, in a professional physiotherapist's voice built from the profession's published standard (APFISIO Padrões de Prática, standard 14), not from the owner's notes. Owner judges the first recording drafts in phase 2.
- **Email:** no automatic email until a domain allows it; document emails go through **Outlook** on every device. Invoices stay recorded by hand.
- **Network:** the AI PC shares the server's home network (owner thinks 192.168.1.66; to reserve in the router); Tailscale goes entirely in phase 3. Mascot for the downtime page: an axolotl. Domain: a .pt name, still to choose (catchy, simple, available).
- **Phase 1 details (chosen by Claude, say if any is wrong):** server data moves from `/DATA/AppData/clinica` to `/DATA/AppData/physiopulse` (backups `/DATA/Backups/physiopulse`) by *copying*, so going back is one command; the database and its user are renamed `physiopulse`; the archive's folder in the container is `/data/PhysioPulse`; the two-step app label (TOTP issuer) for new set-ups is "PhysioPulse" (existing phone entries keep working); the pulse icon stays; the desktop app copies its server address and trusted-computer tokens from the old Clínica app on first start.
- **Sign-in screen (owner, 11 Nov):** no "A sessão termina quando fecha a aplicação" line and no "Ver a primeira configuração" link.
- **Title bar (owner, 11 Nov):** one bar integrated with the OS, without the app's or clinic's name. On Windows the OS caption strip is gone and the frosted bar draws the caption buttons; the Windows menus have no "PhysioPulse" menu (Settings and Lock in File, About in Help). macOS was already integrated.
- Versions: app 0.6.0, server 0.15.0, migration 0015.

## 11 Nov 2026 — Phase 2: session recording
Owner: "lets do phase 2". Asked (questionnaire): speech-to-text → **whisper.cpp** on the AI PC; who spoke → **the note model infers it** from the conversation (no speaker-separation model); AI access → **one umbrella switch per person for all AI features** (instead of one switch per feature); the PC linked to this session **is the AI PC**.
- **Who:** the owner always; a therapist only when the owner turns their AI switch on in Team (off by default); reception never (no clinical data). The switch unlocks every AI feature that role's data allows — recording, note write-up, discharge-letter drafts. Receipts, monthly reports, the assistant and AI settings stay owner-only (money). *(Chosen by Claude within the umbrella answer; say if therapists should get more.)*
- **Capture:** in the desktop app on the treatment-room computer, 16 kHz mono, sent to the server every 30 s while recording; the patient is asked first (reminder on screen; no record of the answer). Max 2 h 30, min 10 s. You can keep using the app while recording (a pill in the title bar: time, level, Pause, Stop, Delete).
- **Draft:** a plain report (no titles/lists/SOAP, «o utente»), proposed values for the note template's blocks, and a short plan for next session; reviewed in the Progress tab ("Rever rascunho" → "Usar na nota"), then signed as usual. The transcript is shown under the draft.
- **Deletion:** audio, transcript and draft are deleted when the note made from it is signed, by hand, after 30 days, or with the patient/day (legal tools). Never in backups.
- **While recording the app doesn't lock itself** (it would end the recording). *(Claude's choice; say if you'd rather it locked the screen without signing out.)*
- Model: whisper `large-v3-turbo` (quantised q5_0, ~550 MB), port 8178, started at sign-in on the AI PC.
- Versions: app 0.7.0, server 0.16.0, migration 0016.


## 11 Nov 2026 — Phase 3: public foundations (device approval)
Owner: "lets start phase 3". Asked (questionnaire):
- **Address:** test the public setup on a free **duckdns.org** address first; the .pt domain is bought later and is only a settings change (`PUBLIC_URL`, the app's server address). *(The owner wrote "physioplus/duckdns": physioplus.duckdns.org is reserved for the old site, so a separate duckdns name for the staff server is suggested, e.g. `physiopulse-clinica.duckdns.org`; owner to pick.)*
- **Exposure:** **router port 443 → Nginx Proxy Manager** (already on ZimaOS) with a free Let's Encrypt certificate; duckdns keeps the home address current. No Cloudflare for the staff API.
- **Computers already trusted:** **approved automatically** on their first sign-in after the update; only new devices wait.
- **Disk:** **keep the plain disk** for now (7 Nov decision confirmed with the API public).
- *(Chosen by Claude, say if wrong):* approval is **per device, for everyone** who uses it (a shared reception PC is approved once); **only the owner approves**; the waiting screen shows a short code (e.g. K7Q-M2X) to match in the list; a waiting sign-in lasts 30 minutes; recovery code (owner only) then server console as the ways out; removed devices stay listed 90 days with "Approve Again"; 30 failures per internet address in 15 minutes lock that address; during the move the app still accepts plain http to private/Tailscale addresses, but internet addresses must be https.
- Versions: app 0.8.0, server 0.17.0, migration 0017.

## 11 Nov 2026 — Phase 4a: the public website and appointment requests
Owner: "start phase 4. it can use the physiopulse-clinica.duckdns.org. the main website will be (for now) on physiopulse.duckdns.org and hopefully someday be migrated to physiopulse.pt". Asked (questionnaire):
- **Addresses:** staff API at **physiopulse-clinica.duckdns.org**; public website at **physiopulse.duckdns.org** for now, **physiopulse.pt** later (a settings change).
- **Old PhysioPulse site:** **moves to physioplus.duckdns.org** (as planned) so the new site can take physiopulse.duckdns.org; it keeps working.
- **This run:** **site + requests first** (phase 4a: public pages, the request form into Schedule › Booking Requests, Settings › Website). Patient accounts and document requests are phase 4b.
- **How the site is made:** **pages made by the server** (a separate Rust service writing plain HTML from the database; no JavaScript, no cookies).
- **Patient sign-in later:** **same address, /conta** (physiopulse.duckdns.org/conta, later physiopulse.pt/conta).
- *(Chosen by Claude, say if wrong):* Portuguese at `/…` and English at `/en/…`; pages: Início, Serviços e preços, Equipa, Pedir marcação, Política de privacidade, Termos, Área de utente ("em breve"); the form asks for the service (first assessment, session for existing patients, home visit), name, mobile and/or email, a day from tomorrow to two months ahead and morning/afternoon/either (stored as 09:00/14:00/10:00 Lisbon), the address for home visits and an optional message (with "no need to describe health problems here"); requests from people who never become patients are deleted after 90 days; the site starts **unpublished** ("being prepared" + contacts; only the privacy and terms pages readable) and publishing needs the clinic's name, NIF and a contact; the team page shows only people the owner turns on (owner/therapists, never reception), with a title and short bio, no photos; every active price shows unless turned off, with VAT-exempt/included marks; "Abertura em breve" replaces the address until switched off; texts drafted by Claude, editable in Settings › Site e portal; privacy policy and terms drafted by Claude (not legal advice) for the owner to approve; the downtime page with the axolotl exists at `/downtime.html` for later. Website port 8096.
- Versions: app 0.9.0, server 0.18.0, migration 0018, website 0.1.0.

## 11 Nov 2026 — the assistant runs the schedule; one-press booking
Owner: "id like the ai assistant to be much more capable … add appointments, understand the ones im talking about, change them, when someone asks for an appointment … book it automatically with a press of a button". Asked (questionnaire):
- **What the AI sees: the agenda** (owner chose it over "only what I type"): appointments 3 days back to 14 ahead with patient names and numbers, therapists, times, status; waiting requests (name, service, day, time). Still never phones, e-mails, request messages, notes or money; still only the clinic's AI PC. Replaces "the assistant gets the price list, staff names and opening hours, never patients" (9 Nov). DPIA updated.
- **Jobs:** book, move/reschedule, cancel, mark done/missed, answer questions about the schedule, "and much more" → Claude added: accept/decline booking requests, add to the waitlist, find free times, book a series. Every change is still a preview → Apply → Undo.
- **Who: "aggregated in the AI switch"** → the assistant is for the owner and anyone whose AI switch is on. **Reception can now have the switch**, and for them it opens only the assistant's schedule jobs (no clinical AI). The owner's jobs (prices, hours, closures, pay, staff, reports) stay owner-only.
- **One press for someone new: create the file and book** (Undo removes both if nothing else was recorded).
- *(Chosen by Claude, say if wrong):* the press picks the time asked for (website: the first free in that part of the day — morning = before 13:00), else the same part of the day on the next 14 days; therapist = the patient's own, else the first free (owner first); assessments and home visits 60 min, sessions 45; the request's message becomes the appointment's admin note; a request that looks like an existing patient (same mobile, e-mail or name) asks which file instead of guessing; a booking the assistant makes for someone without a file is an **assessment** unless said otherwise; the assistant books outside opening hours or over a clash only with a warning on the preview (as by hand); a change of role turns someone's AI switch off; nobody is told automatically (no automatic e-mail/SMS yet) — the preview says so.
- Versions: app 0.10.0, server 0.19.0, migration 0019.

## 12 Nov 2026 — Phase 5: the staff iPhone app
Owner: "start phase 5, base it loosely on these examples" (grouped profile list, soft sign-up screen, home cards and week-strip schedule with a floating tab bar). Follows the plan's decisions of 11 Nov (staff only, iPhone, Xcode on a free Apple account, no patient app, notes/assessments/tests on the PC only, Face ID on every reopen, no idle lock, password after 30 days).
- *(Chosen by Claude, say if wrong):* five tabs — **Hoje, Agenda, Utentes, Gravar, Conta**; Gravar shows only for people with the AI switch who may record (owner, therapists with it on).
- *(Chosen by Claude)* On the phone you can **mark a session done or missed** (with Undo) and **approve devices / book or decline requests** (owner, as the plan says). Nothing else is written from the phone; booking a new appointment stays on the computer.
- *(Chosen by Claude)* A phone session is a **server session of 30 days without idle timeout**, only on a device the owner approved as a phone (the kind is fixed when it first asks). The token is in the keychain behind Face ID **or the phone's passcode** (Apple's standard fallback when Face ID fails). One phone session per person per phone.
- *(Chosen by Claude)* Leaving the app (home screen, another app, screen off) locks it and asks for Face ID on return — except while a session is being recorded. The screen stays on while recording.
- *(Chosen by Claude)* Team › person › **Terminar sessão no iPhone…** ends the phone's session but keeps the phone approved (the person signs in again with password + code). Removing the phone for good stays in Settings › Devices.
- *(Chosen by Claude)* The phone follows the system light/dark look (Apple: no app appearance setting on iOS); language PT/EN in Conta.
- Versions: app 0.11.0, server 0.20.0, migration 0020.

## 12 Nov 2026 — Brand: the pulse-hand mark
Owner: an icon with "a pulse line, with the top part of the physiotherapy hand in the middle" (sketch: a heartbeat whose peaks look like fingertips). Not red ("it was just the concept"); the first, rounder fingertips over the pointier try.
- **The mark:** one continuous line, flat → a small beat → it rises into four round fingertips of an open hand (middle finger tallest) → a small beat → flat. An original drawing of a generic open hand; it doesn't copy the profession's hand emblem.
- **Colourway B · Papel** (owner chose it over teal tile and one-colour ink): teal `#0B7A70` line on paper `#F3F1EC`. Ink `#1C2321` for one-colour print.
- **Type:** Manrope ExtraBold for the name, Inter for text (both free).
- Applied to the desktop and iPhone app icons, the in-app logo (`Mark`), and the website's logo, favicon and home illustration. Files: `brand/`. Brand sheet: https://claude.ai/artifact/MSJqiG7wyZeZ2hR5vvUKjD
- **Teal is the brand's accent** (owner, 12 Nov: "the teal should be the accent color of this franchise"): teal buttons and field focus on the sign-in; the iPhone's sage sign-in look is replaced by teal.
- **Sign-in as a die-cut** (owner, 12 Nov: "keep this one", after also trying a teal brand panel and paper cut along the line): the sign-in and the iPhone lock screen are a paper sheet with the pulse-hand line cut out of it; the brand teal shows through the cut, with a small shadow inside it. The form sits on the paper under the line (`CutoutSheet` in `src/components/ui.tsx`).

## 12–13 Nov 2026 — Desktop look overhaul
Owner: overhaul the desktop app's look after a reference dashboard image and the iPhone app ("it has separate capabilities, it's just an example"), **teal as the accent**, "avoid AI looking designs", "I don't like needless blank space".
- **Pages as pills at the top centre**; the left rail is removed.
- **Profile, Settings and the assistant button on the left** of the title bar; **menus on the right, on the same line as the window buttons**. **No logo and no sidebar toggle anywhere.**
- **The assistant opens as a panel in front of everything**, not as a side column.
- **Teal is the only accent colour**; per-page colours retired. The Lotus frosted frame and wallpaper retired for a flat warm-grey canvas with white cards.
- **Today is a bento that fits the window and never scrolls**, each box ending in a "view more" to the full page.
- **Sections column stays a column**, in a white rounded card (a row of section capsules was rejected). **Settings keeps its previous layout** (a three-column overview was rejected).
- *(Chosen by Claude)* AI shown with words and its icon in neutral ink instead of violet; initials one grey style; summary figures in one white card.

## 13 Nov 2026 — The assistant sees and does what the person can
Owner: "i want the ai to be able to have all the context it needs, be it all the workers, patients, whats written for each patient, etc, all of it. id like it to be able to do whatever i can in the app, only through me talking to it". Asked (questionnaire):
- **Who: same as their app.** Each person's assistant sees and does exactly what their role can in the app: the owner everything, therapists clinical but no money/staff/settings, reception contacts and schedule, never clinical. Still behind the AI switch (owner always on). Replaces "the AI sees the agenda; never notes, contacts or money" (11 Nov) — **it now reads patient files, notes, anamnesis, tests and contacts**, still only on the clinic's AI PC. DPIA updated.
- **Changes: always shown, Apply.** Reading is immediate; every change is a before/after card that waits for Apply, is logged and can be undone. The AI never signs a note or approves a letter.
- **AI PC:** 8 GB graphics card now, 16 GB planned.
- **Build in stages:** 1) look-ups + schedule + patient files (done 13 Nov), 2) money and staff, 3) documents, letters, legal tools, settings.
- *(Chosen by Claude, say if wrong):* the assistant gets **its own model setting** (Settings › IA local › Assistente) because it needs a model that uses tools: **qwen3:8b** now, **qwen3:14b** after the upgrade; letters/notes/receipts stay on gemma4:12b. A model without tools gets the older one-step assistant.
- *(Chosen by Claude)* Notes the assistant writes are saved as **unsigned drafts marked AI**; the anamnesis is changed field by field with the old and new text shown; a new file warns when the person looks like an existing patient.
- *(Chosen by Claude)* What the assistant looked up is shown above its answer ("Abriu a ficha de …") and sent back with the next message so follow-ups work; it stays in the app's memory only, cleared daily like the chat.

## 13 Nov 2026 — The assistant, stage 3: documents, letters, legal tools, settings
Owner: "lets do assistant stage 3" (the stages agreed above). Same rules as stages 1–2: the person's own reach, through the app's endpoints, every change a preview that waits for Apply.
- *(Chosen by Claude, say if wrong):* **what the assistant never does**, even for the owner: approve a discharge letter (the owner's signature), sign anything, anonymise or delete a patient, delete a day, export a record, remove a document, write a consent text. These are irreversible or legal acts done with their own buttons; the assistant checks the file (`legal_check`: what it holds, the 20-year keeping period) and says where the button is.
- *(Chosen by Claude)* **A consent withdrawal can be recorded by the assistant** (everyone, as in the app) but **can't be undone** (it goes into the clinical record with its PDF), like a voided payment.
- *(Chosen by Claude)* **Letters:** without words from the person, a draft starts from the file's template ([a completar] parts); if the person asks the assistant to write it, its text is saved marked AI-drafted. Therapists and the owner draft and edit; only the owner approves, in the app.
- *(Chosen by Claude)* **Reminders:** the assistant only records that a reminder went out; the message is still sent by the person from Reminders. The phone number isn't given to the model for this.
- *(Chosen by Claude)* **Settings through the assistant (owner):** clinic details, reminder message (its placeholders must stay), staff AI switches, the website's switches and texts. Not: passwords, two-step, devices, backups, AI addresses, consent texts.

## 23 Nov 2026 — second UI/UX overhaul
- Owner: "i dont like the design and overall ui/ux" → asked for a prioritized list (`UX-REVIEW-2026-11-23.md`), then "lets go, i trust you" for step 1 (title bar, type scale, shared tokens).
- The five in-window menu words (13 Nov: "the menus on the same line as minimise/close") become **one Menu button in the same place**, so the page pills always show their names. Settings moves from the title bar into the profile menu.
- Body text 14 px on Windows and in the browser (13 pt on the Mac app); five type sizes, one secondary grey, corner tokens.

## 23 Nov 2026 — phase 4b: patient accounts
Claude asked 8 questions with recommendations; the owner's replies, verbatim: "1. yes 2. no need to confirm the birth, just the code 3. reception, his therapist or the owner 4. yes 5. yup 6. goes to reception, and their therapist 7. yes 8. whats switched off on launch?"
1. Patients see upcoming and past sessions, make booking requests, see invoices and what they owe. No clinical notes or test results.
2. Account by invite from the patient file; the patient gets a code sent by staff through their own WhatsApp/SMS. **Code only, no date-of-birth check.**
3. Forgotten password: a one-time reset code from **reception, the patient's therapist or the owner**. No automatic email.
4. Two-step codes optional for patients.
5. Under-16s have no login; a guardian's account shows the child's sessions and invoices.
6. Cancelling and moving go to **reception and the patient's therapist**. *Claude's reading, told to the owner (say if wrong):* cancelling up to 24 h before happens straight away and shows to both; moving is a request both see.
7. Invoice PDFs downloadable in the portal; clinical documents stay as today.
8. Claude explained "switched off" (the /conta area installed but closed, "em breve", no sign-in, until the owner turns it on in Settings › Site e portal after trying a test patient). Not yet confirmed by the owner.

## 25 Nov 2026 — On-device AI in the app (own C++ module)
Owner: "make the app have its own .cpp for tts and chat. use gemma 4-4b or something better that runs well in this app and is light enough for a macbook air m2". Then: "dont need to read anything aloud. what about stt, is it implemented?" → "yes. use the best model for stt, as it isnt time sensitive, but quality is". Details: `LOCAL_AI.md`.
- The desktop app gets its own C++ module (`src-tauri/native`): llama.cpp for chat, whisper.cpp for speech to text. **No read-aloud** (built first, then removed on the owner's word). Adds to "Ollama runs on the AI PC, not on each computer" (3 and 9 Nov) rather than replacing it: the clinic assistant with tools stays on the AI PC.
- Speech to text: **quality over speed** → Whisper **large-v3** (full precision on 16 GB, q5_0 on 8 GB), beam search, European-Portuguese prompt.
- *(Chosen by Claude, say if wrong):* chat model **Gemma 4 E4B Q4_K_M** on 16 GB Macs, **E2B** on 8 GB.
- *(Chosen by Claude, say if wrong):* the Assistant panel gets **Clínica | Neste Mac**; "Neste Mac" has no tools and sees no clinic data, only what is typed or dictated; nothing leaves the computer. A **Ditar** mic button in both modes puts the dictated text in the field to check before sending. Same people as the assistant (owner + AI switch).

## 25 Nov 2026 — In-app updates from GitHub
- Owner: "make it so i can update the app through an in app button. it can fetch the updates from github, but make it safe. i wanna essentially be able to tell you to do something and you release an update there so i can auto download it. (x.y.z) x= major update, y= minor update, z=patch, you choose all of those".
- **Updates come from GitHub Releases**, not from the clinic server (replaces the 10 Nov plan "auto-update from the clinic server"). Signed with an updater key only the owner holds; the app checks the signature and the signed version before installing.
- **One public repository for code and installers: `filipesilva0/PhysioPulse-App`** (owner first chose private code + public downloads, then: "nevermind, one public"). Consequence: nothing secret may be committed (`publish.ps1` checks).
- **Claude chooses the version** (semantic versioning, RELEASING.md table) and releases at the end of every app change unless the owner says otherwise.
- Still no Apple/Windows code-signing purchase (10 Nov decision stands): the Mac app is ad-hoc signed.
