import { useEffect, useState, type ReactNode } from "react";
import { Link, useParams } from "react-router-dom";
import {
  getShow,
  getSkipGuide,
  listGuides,
  type GuideSummary,
  type SkipGuide,
  type SkipGuideEntry,
} from "../lib/api";
import { DISPOSITION_META } from "../lib/guides";
import { useAuth } from "../lib/auth";
import { usePageMeta } from "../lib/meta";
import { LikeButton } from "../components/LikeButton";

type Contested = "canon" | "filler";
type Tab = "community" | "user";

export function SkipGuidePage() {
  const { id = "" } = useParams();
  const [name, setName] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("community");
  usePageMeta(
    name ? `Skip guide — ${name}` : "Skip guide",
    name ? `Crowd-sourced binge-ready watch order for ${name}.` : undefined,
  );

  useEffect(() => {
    let active = true;
    getShow(id).then((s) => active && setName(s.name)).catch(() => {});
    return () => {
      active = false;
    };
  }, [id]);

  return (
    <div className="mx-auto max-w-3xl px-4 py-8">
      <Link to={`/shows/${encodeURIComponent(id)}`} className="mb-4 inline-block text-sm text-zinc-400 hover:text-zinc-200">
        ← {name ?? "Show"}
      </Link>
      <h1 className="text-2xl font-bold">Skip guide</h1>

      <div className="mt-4 flex gap-1 border-b border-zinc-800" role="tablist">
        <TabButton active={tab === "community"} onClick={() => setTab("community")}>
          Community verdict
        </TabButton>
        <TabButton active={tab === "user"} onClick={() => setTab("user")}>
          User guides
        </TabButton>
      </div>

      {tab === "community" ? <CommunityGuide showId={id} /> : <UserGuides showId={id} />}
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

function CommunityGuide({ showId }: { showId: string }) {
  const [guide, setGuide] = useState<SkipGuide | null>(null);
  const [contested, setContested] = useState<Contested>("canon");
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setGuide(null);
    setErr(null);
    getSkipGuide(showId, contested)
      .then((g) => active && setGuide(g))
      .catch((e) => active && setErr(e instanceof Error ? e.message : "failed to load skip guide"));
    return () => {
      active = false;
    };
  }, [showId, contested]);

  return (
    <div>
      <p className="mt-4 text-sm text-zinc-400">
        Crowd-sourced watch order. Unsure episodes (too few or split votes) are{" "}
        <button
          onClick={() => setContested((c) => (c === "canon" ? "filler" : "canon"))}
          className="rounded border border-zinc-700 px-1.5 py-0.5 text-zinc-200 hover:bg-zinc-800"
        >
          {contested === "canon" ? "kept (safe)" : "skipped (aggressive)"}
        </button>
        .
      </p>

      {err && <p className="mt-6 text-rose-400">{err}</p>}
      {!guide && !err && <p className="mt-6 text-zinc-400">Building guide…</p>}

      {guide && (
        <div className="mt-6 space-y-6">
          <Bucket title="Watch" tone="emerald" entries={guide.watch} />
          <Bucket title="Optional — worth it" tone="sky" entries={guide.optional} />
          <Bucket title="Skip" tone="rose" entries={guide.skipped} />
        </div>
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

function Bucket({ title, tone, entries }: { title: string; tone: string; entries: SkipGuideEntry[] }) {
  return (
    <section>
      <h2 className={`mb-2 text-sm font-semibold ${TONES[tone]}`}>
        {title} <span className="text-zinc-600">· {entries.length}</span>
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
}
