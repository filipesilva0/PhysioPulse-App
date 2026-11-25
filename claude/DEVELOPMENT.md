# Development

## Tools

- Node 20+ and npm
- Rust 1.85+ (`rustup`), plus on Linux: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev`
- PostgreSQL 16 (local dev) or Docker
- Python 3 with `requests pyotp` (API tests), `age` (backup tests)
- If an older Cargo picks crate versions that need a newer Rust: `export CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback`

## Run things

```bash
npm install

# UI in a browser, with the stand-in server (no Rust needed)
npm run build:preview && open dist-preview/index.html

# Desktop app (needs a running server to sign in)
npm run tauri dev
npm run tauri build                 # installers in src-tauri/target/release/bundle

# Server locally
createdb physiopulse                 # or: docker compose up db
cd server
DATABASE_URL=postgres://physiopulse:devpass@localhost/physiopulse ARCHIVE_DIR=/tmp/arch BACKUP_CONTROL=/tmp/ctl BIND=127.0.0.1:8080 cargo run
# first app start: Connect → 127.0.0.1:8080 → Set up your clinic
```

## Test

Run all of these before saying a change is done.

Since phase 3 every request needs `X-Device-Key` (the tests send `DEVICE_KEY`, default `a1`×32, which setup approves) and the server should run with `SECRET_KEY=$(openssl rand -hex 32)` (without it two-step secrets stay in plain text and Settings › Server says so).

```bash
npx tsc --noEmit -p .                         # UI types
npm run build && npm run build:preview        # UI builds
cd src-tauri && cargo check                   # desktop shell
cd server && cargo build                      # server
python3 server/tests/e2e.py                   # 30 phase 1 API checks — needs a running server with an EMPTY database
python3 server/tests/e2e_phase2.py            # 71 phase 2 checks — reset the database first (also needs an EMPTY one)
PSQL_URL=postgres://… python3 server/tests/e2e_phase3.py   # 120 money checks — EMPTY database; PSQL_URL adds the trigger checks
ARCHIVE=/tmp/arch PSQL_URL=postgres://… python3 server/tests/e2e_documents.py   # 47 document checks — EMPTY database; ARCHIVE = the server's ARCHIVE_DIR (checks the folders)
ARCHIVE=/tmp/arch PSQL_URL=postgres://… python3 server/tests/e2e_legal.py       # 64 legal-tool checks (export, anonymise, delete) — EMPTY database
ARCHIVE=/tmp/arch LOG=server.log PSQL_URL=postgres://… python3 server/tests/e2e_consents.py   # 41 consent checks (drafts/approval, PDFs, withdrawal, export) — EMPTY database
LOG=server.log PSQL_URL=postgres://… python3 server/tests/e2e_reminders.py      # 30 reminder checks — EMPTY database
ARCHIVE=/tmp/arch LOG=server.log PSQL_URL=postgres://… python3 server/tests/e2e_letters.py   # 98 pack + letter checks — EMPTY database; start the server with OLLAMA_URL=http://127.0.0.1:11999 (the test runs a stand-in Ollama there); uses qpdf + pdftotext if installed
ARCHIVE=/tmp/arch LOG=server.log PSQL_URL=postgres://… python3 server/tests/e2e_clinic.py   # 69 checks: complaints log (ERS working days), delete a day, last contact, VAT flag + IVA column — EMPTY database
ARCHIVE=/tmp/arch LOG=server.log PSQL_URL=postgres://… python3 server/tests/e2e_phase4.py   # 102 checks: note rewrite, receipt reading, reports, assistant (apply/cancel/undo/stale), hours, closures — EMPTY database; start the server with OLLAMA_URL=http://127.0.0.1:11997 (the test runs a stand-in Ollama there)
PSQL_URL=postgres://… python3 server/tests/e2e_trusted.py   # 23 checks: code once per computer, password-only after, revocation, 30-day expiry — EMPTY database
mkdir -p /tmp/ctl && CONTROL=/tmp/ctl LOG=server.log python3 server/tests/e2e_account.py   # 75 checks: password change/reset, recovery codes, owner resets, invites, clinic details on PDFs, unset scales, server/backup status — EMPTY database; start the server with BACKUP_CONTROL=/tmp/ctl PUBLIC_ADDRESS=100.68.180.126:8095; needs pillow (+ pdftotext for the PDF checks)
PSQL_URL=postgres://… python3 server/tests/e2e_phaseb.py   # 37 checks: NIF check digit + no repeats, duplicates, picker paging/?id, move to another therapist, messages in Portuguese (Accept-Language) — EMPTY database
AUDIO=/tmp/audio LOG=server.log PSQL_URL=postgres://… python3 server/tests/e2e_recording.py   # 73 checks: AI switch per person (database keeps owner on/reception off), recording in pieces (order, repeats, limits), WAV join, waiting for the AI PC, whisper + Ollama stand-ins, plain report, values that fit the blocks, plan, access log, nothing in the database/backups/log, link to a note, deletion when signed/by hand/after 30 days/with the patient/with the day, speech settings — EMPTY database; start the server with OLLAMA_URL=http://127.0.0.1:11995 WHISPER_URL=http://127.0.0.1:11996 AUDIO_DIR=/tmp/audio RECORDING_POLL_SECS=1
PSQL_URL=postgres://… SERVER_BIN=server/target/debug/physiopulse-server python3 server/tests/e2e_devices.py   # phase 3 checks: device approval (waiting ticket, owner approves/declines/removes/renames, recovery code, server console), sessions bound to the device, computers trusted before phase 3, rate limits in the database and per internet address (X-Forwarded-For from the proxy only), two-step secrets encrypted — EMPTY database; start the server with SECRET_KEY=<64 hex> on 127.0.0.1
PSQL_URL=postgres://… PORTAL_URL=postgres://physiopulse_portal:<pw>@… python3 server/tests/e2e_website.py   # 95 checks: the website's database login sees only the portal views, unpublished/published pages, prices from the app, team page, legal pages, the request form (token, honeypot, field errors, escaping, rate limit by hashed address), requests in the staff app, switches, owner-only settings — EMPTY database; start the server with PORTAL_DB_PASSWORD=<16+ letters/digits> and the website: cd server/portal && DATABASE_URL=postgres://physiopulse_portal:<pw>@…/physiopulse BIND=127.0.0.1:8090 cargo run
LOG=server.log PSQL_URL=postgres://… python3 server/tests/e2e_agenda.py   # 84 checks: the assistant's schedule jobs (AI switch incl. reception, what the AI sees, book/which one/free times/new person/series, clashes and staleness, move by ref or name, cancel, mark done/missed with charges, questions, requests, waitlist) and one-press booking (suggest, book, new file, look-alikes, website part of day, closures, Undo) — EMPTY database; start the server with OLLAMA_URL=http://127.0.0.1:11994
LOG=server.log PSQL_URL=postgres://… python3 server/tests/e2e_agent.py   # 68 checks: the assistant with tools (13 Nov 2026) — tools per role, look-ups through the app as the person (access log, reception without clinical data), dates, memo, Apply/Undo of new files, patient details, anamnesis, note drafts, give-up, the assistant model setting — EMPTY database; start the server with OLLAMA_URL=http://127.0.0.1:11993 (the test runs a scripted stand-in with tools there)
LOG=server.log PSQL_URL=postgres://… python3 server/tests/e2e_agent_money.py   # 61 checks: the assistant, stage 2 (money and staff) — tools per role, patient money look-up (access log), payments (all owed, by ref, part, month, walk-in, no future), Undo = void, invoice number, void (no Undo, app rule at Apply), packages (sell/cancel, pause/resume), owner: owed list, payments, overview, expenses, salaries, waive, month close, staleness, English, no names in the log — EMPTY database; start the server with OLLAMA_URL=http://127.0.0.1:11992
ARCHIVE=/tmp/arch LOG=server.log PSQL_URL=postgres://… python3 server/tests/e2e_agent_docs.py   # 84 checks: the assistant, stage 3 (documents, letters, legal tools, settings) — tools per role (nobody gets approve/anonymise/delete/export), documents and consents per role (access log), rename (Undo, stale), letter drafts from the template or the model, edits, approved letters locked, reimbursement pack (Apply/Undo), consent withdrawal (no Undo), reminders (no phone to the model, mark/unmark), owner: complaints (ERS deadline, reply, Undo order), legal check, clinic details (NIF check), reminder message (placeholders kept), AI switches, website (publish refused by the app’s own rule), English, nothing in the log — EMPTY database; start the server with OLLAMA_URL=http://127.0.0.1:11991
PSQL_URL=postgres://… PORTAL_URL=postgres://physiopulse_portal:<pw>@… python3 server/tests/e2e_portal.py   # 105 checks: patient accounts (phase 4b) — closed by default, invites (hash only, under-16 → guardian), who makes reset codes, sign-in (same answer, cookie flags, no cookie on public pages), the patient area (only their sessions, owed, invoices; no notes), internal API key/session, cancel ≥24 h, move/book requests, staff see them, booking a move cancels the old one (Undo restores), guardians, two-step, reset, rate limit, the website login still can't read tables, removing access, anonymising — EMPTY database; start the server with PORTAL_DB_PASSWORD and ARCHIVE_DIR, and the website with SERVER_URL=<server> COOKIE_INSECURE=1
cd server/portal && cargo test                # website unit tests: form token, Lisbon time, form checks, escaping, prices, visitor address
PSQL_URL=postgres://… python3 server/tests/e2e_phone.py   # 38 checks: phone devices (kind fixed at first request), 30-day phone sessions without idle timeout, one per phone, a computer can't become a phone, Team phone list and sign-out (owner only), expiry, removal — EMPTY database; start the server with SECRET_KEY=<64 hex>
PSQL_URL=postgres://… python3 server/tests/e2e_phasec.py   # 68 checks: access-log filters/paging/CSV export, test forms scored on the server (NPRS, SPADI, LEFS, PSFS, KOOS), employer Social Security (rates, kept amounts, month result, accountant export) — EMPTY database
cd server && cargo test                       # unit tests (plain-text AI drafts: no titles/lists/SOAP (style.rs), test-form scoring, employer Social Security rounding, every server message has a Portuguese row in i18n.rs, NIF tidying, zip CRC, money format, CSV quoting, PDF wrap + images + JPEG + joining PDFs, confirm name, local-only AI address, letter template, working days/holidays)
cd src-tauri && cargo test --lib             # only message links can be opened; only server files (and the access-log CSV) can be downloaded
DATABASE_URL=... SCRATCH_URL=... sh server/tests/test-backup-restore.sh
```

`cd src-tauri && cargo check` on Linux needs `libwebkit2gtk-4.1-dev libgtk-3-dev` and the `icons/` folder (placeholders are fine for a check; don't commit them).

For UI changes, screenshot the affected screens in light and dark mode (Playwright works well against `dist-preview/index.html`; set `color_scheme`), and at a narrow width.

To reset the dev database: `dropdb physiopulse && createdb physiopulse` (migrations run on server start).

## Deploy the server on ZimaOS

The owner's server: ZimaOS at 192.168.1.64, Tailscale address **100.68.180.126**, PhysioPulse on port **8095** (8080 is taken by Nginx Proxy Manager). Web terminal: http://192.168.1.64:7681 (log in as ExilExo, then `sudo -i`).

ZimaOS quirks: `/root` is read-only (write under `/DATA`); there is no `tailscale` command on the host (Tailscale runs as an app; the installer reads the address from `tailscale0`); the ttyd web terminal drops the connection if you paste a very large block at once (paste in small pieces).

**Install or update (one file):**
1. On a dev machine: `sh server/make-installer.sh` → `server/dist/install-physiopulse.sh` (contains the whole `server/` folder).
2. Get it onto the server as `/DATA/AppData/install-physiopulse.sh` and run `bash /DATA/AppData/install-physiopulse.sh` as root.
3. First run asks for the age **public** key (make the key pair on your own computer: `age-keygen -o physiopulse-backup-key.txt`; keep the file in a password manager and printed, never on the server) and the data/backup folders. It writes `/DATA/AppData/physiopulse/.env` (random database password, `SERVER_PORT` 8095 if 8080 is busy), creates the folders, runs `docker compose up -d --build` and waits for `/health`.
4. Re-running it updates the code and keeps `.env`. Since server 0.12.0 it also creates `$DATA_DIR/backup-control` (owned by uid 10001), the folder the server and the backup container share for the backup status and "Back Up Now".

**Moving from Clínica (server 0.15.0, once):** the same installer, run on a server that still has `/DATA/AppData/clinica`, moves it to `/DATA/AppData/physiopulse` by itself: (1) fresh encrypted backup with the old stack, (2) counts the rows of every table, (3) stops Clínica, (4) *copies* the database folder, archive and backups to the new folders (`…/clinica` → `…/physiopulse`; the old ones are only read), (5) in the copy renames the database and its user `clinica` → `physiopulse` (keeps the password), (6) checks every table has the same rows, then builds and starts PhysioPulse (migration 0015 runs). Any failure after step 3 goes back to Clínica automatically and keeps the failed copy as `*.failed-<time>`. Going back by hand: `cd /DATA/AppData/physiopulse && docker compose down && cd /DATA/AppData/clinica && docker compose up -d`. Delete the old folders after a week. Rehearsed in the cloud sandbox against real PostgreSQL 16 clusters with a stand-in `docker` (success, automatic rollback, update after the move); the real Docker run is the owner's. Backups made before the move hold the archive under `Clinica/`, later ones under `PhysioPulse/`.

**Desktop app after the rename (0.6.0):** new app id `app.physiopulse`, so it installs next to the old "Clínica" app — uninstall Clínica. On first start PhysioPulse copies the server address and remembered accounts (with the trusted-computer tokens) from the old app's settings folder (`pt.clinica.desktop`), so nobody reconnects or redoes the two-step code. Theme/language/sidebar choices start fresh.

**Local AI (discharge-letter drafts):** Ollama runs on the clinic's AI PC, **100.86.97.21:11434** (Tailscale), not on ZimaOS (too slow: no GPU). `/DATA/AppData/physiopulse/.env` has `OLLAMA_URL=http://100.86.97.21:11434` and `OLLAMA_MODEL=gemma4:12b`; the model must be pulled on that PC. The server only accepts LAN/Tailscale addresses (`ai.rs`). Leave `OLLAMA_URL` empty to use the `ollama` container instead (`docker compose --profile ai up -d`, then `docker compose exec ollama ollama pull <model>`). Settings isn't needed: the letter sheet shows whether the AI is available.

**Speech to text (session recordings, PhysioPulse plan phase 2):** whisper.cpp's `whisper-server` on the AI PC, port **8178**, model `ggml-large-v3-turbo-q5_0.bin`. On that PC double-click `tools\install-whisper.cmd` (no admin; downloads the latest release — the CUDA build when there's an NVIDIA card — and the model into `%LOCALAPPDATA%\PhysioPulse\whisper`, starts it hidden now and at every sign-in via `shell:startup\PhysioPulse whisper.vbs`, log in `server.log` there). Windows may ask once to allow it on Private networks: Allow. `tools\test-whisper.cmd` makes an invented conversation with Windows voices and prints the transcript and the time. Then in the app: Settings › Local AI › Speech to text → `100.86.97.21` → Test Connection → Save (or `WHISPER_URL` in `.env`). The server's `audio` folder is created by the installer (`chmod 700`, uid 10001). The gate (owner judges a real two-person reading): `claude/RECORDING_TEST.md`.

**Build image:** `rust:1.97-slim-bookworm` (lopdf 0.42 needs Rust ≥ 1.88 and doesn't declare it; keep a `-bookworm` tag so glibc matches the `debian:bookworm-slim` runtime). The cloud sandbox can't download other Rust toolchains, so check a bump with `docker manifest inspect rust:<tag>` on the server.

**Getting the installer onto ZimaOS:** ZimaOS dashboard is on port **90** (port 80 is Nginx Proxy Manager) → Files › ZimaOS-HD › AppData → right-click › Upload files. Run long jobs detached so a dropped web terminal can't kill them: `setsid nohup bash /DATA/AppData/install-physiopulse.sh > /DATA/AppData/physiopulse-install.log 2>&1 &`. Take a backup first: `docker compose exec -T backup /backup/backup.sh`.

Then:
5. Public address: see **Public address (phase 3)** below. (Before phase 3: a Tailscale access rule limiting the port to clinic devices.)
6. Open the app on the owner's computer → Connect → the clinic's address → Set up your clinic (this computer is approved automatically).
7. Run the restore test against a scratch database. Repeat every quarter.

## Public address (phase 3: device approval, Tailscale out)

Server 0.17.0 + app 0.8.0. **Order matters:** the new server refuses apps without a device key, so install app 0.8.0 on every clinic computer first (it works with the old server too), then update the server.

1. **App 0.8.0 everywhere:** `npm run tauri build` on Windows (and the Mac), install. Nothing changes yet.
2. **Server 0.17.0:** upload `server/dist/install-physiopulse.sh` and run it as before. It adds `SECRET_KEY`, `BIND_IP=0.0.0.0` and `PUBLIC_URL=` to `.env` and **prints the new SECRET_KEY once: write it down with the backup key** (password manager + paper). Migration 0017 signs everyone out once. First sign-in on each computer that was trusted before: password only, approved automatically. Others: password + code, then "Waiting for approval" until you approve it in Settings › Devices.
3. **duckdns name** (free): on duckdns.org add the sub-domain for the staff server: **`physiopulse-clinica`** (owner, 11 Nov 2026; `physioplus` is for the old site, `physiopulse` for the public website — see Website below). The same updater that keeps the old site's duckdns address current must update this one too (one duckdns token covers all your names; add the new name to its list).
4. **Router:** forward TCP **443** and **80** to the ZimaOS server (192.168.1.64) — Nginx Proxy Manager listens there (80 is needed for the Let's Encrypt check). **Never forward 8095.** If 443/80 already go to NPM for the old site, nothing to do.
5. **Nginx Proxy Manager** (ZimaOS, admin on port 81) → Hosts › Proxy Hosts › Add: domain `physiopulse-clinica.duckdns.org`; scheme `http`; forward host `192.168.1.64`; port `8095`; turn on *Block Common Exploits*. SSL tab: *Request a new SSL Certificate* (Let's Encrypt), *Force SSL*, *HTTP/2*, *HSTS*. Advanced tab: `client_max_body_size 16m;` (patient documents and recording pieces). NPM sends `X-Forwarded-For` by default; the server trusts it only from Docker's range (`TRUSTED_PROXIES`). Check: Settings › Devices shows each device's internet address; if they all show `192.168.1.64` or `172.x`, NPM reaches the server another way — add that address to `TRUSTED_PROXIES` in `.env` (e.g. `127.0.0.1/32,::1/128,172.16.0.0/12,192.168.1.64/32`), or every device shares one rate limit.
6. In `/DATA/AppData/physiopulse/.env` set `PUBLIC_URL=https://physiopulse-clinica.duckdns.org`, then `docker compose up -d` (Settings › Server shows it).
7. **Test from outside** (phone on mobile data, Wi-Fi off): `https://physiopulse-clinica.duckdns.org/health` answers `{"ok":true,"version":"0.17.0",…}`; `http://<home IP>:8095` must NOT answer.
8. **Each computer:** in the app, sign-in screen › Change › `physiopulse-clinica.duckdns.org` (https is added). Inside the clinic this needs the router to support "NAT loopback" (most do); if the address doesn't answer inside the clinic but does on mobile data, add a local DNS entry in the router pointing the name to 192.168.1.64, or keep the LAN address `192.168.1.64:8095` on clinic PCs.
9. **AI PC:** reserve 192.168.1.66 for it in the router (DHCP reservation); in Settings › Local AI set Ollama to `192.168.1.66` and Speech to text to `192.168.1.66`, Test, Save. Windows may ask to allow Ollama/whisper on Private networks: Allow.
10. **Then Tailscale off** (owner's call, after a few days of normal use): uninstall/disable it on the clinic computers, the AI PC and the ZimaOS Tailscale app. Set `BIND_IP=192.168.1.64` in `.env` if you want the plain port reachable only on the home network address.
11. **Later, the .pt domain:** point it (CNAME) at the duckdns name, add it as a second domain in the NPM proxy host and re-request the certificate, change `PUBLIC_URL` and the address in each app. Nothing else changes.

If no approved device is at hand: the owner's waiting screen › "Use a Recovery Code"; without codes, on ZimaOS: `cd /DATA/AppData/physiopulse && docker compose exec server physiopulse-server devices` then `… approve K7Q-M2X`.

**Restoring a backup on a new server** needs the same `SECRET_KEY` in `.env` (two-step codes keep working). Without it everyone sets up two-step again: the owner signs in with a recovery code, then resets the others in Team.

## Website (phase 4a: physiopulse.duckdns.org)

Server 0.18.0 + app 0.9.0. The website is a second container (`site`, port **8096** on ZimaOS) built by the same installer. Its address: **physiopulse.duckdns.org** for now (later physiopulse.pt). The staff API is **physiopulse-clinica.duckdns.org** (owner, 11 Nov 2026). The old PhysioPulse website currently holds physiopulse.duckdns.org, so it moves to **physioplus.duckdns.org** first.

**A. Move the old site to physioplus.duckdns.org** (it keeps working; nothing in its code has to change unless it has its own address written in it)
1. duckdns.org: add the sub-domains `physioplus` and `physiopulse-clinica` (same account and token). Whatever keeps physiopulse.duckdns.org pointing at home (the duckdns updater on ZimaOS or the router) must list all three names.
2. Firebase console › Authentication › Settings › Authorised domains: add `physioplus.duckdns.org` (keep the old one until the move is done). Stripe dashboard: if a webhook or a success/cancel URL uses `physiopulse.duckdns.org`, change it to `physioplus.duckdns.org`. Search the old site's repository for `physiopulse.duckdns.org`; if found, change it, push, and `/DATA/Websites/deploy.sh physiopulse`.
3. Nginx Proxy Manager (http://192.168.1.64:81) › Proxy Hosts › the `physiopulse.duckdns.org` host (forwards to the websites container, port 8701) › Edit › Domain Names: replace it with `physioplus.duckdns.org` › SSL › Request a new certificate (Force SSL on) › Save. Check https://physioplus.duckdns.org (sign in, pay page).

**B. Put the new site on physiopulse.duckdns.org**
4. Install server 0.18.0 as usual (it includes 0.16/0.17 if not done yet). It adds `PORTAL_DB_PASSWORD`, `FORM_KEY`, `SITE_PORT=8096` and `SITE_URL=` to `.env` once (nothing to write down) and starts the `site` container. Check on the server: `curl -s http://127.0.0.1:8096/health` → `{"ok":true,…}`.
5. NPM › Proxy Hosts › Add: domain `physiopulse.duckdns.org`; scheme `http`; forward host `192.168.1.64`; port `8096`; *Block Common Exploits* on. SSL: new Let's Encrypt certificate, *Force SSL*, *HTTP/2*, *HSTS*. (NPM sends `X-Forwarded-For`; the website trusts it only from Docker's range, like the API.) **Never forward 8096 on the router.**
6. NPM: the staff API host is `physiopulse-clinica.duckdns.org` → port 8095 (DEVELOPMENT.md › Public address, step 5).
7. In `/DATA/AppData/physiopulse/.env`: `SITE_URL=https://physiopulse.duckdns.org` and `PUBLIC_URL=https://physiopulse-clinica.duckdns.org`, then `docker compose up -d`.
8. App 0.9.0 › Settings › Clínica: name, NIF, ERS registration, phone, email (the site's footer and legal pages use them). Settings › Site e portal: texts, who shows on the team page, which prices show. Read https://physiopulse.duckdns.org/privacidade and /termos (readable before publishing; drafts by Claude, not legal advice). Then switch **Site publicado** on.
9. From a phone on mobile data: the home page, a request (it appears in Agenda › Pedidos de marcação with a "Site" badge), and that http://<home IP>:8096 does not answer.
10. Later, the .pt domain: add `physiopulse.pt` (and `www.`) as domains of the same NPM host, re-request the certificate, set `SITE_URL`. Cloudflare can then sit in front of the public pages only (plan), never the API.

Done 11 Nov 2026 (STATUS.md). The website also answers `/sw.js` with a script that removes the old site's service worker from returning visitors' browsers — keep it while people may still have the old site cached.

The downtime page (axolotl) is at `/downtime.html` with the clinic's phone filled in; save it and serve it from outside the home network when the .pt domain exists (not set up yet).

Restore on a new server: nothing extra — the API server re-creates the website's database login and grants at start-up (`restore.sh` uses `--no-privileges`).

## iPhone app (phase 5)

The staff iPhone app is the same project built for iOS (Tauri 2 mobile). It needs a **Mac with Xcode** (free from the App Store) and the iPhone with a cable. A free Apple account is enough (owner, 11 Nov 2026); the install then stops opening after **7 days** and has to be built again from the Mac (no push notifications on a free account; the app doesn't use them).

**Browser preview of the phone screens:** `npm run build:preview`, then open `dist-preview/index.html?phone` (narrow the window or use the browser's device mode).

**First time on the Mac:**
1. Install Xcode, open it once and accept the licence; Xcode › Settings › Accounts › add your Apple ID (a "Personal Team" appears).
2. `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`, `brew install cocoapods` (if `tauri ios init` asks for it).
3. In the project folder: `npm install`, then `npm run tauri ios init` (creates `src-tauri/gen/apple`, the Xcode project; it isn't in the repository).
4. `npm run tauri ios build -- --open` opens Xcode, or `npm run tauri ios dev --open`. In Xcode: target **PhysioPulse_iOS** › Signing & Capabilities › Team = your Personal Team; Bundle Identifier `app.physiopulse` (if Xcode says it's taken for a free account, add a suffix like `app.physiopulse.filipe` — only on the phone build). Check target › Info has *Privacy - Face ID Usage Description*, *Privacy - Microphone Usage Description* and *Required background modes: audio* (from `src-tauri/Info.ios.plist`; add them by hand if a Tauri version didn't merge them).
5. Plug in the iPhone, choose it as the run destination, press ▶. On the iPhone: Settings › General › VPN & Device Management › your Apple ID › Trust. iOS 16+ also asks to turn on **Developer Mode** (Settings › Privacy & Security) and restart.
6. Open PhysioPulse on the phone: server `physiopulse-clinica.duckdns.org` (on the clinic Wi-Fi `192.168.1.64:8095` also works), sign in with password and code, then approve the phone on the computer (Today alert or Settings › Devices: it shows "iPhone" and the same code as the phone). The app then asks for Face ID every time it opens.

**Every week (free account):** plug the phone in and press ▶ in Xcode again (nothing is lost: the session stays in the keychain until its 30 days end).

**Things to check on the first build** (STATUS.md): Face ID on reopen; Cancel stays on the lock screen; marking a session done; a patient file appears in the access log; a short recording reaches the patient's notes on the computer; then the same recording with the screen locked (if iOS stops the microphone, keep the screen on — the app already asks iOS to — or record on the room PC).

**Type-checking the iOS build without a Mac** (what was done here, 12 Nov): `rustup` can't download the iOS std, so std was built from source (`RUSTC_BOOTSTRAP=1 cargo check -Zbuild-std=std,panic_abort --target aarch64-apple-ios` with the Rust `library/` sources in the toolchain's `rustlib/src`, a stand-in `xcrun`, and `CC_aarch64_apple_ios`/`AR_aarch64_apple_ios` stubs). It proves the Rust compiles for iOS, not that it links or runs.

## Conventions

- **Strings:** `L("pt", "en")` everywhere. pt-PT, sentence case. English Title Case for buttons and menus.
- **Server calls:** only via `api(method, path, body)` from `src/lib/api.ts`. When you add an endpoint, add a stand-in answer in `src/lib/preview.ts` (phase 2+) or the `mock()` switch (phase 1) so the preview keeps working. Read data with `useApi(path)` from `src/lib/data.ts`.
- **Roles:** check on the server with `auth.require(&["owner", ...])`. Then also hide it in the UI.
- **Access log:** `auth::log(pool, staff_id, "area.action", subject, device, detail)`. Action names are `area.verb_past` (`login.ok`, `staff.deactivated`, later `patient.viewed`). Add a PT/EN label for new actions in `AccessLogSheet` (`src/pages/Settings.tsx`).
- **Errors:** return `AppError` variants with a sentence a receptionist could understand. Never leak internal details. Write it in English and add its pt-PT row to `server/src/i18n.rs` (`cargo test` fails otherwise); the server answers in the app's language.
- **Migrations:** new numbered `.sql` file + add it to `MIGRATIONS` in `server/src/db.rs`. Never edit an applied migration.
- **Menus:** ids must match between `App.tsx` (`menus`, `run()`) and `src-tauri/src/menu.rs`.
- **Sample data:** pages not yet wired use `src/data/mock.ts`. Replace page by page; remove the "Sample data" badge in `App.tsx` when nothing uses it.
- **Dependencies:** pin exact versions in `package.json`; choose releases at least two weeks old.
- **Commits:** small, one concern each; update `claude/STATUS.md` in the same commit.
