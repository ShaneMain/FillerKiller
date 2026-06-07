import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { getShow, getSkipGuide, type SkipGuide, type SkipGuideEntry } from "../lib/api";

type Contested = "canon" | "filler";

export function SkipGuidePage() {
  const { id = "" } = useParams();
  const [name, setName] = useState<string | null>(null);
  const [guide, setGuide] = useState<SkipGuide | null>(null);
  const [contested, setContested] = useState<Contested>("canon");
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    getShow(id).then((s) => active && setName(s.name)).catch(() => {});
    return () => {
      active = false;
    };
  }, [id]);

  useEffect(() => {
    let active = true;
    setGuide(null);
    setErr(null);
    getSkipGuide(id, contested)
      .then((g) => active && setGuide(g))
      .catch((e) => active && setErr(e instanceof Error ? e.message : "failed to load skip guide"));
    return () => {
      active = false;
    };
  }, [id, contested]);

  return (
    <div className="mx-auto max-w-3xl px-4 py-8">
      <Link to={`/shows/${encodeURIComponent(id)}`} className="mb-4 inline-block text-sm text-zinc-400 hover:text-zinc-200">
        ← {name ?? "Show"}
      </Link>
      <h1 className="text-2xl font-bold">Skip guide</h1>
      <p className="mt-1 text-sm text-zinc-400">
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
