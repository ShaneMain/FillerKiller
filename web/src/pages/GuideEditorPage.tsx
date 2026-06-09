import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  createGuide,
  getEpisodes,
  getGuide,
  getShow,
  updateGuide,
  type Disposition,
  type Episode,
} from "../lib/api";
import { DISPOSITIONS, DISPOSITION_META, statusToDisposition } from "../lib/guides";
import { useAuth } from "../lib/auth";
import { usePageMeta } from "../lib/meta";

const MAX_TITLE = 80;
const MAX_DESCRIPTION = 500;

export function GuideEditorPage() {
  const { id = "", guideId } = useParams();
  const editing = !!guideId;
  const navigate = useNavigate();
  const { user, loading: authLoading } = useAuth();
  usePageMeta(editing ? "Edit skip guide" : "Create a skip guide");

  const [showName, setShowName] = useState<string>("");
  const [episodes, setEpisodes] = useState<Episode[] | null>(null);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [dispositions, setDispositions] = useState<Record<string, Disposition>>({});
  const [err, setErr] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!user) return;
    let active = true;
    setErr(null);
    Promise.all([getShow(id), getEpisodes(id), editing ? getGuide(guideId!) : Promise.resolve(null)])
      .then(([show, eps, guide]) => {
        if (!active) return;
        setShowName(show.name);
        setEpisodes(eps.episodes);
        const initial: Record<string, Disposition> = {};
        for (const ep of eps.episodes) initial[ep.id] = statusToDisposition(ep.score.status);
        if (guide) {
          setTitle(guide.title);
          setDescription(guide.description ?? "");
          for (const e of guide.entries) initial[e.episodeId] = e.disposition;
        }
        setDispositions(initial);
      })
      .catch((e) => active && setErr(e instanceof Error ? e.message : "failed to load"));
    return () => {
      active = false;
    };
  }, [id, guideId, editing, user]);

  const bySeason = useMemo(() => {
    const groups = new Map<number, Episode[]>();
    for (const ep of episodes ?? []) {
      const arr = groups.get(ep.seasonNumber) ?? [];
      arr.push(ep);
      groups.set(ep.seasonNumber, arr);
    }
    return [...groups.entries()].sort((a, b) => a[0] - b[0]);
  }, [episodes]);

  async function save(published: boolean) {
    if (!episodes) return;
    if (!title.trim()) {
      setErr("Give your guide a title.");
      return;
    }
    setSaving(true);
    setErr(null);
    const input = {
      title: title.trim(),
      description: description.trim() || null,
      entries: episodes.map((ep) => ({ episodeId: ep.id, disposition: dispositions[ep.id] ?? "WATCH" })),
      published,
    };
    try {
      if (editing) {
        await updateGuide(guideId!, input);
        navigate(`/shows/${encodeURIComponent(id)}/guides/${guideId}`);
      } else {
        const { id: newId } = await createGuide(id, input);
        navigate(`/shows/${encodeURIComponent(id)}/guides/${newId}`);
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : "could not save guide");
      setSaving(false);
    }
  }

  if (authLoading) {
    return <div className="mx-auto max-w-3xl px-4 py-10 text-zinc-400">Loading…</div>;
  }
  if (!user) {
    return (
      <div className="mx-auto max-w-sm px-4 py-12">
        <h1 className="text-2xl font-bold">Create a skip guide</h1>
        <p className="mt-3 text-zinc-400">
          <Link to="/login" className="text-rose-400 hover:text-rose-300">Sign in</Link> to build
          and share your own skip guide.
        </p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-3xl px-4 py-8">
      <Link
        to={`/shows/${encodeURIComponent(id)}/skip-guide`}
        className="mb-4 inline-block text-sm text-zinc-400 hover:text-zinc-200"
      >
        ← {showName || "Show"} skip guides
      </Link>
      <h1 className="text-2xl font-bold">{editing ? "Edit your skip guide" : "Create a skip guide"}</h1>
      <p className="mt-1 text-sm text-zinc-400">
        Start from the community verdict and adjust each episode. Save a draft or publish it for
        others to find and like.
      </p>

      <div className="mt-6 space-y-4">
        <div>
          <label htmlFor="g-title" className="block text-sm font-medium text-zinc-300">Title</label>
          <input
            id="g-title"
            value={title}
            maxLength={MAX_TITLE}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="e.g. Story-only watch order"
            className="mt-1 w-full rounded-md border border-zinc-700 bg-zinc-900 px-3 py-2 text-zinc-100 placeholder-zinc-500 outline-none focus:border-zinc-500"
          />
        </div>
        <div>
          <label htmlFor="g-desc" className="block text-sm font-medium text-zinc-300">
            Description <span className="text-zinc-500">(optional)</span>
          </label>
          <textarea
            id="g-desc"
            value={description}
            maxLength={MAX_DESCRIPTION}
            onChange={(e) => setDescription(e.target.value)}
            rows={2}
            placeholder="What's the idea behind this guide?"
            className="mt-1 w-full rounded-md border border-zinc-700 bg-zinc-900 px-3 py-2 text-zinc-100 placeholder-zinc-500 outline-none focus:border-zinc-500"
          />
        </div>
      </div>

      {err && <p className="mt-4 text-sm text-rose-400">{err}</p>}

      {!episodes && <p className="mt-6 text-zinc-400">Loading episodes…</p>}
      {episodes && episodes.length === 0 && (
        <p className="mt-6 text-zinc-500">This show has no episodes to build a guide from.</p>
      )}

      {episodes && episodes.length > 0 && (
        <div className="mt-6 space-y-6">
          {bySeason.map(([season, eps]) => (
            <section key={season}>
              <h2 className="mb-2 text-sm font-semibold text-zinc-200">
                {season === 0 ? "Specials" : `Season ${season}`}
              </h2>
              <div className="divide-y divide-zinc-800 rounded-lg border border-zinc-800">
                {eps.map((ep) => (
                  <div key={ep.id} className="flex flex-col gap-2 p-3 sm:flex-row sm:items-center sm:justify-between">
                    <div className="min-w-0 text-sm">
                      <span className="mr-2 text-zinc-500">E{ep.episodeNumber}</span>
                      <span className="text-zinc-100">{ep.name ?? "Untitled"}</span>
                    </div>
                    <div className="grid shrink-0 grid-cols-3 gap-1 sm:flex">
                      {DISPOSITIONS.map((d) => {
                        const active = (dispositions[ep.id] ?? "WATCH") === d;
                        return (
                          <button
                            key={d}
                            onClick={() => setDispositions((m) => ({ ...m, [ep.id]: d }))}
                            aria-pressed={active}
                            className={`rounded-md px-2.5 py-1 text-xs font-medium ring-1 ring-inset ring-zinc-700 ${
                              active ? DISPOSITION_META[d].activeBtn : "text-zinc-300 hover:bg-zinc-800"
                            }`}
                          >
                            {DISPOSITION_META[d].label}
                          </button>
                        );
                      })}
                    </div>
                  </div>
                ))}
              </div>
            </section>
          ))}

          <div className="sticky bottom-0 flex gap-2 border-t border-zinc-800 bg-zinc-950/90 py-3 backdrop-blur">
            <button
              onClick={() => void save(true)}
              disabled={saving}
              className="rounded-md bg-rose-600 px-4 py-2 font-medium text-white hover:bg-rose-500 disabled:opacity-50"
            >
              {saving ? "Saving…" : "Publish"}
            </button>
            <button
              onClick={() => void save(false)}
              disabled={saving}
              className="rounded-md border border-zinc-700 px-4 py-2 font-medium text-zinc-200 hover:bg-zinc-800 disabled:opacity-50"
            >
              Save draft
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
