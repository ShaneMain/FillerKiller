import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { deleteGuide, getGuide, type GuideDetail, type GuideEntry } from "../lib/api";
import { DISPOSITION_META } from "../lib/guides";
import { useAuth } from "../lib/auth";
import { usePageMeta } from "../lib/meta";
import { LikeButton } from "../components/LikeButton";

export function GuideDetailPage() {
  const { id = "", guideId = "" } = useParams();
  const navigate = useNavigate();
  const { user } = useAuth();
  const [guide, setGuide] = useState<GuideDetail | null>(null);
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
      .then((g) => active && setGuide(g))
      .catch((e) => active && setErr(e instanceof Error ? e.message : "failed to load guide"));
    return () => {
      active = false;
    };
  }, [guideId, user?.id]);

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
        <div className="min-w-0">
          <h1 className="text-2xl font-bold">{guide.title}</h1>
          <p className="mt-1 text-sm text-zinc-500">
            by {guide.authorName ?? "a former member"}
            {!guide.isPublished && (
              <span className="ml-2 rounded bg-amber-500/15 px-1.5 py-0.5 text-xs text-amber-300">Draft</span>
            )}
          </p>
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

      <div className="mt-6 space-y-6">
        {buckets.map(({ key, entries }) => {
          const meta = DISPOSITION_META[key as keyof typeof DISPOSITION_META];
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
                  {entries.map((e) => (
                    <li key={e.episodeId} className="text-sm text-zinc-300">
                      <span className="mr-2 text-zinc-500">
                        S{e.seasonNumber}E{e.episodeNumber}
                      </span>
                      {e.name ?? "Untitled"}
                    </li>
                  ))}
                </ul>
              )}
            </section>
          );
        })}
      </div>
    </div>
  );
}
