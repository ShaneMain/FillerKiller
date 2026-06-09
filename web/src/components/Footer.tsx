import { Link } from "react-router-dom";

export function Footer() {
  return (
    <footer className="mt-auto border-t border-zinc-800">
      <div className="mx-auto flex max-w-3xl flex-col gap-3 px-4 py-6 text-sm text-zinc-500 sm:flex-row sm:items-center sm:justify-between">
        <p>
          Filler<span className="text-rose-500">Killer</span> · crowd-sourced episode guide
        </p>
        <nav className="flex flex-wrap gap-x-4 gap-y-1">
          <Link to="/about" className="hover:text-zinc-300">About</Link>
          <Link to="/support" className="hover:text-zinc-300">Support</Link>
          <Link to="/privacy" className="hover:text-zinc-300">Privacy</Link>
          <Link to="/terms" className="hover:text-zinc-300">Terms</Link>
          <a
            href="https://github.com/ShaneMain/FillerKiller"
            target="_blank"
            rel="noreferrer"
            className="hover:text-zinc-300"
          >
            Source
          </a>
        </nav>
      </div>
      <p className="mx-auto max-w-3xl px-4 pb-6 text-xs text-zinc-600">
        TV metadata and images courtesy of{" "}
        <a href="https://www.themoviedb.org/" target="_blank" rel="noreferrer" className="underline hover:text-zinc-400">
          TMDB
        </a>
        . This product uses the TMDB API but is not endorsed or certified by TMDB.
      </p>
    </footer>
  );
}
