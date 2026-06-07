import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import {
  getEpisodes,
  getShow,
  imageUrl,
  type Episode,
  type ShowDetail,
} from "../lib/api";
import { useAuth } from "../lib/auth";
import { EpisodeRow } from "../components/EpisodeRow";

export function ShowPage() {
  const { id = "" } = useParams();
  const { user } = useAuth();
  const [show, setShow] = useState<ShowDetail | null>(null);
  const [season, setSeason] = useState<number | null>(null);
  const [episodes, setEpisodes] = useState<Episode[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loadingEps, setLoadingEps] = useState(false);

  // Load the show (imports on first open — can take a moment).
  useEffect(() => {
    let active = true;
    setShow(null);
    setErr(null);
    getShow(id)
      .then((s) => {
        if (!active) return;
        setShow(s);
        const seasons = s.seasons.map((x) => x.seasonNumber);
        setSeason(seasons.includes(1) ? 1 : (seasons.find((n) => n > 0) ?? seasons[0] ?? null));
      })
      .catch((e) => active && setErr(e instanceof Error ? e.message : "failed to load show"));
    return () => {
      active = false;
    };
  }, [id]);

  // Load episodes for the selected season (re-runs when sign-in changes to refresh myVote).
  useEffect(() => {
    if (season == null) return;
    let active = true;
    setLoadingEps(true);
    setEpisodes(null);
    getEpisodes(id, season)
      .then((r) => active && setEpisodes(r.episodes))
      .catch((e) => active && setErr(e instanceof Error ? e.message : "failed to load episodes"))
      .finally(() => active && setLoadingEps(false));
    return () => {
      active = false;
    };
  }, [id, season, user?.id]);

  if (err) {
    return (
      <div className="mx-auto max-w-3xl px-4 py-8">
        <p className="text-rose-400">{err}</p>
        <Link to="/" className="mt-3 inline-block text-zinc-400 hover:text-zinc-200">← Back to search</Link>
      </div>
    );
  }

  if (!show) {
    return <div className="mx-auto max-w-3xl px-4 py-8 text-zinc-400">Loading show…</div>;
  }

  const poster = imageUrl(show.posterPath, "w154");

  return (
    <div className="mx-auto max-w-3xl px-4 py-8">
      <Link to="/" className="mb-4 inline-block text-sm text-zinc-400 hover:text-zinc-200">← Search</Link>

      <div className="flex gap-4">
        {poster && <img src={poster} alt="" className="h-36 w-24 shrink-0 rounded object-cover" />}
        <div className="min-w-0">
          <h1 className="text-2xl font-bold">{show.name}</h1>
          {show.overview && <p className="mt-2 text-sm text-zinc-400 line-clamp-4">{show.overview}</p>}
        </div>
      </div>

      <div className="mt-4">
        <Link
          to={`/shows/${encodeURIComponent(id)}/skip-guide`}
          className="inline-flex items-center gap-1 rounded-md bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 hover:bg-white"
        >
          View skip guide →
        </Link>
      </div>

      {!user && (
        <p className="mt-5 rounded-md border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm text-zinc-400">
          Sign in (top right) to vote on episodes.
        </p>
      )}

      <div className="mt-6 flex flex-wrap gap-2">
        {show.seasons.map((s) => (
          <button
            key={s.id}
            onClick={() => setSeason(s.seasonNumber)}
            className={`rounded-md px-3 py-1 text-sm ring-1 ring-inset ring-zinc-700 ${
              s.seasonNumber === season ? "bg-zinc-100 text-zinc-900" : "text-zinc-300 hover:bg-zinc-800"
            }`}
          >
            {s.seasonNumber === 0 ? "Specials" : `Season ${s.seasonNumber}`}
          </button>
        ))}
      </div>

      <div className="mt-5 flex items-center gap-4 text-xs text-zinc-500">
        <span className="flex items-center gap-1.5"><i className="h-2 w-2 rounded-full bg-rose-500" />Filler</span>
        <span className="flex items-center gap-1.5"><i className="h-2 w-2 rounded-full bg-sky-500" />Worth it</span>
        <span className="flex items-center gap-1.5"><i className="h-2 w-2 rounded-full bg-emerald-500" />Canon</span>
      </div>

      <div className="mt-2">
        {loadingEps && <p className="text-zinc-400">Loading episodes…</p>}
        {episodes?.map((ep) => (
          <EpisodeRow key={ep.id} episode={ep} signedIn={!!user} />
        ))}
        {episodes && episodes.length === 0 && !loadingEps && (
          <p className="text-zinc-500">No episodes in this season.</p>
        )}
      </div>
    </div>
  );
}
