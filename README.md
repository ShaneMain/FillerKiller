# FillerKiller

Crowd-sourced **filler vs. canon** voting for TV episodes. Browse a show, vote on
whether each episode is skippable filler, and generate a **skip guide** — the
canon-only watch order.

> **Spec lives in a separate repo:** `internal docs` is the
> living specification (product, data model, API, voting math, ADRs). The spec leads;
> this code implements it. When in doubt, the spec wins.

## Stack

A split, scale-to-zero stack:

- **`api/`** — Rust + **Axum** API with **`sqlx`** + **PostgreSQL**. Deploys to a
  scale-to-zero host (Fly Machines / Lambda).
- **`web/`** — static **React + Vite** (TypeScript + Tailwind) SPA, served from a CDN.
- **TMDB** — TV catalog source of truth, accessed **server-side only** from the API.

The SPA talks only to the API; the API holds the TMDB token. Cost note: the dominant
costs are the database and bandwidth, not compute — the catalog is cached hard
, and both compute and DB scale to zero.

## Layout

```
api/                       Rust + Axum service
  src/main.rs              app wiring, CORS, health routes
  src/scoring.rs           filler score + status + skip guide — mirrors the design notes
  src/tmdb.rs              server-side TMDB client
  src/config.rs            env config
  migrations/0001_init.sql schema — mirrors the design notes
  Dockerfile               container build (Fly Machines / containers)
web/                       React + Vite SPA
  src/App.tsx              landing page (pings the API)
  src/lib/api.ts           tiny API client
```

## Getting started

### API (`api/`)

```bash
cd api
cp .env.example .env       # set DATABASE_URL (pooled), TMDB_API_READ_TOKEN, ...
cargo test                 # unit tests; no DB needed (uses the committed .sqlx cache)
cargo run                  # starts the API on :8080 (applies migrations if DB is reachable)
```

You need a PostgreSQL (Neon/Supabase pooled URL recommended) and a TMDB API read
token (https://www.themoviedb.org/settings/api).

A throwaway local Postgres for development:

```bash
docker run -d --name fk-pg -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=fillerkiller -p 5433:5432 postgres:16
export DATABASE_URL="postgres://postgres:postgres@localhost:5433/fillerkiller"
cargo sqlx migrate run     # apply migrations (needs sqlx-cli)
```

**Compile-time-checked queries (`sqlx`):** queries are verified against the schema at
build time. The generated `.sqlx/` cache is committed, so normal builds and CI need
**no database** (`SQLX_OFFLINE=true`). After changing any SQL query, regenerate it with
a live dev DB: `cargo sqlx prepare`, and commit the updated `.sqlx/`.

#### Catalog endpoints

| Method | Path | Notes |
|---|---|---|
| `GET` | `/api/search?q=` | Proxy TMDB search; annotates imported shows. |
| `GET` | `/api/shows/{id}` | Show + seasons. `{id}` is our uuid or `tmdb:<n>` (imports on demand). |
| `GET` | `/api/shows/{id}/episodes?season=` | Episodes with aggregate filler scores. |
| `GET` | `/health`, `/health/db` | Liveness / DB readiness. |

Catalog responses set `Cache-Control` (longer for static catalog, short for vote-derived
scores) per the design notes.

### Web (`web/`)

```bash
cd web
npm install
cp .env.example .env.local # set VITE_API_BASE_URL (defaults to http://localhost:8080)
npm run dev                # http://localhost:5173
```

## Conventions

- **TMDB token is server-only**, held by the API. The SPA never sees it.
- **Scoring constants** (`MIN_VOTES`, `CANON_BELOW`, `FILLER_ABOVE`) live only in
  `api/src/scoring.rs` and must match `the design notes`. Changing them is a spec change —
  update the spec repo too.
- **Catalog responses are cacheable** (the real cost lever) — set `Cache-Control` per
  the design notes as endpoints land.

## Attribution

This product uses the TMDB API but is not endorsed or certified by TMDB.
