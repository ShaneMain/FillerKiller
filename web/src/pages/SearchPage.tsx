import { useState, type FormEvent } from "react";
import { Link } from "react-router-dom";
import { imageUrl, searchShows, type SearchItem } from "../lib/api";

export function SearchPage() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchItem[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    const q = query.trim();
    if (!q) return;
    setLoading(true);
    setErr(null);
    try {
      const { results } = await searchShows(q);
      setResults(results);
    } catch (e) {
      setErr(e instanceof Error ? e.message : "search failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="mx-auto max-w-3xl px-4 py-8">
      <h1 className="mb-2 text-2xl font-bold">Find a show</h1>
      <p className="mb-5 text-sm text-zinc-400">
        Search a TV series, then vote on which episodes are filler.
      </p>

      <form onSubmit={onSubmit} className="flex gap-2">
        <input
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="e.g. Breaking Bad"
          className="flex-1 rounded-md border border-zinc-700 bg-zinc-900 px-3 py-2 text-zinc-100 placeholder-zinc-500 outline-none focus:border-zinc-500"
        />
        <button
          type="submit"
          className="rounded-md bg-rose-600 px-4 py-2 font-medium text-white hover:bg-rose-500"
        >
          Search
        </button>
      </form>

      {loading && <p className="mt-6 text-zinc-400">Searching…</p>}
      {err && <p className="mt-6 text-rose-400">{err}</p>}
      {results && !loading && (
        <ul className="mt-6 space-y-2">
          {results.length === 0 && <li className="text-zinc-500">No matches.</li>}
          {results.map((r) => {
            const to = `/shows/${encodeURIComponent(r.showId ?? `tmdb:${r.tmdbId}`)}`;
            const poster = imageUrl(r.posterPath, "w92");
            return (
              <li key={r.tmdbId}>
                <Link
                  to={to}
                  className="flex items-center gap-3 rounded-lg border border-zinc-800 bg-zinc-900 p-3 hover:border-zinc-600"
                >
                  {poster ? (
                    <img src={poster} alt="" className="h-16 w-11 shrink-0 rounded object-cover" />
                  ) : (
                    <div className="h-16 w-11 shrink-0 rounded bg-zinc-800" />
                  )}
                  <div className="min-w-0">
                    <div className="truncate font-medium">{r.name}</div>
                    <div className="text-sm text-zinc-500">{r.firstAirYear ?? "—"}</div>
                  </div>
                </Link>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
