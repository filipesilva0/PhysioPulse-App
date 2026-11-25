#!/bin/sh
# Container entry: runs backup.sh every night at BACKUP_HOUR (UTC), once at start, and whenever the
# app asks (Settings › Server and backups › Back Up Now writes $CONTROL/request; checked every 30 s).
set -u
CONTROL="${BACKUP_CONTROL:-/backup-control}"
run() {
  /backup/status.sh running || true
  if /backup/backup.sh; then :; else
    echo "$(date -u +%FT%TZ) backup FAILED" >> "${BACKUP_DIR:-/backups}/backup.log"
    /backup/status.sh failed || true
  fi
}
next_night() {
  now=$(date -u +%s); t=$(date -u -d "$(date -u +%F) ${BACKUP_HOUR:-03}:00" +%s 2>/dev/null || echo $((now + 86400)))
  [ "$t" -le "$now" ] && t=$((t + 86400))
  echo "$t"
}
run
target=$(next_night)
/backup/status.sh next "$(date -u -d "@$target" +%FT%TZ)" || true
while true; do
  sleep 30
  if [ -f "$CONTROL/request" ]; then
    rm -f "$CONTROL/request"
    echo "$(date -u +%FT%TZ) backup asked for in the app" >> "${BACKUP_DIR:-/backups}/backup.log"
    run
  fi
  if [ "$(date -u +%s)" -ge "$target" ]; then
    run
    target=$(next_night)
    /backup/status.sh next "$(date -u -d "@$target" +%FT%TZ)" || true
  fi
done
