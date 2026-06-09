import { Link } from "react-router-dom";
import { usePageMeta } from "../lib/meta";

export function PrivacyPage() {
  usePageMeta(
    "Privacy",
    "What FillerKiller stores about you, why, and how to delete your account.",
  );

  return (
    <div className="mx-auto max-w-3xl px-4 py-10">
      <h1 className="text-3xl font-bold">Privacy</h1>
      <p className="mt-2 text-sm text-zinc-500">Last updated June 2026.</p>

      <div className="mt-6 space-y-5 text-zinc-300">
        <section>
          <h2 className="text-lg font-semibold text-zinc-100">What we store</h2>
          <ul className="mt-2 list-disc space-y-1 pl-5">
            <li>
              Your email address and display name, received from your chosen sign-in provider
              (Google or GitHub) when you log in. We use the email only to identify your
              account and enforce one vote per person per episode.
            </li>
            <li>The episode votes you cast.</li>
          </ul>
        </section>

        <section>
          <h2 className="text-lg font-semibold text-zinc-100">What we don't do</h2>
          <p className="mt-2">
            We don't post anything on your behalf, we don't send you marketing email, and we
            don't sell or share your personal data. Your individual votes are shown only as
            anonymous aggregate counts — no one can see how a specific person voted.
          </p>
        </section>

        <section>
          <h2 className="text-lg font-semibold text-zinc-100">Sign-in providers</h2>
          <p className="mt-2">
            Authentication is handled by Google and GitHub via OAuth. We never see your
            password. Their handling of your data is governed by their own privacy policies.
          </p>
        </section>

        <section>
          <h2 className="text-lg font-semibold text-zinc-100">Cookies</h2>
          <p className="mt-2">
            We set a single httpOnly session cookie to keep you signed in, and a short-lived
            cookie during the sign-in round-trip. We use no third-party tracking or
            advertising cookies.
          </p>
        </section>

        <section>
          <h2 className="text-lg font-semibold text-zinc-100">Deleting your data</h2>
          <p className="mt-2">
            You can permanently delete your account at any time from your{" "}
            <Link to="/account" className="text-rose-400 hover:text-rose-300">account page</Link>.
            This erases your personal data — your email and display name — and any skip guides
            you've created. Your past votes are retained as anonymous aggregate counts that are
            no longer linked to you, so the community verdicts stay intact. Deletion is immediate
            and irreversible.
          </p>
        </section>

        <section>
          <h2 className="text-lg font-semibold text-zinc-100">Contact</h2>
          <p className="mt-2">
            Questions about privacy? Open an issue on{" "}
            <a href="https://github.com/ShaneMain/FillerKiller/issues" target="_blank" rel="noreferrer" className="underline hover:text-zinc-100">
              GitHub
            </a>
            .
          </p>
        </section>
      </div>
    </div>
  );
}
