import { useState } from "react";
import {
  ApiError,
  castVote,
  markWatched,
  unmarkWatched,
  removeVote,
  REASON_LABELS,
  VOTE_REASONS,
  type Episode,
  type EpisodeScore,
  type VoteReason,
  type VoteValue,
} from "../lib/api";
import { useAuth } from "../lib/auth";
import { StatusBadge } from "./StatusBadge";
import { WatchedToggle } from "./WatchedToggle";

export function EpisodeRow({
  episode,
  signedIn,
  onWatchedChange,
}: {
  episode: Episode;
  signedIn: boolean;
  /** Notifies the parent of an optimistic watched toggle (+1 / -1), so a
   *  show-level progress counter can stay in sync without refetching. */
  onWatchedChange?: (delta: 1 | -1) => void;
}) {
  const { refresh } = useAuth();
  const [score, setScore] = useState<EpisodeScore>(episode.score);
  const [myReason, setMyReason] = useState<VoteReason | null>(episode.score.myReason ?? null);
  const [watched, setWatched] = useState<boolean>(episode.score.watched);
  const [watchBusy, setWatchBusy] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function vote(value: VoteValue) {
    if (!signedIn || busy) return;
    setBusy(true);
    setErr(null);
    try {
      // Clicking the current vote removes it; otherwise cast/replace (no reason
      // on a value switch — the chips appear afterward for the user to tag).
      const removing = score.myVote === value;
      const res = removing
        ? await removeVote(episode.id)
        : await castVote(episode.id, value);
      // The vote response carries no `watched` — preserve the local flag.
      setScore((prev) => ({
        ...res.score,
        myVote: res.myVote,
        myReason: res.myReason,
        reasonCounts: res.score.reasonCounts ?? {},
        watched: prev.watched,
      }));
      setMyReason(res.myReason ?? null);
    } catch (e) {
      // An expired/cleared session (the 7-day JWT lapsed while the tab stayed
      // open) returns 401. Re-sync auth so the UI reflects signed-out state and
      // tell the user plainly, rather than echoing "authentication required".
      if (e instanceof ApiError && e.status === 401) {
        setErr("Your session expired — please sign in again.");
        void refresh();
      } else if (e instanceof ApiError && e.status === 429) {
        setErr("You're voting too fast — give it a moment.");
      } else {
        setErr(e instanceof Error ? e.message : "vote failed");
      }
    } finally {
      setBusy(false);
    }
  }

  async function pickReason(reason: VoteReason) {
    if (!signedIn || !score.myVote || busy) return;
    setBusy(true);
    setErr(null);
    // Toggle: tapping the selected chip clears it.
    const next = myReason === reason ? null : reason;
    try {
      const res = await castVote(episode.id, score.myVote, next);
      // The vote response carries no `watched` — preserve the local flag.
      setScore((prev) => ({
        ...res.score,
        myVote: res.myVote,
        myReason: res.myReason,
        reasonCounts: res.score.reasonCounts ?? {},
        watched: prev.watched,
      }));
      setMyReason(res.myReason ?? null);
    } catch (e) {
      setErr(e instanceof Error ? e.message : "tag failed");
    } finally {
      setBusy(false);
    }
  }

  async function toggleWatched() {
    if (!signedIn || watchBusy) return;
    const next = !watched;
    setWatched(next); // optimistic
    onWatchedChange?.(next ? 1 : -1);
    setWatchBusy(true);
    try {
      if (next) {
        await markWatched(episode.id);
      } else {
        await unmarkWatched(episode.id);
      }
    } catch (e) {
      setWatched(!next); // revert on error
      onWatchedChange?.(next ? -1 : 1);
      if (e instanceof ApiError && e.status === 401) {
        void refresh();
      }
    } finally {
      setWatchBusy(false);
    }
  }

  const total = score.fillerVotes + score.worthWatchingVotes + score.canonVotes;

  // Top reason annotation for the episode row: show only when count >= 3.
  const topReasonAnnotation = buildTopReasonAnnotation(score);

  return (
    <div className="flex flex-col gap-2 border-b border-zinc-800 py-3 sm:flex-row sm:items-center sm:gap-3">
      <div className="flex min-w-0 flex-1 items-start gap-3">
        {signedIn && (
          <WatchedToggle
            watched={watched}
            busy={watchBusy}
            onToggle={() => void toggleWatched()}
            className="mt-0.5 h-5 w-5"
          />
        )}
        <div className="w-9 shrink-0 pt-0.5 text-center text-sm text-zinc-500">
          E{episode.episodeNumber}
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="font-medium text-zinc-100">{episode.name ?? "Untitled"}</span>
            <StatusBadge status={score.status} />
            {episode.tmdbRating != null &&
              episode.tmdbVoteCount != null &&
              episode.tmdbVoteCount > 0 && (
                <span
                  className="inline-flex items-center gap-0.5 text-xs text-amber-300"
                  title={`TMDB rating ${episode.tmdbRating.toFixed(1)}/10 from ${episode.tmdbVoteCount} vote${episode.tmdbVoteCount === 1 ? "" : "s"}`}
                >
                  ★ {episode.tmdbRating.toFixed(1)}
                </span>
              )}
          </div>

          <VoteBar
            filler={score.fillerVotes}
            worth={score.worthWatchingVotes}
            canon={score.canonVotes}
          />

          <div className="mt-1 text-xs text-zinc-500">
            {total === 0
              ? "No votes yet — be the first"
              : `${score.fillerVotes} filler · ${score.worthWatchingVotes} worth it · ${score.canonVotes} canon`}
            {topReasonAnnotation && (
              <span className="ml-1.5 text-zinc-600">{topReasonAnnotation}</span>
            )}
            {err && <span className="ml-2 text-rose-400">{err}</span>}
          </div>

          {/* Reason chips — shown only when the user has an active vote. */}
          {signedIn && score.myVote && (
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              <span className="text-xs text-zinc-500">Why? (optional)</span>
              {VOTE_REASONS[score.myVote].map((r) => (
                <ReasonChip
                  key={r}
                  reason={r}
                  active={myReason === r}
                  disabled={busy}
                  onClick={() => void pickReason(r)}
                />
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Full-width, tappable on mobile; inline beside the row on sm+. Hidden
          entirely when signed out — a page of disabled buttons is just noise
          (the show page's sign-in banner covers the call to action). */}
      {signedIn && (
        <div className="grid grid-cols-3 gap-1.5 pl-12 sm:flex sm:shrink-0 sm:pl-0">
          <VoteButton label="Filler" active={score.myVote === "FILLER"} activeCls="bg-rose-600 text-white"
            disabled={busy} onClick={() => void vote("FILLER")} />
          <VoteButton label="Worth It" active={score.myVote === "WORTH_WATCHING"} activeCls="bg-sky-600 text-white"
            disabled={busy} onClick={() => void vote("WORTH_WATCHING")} />
          <VoteButton label="Canon" active={score.myVote === "CANON"} activeCls="bg-emerald-600 text-white"
            disabled={busy} onClick={() => void vote("CANON")} />
        </div>
      )}
    </div>
  );
}

/** Build the muted "Filler — 62% say recap episode" annotation line, or null. */
function buildTopReasonAnnotation(score: EpisodeScore): string | null {
  const { status, reasonCounts, fillerVotes, worthWatchingVotes, canonVotes } = score;
  if (!reasonCounts || Object.keys(reasonCounts).length === 0) return null;

  // Only show for episodes with a clear verdict.
  const pluralityLabel =
    status === "FILLER" ? "Filler"
    : status === "WORTH_WATCHING" ? "Worth It"
    : status === "CANON" ? "Canon"
    : null;
  if (!pluralityLabel) return null;

  // Total votes for the plurality value.
  const pluralityTotal =
    status === "FILLER" ? fillerVotes
    : status === "WORTH_WATCHING" ? worthWatchingVotes
    : canonVotes;
  if (pluralityTotal <= 0) return null;

  // Pick the top reason by count; skip if count < 3.
  const entries = Object.entries(reasonCounts) as [VoteReason, number][];
  if (entries.length === 0) return null;
  entries.sort((a, b) => b[1] - a[1]);
  const [topReason, topCount] = entries[0];
  if (topCount < 3) return null;

  const pct = Math.round((topCount / pluralityTotal) * 100);
  const label = REASON_LABELS[topReason];
  return `· ${pct}% say ${label.toLowerCase()}`;
}

/** At-a-glance stacked bar of the filler / worth-watching / canon vote ratio. */
function VoteBar({ filler, worth, canon }: { filler: number; worth: number; canon: number }) {
  const total = filler + worth + canon;
  if (total === 0) {
    return <div className="mt-1.5 h-1.5 w-full rounded-full bg-zinc-800" aria-hidden="true" />;
  }
  const pct = (n: number) => `${(n / total) * 100}%`;
  const label = `${filler} filler, ${worth} worth it, ${canon} canon`;
  return (
    <div
      className="mt-1.5 flex h-1.5 w-full overflow-hidden rounded-full bg-zinc-800"
      role="img"
      aria-label={`Votes: ${label}`}
      title={label}
    >
      <div className="bg-rose-500" style={{ width: pct(filler) }} />
      <div className="bg-sky-500" style={{ width: pct(worth) }} />
      <div className="bg-emerald-500" style={{ width: pct(canon) }} />
    </div>
  );
}

function VoteButton({
  label,
  active,
  activeCls,
  disabled,
  onClick,
}: {
  label: string;
  active: boolean;
  activeCls: string;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      aria-pressed={active}
      aria-label={`Vote ${label}${active ? " (your current vote — click to remove)" : ""}`}
      title={`Vote ${label}`}
      className={`whitespace-nowrap rounded-md px-3 py-2.5 text-sm font-medium ring-1 ring-inset ring-zinc-700 transition
        ${active ? activeCls : "text-zinc-300 hover:bg-zinc-800"}
        ${disabled ? "cursor-not-allowed opacity-50" : ""}`}
    >
      {label}
    </button>
  );
}

/** A single reason tag chip. Active state shows a ring; tapping the active chip clears it. */
function ReasonChip({
  reason,
  active,
  disabled,
  onClick,
}: {
  reason: VoteReason;
  active: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      aria-pressed={active}
      aria-label={`Tag: ${REASON_LABELS[reason]}${active ? " (selected — click to clear)" : ""}`}
      className={`rounded-full px-2.5 py-0.5 text-xs transition
        ring-1 ring-inset
        ${active
          ? "bg-zinc-700 text-zinc-100 ring-zinc-500"
          : "bg-transparent text-zinc-400 ring-zinc-700 hover:bg-zinc-800 hover:text-zinc-300"}
        ${disabled ? "cursor-not-allowed opacity-50" : ""}`}
    >
      {REASON_LABELS[reason]}
    </button>
  );
}
