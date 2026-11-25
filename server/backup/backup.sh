#!/bin/sh
# Encrypted backup of the database and the archive folders.
# Encrypted with an age PUBLIC key: the server can write backups but cannot read them.
# Keep the matching private key OFF the server (password manager + printed copy).
# Writes $CONTROL/status.json (read by the app: Settings › Server and backups, and Today's alert).
set -eu
: "${AGE_RECIPIENT:?set AGE_RECIPIENT to your age public key (age1...)}"
: "${DATABASE_URL:?}"
OUT="${BACKUP_DIR:-/backups}"; ARCHIVE="${ARCHIVE_DIR:-/data/PhysioPulse}"; KEEP_DAYS="${KEEP_DAYS:-30}"
CONTROL="${BACKUP_CONTROL:-/backup-control}"
STAMP=$(date -u +%Y%m%d-%H%M%S)
mkdir -p "$OUT"
pg_dump --format=custom --no-owner "$DATABASE_URL" | age -r "$AGE_RECIPIENT" > "$OUT/db-$STAMP.dump.age.part"
mv "$OUT/db-$STAMP.dump.age.part" "$OUT/db-$STAMP.dump.age"
FILES=""
if [ -d "$ARCHIVE" ]; then
  tar -C "$(dirname "$ARCHIVE")" -cf - "$(basename "$ARCHIVE")" | age -r "$AGE_RECIPIENT" > "$OUT/files-$STAMP.tar.age.part"
  mv "$OUT/files-$STAMP.tar.age.part" "$OUT/files-$STAMP.tar.age"
  FILES="files-$STAMP.tar.age"
fi
# Optional off-site copy (any rclone remote: Backblaze B2, a second NAS, etc.)
OFFSITE=off
if [ -n "${RCLONE_REMOTE:-}" ] && command -v rclone >/dev/null; then
  if rclone copy "$OUT" "$RCLONE_REMOTE" --include "*-$STAMP.*"; then OFFSITE=ok; else OFFSITE=failed; fi
fi
find "$OUT" -name '*.age' -mtime +"$KEEP_DAYS" -delete
BYTES=$(cat "$OUT/db-$STAMP.dump.age" ${FILES:+"$OUT/$FILES"} | wc -c | tr -d ' ')
echo "$(date -u +%FT%TZ) backup ok: db-$STAMP.dump.age${FILES:+ $FILES} offsite=$OFFSITE" | tee -a "$OUT/backup.log"
if [ -d "$CONTROL" ]; then
  /backup/status.sh ok "db-$STAMP.dump.age" "$FILES" "$BYTES" "$OFFSITE"
fi
