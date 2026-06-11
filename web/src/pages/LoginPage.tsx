import { Link, useSearchParams } from "react-router-dom";
import { loginUrl } from "../lib/api";
import { isSafeNext } from "../lib/loginNav";
import { useAuth } from "../lib/auth";
import { usePageMeta } from "../lib/meta";
import { Wordmark } from "../components/Wordmark";

export function LoginPage() {
  usePageMeta("Sign in", "Sign in to FillerKiller to vote on episodes.");
  const { user } = useAuth();
  const [params] = useSearchParams();
  // Drop an off-site `next` before it reaches the login URL (the server also
  // re-validates it; this stops the client forwarding a hostile value at all).
  const rawNext = params.get("next");
  const next = rawNext && isSafeNext(rawNext) ? rawNext : undefined;

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
            className="flex w-full items-center justify-center gap-2.5 rounded-md border border-zinc-700 bg-zinc-900 px-4 py-2.5 font-medium text-zinc-100 hover:bg-zinc-800"
          >
            <GitHubMark />
            Continue with GitHub
          </a>
          <a
            href={loginUrl("google", next)}
            className="flex w-full items-center justify-center gap-2.5 rounded-md border border-zinc-700 bg-zinc-900 px-4 py-2.5 font-medium text-zinc-100 hover:bg-zinc-800"
          >
            <GoogleMark />
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

/** The GitHub "invertocat" mark, monochrome. */
function GitHubMark() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" className="h-5 w-5 fill-current">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}

/** The multicolor Google "G" mark. */
function GoogleMark() {
  return (
    <svg viewBox="0 0 18 18" aria-hidden="true" className="h-5 w-5">
      <path fill="#4285F4" d="M17.64 9.2c0-.64-.06-1.25-.16-1.84H9v3.48h4.84a4.14 4.14 0 0 1-1.8 2.72v2.26h2.92c1.7-1.57 2.68-3.88 2.68-6.62Z" />
      <path fill="#34A853" d="M9 18c2.43 0 4.47-.8 5.96-2.18l-2.92-2.26c-.8.54-1.84.86-3.04.86-2.34 0-4.32-1.58-5.03-3.7H.96v2.33A9 9 0 0 0 9 18Z" />
      <path fill="#FBBC05" d="M3.97 10.72a5.4 5.4 0 0 1 0-3.44V4.95H.96a9 9 0 0 0 0 8.1l3.01-2.33Z" />
      <path fill="#EA4335" d="M9 3.58c1.32 0 2.5.45 3.44 1.35l2.58-2.59A8.97 8.97 0 0 0 9 0 9 9 0 0 0 .96 4.95l3.01 2.33C4.68 5.16 6.66 3.58 9 3.58Z" />
    </svg>
  );
}
