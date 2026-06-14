import { useEffect, useState, type FormEvent } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { imageUrl, popularShows, searchShows, type PopularShow, type SearchItem } from "../lib/api";
import { usePageMeta } from "../lib/meta";

// Auto-focusing the search box is helpful with a keyboard but hostile on touch
// devices, where it pops the on-screen keyboard over the page on every visit.
const FINE_POINTER =
  typeof window !== "undefined" && window.matchMedia("(pointer: fine)").matches;

/** A finished search, remembered together with the query that produced it so
 *  stale results never render against a different (or cleared) query. */
interface SearchOutcome {
  q: string;
  results: SearchItem[] | null;
  error: string | null;
}

export function SearchPage() {
  usePageMeta();
  // The query lives in the URL (?q=) so results survive back/refresh and the
  // page is shareable. The text box is uncontrolled, keyed on the committed
  // query — navigating (back/forward, shared link) remounts it with the new
  // value without any state syncing.
  const [searchParams, setSearchParams] = useSearchParams();
  const q = searchParams.get("q")?.trim() ?? "";
  const [outcome, setOutcome] = useState<SearchOutcome | null>(null);
  const [popular, setPopular] = useState<PopularShow[] | null>(null);

  // The OAuth callback bounces failed sign-ins back here with ?auth_error.
  const authError = searchParams.get("auth_error");
  function dismissAuthError() {
    const next = new URLSearchParams(searchParams);
    next.delete("auth_error");
    setSearchParams(next, { replace: true });
  }

  // Run the search whenever the committed query changes (submit, back/forward,
  // or landing on a shared /?q= link). All state updates happen async in the
  // promise handlers; loading/stale handling is derived from `outcome` below.
  useEffect(() => {
    if (!q) return;
    let active = true;
    searchShows(q)
      .then((r) => active && setOutcome({ q, results: r.results, error: null }))
      .catch(
        (e) =>
          active &&
          setOutcome({
            q,
            results: null,
            error: e instanceof Error ? e.message : "search failed",
          }),
      );
    return () => {
      active = false;
    };
  }, [q]);

  // Only an outcome for the *current* query counts; anything else is stale.
  const current = outcome?.q === q ? outcome : null;
  const loading = !!q && !current;
  const results = current?.results ?? null;
  const err = current?.error ?? null;

  // Popular shows fill the otherwise-empty home page with somewhere to go.
  useEffect(() => {
    let active = true;
    popularShows()
      .then((r) => active && setPopular(r.shows))
      .catch(() => { /* best-effort; the section just stays hidden */ });
    return () => {
      active = false;
    };
  }, []);

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const next = (new FormData(e.currentTarget).get("q") as string | null)?.trim() ?? "";
    if (!next || next === q) return;
    const params = new URLSearchParams(searchParams);
    params.set("q", next);
    setSearchParams(params);
  }

  return (
    <div className="mx-auto max-w-3xl px-4 py-8">
      {authError && (
        <div className="mb-6 flex items-start justify-between gap-3 rounded-md border border-rose-900/60 bg-rose-950/30 px-3 py-2 text-sm text-rose-300">
          <span>Sign-in didn't complete. Please try again.</span>
          <button onClick={dismissAuthError} aria-label="Dismiss" className="text-rose-400 hover:text-rose-200">
            ✕
          </button>
        </div>
      )}
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
            ["See the ratio", "Vote bars, a confident status, and reason tags like “recap”."],
            ["Skip-guide modes", "Binge, standard, canon-only, or completionist — your call."],
            ["Build your own guide", "Author and share a custom watch order for any show."],
            ["Track what you've watched", "Tick off episodes; progress bars follow your guides."],
            ["See the time saved", "Every guide totals the hours you skip."],
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
          key={q}
          autoFocus={FINE_POINTER}
          type="search"
          name="q"
          defaultValue={q}
          placeholder="e.g. Breaking Bad"
          aria-label="Search TV shows"
          className="min-w-0 flex-1 rounded-md border border-zinc-700 bg-zinc-900 px-3 py-2 text-zinc-100 placeholder-zinc-500 outline-none focus:border-zinc-500"
        />
        <button
          type="submit"
          className="shrink-0 rounded-md bg-rose-600 px-4 py-2 font-medium text-white hover:bg-rose-500"
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
                    <div className="flex items-center gap-2 text-sm text-zinc-500">
                      <span>{r.firstAirYear ?? "—"}</span>
                      {r.fillerCoverage != null && (
                        <span className="rounded-full bg-zinc-800 px-2 py-0.5 text-xs text-zinc-400">
                          {r.fillerCoverage > 0
                            ? `${Math.round(r.fillerCoverage * 100)}% rated`
                            : "Not yet rated"}
                        </span>
                      )}
                    </div>
                  </div>
                </Link>
              </li>
            );
          })}
        </ul>
      )}

      {/* Browse entry point for visitors who don't have a title in mind. Hidden
          while a search is showing so it never competes with results. */}
      {!q && popular && popular.length > 0 && (
        <section className="mt-10">
          <h2 className="mb-3 text-lg font-semibold">Popular shows</h2>
          <ul className="grid grid-cols-3 gap-3 sm:grid-cols-4 md:grid-cols-6">
            {popular.map((s) => {
              const poster = imageUrl(s.posterPath, "w154");
              // The poster chip carries the same verdict as the show's OG card,
              // so the front page sells the product, not just the catalog.
              const chip = !s.rated
                ? { text: "Not yet rated", cls: "text-zinc-400" }
                : s.fillerPct > 0
                  ? { text: `${s.fillerPct}% filler`, cls: "text-rose-300" }
                  : { text: "0% filler", cls: "text-emerald-300" };
              return (
                <li key={s.tmdbId}>
                  <Link
                    to={`/shows/${encodeURIComponent(s.slug)}`}
                    className="group block"
                    title={s.name}
                  >
                    <div className="relative">
                      {poster ? (
                        <img
                          src={poster}
                          alt={`${s.name} poster`}
                          loading="lazy"
                          className="aspect-2/3 w-full rounded-md object-cover ring-1 ring-inset ring-zinc-800 transition group-hover:ring-zinc-500"
                        />
                      ) : (
                        <div className="flex aspect-2/3 w-full items-center justify-center rounded-md bg-zinc-900 p-2 text-center text-xs text-zinc-500 ring-1 ring-inset ring-zinc-800">
                          {s.name}
                        </div>
                      )}
                      {s.episodeCount > 0 && (
                        <span
                          className={`absolute bottom-1 left-1 rounded bg-zinc-950/85 px-1.5 py-0.5 text-[11px] font-semibold ${chip.cls}`}
                        >
                          {chip.text}
                        </span>
                      )}
                    </div>
                    <div className="mt-1.5 truncate text-sm text-zinc-300 group-hover:text-zinc-100">
                      {s.name}
                    </div>
                    <div className="text-xs text-zinc-500">
                      {s.skipCount > 0
                        ? `Skip ${s.skipCount} of ${s.episodeCount}`
                        : (s.firstAirYear ?? "")}
                    </div>
                  </Link>
                </li>
              );
            })}
          </ul>
        </section>
      )}
    </div>
  );
}
