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
      <section className="mb-8">
        <h1 className="text-3xl font-bold tracking-tight sm:text-4xl">
          Skip the <span className="text-rose-500">filler</span>.
        </h1>
        <p className="mt-3 max-w-2xl text-zinc-300">
          FillerKiller is a crowd-sourced guide to which TV episodes are actually worth
          your time. For every episode, viewers vote{" "}
          <span className="font-medium text-rose-400">Filler</span>,{" "}
          <span className="font-medium text-sky-400">Worth It</span>, or{" "}
          <span className="font-medium text-emerald-400">Canon</span> — so you can see at a
          glance what to skip, what's optional, and what's essential to the story.
        </p>
        <ul className="mt-5 grid gap-2 sm:grid-cols-3">
          {[
            ["Vote per episode", "Filler · Worth It · Canon — one vote per person."],
            ["See the ratio", "At-a-glance vote bars and a confident status."],
            ["Skip guide", "A binge-ready watch order that drops the fluff."],
          ].map(([title, body]) => (
            <li key={title} className="rounded-lg border border-zinc-800 bg-zinc-900 p-3">
              <div className="text-sm font-semibold text-zinc-100">{title}</div>
              <div className="mt-0.5 text-xs text-zinc-400">{body}</div>
            </li>
          ))}
        </ul>
      </section>

      <h2 className="mb-1 text-lg font-semibold">Find a show</h2>
      <p className="mb-4 text-sm text-zinc-400">
        Search a TV series to see the community's verdict and add your votes.
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
            const to = r.slug
              ? `/shows/${encodeURIComponent(r.slug)}`
              : `/shows/tmdb:${r.tmdbId}`;
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
