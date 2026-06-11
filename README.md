# FillerKiller

[![CI](https://github.com/ShaneMain/FillerKiller/actions/workflows/ci.yml/badge.svg)](https://github.com/ShaneMain/FillerKiller/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-Axum-000000?logo=rust&logoColor=white)](api/)
[![React](https://img.shields.io/badge/React-Vite-20232a?logo=react&logoColor=61dafb)](web/)
[![Live](https://img.shields.io/badge/live-fillerkiller.app-2ea44f)](https://fillerkiller.app)

**Crowd-sourced _filler vs. canon_ voting for TV episodes.** Browse a show, vote on
whether each episode is essential canon, fun-but-skippable, or pure filler, and get a
**skip guide** — the canon-only watch order for any series.

🔗 **Live at [fillerkiller.app](https://fillerkiller.app)**

![FillerKiller — voting on Star Trek: The Next Generation episodes](docs/show-page.png)

## What it does

- **Search any TV show** — the catalog is backed by [TMDB](https://www.themoviedb.org/),
  imported on demand the first time someone looks a show up.
- **Vote on every episode** as one of three values:
  - `CANON` — essential, don't skip it
  - `WORTH_WATCHING` — enjoyable but inessential (a fun standalone)
  - `FILLER` — skippable
- **See the crowd's verdict.** Each episode shows an aggregate status derived from the
  votes:

  | Status | Meaning |
  |---|---|
  | **Canon** / **Worth Watching** / **Filler** | The plurality vote, once there are enough votes |
  | **Contested** | Votes are split — the leading option's margin is within 10% of the total |
  | **Not enough votes** | Fewer than 5 votes so far |

- **Generate a skip guide** for the whole series, partitioned into **watch** (canon),
  **optional** (worth-watching), and **skipped** (filler). When the crowd is unsure, the
  guide keeps the episode in the watch list — wrongly skipping canon is worse than
  wrongly watching filler.
- **Write and share your own skip guides.** Beyond the crowd-derived guide, signed-in
  users can curate a per-episode guide for a show, publish it, and others can browse and
  **upvote** it — capped at five published guides per show to keep them curated.
- **TMDB ratings** for the show and each episode sit alongside the crowd verdict, so you
  can weigh "is it filler?" against "is it any good?".
- **Your account, your data.** Sign in with Google or GitHub to vote; set an optional
  **screen name** that displays instead of your provider name. Deleting your account
  removes your guides but **keeps your votes (anonymized)** — the crowd data stays intact.

## Stack

A split, scale-to-zero stack:

- **`api/`** — Rust + **[Axum](https://github.com/tokio-rs/axum)** service with
  **[`sqlx`](https://github.com/launchbadge/sqlx)** + **PostgreSQL**. Compile-time-checked
  queries, scales to zero.
- **`web/`** — static **React + Vite** (TypeScript + Tailwind) single-page app.
- **TMDB** — the TV catalog source of truth, accessed **server-side only** from the API.

The SPA talks only to the API; the API holds the TMDB token, so it never reaches the
browser. The catalog is cached hard and both the compute and the database can scale to
zero, so the dominant costs are storage and bandwidth rather than compute.

Two extras worth calling out:

- **SEO without a JS runtime.** For crawlers and link unfurlers, the API server-renders
  the show, skip-guide, and shared-guide pages — per-page title / Open Graph / JSON-LD
  (`TVSeries` + `AggregateRating` + breadcrumbs) plus a crawlable content snapshot — and
  serves a DB-driven `sitemap.xml`. The React app then hydrates over the top.
- **A self-healing catalog.** An imported show refreshes its TMDB metadata and ratings in
  the background when viewed, gated by a TTL and tiered by recency — a long-ended series
  refreshes far less often than one still airing — so the cache stays fresh with minimal
  TMDB calls and never blocks a request.

## Layout

```
api/                       Rust + Axum service
  src/main.rs              app wiring, routing, CORS, health
  src/scoring.rs           filler score + status + skip-guide math (pure functions)
  src/import.rs            import-on-demand + self-healing refresh from TMDB
  src/tmdb.rs              server-side TMDB client
  src/db.rs                database access (compile-time-checked queries)
  src/guides.rs            user-authored skip guides (create / list / like)
  src/seo.rs               server-rendered pages + sitemap for crawlers
  src/auth.rs / oauth.rs   OAuth → JWT session cookie
  migrations/             SQL schema + migrations
  Dockerfile              container build
web/                       React + Vite SPA
  src/pages/              search, show, skip-guide, guides, account, login, legal
  src/lib/api.ts          API client
deploy/                    Docker Compose + deploy runbook
Dockerfile                 single image: builds the SPA + API together
```

## Getting started

You need **PostgreSQL** and a **TMDB API read token**
(create one at <https://www.themoviedb.org/settings/api>).

### API (`api/`)

```bash
cd api
cp .env.example .env       # set DATABASE_URL (pooled), TMDB_API_READ_TOKEN, ...
cargo test                 # unit tests; no DB needed (uses the committed .sqlx cache)
cargo run                  # starts the API on :8080
```

A throwaway local Postgres for development:

```bash
docker run -d --name fk-pg -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=fillerkiller -p 5433:5432 postgres:16
export DATABASE_URL="postgres://postgres:postgres@localhost:5433/fillerkiller"
cargo run -- migrate       # apply migrations (or: cargo sqlx migrate run)
```

**Compile-time-checked queries (`sqlx`):** queries are verified against the schema at
build time. The generated `.sqlx/` cache is committed, so normal builds and CI need
**no database** (`SQLX_OFFLINE=true`). After changing any SQL query, regenerate it with a
live dev DB (`cargo sqlx prepare`) and commit the updated `.sqlx/`.

### Web (`web/`)

```bash
cd web
npm install
cp .env.example .env.local # set VITE_API_BASE_URL (defaults to http://localhost:8080)
npm run dev                # http://localhost:5173
```

## API endpoints

| Method | Path | Notes |
|---|---|---|
| `GET` | `/api/search?q=` | Proxy TMDB search; annotates already-imported shows. |
| `GET` | `/api/shows?limit=` | Most-voted shows, for the home page browse grid. |
| `GET` | `/api/shows/{id}` | Show + seasons + TMDB rating. `{id}` is a slug, our UUID, or `tmdb:<n>` (imports on demand). |
| `GET` | `/api/shows/{id}/episodes?season=` | Episodes with aggregate scores + TMDB ratings; `myVote` when signed in. |
| `GET` | `/api/shows/{id}/skip-guide` | The watch / optional / skipped partition for the show. |
| `GET` | `/api/shows/{id}/guides` | Published user-authored guides for the show. |
| `POST` | `/api/shows/{id}/guides` | Create a guide (max 5 published per show). Auth required. |
| `GET` | `/api/guides/{id}` | A user guide's detail (drafts visible only to the author). |
| `PUT` `DELETE` | `/api/guides/{id}` | Update / delete your own guide. Auth required. |
| `PUT` `DELETE` | `/api/guides/{id}/like` | Add / remove your upvote. Auth required. |
| `PUT` | `/api/episodes/{id}/vote` | Cast/change a vote: `{ "value": "FILLER" \| "WORTH_WATCHING" \| "CANON" }`. Auth required. |
| `DELETE` | `/api/episodes/{id}/vote` | Remove the caller's vote. Auth required. |
| `GET` | `/api/auth/{provider}/login` | OAuth sign-in (`google` / `github`); `?next=` returns you to where you started. |
| `GET` | `/api/auth/{provider}/callback` | OAuth callback → sets the session cookie. |
| `POST` | `/api/auth/logout` | Clear the session. |
| `GET` | `/api/me` | Current user (from the session cookie) or `null`. |
| `PUT` | `/api/me` | Update your optional screen name. Auth required. |
| `DELETE` | `/api/me` | Delete your account; guides removed, votes kept anonymized. Auth required. |
| `GET` | `/api/me/guides` | Your own guides, published or draft. Auth required. |
| `GET` | `/health`, `/health/db` | Liveness / DB readiness. |

When the API also serves the SPA, the SEO routes `/shows/{slug}`,
`/shows/{slug}/skip-guide`, and `/shows/{slug}/guides/{id}` return server-rendered HTML
(unknown shows / unpublished guides get a real `404`), alongside `/sitemap.xml` and a
static `/robots.txt`.

**Auth** is OAuth → a stateless JWT in an httpOnly cookie. To test sign-in locally,
register an OAuth app, set its redirect URI to
`http://localhost:8080/api/auth/{provider}/callback`, then set the provider's
`*_CLIENT_ID` / `*_CLIENT_SECRET` and `AUTH_JWT_SECRET` in `api/.env`. A provider with no
credentials is simply disabled, so you can run with just Google, just GitHub, or neither.

Catalog responses set `Cache-Control` — longer for the static catalog, short for
vote-derived scores — so a CDN can absorb the read traffic.

## Deploy

The root `Dockerfile` builds the SPA and API into a **single image**. Two deployment
shapes are documented in **[`deploy/README.md`](deploy/README.md)**:

- **Cloud Run + Neon**, fronted by a CDN — ephemeral compute that scales with traffic.
- **Self-hosted single box** — the whole stack (Postgres + API + Caddy) on one VPS via
  Docker Compose for a few dollars a month. You own the data.

Both use the same standard Postgres and the same image; switching is a `DATABASE_URL` +
target change, not a rewrite.

## Contributing

Issues and pull requests are welcome. A good change keeps the API's compile-time-checked
queries green (`cargo test`, and `cargo sqlx prepare` if you touched SQL) and the web
build clean (`npm run build`, `npm run lint`).

The filler-scoring math lives in one place — `api/src/scoring.rs` — as pure functions
with unit tests; that's the spot to look first if you want to understand or change how
statuses and skip guides are computed.

## License

FillerKiller is free software, licensed under the **GNU General Public License v3.0**.
See [`LICENSE`](LICENSE).

## Attribution

This product uses the TMDB API but is not endorsed or certified by TMDB.
