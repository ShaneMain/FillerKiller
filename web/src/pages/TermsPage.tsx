import { usePageMeta } from "../lib/meta";

export function TermsPage() {
  usePageMeta("Terms", "The terms of use for FillerKiller.");

  return (
    <div className="mx-auto max-w-3xl px-4 py-10">
      <h1 className="text-3xl font-bold">Terms of use</h1>
      <p className="mt-2 text-sm text-zinc-500">Last updated June 2026.</p>

      <div className="mt-6 space-y-5 text-zinc-300">
        <section>
          <h2 className="text-lg font-semibold text-zinc-100">The service</h2>
          <p className="mt-2">
            FillerKiller is provided free of charge, as-is and without warranty of any kind. We
            may change, suspend, or discontinue any part of it at any time.
          </p>
        </section>

        <section>
          <h2 className="text-lg font-semibold text-zinc-100">Your votes</h2>
          <p className="mt-2">
            Be honest: vote based on your genuine opinion of an episode. Don't attempt to
            manipulate results through automated voting, multiple accounts, or coordinated
            brigading. We may rate-limit, reset, or remove votes and accounts that abuse the
            service.
          </p>
        </section>

        <section>
          <h2 className="text-lg font-semibold text-zinc-100">Content &amp; attribution</h2>
          <p className="mt-2">
            Show metadata and images are provided by TMDB and remain subject to TMDB's terms.
            FillerKiller uses the TMDB API but is not endorsed or certified by TMDB.
          </p>
        </section>

        <section>
          <h2 className="text-lg font-semibold text-zinc-100">Liability</h2>
          <p className="mt-2">
            To the fullest extent permitted by law, FillerKiller and its maintainers are not
            liable for any damages arising from your use of the service.
          </p>
        </section>
      </div>
    </div>
  );
}
