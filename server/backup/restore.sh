#!/bin/sh
# Restore a database backup into a database (use a NEW empty one for the quarterly test).
#   ./restore.sh db-20261104-030000.dump.age key.txt postgres://physiopulse:...@db:5432/physiopulse_restore
set -eu
# --no-privileges: grants (e.g. to the website's login, physiopulse_portal) are re-applied by the server at start-up,
# so a restore into a new database server never fails on a login that doesn't exist there yet.
FILE="$1"; KEY="$2"; TARGET="$3"
age -d -i "$KEY" "$FILE" | pg_restore --no-owner --no-privileges --clean --if-exists --dbname="$TARGET"
echo "restored $FILE into $(echo "$TARGET" | sed -E 's#//([^:/@]+):[^@]*@#//\1:***@#')"
