import { Link } from "react-router-dom";
import { usePageMeta } from "../lib/meta";

export function AboutPage() {
  usePageMeta(
    "About",
    "What FillerKiller is, how episode votes work, and where the data comes from.",
  );

  return (
    <div className="mx-auto max-w-3xl px-4 py-10">
      <h1 className="text-3xl font-bold">About FillerKiller</h1>

      <div className="mt-6 space-y-4 text-zinc-300">
        <p>
          FillerKiller is a crowd-sourced guide to which TV episodes are actually worth your
          time. For every episode, signed-in viewers cast one vote —{" "}
          <span className="font-medium text-rose-400">Filler</span>,{" "}
          <span className="font-medium text-sky-400">Worth It</span>, or{" "}
          <span className="font-medium text-emerald-400">Canon</span> — and the community
          verdict shows at a glance what to skip, what's optional, and what's essential.
        </p>
        <p>
          The <Link to="/" className="text-rose-400 hover:text-rose-300">skip guide</Link>{" "}
          turns those votes into a binge-ready watch order. Episodes without enough votes, or
          where the community is split, default to "watch" so you never miss something canon.
        </p>
        <h2 className="pt-2 text-lg font-semibold text-zinc-100">Where the data comes from</h2>
        <p>
          Show, season, and episode metadata and images come from{" "}
          <a href="https://www.themoviedb.org/" target="_blank" rel="noreferrer" className="underline hover:text-zinc-100">
            TMDB
          </a>
          . The votes and the resulting verdicts are contributed by FillerKiller's community.
        </p>
        <div className="mt-2 flex flex-col gap-3 rounded-lg border border-zinc-800 bg-zinc-900/50 p-4 sm:flex-row sm:items-center sm:gap-4">
          <a
            href="https://www.themoviedb.org/"
            target="_blank"
            rel="noreferrer"
            className="shrink-0"
          >
            <img src="/tmdb.svg" alt="TMDB" className="h-6 w-auto" />
          </a>
          <p className="text-sm text-zinc-400">
            This product uses the TMDB API but is not endorsed or certified by TMDB.
          </p>
        </div>
        <h2 className="pt-2 text-lg font-semibold text-zinc-100">Open source</h2>
        <p>
          FillerKiller is open source under the GPL-3.0 license. The code lives on{" "}
          <a href="https://github.com/ShaneMain/FillerKiller" target="_blank" rel="noreferrer" className="underline hover:text-zinc-100">
            GitHub
          </a>
          .
        </p>
      </div>
    </div>
  );
}
