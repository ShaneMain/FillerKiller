import { Link, useSearchParams } from "react-router-dom";
import { loginUrl } from "../lib/api";
import { useAuth } from "../lib/auth";
import { usePageMeta } from "../lib/meta";
import { Wordmark } from "../components/Wordmark";

export function LoginPage() {
  usePageMeta("Sign in", "Sign in to FillerKiller to vote on episodes.");
  const { user } = useAuth();
  const [params] = useSearchParams();
  const next = params.get("next") ?? undefined;

  return (
    <div className="mx-auto max-w-sm px-4 py-12">
      <Link to="/" className="text-sm text-zinc-400 hover:text-zinc-200">
        ← Back
      </Link>

      <h1 className="mt-5 text-2xl font-bold">
        Sign in to <Wordmark />
      </h1>
      <p className="mt-2 text-sm text-zinc-400">
        Sign in to vote on episodes. We only use your account to tie votes to a person —
        one vote per person per episode. No posting, no email.
      </p>

      {user ? (
        <p className="mt-8 rounded-md border border-zinc-800 bg-zinc-900 px-3 py-3 text-sm text-zinc-300">
          You're signed in as{" "}
          <span className="text-zinc-100">{user.displayName ?? user.email}</span>.{" "}
          <Link to="/" className="text-rose-400 hover:text-rose-300">
            Go home →
          </Link>
        </p>
      ) : (
        <div className="mt-8 space-y-3">
          <a
            href={loginUrl("github", next)}
            className="flex w-full items-center justify-center rounded-md border border-zinc-700 bg-zinc-900 px-4 py-2.5 font-medium text-zinc-100 hover:bg-zinc-800"
          >
            Continue with GitHub
          </a>
          <a
            href={loginUrl("google", next)}
            className="flex w-full items-center justify-center rounded-md border border-zinc-700 bg-zinc-900 px-4 py-2.5 font-medium text-zinc-100 hover:bg-zinc-800"
          >
            Continue with Google
          </a>
          <p className="pt-2 text-center text-xs text-zinc-600">
            You'll be returned to where you left off after signing in.
          </p>
        </div>
      )}
    </div>
  );
}
