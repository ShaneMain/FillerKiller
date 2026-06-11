import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import {
  deleteAccount,
  deleteGuide,
  listMyGuides,
  updateScreenName,
  type MyGuide,
} from "../lib/api";
import { useAuth } from "../lib/auth";
import { useLoginHref } from "../lib/loginNav";
import { usePageMeta } from "../lib/meta";

export function AccountPage() {
  usePageMeta("Account", "Manage your FillerKiller account.");
  const { user, loading, signOut, refresh } = useAuth();
  const loginHref = useLoginHref();
  const navigate = useNavigate();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // Display-name editor.
  const [name, setName] = useState(user?.displayName ?? "");
  const [savingName, setSavingName] = useState(false);
  const [nameErr, setNameErr] = useState<string | null>(null);
  const [nameSaved, setNameSaved] = useState(false);

  // The user's own skip guides.
  const [guides, setGuides] = useState<MyGuide[] | null>(null);
  const [guidesErr, setGuidesErr] = useState<string | null>(null);

  // Keep the editor in sync when the signed-in user (re)loads.
  useEffect(() => {
    setName(user?.displayName ?? "");
  }, [user?.displayName]);

  // Load the user's guides on mount / sign-in change.
  const userId = user?.id;
  useEffect(() => {
    if (!userId) return;
    let active = true;
    setGuides(null);
    setGuidesErr(null);
    listMyGuides()
      .then((g) => active && setGuides(g))
      .catch((e) => {
        if (!active) return;
        setGuidesErr(e instanceof Error ? e.message : "failed to load your guides");
        setGuides([]);
      });
    return () => {
      active = false;
    };
  }, [userId]);

  async function onSaveName() {
    setSavingName(true);
    setNameErr(null);
    setNameSaved(false);
    try {
      await updateScreenName(name.trim() || null);
      await refresh();
      setNameSaved(true);
    } catch (e) {
      setNameErr(e instanceof Error ? e.message : "could not save name");
    } finally {
      setSavingName(false);
    }
  }

  async function onDeleteGuide(g: MyGuide) {
    if (!confirm(`Delete "${g.title}"? This cannot be undone.`)) return;
    try {
      await deleteGuide(g.id);
      setGuides((gs) => (gs ?? []).filter((x) => x.id !== g.id));
    } catch (e) {
      setGuidesErr(e instanceof Error ? e.message : "could not delete guide");
    }
  }

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
          <Link to={loginHref} className="text-rose-400 hover:text-rose-300">Sign in</Link> to
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

      <section className="mt-10">
        <h2 className="text-lg font-semibold">Display name</h2>
        <p className="mt-1 text-sm text-zinc-400">
          Shown instead of the name from your sign-in account. Leave blank to use your account name.
        </p>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <input
            type="text"
            value={name}
            maxLength={40}
            onChange={(e) => {
              setName(e.target.value);
              setNameSaved(false);
              setNameErr(null);
            }}
            placeholder="Your display name"
            className="w-full max-w-xs rounded-md border border-zinc-700 bg-zinc-900 px-3 py-1.5 text-base text-zinc-100 placeholder:text-zinc-500 focus:border-rose-500 focus:outline-none sm:text-sm"
          />
          <button
            onClick={() => void onSaveName()}
            disabled={savingName}
            className="rounded-md bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 hover:bg-white disabled:opacity-50"
          >
            {savingName ? "Saving…" : "Save"}
          </button>
          {nameSaved && <span className="text-sm text-emerald-400">Saved</span>}
        </div>
        {nameErr && <p className="mt-2 text-sm text-rose-400">{nameErr}</p>}
      </section>

      <section className="mt-10">
        <h2 className="text-lg font-semibold">Your skip guides</h2>
        {guidesErr && <p className="mt-2 text-sm text-rose-400">{guidesErr}</p>}
        {guides == null ? (
          <p className="mt-3 text-sm text-zinc-400">Loading…</p>
        ) : guides.length === 0 ? (
          <p className="mt-3 text-sm text-zinc-400">You haven't created any skip guides yet.</p>
        ) : (
          <ul className="mt-3 divide-y divide-zinc-800 rounded-lg border border-zinc-800">
            {guides.map((g) => (
              <li key={g.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <Link
                      to={`/shows/${encodeURIComponent(g.showSlug)}/guides/${g.id}`}
                      className="truncate font-medium text-zinc-100 hover:text-rose-300"
                    >
                      {g.title}
                    </Link>
                    {!g.isPublished && (
                      <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-xs font-medium text-zinc-300">
                        Draft
                      </span>
                    )}
                  </div>
                  <p className="truncate text-sm text-zinc-500">{g.showName}</p>
                </div>
                <span className="text-sm text-zinc-400">
                  {g.likeCount} {g.likeCount === 1 ? "like" : "likes"}
                </span>
                <Link
                  to={`/shows/${encodeURIComponent(g.showSlug)}/guides/${g.id}/edit`}
                  className="rounded-md border border-zinc-700 px-2.5 py-1 text-sm text-zinc-300 hover:bg-zinc-800"
                >
                  Edit
                </Link>
                <button
                  onClick={() => void onDeleteGuide(g)}
                  className="rounded-md border border-rose-900/60 px-2.5 py-1 text-sm text-rose-300 hover:bg-rose-950/40"
                >
                  Delete
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

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
