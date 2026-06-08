# FillerKiller — web

The FillerKiller single-page app: **React + Vite + TypeScript + Tailwind**. It talks to
the API in [`../api`](../api) and never holds any secrets (the TMDB token lives
server-side).

```bash
npm install
cp .env.example .env.local   # set VITE_API_BASE_URL (defaults to http://localhost:8080)
npm run dev                  # http://localhost:5173
npm run build                # production build → dist/
npm run lint                 # eslint
```

For the full picture — the API, scoring rules, and deployment — see the
[project README](../README.md). Licensed under GPL-3.0-or-later.
