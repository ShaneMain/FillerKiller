import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { deleteAccount } from "../lib/api";
import { useAuth } from "../lib/auth";
import { usePageMeta } from "../lib/meta";

export function AccountPage() {
  usePageMeta("Account", "Manage your FillerKiller account.");
  const { user, loading, signOut, refresh } = useAuth();
  const navigate = useNavigate();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function onDelete() {
    setBusy(true);
    setErr(null);
    try {
      await deleteAccount();
      // The server clears the session cookie; re-sync the client and go home.
      await refresh();
      navigate("/", { replace: true });
    } catch (e) {
      setErr(e instanceof Error ? e.message : "could not delete account");
      setBusy(false);
    }
  }

  if (loading) {
    return <div className="mx-auto max-w-3xl px-4 py-10 text-zinc-400">Loading…</div>;
  }

  if (!user) {
    return (
      <div className="mx-auto max-w-sm px-4 py-12">
        <h1 className="text-2xl font-bold">Account</h1>
        <p className="mt-3 text-zinc-400">
          You're not signed in.{" "}
          <Link to="/login" className="text-rose-400 hover:text-rose-300">Sign in</Link> to
          manage your account.
        </p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-2xl px-4 py-10">
      <h1 className="text-3xl font-bold">Account</h1>

      <dl className="mt-6 divide-y divide-zinc-800 rounded-lg border border-zinc-800">
        <div className="flex justify-between gap-4 px-4 py-3">
          <dt className="text-zinc-400">Name</dt>
          <dd className="text-zinc-100">{user.displayName ?? "—"}</dd>
        </div>
        <div className="flex justify-between gap-4 px-4 py-3">
          <dt className="text-zinc-400">Email</dt>
          <dd className="truncate text-zinc-100">{user.email}</dd>
        </div>
      </dl>

      <button
        onClick={() => void signOut()}
        className="mt-4 rounded-md border border-zinc-700 px-3 py-1.5 text-sm text-zinc-300 hover:bg-zinc-800"
      >
        Sign out
      </button>

      <section className="mt-10 rounded-lg border border-rose-900/60 bg-rose-950/20 p-4">
        <h2 className="text-lg font-semibold text-rose-300">Delete account</h2>
        <p className="mt-1 text-sm text-zinc-400">
          Permanently deletes your account, personal info (your name and email), and any skip
          guides you've created. Your past votes are kept anonymously as part of the community
          totals — they're no longer linked to you. This cannot be undone.
        </p>

        {err && <p className="mt-3 text-sm text-rose-400">{err}</p>}

        {!confirming ? (
          <button
            onClick={() => setConfirming(true)}
            className="mt-3 rounded-md bg-rose-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-rose-600"
          >
            Delete my account
          </button>
        ) : (
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <span className="text-sm text-zinc-300">Are you sure?</span>
            <button
              onClick={() => void onDelete()}
              disabled={busy}
              className="rounded-md bg-rose-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-rose-600 disabled:opacity-50"
            >
              {busy ? "Deleting…" : "Yes, delete my account"}
            </button>
            <button
              onClick={() => setConfirming(false)}
              disabled={busy}
              className="rounded-md border border-zinc-700 px-3 py-1.5 text-sm text-zinc-300 hover:bg-zinc-800 disabled:opacity-50"
            >
              Cancel
            </button>
          </div>
        )}
      </section>
    </div>
  );
}
