#!/bin/sh
# Updates $CONTROL/status.json, keeping fields it isn't told about.
#   status.sh ok <db file> <files file> <bytes> <offsite: off|ok|failed>
#   status.sh failed | running | idle | next <ISO time>
set -eu
CONTROL="${BACKUP_CONTROL:-/backup-control}"
F="$CONTROL/status.json"
[ -d "$CONTROL" ] || exit 0
[ -s "$F" ] && jq -e . "$F" >/dev/null 2>&1 || echo '{}' > "$F"
NOW=$(date -u +%FT%TZ)
case "$1" in
  ok)      jq --arg t "$NOW" --arg db "$2" --arg fl "$3" --argjson b "${4:-0}" --arg o "$5" --argjson k "${KEEP_DAYS:-30}" \
             '.last_ok=$t | .db_file=$db | .files_file=(if $fl=="" then null else $fl end) | .bytes=$b | .offsite=$o | .kept_days=$k | .running=false' "$F" > "$F.tmp" ;;
  failed)  jq --arg t "$NOW" '.last_error=$t | .running=false' "$F" > "$F.tmp" ;;
  running) jq '.running=true' "$F" > "$F.tmp" ;;
  idle)    jq '.running=false' "$F" > "$F.tmp" ;;
  next)    jq --arg t "$2" '.next_at=$t' "$F" > "$F.tmp" ;;
esac
chmod 644 "$F.tmp"
mv "$F.tmp" "$F"
