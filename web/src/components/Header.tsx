import { Link } from "react-router-dom";
import { useAuth } from "../lib/auth";
import { useLoginHref } from "../lib/loginNav";
import { Wordmark } from "./Wordmark";

export function Header() {
  const { user, loading, signOut } = useAuth();
  const loginHref = useLoginHref();

  return (
    <header className="border-b border-zinc-800 bg-zinc-950/80 backdrop-blur sticky top-0 z-10">
      <div className="mx-auto max-w-3xl flex items-center justify-between px-4 py-3">
        <Link to="/" className="text-lg font-bold tracking-tight">
          <Wordmark />
        </Link>
        <div className="text-sm">
          {loading ? (
            <span className="text-zinc-500">…</span>
          ) : user ? (
            <div className="flex items-center gap-3">
              <Link to="/account" className="text-zinc-400 hover:text-zinc-200">
                {user.displayName ?? user.email}
              </Link>
              <button
                onClick={() => void signOut()}
                className="rounded-md border border-zinc-700 px-2 py-1 text-zinc-300 hover:bg-zinc-800"
              >
                Sign out
              </button>
            </div>
          ) : (
            <Link
              to={loginHref}
              className="rounded-md border border-zinc-700 px-3 py-1 text-zinc-200 hover:bg-zinc-800"
            >
              Sign in
            </Link>
          )}
        </div>
      </div>
    </header>
  );
}
