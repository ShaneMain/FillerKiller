# FillerKiller

Crowd-sourced **filler vs. canon** voting for TV episodes. Browse a show, vote on
whether each episode is skippable filler, and generate a **skip guide** — the
canon-only watch order.

> **Spec lives in a separate repo:** `internal docs` is
> the living specification (product, data model, API, voting math, ADRs). The spec
> leads; this code implements it. When in doubt, the spec wins.

## Stack

- **Next.js** (App Router, TypeScript) — UI + API route handlers
- **PostgreSQL** + **Prisma** — catalog mirror + the vote/opinion layer
- **Tailwind CSS** — styling
- **TMDB** — TV catalog source of truth (server-side only)

See `internal docs` and the design notes.

## Getting started

```bash
npm install
cp .env.example .env        # fill in DATABASE_URL, TMDB_API_READ_TOKEN, AUTH_SECRET
npm run db:generate         # generate the Prisma client
npm run db:migrate          # apply migrations to your Postgres
npm run dev                 # http://localhost:3000
```

You need a running PostgreSQL and a TMDB API read token
(https://www.themoviedb.org/settings/api).

## Scripts

| Script | Does |
|---|---|
| `npm run dev` | Start the dev server. |
| `npm run build` / `start` | Production build / serve. |
| `npm test` | Run unit tests (`node --test`). |
| `npm run db:generate` | Generate the Prisma client. |
| `npm run db:migrate` | Create/apply a dev migration. |
| `npm run db:studio` | Open Prisma Studio. |
| `npm run lint` | ESLint. |

## Layout

```
prisma/schema.prisma   # DB schema — mirrors the design notes
src/lib/scoring.ts     # filler score + status + skip guide — mirrors the design notes
src/lib/scoring.test.ts
src/lib/tmdb.ts        # server-side TMDB client
src/lib/db.ts          # Prisma client singleton
src/app/               # Next.js App Router
```

## Conventions

- **TMDB token is server-only.** Never import `src/lib/tmdb.ts` into a client
  component; proxy through `/api`.
- **Scoring constants** (`MIN_VOTES`, `CANON_BELOW`, `FILLER_ABOVE`) live only in
  `src/lib/scoring.ts` and must match `the design notes`. Changing them is a spec
  change — update the spec repo too.

## Attribution

This product uses the TMDB API but is not endorsed or certified by TMDB.
