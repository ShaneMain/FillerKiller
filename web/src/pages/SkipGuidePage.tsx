import { useEffect, useRef, useState, type ReactNode } from "react";
import { Link, useParams, useSearchParams } from "react-router-dom";
import {
  getEpisodes,
  getShow,
  getSkipGuide,
  imageUrl,
  listGuides,
  markWatched,
  unmarkWatched,
  type GuideMode,
  type GuideSummary,
  type ShowDetail,
  type SkipGuide,
  type SkipGuideEntry,
} from "../lib/api";
import { DISPOSITION_META } from "../lib/guides";
import { useAuth } from "../lib/auth";
import { usePageMeta } from "../lib/meta";
import { LikeButton } from "../components/LikeButton";
import { WatchProgressBar } from "../components/WatchProgressBar";
import { WatchedToggle } from "../components/WatchedToggle";

type Tab = "community" | "user";

const MODE_LABELS: Record<GuideMode, string> = {
  completionist: "Completionist",
  standard: "Standard",
  "canon-only": "Canon only",
  binge: "Binge cut",
};

const MODE_DESCRIPTIONS: Record<GuideMode, string> = {
  completionist: "Everything except confirmed filler — the fullest experience.",
  standard: "Canon plus standout standalone episodes, filler skipped.",
  "canon-only": "The canon-only spine — all filler and standalone skipped.",
  binge: "Canon plus the best-rated standalone episodes — TMDB 7.0+.",
};

function formatTimeSaved(minutes: number): string {
  if (minutes < 120) return `${minutes} min`;
  return `${Math.round(minutes / 60)} h`;
}

export function SkipGuidePage() {
  const { id = "" } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const [show, setShow] = useState<ShowDetail | null>(null);
  const [tab, setTab] = useState<Tab>("community");

  const rawMode = searchParams.get("mode") as GuideMode | null;
  const mode: GuideMode =
    rawMode === "completionist" ||
    rawMode === "standard" ||
    rawMode === "canon-only" ||
    rawMode === "binge"
      ? rawMode
      : "standard";

  const name = show?.name ?? null;
  usePageMeta(
    name ? `Skip guide — ${name}` : "Skip guide",
    name ? `Crowd-sourced binge-ready watch order for ${name}.` : undefined,
  );

  useEffect(() => {
    let active = true;
    getShow(id).then((s) => active && setShow(s)).catch(() => {});
    return () => {
      active = false;
    };
  }, [id]);

  const poster = imageUrl(show?.posterPath ?? null, "w154");

  function handleModeChange(m: GuideMode) {
    // Replace, don't push — flipping through modes shouldn't bury the Back
    // button under one history entry per click.
    setSearchParams({ mode: m }, { replace: true });
  }

  return (
    <div className="mx-auto max-w-3xl px-4 py-8">
      <Link to={`/shows/${encodeURIComponent(id)}`} className="mb-4 inline-block text-sm text-zinc-400 hover:text-zinc-200">
        ← {name ?? "Show"}
      </Link>

      <div className="flex items-center gap-3">
        {poster && (
          <img
            src={poster}
            alt={`${name ?? "Show"} poster`}
            className="h-20 w-14 shrink-0 rounded object-cover"
          />
        )}
        <div className="min-w-0">
          <h1 className="text-2xl font-bold">Skip guide</h1>
          {name && <p className="truncate text-sm text-zinc-400">{name}</p>}
        </div>
      </div>

      <div className="mt-4 flex gap-1 border-b border-zinc-800" role="tablist">
        <TabButton active={tab === "community"} onClick={() => setTab("community")}>
          Community verdict
        </TabButton>
        <TabButton active={tab === "user"} onClick={() => setTab("user")}>
          User guides
        </TabButton>
      </div>

      {tab === "community" ? (
        <CommunityGuide showId={id} mode={mode} onModeChange={handleModeChange} />
      ) : (
        <UserGuides showId={id} />
      )}
    </div>
  );
}

function TabButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={`-mb-px border-b-2 px-3 py-2 text-sm font-medium ${
        active ? "border-rose-500 text-zinc-100" : "border-transparent text-zinc-400 hover:text-zinc-200"
      }`}
    >
      {children}
    </button>
  );
}

interface CommunityGuideProps {
  showId: string;
  mode: GuideMode;
  onModeChange: (m: GuideMode) => void;
}

function CommunityGuide({ showId, mode, onModeChange }: CommunityGuideProps) {
  const { user } = useAuth();
  const [guide, setGuide] = useState<SkipGuide | null>(null);
  /** Set of episode IDs the signed-in user has watched (null = not yet loaded). */
  const [watchedIds, setWatchedIds] = useState<Set<string> | null>(null);
  /** Episode IDs with an in-flight watched toggle. */
  const [busyIds, setBusyIds] = useState<Set<string>>(new Set());
  const [err, setErr] = useState<string | null>(null);

  // Toggle an episode's watched state straight from the checklist (optimistic,
  // reverted on failure). Mirrors EpisodeRow's toggle on the show page.
  async function toggleWatched(episodeId: string) {
    if (!watchedIds || busyIds.has(episodeId)) return;
    const next = !watchedIds.has(episodeId);
    const apply = (ids: Set<string> | null, on: boolean) => {
      if (!ids) return ids;
      const copy = new Set(ids);
      if (on) copy.add(episodeId);
      else copy.delete(episodeId);
      return copy;
    };
    setWatchedIds((ids) => apply(ids, next));
    setBusyIds((ids) => new Set(ids).add(episodeId));
    try {
      if (next) await markWatched(episodeId);
      else await unmarkWatched(episodeId);
    } catch {
      setWatchedIds((ids) => apply(ids, !next)); // revert
    } finally {
      setBusyIds((ids) => {
        const copy = new Set(ids);
        copy.delete(episodeId);
        return copy;
      });
    }
  }

  useEffect(() => {
    let active = true;
    setGuide(null);
    setErr(null);
    getSkipGuide(showId, mode)
      .then((g) => active && setGuide(g))
      .catch((e) => active && setErr(e instanceof Error ? e.message : "failed to load skip guide"));
    return () => {
      active = false;
    };
  }, [showId, mode]);

  // Fetch all episodes (no season filter) to get the user's watched state when
  // signed in. This gives us `score.watched` for every episode, enabling the
  // client-side intersection with the guide's watch list. We skip the fetch for
  // anonymous visitors — the guide itself is already cached and this would be
  // a private (uncached) request we don't need.
  const userId = user?.id;
  useEffect(() => {
    let active = true;
    if (!userId) {
      // Delay the clear until effect cleanup to avoid the synchronous-setState
      // warning: return a cleanup that sets the stale ids to null.
      return () => {
        setWatchedIds(null);
      };
    }
    getEpisodes(showId)
      .then((r) => {
        if (!active) return;
        const ids = new Set(r.episodes.filter((e) => e.score.watched).map((e) => e.id));
        setWatchedIds(ids);
      })
      .catch(() => { /* best-effort; progress is non-critical */ });
    return () => {
      active = false;
    };
  }, [showId, userId]);

  const modes: GuideMode[] = ["completionist", "standard", "canon-only", "binge"];

  // Keep the active pill visible inside the scrollable mode strip — a shared
  // ?mode= link would otherwise land with the selection scrolled off-screen.
  const activeModeRef = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    activeModeRef.current?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [mode]);

  return (
    <div>
      {/* Mode segmented control. The wrapper scrolls horizontally on narrow
          screens (full-bleed via the negative margin) — the four pills are
          wider than a phone viewport and must not stretch the page. */}
      <div className="mt-4">
        <div className="-mx-4 overflow-x-auto px-4">
          <div className="inline-flex rounded-full border border-zinc-700 bg-zinc-900 p-0.5">
            {modes.map((m) => (
              <button
                key={m}
                ref={mode === m ? activeModeRef : null}
                onClick={() => onModeChange(m)}
                className={`shrink-0 whitespace-nowrap rounded-full px-3 py-1 text-sm font-medium transition-colors ${
                  mode === m
                    ? "bg-zinc-700 text-zinc-100"
                    : "text-zinc-400 hover:text-zinc-200"
                }`}
              >
                {MODE_LABELS[m]}
              </button>
            ))}
          </div>
        </div>
        <p className="mt-2 text-sm text-zinc-400">
          {MODE_DESCRIPTIONS[mode]}
        </p>
      </div>

      {err && <p className="mt-6 text-rose-400">{err}</p>}
      {!guide && !err && <p className="mt-6 text-zinc-400">Building guide…</p>}

      {guide && (
        <>
          {/* Time-saved banner */}
          {guide.minutesSkipped != null && (
            <p className="mt-4 text-sm text-zinc-400">
              Skip {guide.skipped.length} of{" "}
              {guide.watch.length + guide.optional.length + guide.skipped.length} episodes
              {" "}— save ≈ {formatTimeSaved(guide.minutesSkipped)}
            </p>
          )}

          {/* Watch-list progress for signed-in users */}
          {user && watchedIds != null && guide.watch.length > 0 && (
            <WatchProgressBar
              watched={guide.watch.filter((e) => watchedIds.has(e.episodeId)).length}
              total={guide.watch.length}
              label="watch-list episodes"
              className="mt-3"
            />
          )}

          <div className="mt-6 space-y-6">
            <Bucket title="Watch" tone="emerald" entries={guide.watch}
              watchedIds={watchedIds} busyIds={busyIds} onToggle={user ? (id) => void toggleWatched(id) : undefined} />
            <Bucket title="Optional — worth it" tone="sky" entries={guide.optional}
              watchedIds={watchedIds} busyIds={busyIds} onToggle={user ? (id) => void toggleWatched(id) : undefined} />
            <Bucket title="Skip" tone="rose" entries={guide.skipped} />
          </div>
        </>
      )}
    </div>
  );
}

function UserGuides({ showId }: { showId: string }) {
  const { user } = useAuth();
  const [guides, setGuides] = useState<GuideSummary[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setGuides(null);
    setErr(null);
    listGuides(showId)
      .then((g) => active && setGuides(g))
      .catch((e) => active && setErr(e instanceof Error ? e.message : "failed to load guides"));
    return () => {
      active = false;
    };
  }, [showId, user?.id]);

  return (
    <div className="mt-4">
      <div className="flex items-center justify-between gap-3">
        <p className="text-sm text-zinc-400">Skip guides built and shared by viewers.</p>
        <Link
          to={`/shows/${encodeURIComponent(showId)}/guides/new`}
          className="shrink-0 rounded-md bg-rose-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-rose-500"
        >
          Create a guide
        </Link>
      </div>

      {err && <p className="mt-6 text-rose-400">{err}</p>}
      {!guides && !err && <p className="mt-6 text-zinc-400">Loading guides…</p>}
      {guides && guides.length === 0 && (
        <p className="mt-8 text-center text-zinc-500">
          No user guides yet — be the first to make one.
        </p>
      )}

      {guides && guides.length > 0 && (
        <ul className="mt-5 space-y-3">
          {guides.map((g) => (
            <li key={g.id} className="rounded-lg border border-zinc-800 bg-zinc-900 p-4">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <Link
                    to={`/shows/${encodeURIComponent(showId)}/guides/${g.id}`}
                    className="font-medium text-zinc-100 hover:text-white"
                  >
                    {g.title}
                  </Link>
                  <p className="mt-0.5 text-xs text-zinc-500">
                    by {g.authorName ?? "a former member"}
                    {g.mine && <span className="ml-1 text-zinc-600">· yours</span>}
                  </p>
                </div>
                <LikeButton
                  guideId={g.id}
                  initialCount={g.likeCount}
                  initialLiked={g.myLike}
                  signedIn={!!user}
                />
              </div>
              {g.description && <p className="mt-2 line-clamp-2 text-sm text-zinc-400">{g.description}</p>}
              {user && g.watchCount > 0 && (
                <WatchProgressBar
                  watched={g.myWatchedWatchCount}
                  total={g.watchCount}
                  label="watched"
                  className="mt-3"
                />
              )}
              <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-xs text-zinc-500">
                <span className="flex items-center gap-1">
                  <i className={`h-2 w-2 rounded-full ${DISPOSITION_META.WATCH.dot}`} /> {g.watchCount} watch
                </span>
                <span className="flex items-center gap-1">
                  <i className={`h-2 w-2 rounded-full ${DISPOSITION_META.OPTIONAL.dot}`} /> {g.optionalCount} optional
                </span>
                <span className="flex items-center gap-1">
                  <i className={`h-2 w-2 rounded-full ${DISPOSITION_META.SKIP.dot}`} /> {g.skipCount} skip
                </span>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

const TONES: Record<string, string> = {
  emerald: "text-emerald-300",
  sky: "text-sky-300",
  rose: "text-rose-300",
};

function Bucket({
  title,
  tone,
  entries,
  watchedIds,
  busyIds,
  onToggle,
}: {
  title: string;
  tone: string;
  entries: SkipGuideEntry[];
  /** When provided (signed-in user), watched episodes are dimmed and checked. */
  watchedIds?: Set<string> | null;
  busyIds?: Set<string>;
  /** When provided, each entry gets a tappable watched toggle (a checklist). */
  onToggle?: (episodeId: string) => void;
}) {
  const interactive = !!onToggle && watchedIds != null;
  return (
    <section>
      <h2 className={`mb-2 text-sm font-semibold ${TONES[tone]}`}>
        {title} <span className="text-zinc-600">· {entries.length}</span>
      </h2>
      {entries.length === 0 ? (
        <p className="text-sm text-zinc-600">None.</p>
      ) : (
        <ul className="space-y-1">
          {entries.map((e) => {
            const watched = watchedIds?.has(e.episodeId) ?? false;
            return (
              <li
                key={e.episodeId}
                className={`flex items-center gap-2 text-sm ${watched ? "text-zinc-500" : "text-zinc-300"}`}
              >
                {interactive && (
                  <WatchedToggle
                    watched={watched}
                    busy={busyIds?.has(e.episodeId) ?? false}
                    onToggle={() => onToggle(e.episodeId)}
                    className="h-4 w-4"
                  />
                )}
                <span>
                  <span className="mr-2 text-zinc-500">
                    S{e.seasonNumber}E{e.episodeNumber}
                  </span>
                  {e.name ?? "Untitled"}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
