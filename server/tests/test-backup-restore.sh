#!/bin/sh
# Proves a backup can be restored: backs up the live DB, restores into a scratch DB, compares row counts.
# Run on the server (or anywhere with pg_dump/pg_restore/age/psql):
#   DATABASE_URL=postgres://physiopulse:...@localhost/physiopulse SCRATCH_URL=postgres://physiopulse:...@localhost/physiopulse_restore sh server/tests/test-backup-restore.sh
set -eu
T=$(mktemp -d); age-keygen -o "$T/key.txt" 2>/dev/null; PUB=$(grep 'public key' "$T/key.txt" | awk '{print $4}')
AGE_RECIPIENT=$PUB BACKUP_DIR=$T ARCHIVE_DIR=/nonexistent sh "$(dirname "$0")/../backup/backup.sh"
sh "$(dirname "$0")/../backup/restore.sh" "$T"/db-*.dump.age "$T/key.txt" "$SCRATCH_URL"
fail=0
for t in staff sessions invites access_log; do
  a=$(psql "$DATABASE_URL" -tAc "SELECT count(*) FROM $t"); b=$(psql "$SCRATCH_URL" -tAc "SELECT count(*) FROM $t")
  echo "$t: live=$a restored=$b"; [ "$a" = "$b" ] || fail=1
done
rm -rf "$T"; [ $fail = 0 ] && echo "RESTORE TEST PASSED" || { echo "RESTORE TEST FAILED"; exit 1; }
