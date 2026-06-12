# Deploy

Two supported deployments, same app and same standard Postgres — switching is a
`DATABASE_URL` + target change, no rewrite:

- **[Cloud Run + Neon](#cloud-run--neon-primary)** — ephemeral compute that scales with
  traffic, fronted by Cloudflare (orange-cloud proxy → Cloud Run domain mapping).
- **[Self-hosted single box](#self-hosted-single-box-fallback)** — the cheapest flat-cost
  fallback.

---

## Self-hosted single box (fallback)

The whole stack on one VPS via Docker Compose: Postgres +
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
session cookie is first-party and works without cross-site handling.

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
- Scaling beyond one box (resize → split API/DB → read replicas) changes none of the
  application code.

---

## Cloud Run + Neon (primary)

Ephemeral Rust container on Cloud Run + managed Postgres on Neon, fronted by
Cloudflare. The single image serves **both** the API and the SPA same-origin (the
root `Dockerfile` builds `web/dist` into the API image, which serves it as a static
fallback) — there is **no** separate SPA host (not Cloudflare Pages). Cloudflare
proxies (orange-cloud) to a **Cloud Run domain mapping**: the mapping makes Cloud
Run accept the custom `Host` (it 404s on an unrecognized one), so the proxy can sit
in front for TLS + edge caching.

```
internet ──▶ Cloudflare (orange-cloud: TLS, CDN cache, respects Cache-Control)
               └─▶ Cloud Run domain mapping (fillerkiller.app)
                     └─ one Rust container:
                          ├─ /api/*, /health  ──▶ Rust API  ──▶ Neon (pooled)
                          └─ everything else  ──▶ static SPA (web/dist, same image)
```

> The Cloud Run domain mapping is what lets the proxy work on Cloudflare's **Free**
> plan: without it, proxying to the bare `*.run.app` origin would need a Host-header
> rewrite (a Worker or paid plan), since Cloud Run 404s on an unrecognized Host. The
> mapping registers `fillerkiller.app` on the origin, so no rewrite is needed.
> Cloudflare respects the API's `Cache-Control` (catalog reads cache long, aggregates
> briefly). A global edge rate-limit rule can be added later; the in-API limiters
> (per-IP votes, per-IP search, per-instance import) are the current controls.

### Continuous deployment (GitHub Actions)

Every green CI run on a `master` push deploys automatically — the `deploy` job in
`.github/workflows/ci.yml` runs the same sequence as the manual steps below
(Cloud Build image → migrate Job → patch the `app` container) and then smoke-checks
`/health` + `/health/db`. Manual deploys remain valid for emergencies.

Auth is **keyless** via Workload Identity Federation: GitHub's OIDC token is
exchanged for `fillerkiller-deploy@` (no service-account key is stored anywhere).
One-time setup, already applied:

```bash
gcloud iam workload-identity-pools create github --location=global
gcloud iam workload-identity-pools providers create-oidc github \
  --location=global --workload-identity-pool=github \
  --issuer-uri="https://token.actions.githubusercontent.com" \
  --attribute-mapping="google.subject=assertion.sub,attribute.repository=assertion.repository" \
  --attribute-condition="assertion.repository == 'ShaneMain/FillerKiller'"
gcloud iam service-accounts create fillerkiller-deploy
# Deploy + build-submit rights; actAs the build SA (builds) and runtime SA (rollouts).
gcloud projects add-iam-policy-binding $PROJECT --member=serviceAccount:$DEPLOY_SA --role=roles/run.admin
gcloud projects add-iam-policy-binding $PROJECT --member=serviceAccount:$DEPLOY_SA --role=roles/cloudbuild.builds.editor
gcloud iam service-accounts add-iam-policy-binding $BUILD_SA_EMAIL --member=serviceAccount:$DEPLOY_SA --role=roles/iam.serviceAccountUser
gcloud iam service-accounts add-iam-policy-binding $RUNTIME_SA --member=serviceAccount:$DEPLOY_SA --role=roles/iam.serviceAccountUser
# `builds submit` needs to consume the API, LIST the project's buckets (it
# locates the staging bucket that way — a bucket-scoped grant can't satisfy
# this; the resulting error misleadingly blames bucket access), and
# read/write the source bucket itself.
gcloud projects add-iam-policy-binding $PROJECT --member=serviceAccount:$DEPLOY_SA --role=roles/serviceusage.serviceUsageConsumer
gcloud projects add-iam-policy-binding $PROJECT --member=serviceAccount:$DEPLOY_SA --role=roles/storage.bucketViewer
gsutil iam ch "serviceAccount:$DEPLOY_SA:roles/storage.admin" gs://${PROJECT}_cloudbuild
# Let ONLY this repo's workflows impersonate the deployer.
gcloud iam service-accounts add-iam-policy-binding $DEPLOY_SA \
  --member="principalSet://iam.googleapis.com/projects/$PROJECT_NUMBER/locations/global/workloadIdentityPools/github/attribute.repository/ShaneMain/FillerKiller" \
  --role=roles/iam.workloadIdentityUser
```

The migrate Job runs as the runtime SA (`--service-account $RUNTIME_SA`), so the
deployer's single `serviceAccountUser` grant covers both the service and the Job.

### Prerequisites

- `gcloud` CLI authenticated; a GCP project with Cloud Run + Artifact Registry enabled.
- A **Neon** project. Copy its **pooled** connection string (host has `-pooler`):
  `postgresql://USER:PASS@ep-xxx-pooler.REGION.aws.neon.tech/neondb?sslmode=require`.
- TMDB read token; Google/GitHub OAuth apps; a domain on Cloudflare (the domain is
  verified to Google, mapped on Cloud Run, and proxied orange-cloud by Cloudflare).
- Two service accounts (set up already): a **runtime** SA the service runs as
  (granted `secretAccessor` on the secrets only) and a **build** SA Cloud Build
  uses (build/push/deploy roles only). The default compute SA has **no roles**,
  so `--source` deploys/jobs **must** pass `--build-service-account`. Set these
  for the commands below, and run them **from the repo root** (the root
  `Dockerfile` builds the SPA + API into one image — not `api/`):

```bash
RUNTIME_SA=fillerkiller-api@$PROJECT.iam.gserviceaccount.com
BUILD_SA=projects/$PROJECT/serviceAccounts/fillerkiller-build@$PROJECT.iam.gserviceaccount.com
```

### 1. Migrate FIRST (required — do not skip)

Migrations are an explicit step, never run on boot under Cloud Run (concurrent cold
starts would race). A fresh service against an un-migrated DB returns `500`s
on data routes while `/health` still looks green, so run this **before** sending traffic
and **after every deploy that adds a migration**:

```bash
# From the repo root. One-off Cloud Run Job that runs the `migrate` subcommand.
gcloud run jobs deploy fillerkiller-migrate \
  --source . --region $REGION \
  --build-service-account=$BUILD_SA \
  --service-account=$RUNTIME_SA \
  --command fillerkiller-api --args migrate \
  --set-secrets DATABASE_URL=fk-database-url:latest \
  --set-env-vars TMDB_API_READ_TOKEN=unused,AUTH_JWT_SECRET=$(openssl rand -base64 32)
gcloud run jobs execute fillerkiller-migrate --region $REGION --wait
```

> The job needs `DATABASE_URL` and the config's required vars to start; `migrate` only
> touches the DB. The `AUTH_JWT_SECRET=$(openssl rand -base64 32)` here is a **throwaway**
> just to satisfy config validation — it signs nothing and is discarded with the job; it
> is NOT the production signing secret (that's `fk-jwt` in Secret Manager). Locally you
> can equivalently run `DATABASE_URL=… cargo run -- migrate`.

### 2. Deploy the API

```bash
# From the repo root (root Dockerfile builds SPA + API into one image).
gcloud run deploy fillerkiller-api \
  --source . --region $REGION --allow-unauthenticated \
  --service-account=$RUNTIME_SA --build-service-account=$BUILD_SA \
  --set-secrets DATABASE_URL=fk-database-url:latest,AUTH_JWT_SECRET=fk-jwt:latest,\
TMDB_API_READ_TOKEN=fk-tmdb:latest,GOOGLE_CLIENT_SECRET=fk-google:latest,\
GITHUB_CLIENT_SECRET=fk-github:latest \
  --set-env-vars CORS_ALLOWED_ORIGIN=https://$DOMAIN,AUTH_BASE_URL=https://$DOMAIN,\
WEB_POST_LOGIN_URL=https://$DOMAIN,AUTH_COOKIE_SECURE=true,\
GOOGLE_CLIENT_ID=$GOOGLE_CLIENT_ID,GITHUB_CLIENT_ID=$GITHUB_CLIENT_ID
```

Notes:
- **`PORT` is injected by Cloud Run** — the API binds it automatically; don't set
  `BIND_ADDR`.
- **Leave `RUN_MIGRATIONS_ON_BOOT` unset** (defaults false) — step 1 owns migrations.
- Put real secrets in **Secret Manager** (`--set-secrets`), not `--set-env-vars`.
- Use the Neon **pooled** string; the `sqlx` pool is already kept small + lazy.

### 3. Map the domain + front with Cloudflare

The SPA is already served same-origin by the API image (step 2), so there is no
separate SPA host to deploy. Map the domain on Cloud Run, then proxy it through
Cloudflare:

1. **Verify the domain to Google** (one-time, interactive): `gcloud domains verify
   $DOMAIN` opens Search Console → add `$DOMAIN` as a **Domain property** → it gives a
   `google-site-verification=...` TXT record → add that TXT in Cloudflare DNS → click
   Verify. Confirm with `gcloud domains list-user-verified`.
2. **Create the mapping** and read back its DNS targets:
   ```bash
   gcloud beta run domain-mappings create --service fillerkiller-api \
     --domain $DOMAIN --region $REGION
   gcloud beta run domain-mappings describe --domain $DOMAIN --region $REGION \
     --format="value(status.resourceRecords)"   # the A/AAAA (apex) or CNAME (subdomain)
   ```
3. **Add those records in Cloudflare as proxied (orange-cloud).** The mapping registers
   `$DOMAIN` on the Cloud Run origin, so the proxy needs no Host-header rewrite (Cloud
   Run 404s on an unrecognized Host otherwise). First let the mapping's Google-managed
   cert reach `Ready` (watch the `describe` output) — `.app` is HSTS-preloaded
   (HTTPS-only), so the origin must have valid TLS before the proxy can reach it;
   Cloudflare's SSL mode should be **Full (strict)**.
4. Register each OAuth redirect URI as `https://$DOMAIN/api/auth/<provider>/callback`.

> Cloudflare respects the API's `Cache-Control` (catalog reads cache long, aggregates
> briefly). A global edge rate-limit rule can be added on `/api/episodes/*/vote`; until
> then the in-API limiters (per-IP votes, per-IP search, per-instance import) are the
> rate controls.

### 4. Recompute job + backups (scheduled)

```bash
# Drift-correction / backfill for episode_score (triggers keep it current; this is a
# safety net). Schedule via Cloud Scheduler if desired.
gcloud run jobs deploy fillerkiller-recompute \
  --source . --region $REGION \
  --build-service-account=$BUILD_SA \
  --command fillerkiller-api --args recompute-scores \
  --set-secrets DATABASE_URL=fk-database-url:latest \
  --set-env-vars TMDB_API_READ_TOKEN=unused,AUTH_JWT_SECRET=$(openssl rand -base64 32)
```

- **Backups (own your data):** Neon has its own PITR, but keep an owned copy — schedule
  `pg_dump` of the Neon DB to object storage you control (Cloudflare R2 / Backblaze B2),
  e.g. a GitHub Action or a Cloud Run Job. This is what preserves data ownership
  independent of the vendor.

### 5. App metrics (GMP collector sidecar) — optional

The API exposes Prometheus RED + business metrics (`http_requests_total`,
`http_request_duration_seconds`, `votes_total`, `show_imports_total`, …) on a
**private** port (`METRICS_ADDR`, default `127.0.0.1:9090`) that Cloud Run never routes
public traffic to. A **Google Managed Service for Prometheus (GMP) collector
sidecar** scrapes it over the instance-local network and pushes to Managed
Prometheus, where a Grafana instance can read it back through the Cloud Monitoring
data source.

This is push-based on purpose: scraping Cloud Run from outside is lossy because the
service autoscales and cold-starts, so each external scrape would hit one random
instance. The sidecar gives each instance its own series and Google aggregates them.

Adding the sidecar makes the service **multi-container**, which can't be built with
`--source`, so the flow becomes build-image → `services replace`:

```bash
# a. One-time: grant the runtime SA metric/log write + the scrape-config secret.
for ROLE in roles/monitoring.metricWriter roles/logging.logWriter; do
  gcloud projects add-iam-policy-binding $PROJECT \
    --member=serviceAccount:$RUNTIME_SA --role=$ROLE
done
gcloud secrets create fk-run-monitoring --data-file=deploy/run-monitoring.yaml
gcloud secrets add-iam-policy-binding fk-run-monitoring \
  --member=serviceAccount:$RUNTIME_SA --role=roles/secretmanager.secretAccessor

# b. Build + push the API image (Cloud Build, root Dockerfile = SPA + API).
IMAGE=$REGION-docker.pkg.dev/$PROJECT/fillerkiller/fillerkiller-api:$(git rev-parse --short HEAD)
gcloud builds submit --tag $IMAGE .

# c. Fill placeholders in deploy/cloudrun-service.yaml and apply.
sed -e "s/PROJECT_ID/$PROJECT/g" -e "s/REGION/$REGION/g" \
    -e "s|IMAGE|$IMAGE|g" -e "s/DOMAIN/$DOMAIN/g" \
    -e "s/GOOGLE_CLIENT_ID_VALUE/$GOOGLE_CLIENT_ID/g" \
    -e "s/GITHUB_CLIENT_ID_VALUE/$GITHUB_CLIENT_ID/g" \
    deploy/cloudrun-service.yaml | gcloud run services replace - --region $REGION
```

Notes:
- `services replace` updates only the service **spec** — it does **not** set the
  public invoker the way step 2's `--allow-unauthenticated` does. If you ran step 2
  first, that binding persists. If the sidecar deploy is the service's first
  creation, grant it once:
  `gcloud run services add-iam-policy-binding fillerkiller-api --member=allUsers --role=roles/run.invoker --region $REGION`.
- The `sed | replace` stream is throwaway (piped straight to gcloud); it also
  rewrites the placeholder list in the file's header comments, which is harmless —
  don't redirect it to a file expecting clean docs.
- The sidecar image is Google's: `us-docker.pkg.dev/cloud-ops-agents-artifacts/cloud-run-gmp-sidecar/cloud-run-gmp-sidecar:1.2.0`.
- `cpu-throttling: 'false'` (in the YAML) keeps the collector able to flush between
  requests; it raises per-instance cost. Flip to `'true'` to save cost at the price
  of some metric gaps. App metrics are naturally absent while the service is scaled
  to zero — that's fine, there's no traffic to measure, and the **built-in Cloud Run
  metrics** (request count, latency, instances, CPU/mem) keep flowing regardless.
- Don't want metrics? Keep deploying with the single-container `--source` command in
  step 2 (the private metrics port simply goes unscraped) and skip this section.

### Updating

```bash
git pull
# If the change added a migration, run step 1 (migrate job) first.
#
# Single-container (no metrics sidecar): redeploy from the repo root — the root
# Dockerfile rebuilds the SPA + API into one image:
gcloud run deploy fillerkiller-api --source . --region $REGION \
  --service-account=$RUNTIME_SA --build-service-account=$BUILD_SA

# Multi-container (with the GMP sidecar): rebuild the image and re-apply the
# service (steps 5b–5c above).
```
