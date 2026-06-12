import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  deleteGuide,
  getGuide,
  imageUrl,
  markWatched,
  unmarkWatched,
  type GuideDetail,
  type GuideEntry,
} from "../lib/api";
import { DISPOSITION_META } from "../lib/guides";
import { useAuth } from "../lib/auth";
import { usePageMeta } from "../lib/meta";
import { LikeButton } from "../components/LikeButton";
import { WatchProgressBar } from "../components/WatchProgressBar";
import { WatchedToggle } from "../components/WatchedToggle";

export function GuideDetailPage() {
  const { id = "", guideId = "" } = useParams();
  const navigate = useNavigate();
  const { user } = useAuth();
  const [guide, setGuide] = useState<GuideDetail | null>(null);
  /** Episode IDs the viewer has watched — seeded from the guide's entries. */
  const [watchedIds, setWatchedIds] = useState<Set<string>>(new Set());
  /** Episode IDs with an in-flight watched toggle. */
  const [busyIds, setBusyIds] = useState<Set<string>>(new Set());
  const [err, setErr] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

  usePageMeta(
    guide ? `${guide.title} — ${guide.showName} skip guide` : "Skip guide",
    guide?.description ?? undefined,
  );

  useEffect(() => {
    let active = true;
    setGuide(null);
    setErr(null);
    getGuide(guideId)
      .then((g) => {
        if (!active) return;
        setGuide(g);
        setWatchedIds(new Set(g.entries.filter((e) => e.watched).map((e) => e.episodeId)));
      })
      .catch((e) => active && setErr(e instanceof Error ? e.message : "failed to load guide"));
    return () => {
      active = false;
    };
  }, [guideId, user?.id]);

  // Toggle an episode's watched state from the checklist (optimistic, reverted
  // on failure). Same behavior as the community guide and episode rows.
  async function toggleWatched(episodeId: string) {
    if (busyIds.has(episodeId)) return;
    const next = !watchedIds.has(episodeId);
    const apply = (ids: Set<string>, on: boolean) => {
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

  async function onDelete() {
    if (!confirm("Delete this guide? This can't be undone.")) return;
    setDeleting(true);
    try {
      await deleteGuide(guideId);
      navigate(`/shows/${encodeURIComponent(id)}/skip-guide`, { replace: true });
    } catch (e) {
      setErr(e instanceof Error ? e.message : "could not delete guide");
      setDeleting(false);
    }
  }

  if (err) {
    return (
      <div className="mx-auto max-w-3xl px-4 py-10">
        <p className="text-rose-400">{err}</p>
        <Link to={`/shows/${encodeURIComponent(id)}/skip-guide`} className="mt-3 inline-block text-zinc-400 hover:text-zinc-200">
          ← Back to skip guides
        </Link>
      </div>
    );
  }
  if (!guide) {
    return <div className="mx-auto max-w-3xl px-4 py-10 text-zinc-400">Loading guide…</div>;
  }

  const buckets: { key: string; entries: GuideEntry[] }[] = [
    { key: "WATCH", entries: guide.entries.filter((e) => e.disposition === "WATCH") },
    { key: "OPTIONAL", entries: guide.entries.filter((e) => e.disposition === "OPTIONAL") },
    { key: "SKIP", entries: guide.entries.filter((e) => e.disposition === "SKIP") },
  ];

  return (
    <div className="mx-auto max-w-3xl px-4 py-8">
      <Link
        to={`/shows/${encodeURIComponent(guide.showSlug)}/skip-guide`}
        className="mb-4 inline-block text-sm text-zinc-400 hover:text-zinc-200"
      >
        ← {guide.showName} skip guides
      </Link>

      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex min-w-0 gap-3">
          {imageUrl(guide.posterPath, "w154") && (
            <img
              src={imageUrl(guide.posterPath, "w154")!}
              alt={`${guide.showName} poster`}
              className="h-20 w-14 shrink-0 rounded object-cover"
            />
          )}
          <div className="min-w-0">
            <h1 className="text-2xl font-bold">{guide.title}</h1>
            <p className="mt-1 text-sm text-zinc-500">
              <span className="text-zinc-400">{guide.showName}</span> · by{" "}
              {guide.authorName ?? "a former member"}
              {!guide.isPublished && (
                <span className="ml-2 rounded bg-amber-500/15 px-1.5 py-0.5 text-xs text-amber-300">Draft</span>
              )}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <LikeButton
            guideId={guide.id}
            initialCount={guide.likeCount}
            initialLiked={guide.myLike}
            signedIn={!!user}
          />
          {guide.mine && (
            <>
              <Link
                to={`/shows/${encodeURIComponent(guide.showSlug)}/guides/${guide.id}/edit`}
                className="rounded-md border border-zinc-700 px-2.5 py-1 text-sm text-zinc-300 hover:bg-zinc-800"
              >
                Edit
              </Link>
              <button
                onClick={() => void onDelete()}
                disabled={deleting}
                className="rounded-md border border-rose-900 px-2.5 py-1 text-sm text-rose-300 hover:bg-rose-950/40 disabled:opacity-50"
              >
                Delete
              </button>
            </>
          )}
        </div>
      </div>

      {guide.description && <p className="mt-4 text-zinc-300">{guide.description}</p>}

      {/* Watch-list progress for signed-in viewers. */}
      {user && (() => {
        const watchEntries = guide.entries.filter((e) => e.disposition === "WATCH");
        if (watchEntries.length === 0) return null;
        return (
          <WatchProgressBar
            watched={watchEntries.filter((e) => watchedIds.has(e.episodeId)).length}
            total={watchEntries.length}
            label="watch-list episodes"
            className="mt-4"
          />
        );
      })()}

      <div className="mt-6 space-y-6">
        {buckets.map(({ key, entries }) => {
          const meta = DISPOSITION_META[key as keyof typeof DISPOSITION_META];
          // Watch and Optional are checklists for signed-in viewers; Skip stays
          // a plain list (checking off skipped episodes has no meaning).
          const interactive = !!user && key !== "SKIP";
          return (
            <section key={key}>
              <h2 className={`mb-2 flex items-center gap-2 text-sm font-semibold ${meta.text}`}>
                <i className={`h-2 w-2 rounded-full ${meta.dot}`} />
                {meta.label} <span className="text-zinc-600">· {entries.length}</span>
              </h2>
              {entries.length === 0 ? (
                <p className="text-sm text-zinc-600">None.</p>
              ) : (
                <ul className="space-y-1">
                  {entries.map((e) => {
                    const watched = watchedIds.has(e.episodeId);
                    return (
                      <li
                        key={e.episodeId}
                        className={`flex items-center gap-2 text-sm ${watched ? "text-zinc-500" : "text-zinc-300"}`}
                      >
                        {interactive && (
                          <WatchedToggle
                            watched={watched}
                            busy={busyIds.has(e.episodeId)}
                            onToggle={() => void toggleWatched(e.episodeId)}
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
        })}
      </div>
    </div>
  );
}
