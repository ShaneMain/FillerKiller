# Deploy — self-hosted single box

The whole stack on one VPS via Docker Compose, per
the design notes: Postgres +
the Rust API + Caddy (TLS + static SPA). Only Caddy is exposed to the internet; the
database and API stay on the internal Docker network.

```
internet ──▶ Caddy (:80/:443, auto-HTTPS)
               ├─ /api/*, /health  ──▶ api:8080  (Rust)
               └─ everything else  ──▶ static SPA (web/dist)
                                api ──▶ postgres:5432  (private volume)
```

## Prerequisites

- A small VPS (~$5/mo, e.g. Hetzner CX22) with Docker + Compose.
- A domain pointed at the box (A/AAAA record) for automatic HTTPS.

## First deploy

```bash
# 1. Build the SPA (served same-origin by Caddy).
cd web && npm install && npm run build && cd ..

# 2. Configure.
cd deploy
cp .env.example .env        # set DOMAIN, POSTGRES_PASSWORD, TMDB token,
                            # AUTH_JWT_SECRET (openssl rand -base64 32), OAuth apps

# 3. Launch. The API applies migrations on boot.
docker compose up -d --build
docker compose logs -f api  # watch it come up
```

Then register each OAuth app's redirect URI as
`https://<DOMAIN>/api/auth/<provider>/callback`.

The SPA and API are **same-origin** (both behind Caddy at `https://<DOMAIN>`), so the
`SameSite=Lax` session cookie works without cross-site handling.

## Updating

```bash
git pull
cd web && npm run build && cd ../deploy
docker compose up -d --build
```

## Backups

`backup.sh` runs `pg_dump` and (once you configure rclone/aws) uploads it off-box.
Schedule it with cron:

```cron
0 3 * * * /opt/fillerkiller/deploy/backup.sh >> /var/log/fk-backup.log 2>&1
```

Restore:

```bash
gunzip -c fillerkiller-YYYYMMDD.sql.gz | \
  docker compose exec -T postgres psql -U fillerkiller fillerkiller
```

## Notes

- Postgres data lives in the `pgdata` named volume — back it up (above); it's yours.
- The DB is **not** published to the host; reach it for admin via
  `docker compose exec postgres psql -U fillerkiller fillerkiller`.
- Local smoke test without a real domain: set `DOMAIN=localhost` — Caddy serves an
  internal cert, so it's still HTTPS and the `Secure` cookie works unchanged (no need
  to touch `AUTH_COOKIE_SECURE`).
- Scaling beyond one box (resize → split API/DB → read replicas) is in the design notes; none
  of it changes application code.
