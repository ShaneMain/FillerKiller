#!/usr/bin/env bash
# Off-box Postgres backup. Dumps the DB and (optionally) uploads it to
# object storage. Schedule via cron, e.g.:
#   0 3 * * * /opt/fillerkiller/deploy/backup.sh >> /var/log/fk-backup.log 2>&1
set -euo pipefail
cd "$(dirname "$0")"

# Load POSTGRES_* from the compose env file.
if [ -f .env ]; then
	set -a
	# shellcheck disable=SC1091
	. ./.env
	set +a
fi

PGUSER="${POSTGRES_USER:-fillerkiller}"
PGDB="${POSTGRES_DB:-fillerkiller}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="/tmp/fillerkiller-${STAMP}.sql.gz"

docker compose exec -T postgres pg_dump -U "$PGUSER" "$PGDB" | gzip > "$OUT"
echo "backup written: $OUT"

# Upload to your object storage, then remove the local copy. Configure ONE of
# these (install rclone or the aws CLI and set up credentials first):
#   rclone copy "$OUT" "r2:fillerkiller-backups/" && rm -f "$OUT"
#   aws s3 cp "$OUT" "s3://fillerkiller-backups/" && rm -f "$OUT"

# Until off-box upload is enabled, prune local dumps so /tmp doesn't fill up with
# plaintext copies of the whole database. Tune the retention window as needed.
find /tmp -maxdepth 1 -name 'fillerkiller-*.sql.gz' -mtime +7 -delete

# Restore (manual): gunzip -c <file>.sql.gz | \
#   docker compose exec -T postgres psql -U "$PGUSER" "$PGDB"
