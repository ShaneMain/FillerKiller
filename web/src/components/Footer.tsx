import { Link } from "react-router-dom";
import { Wordmark } from "./Wordmark";

export function Footer() {
  return (
    <footer className="mt-auto border-t border-zinc-800">
      <div className="mx-auto flex max-w-3xl flex-col gap-3 px-4 py-6 text-sm text-zinc-500 sm:flex-row sm:items-center sm:justify-between">
        <p>
          <Wordmark /> · crowd-sourced episode guide
        </p>
        <nav className="flex flex-wrap gap-x-4 gap-y-1">
          <Link to="/about" className="hover:text-zinc-300">About</Link>
          <Link to="/support" className="hover:text-zinc-300">Support</Link>
          <Link to="/privacy" className="hover:text-zinc-300">Privacy</Link>
          <Link to="/terms" className="hover:text-zinc-300">Terms</Link>
        </nav>
      </div>
    </footer>
  );
}
