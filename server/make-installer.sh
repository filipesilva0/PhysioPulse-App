#!/bin/sh
# Builds dist/install-physiopulse.sh: one self-contained file that installs or updates the
# server on ZimaOS (run it on the server as root). Run from server/: sh make-installer.sh
#
# The same file also moves an existing Clínica install (/DATA/AppData/clinica) over to PhysioPulse
# (/DATA/AppData/physiopulse) the first time it runs: backup, copy, rename, check, start. The old
# folders are never changed, so going back is one command (printed at the end).
set -eu
cd "$(dirname "$0")"
mkdir -p dist
rm -f dist/install-clinica.sh
T=$(mktemp -d)
mkdir "$T/server"
cp -r Cargo.toml Cargo.lock Dockerfile docker-compose.yml .env.example src migrations backup tests portal "$T/server/"
rm -rf "$T/server/portal/target"   # the website (phase 4a) is built on the server like the API
tar -czf "$T/s.tgz" -C "$T" --owner=0 --group=0 server
base64 "$T/s.tgz" > "$T/b64"
awk -v f="$T/b64" '$0=="@@B64@@"{while((getline l<f)>0)print l;next}{print}' <<'TEMPLATE' > dist/install-physiopulse.sh
#!/bin/bash
# PhysioPulse server installer/updater for ZimaOS. Run as root: bash install-physiopulse.sh
# Re-running updates the code and keeps the existing .env (passwords, keys).
# First run on a server that still has Clínica: moves it over (see "move" below). Nothing in the old
# folders is changed or deleted.
set -Eeuo pipefail
APP=${PP_APP:-/DATA/AppData/physiopulse}
OLD=${PP_OLD:-/DATA/AppData/clinica}
[ "$(id -u)" = 0 ] || [ -n "${PP_TEST:-}" ] || { echo "Run as root (sudo -i first)."; exit 1; }
command -v docker >/dev/null || { echo "Docker not found."; exit 1; }
docker compose version >/dev/null 2>&1 || { echo "docker compose not available."; exit 1; }

TMP=$(mktemp -d)
base64 -d > "$TMP/server.tgz" <<'B64END'
@@B64@@
B64END
tar -xzf "$TMP/server.tgz" -C "$TMP"
mkdir -p "$APP"
[ -f "$APP/.env" ] && cp -p "$APP/.env" "$TMP/env.keep"
cp -r "$TMP/server/." "$APP/"
[ -f "$TMP/env.keep" ] && cp -p "$TMP/env.keep" "$APP/.env"
rm -rf "$TMP"
cd "$APP"

health() {   # waits up to 5 minutes for /health on this server's own port
  local host=${BIND_IP:-0.0.0.0}; [ "$host" = 0.0.0.0 ] && host=127.0.0.1
  local url="http://$host:${SERVER_PORT:-8080}"
  echo -n "Waiting for the server"
  for i in $(seq 1 "${PP_TRIES:-60}"); do
    if curl -fsS "$url/health" >/dev/null 2>&1; then echo; echo "OK: $url is up."; curl -s "$url/api/status"; echo; return 0; fi
    echo -n "."; sleep 5
  done
  echo; return 1
}

# ───────── move from Clínica (first run only) ─────────
if [ ! -f "$APP/.env" ] && [ -f "$OLD/.env" ]; then
  echo "Found Clínica in $OLD: moving it to PhysioPulse in $APP."
  echo "The old folders are only read, never changed."
  oldenv() { grep -E "^$1=" "$OLD/.env" | tail -1 | cut -d= -f2- | sed -E 's/[[:space:]]+#.*$//; s/[[:space:]]+$//'; }
  O_DATA=$(oldenv DATA_DIR); O_BACKUP=$(oldenv BACKUP_DIR); PW=$(oldenv POSTGRES_PASSWORD)
  [ -n "$O_DATA" ] && [ -n "$O_BACKUP" ] && [ -n "$PW" ] || { echo "Can't read DATA_DIR, BACKUP_DIR and POSTGRES_PASSWORD from $OLD/.env. Nothing was changed."; exit 1; }
  newpath() { case "$1" in "$OLD") echo "$APP";; */clinica) echo "${1%/clinica}/physiopulse";; *) echo "";; esac; }
  N_DATA=$(newpath "$O_DATA"); N_BACKUP=$(newpath "$O_BACKUP")
  [ -n "$N_DATA" ] && [ -n "$N_BACKUP" ] || { echo "The data or backup folder in $OLD/.env doesn't end in /clinica ($O_DATA, $O_BACKUP). Move it by hand (see DEVELOPMENT.md). Nothing was changed."; exit 1; }
  if [ -e "$N_DATA/postgres" ] || [ -e "$N_DATA/archive" ]; then
    echo "$N_DATA already has a database or archive. Nothing was changed. If an earlier move failed, rename those folders first."; exit 1
  fi
  STAMP=$(date +%Y%m%d-%H%M%S)
  oc() { (cd "$OLD" && docker compose "$@"); }
  nc() { (cd "$APP" && docker compose "$@"); }
  counts() { # one line per table: name=rows (exact counts, all tables)
    "$@" -v ON_ERROR_STOP=1 -tA <<'SQL' | sort
SELECT format('SELECT %L || ''='' || count(*) FROM %I.%I', tablename, schemaname, tablename) FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename \gexec
SQL
  }
  oc ps --status running --services 2>/dev/null | grep -qx db || { echo "Clínica isn't running ($OLD). Start it first (cd $OLD && docker compose up -d) so a fresh backup can be made. Nothing was changed."; exit 1; }

  echo "1/6 Fresh encrypted backup of Clínica…"
  oc exec -T backup /backup/backup.sh || { echo "The backup failed, so nothing was moved. Clínica is still running."; exit 1; }
  BEFORE=$(counts oc exec -T db psql -U clinica -d clinica) || { echo "Couldn't count the rows, so nothing was moved."; exit 1; }
  echo "   $(echo "$BEFORE" | wc -l) tables counted."

  rollback() {
    echo; echo "Something went wrong — going back to Clínica."
    nc down >/dev/null 2>&1 || true
    [ -f "$APP/.env" ] && mv "$APP/.env" "$APP/.env.failed-$STAMP"
    for d in postgres archive backup-control; do [ -e "$N_DATA/$d" ] && mv "$N_DATA/$d" "$N_DATA/$d.failed-$STAMP"; done
    oc up -d && echo "Clínica is running again from $OLD, exactly as before. The failed copy was kept with names ending .failed-$STAMP (safe to delete)."
    exit 1
  }

  echo "2/6 Stopping Clínica…"
  oc down
  trap rollback ERR

  echo "3/6 Copying data to $N_DATA and backups to $N_BACKUP…"
  mkdir -p "$N_DATA" "$N_BACKUP"
  cp -a "$O_DATA/postgres" "$O_DATA/archive" "$N_DATA/"
  [ -d "$O_DATA/backup-control" ] && cp -a "$O_DATA/backup-control" "$N_DATA/" || mkdir -p "$N_DATA/backup-control"
  cp -a "$O_BACKUP/." "$N_BACKUP/"
  chmod 700 "$N_BACKUP"
  umask 077
  sed -E "s#^DATA_DIR=.*#DATA_DIR=$N_DATA#; s#^BACKUP_DIR=.*#BACKUP_DIR=$N_BACKUP#" "$OLD/.env" > "$APP/.env"
  umask 022

  echo "4/6 Renaming the database and its user from clinica to physiopulse…"
  set -a; . "$APP/.env"; set +a
  nc up -d db
  for i in $(seq 1 60); do nc exec -T db pg_isready -q && break; sleep 2; done
  nc exec -T db pg_isready -q
  nc exec -T db psql -U clinica -d postgres -v ON_ERROR_STOP=1 -qc "CREATE ROLE pp_move SUPERUSER LOGIN"
  nc exec -T db psql -U pp_move -d postgres -v ON_ERROR_STOP=1 -q -v pw="$PW" <<'SQL'
ALTER DATABASE clinica RENAME TO physiopulse;
ALTER ROLE clinica RENAME TO physiopulse;
ALTER ROLE physiopulse PASSWORD :'pw';
SQL
  nc exec -T db psql -U physiopulse -d postgres -v ON_ERROR_STOP=1 -qc "DROP ROLE pp_move"

  echo "5/6 Checking every table has the same number of rows…"
  AFTER=$(counts nc exec -T db psql -U physiopulse -d physiopulse)
  if [ "$BEFORE" != "$AFTER" ]; then
    diff <(echo "$BEFORE") <(echo "$AFTER") || true
    false
  fi
  echo "   Same rows in all $(echo "$AFTER" | wc -l) tables."

  echo "6/6 Building and starting PhysioPulse (first build compiles Rust: 5–15 minutes)…"
  mkdir -p "$N_DATA/audio" && chmod 700 "$N_DATA/audio"
  chown -R 10001:10001 "$N_DATA/archive" "$N_DATA/backup-control" "$N_DATA/audio"
  nc up -d --build
  health || { nc logs --tail 40 server; false; }
  trap - ERR
  cat <<DONE

Done: Clínica is now PhysioPulse.
  Code and settings: $APP
  Data:              $N_DATA
  Backups:           $N_BACKUP (old backups copied; files inside them are under "Clinica/", new ones under "PhysioPulse/")
  The old folders ($OLD$( [ "$O_DATA" != "$OLD" ] && echo ", $O_DATA"), $O_BACKUP) were not changed. Clínica is stopped.

If anything looks wrong in the app, go back to Clínica with:
  cd $APP && docker compose down && cd $OLD && docker compose up -d
(anything done in PhysioPulse after the move would be lost by going back).

After a week of normal use, free the space by deleting the old folders:
  rm -rf $OLD $O_BACKUP$( [ "$O_DATA" != "$OLD" ] && echo " $O_DATA")
DONE
  exit 0
fi

# ───────── first install ─────────
if [ ! -f .env ]; then
  read -rp "age PUBLIC key (starts with age1): " AGEKEY
  case "$AGEKEY" in age1*) ;; *) echo "That is not an age public key."; exit 1;; esac
  read -rp "Data folder [/DATA/AppData/physiopulse]: " DD; DD=${DD:-/DATA/AppData/physiopulse}
  read -rp "Backup folder [/DATA/Backups/physiopulse]: " BD; BD=${BD:-/DATA/Backups/physiopulse}
  read -rp "Public HTTPS address, if you have it already (e.g. https://clinica.duckdns.org; Enter to skip): " PURL
  PORT=8080
  if ss -ltn 2>/dev/null | awk '{print $4}' | grep -qE '(^|:)8080$'; then PORT=8095; echo "Port 8080 is taken on this server; using 8095."; fi
  PW=$(head -c 32 /dev/urandom | base64 | tr -d '/+=' | head -c 40)
  umask 077
  cat > .env <<ENVEOF
POSTGRES_PASSWORD=$PW
DATA_DIR=$DD
BACKUP_DIR=$BD
AGE_RECIPIENT=$AGEKEY
RCLONE_REMOTE=
IDLE_MINUTES=15
SERVER_PORT=$PORT
BIND_IP=0.0.0.0
PUBLIC_URL=$PURL
ENVEOF
  echo "Created $APP/.env (database password generated; readable by root only)."
fi

# ───────── phase 3: server key for the two-step secrets, public address ─────────
# Added once to an existing .env; never changed afterwards (changing SECRET_KEY would lock everyone's two-step codes).
if ! grep -qE '^SECRET_KEY=[0-9a-f]{64}' .env; then
  NEWKEY=$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')
  umask 077
  { echo ""; echo "# Phase 3: encrypts the two-step secrets in the database. KEEP A COPY with the backup key — restoring a backup on a new server needs it."; echo "SECRET_KEY=$NEWKEY"; } >> .env
  grep -qE '^BIND_IP=' .env || echo "BIND_IP=0.0.0.0" >> .env
  grep -qE '^PUBLIC_URL=' .env || echo "PUBLIC_URL=" >> .env
  cat <<KEYMSG

──────────────────────────────────────────────────────────────────────────────
 New server key (SECRET_KEY). Write it down and keep it with your backup key
 (password manager + printed). It is not in the backups on purpose.

   $NEWKEY

──────────────────────────────────────────────────────────────────────────────
KEYMSG
fi

# ───────── phase 4a: the public website's database login and form key ─────────
# Generated once. PORTAL_DB_PASSWORD is set on the database login by the API server at start-up; FORM_KEY signs
# the request form. Neither needs to be written down: a new one works after a restore (run this installer again).
if ! grep -qE '^PORTAL_DB_PASSWORD=[A-Za-z0-9]{16,}' .env; then
  umask 077
  { echo ""; echo "# Phase 4a: the public website (its own database login, which sees only the website's views)."
    echo "PORTAL_DB_PASSWORD=$(head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 32)"
    echo "FORM_KEY=$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')"; } >> .env
  grep -qE '^SITE_PORT=' .env || echo "SITE_PORT=8096" >> .env
  grep -qE '^SITE_URL=' .env || echo "SITE_URL=" >> .env
  echo "Added the website's settings to .env (SITE_PORT 8096; set SITE_URL once its address works)."
fi

# ───────── update ─────────
chmod 600 .env
set -a; . ./.env; set +a
mkdir -p "$DATA_DIR/archive" "$DATA_DIR/postgres" "$BACKUP_DIR" "$DATA_DIR/backup-control" "$DATA_DIR/audio"
chown -R 10001:10001 "$DATA_DIR/archive" "$DATA_DIR/backup-control" "$DATA_DIR/audio"   # the server runs as uid 10001
chmod 700 "$DATA_DIR/audio"   # session recordings: never backed up, deleted when the note is signed
chmod 700 "$BACKUP_DIR"

echo "Building and starting (first build compiles Rust: 5–15 minutes)..."
docker compose up -d --build
site_health() {   # the website (phase 4a): a warning only, the clinic's API is what matters
  local host=${BIND_IP:-0.0.0.0}; [ "$host" = 0.0.0.0 ] && host=127.0.0.1
  for i in $(seq 1 "${PP_TRIES:-24}"); do curl -fsS "http://$host:${SITE_PORT:-8096}/health" >/dev/null 2>&1 && { echo "OK: the website answers on port ${SITE_PORT:-8096}."; return 0; }; sleep 5; done
  echo "The website did not answer on port ${SITE_PORT:-8096}. Logs: docker compose logs --tail 40 site"
}
health && { site_health; exit 0; }
echo "Server did not answer. Logs:"; docker compose logs --tail 40 server; exit 1
TEMPLATE
rm -rf "$T"
echo "Wrote dist/install-physiopulse.sh"
