import { useState } from "react";
import { castVote, removeVote, type Episode, type EpisodeScore, type VoteValue } from "../lib/api";
import { StatusBadge } from "./StatusBadge";

export function EpisodeRow({ episode, signedIn }: { episode: Episode; signedIn: boolean }) {
  const [score, setScore] = useState<EpisodeScore>(episode.score);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function vote(value: VoteValue) {
    if (!signedIn || busy) return;
    setBusy(true);
    setErr(null);
    try {
      // Clicking the current vote removes it; otherwise cast/replace.
      const res = score.myVote === value ? await removeVote(episode.id) : await castVote(episode.id, value);
      setScore({ ...res.score, myVote: res.myVote });
    } catch (e) {
      setErr(e instanceof Error ? e.message : "vote failed");
    } finally {
      setBusy(false);
    }
  }

  const total = score.fillerVotes + score.worthWatchingVotes + score.canonVotes;

  return (
    <div className="flex flex-col gap-2 border-b border-zinc-800 py-3 sm:flex-row sm:items-center sm:gap-3">
      <div className="flex min-w-0 flex-1 items-start gap-3">
        <div className="w-9 shrink-0 pt-0.5 text-center text-sm text-zinc-500">
          E{episode.episodeNumber}
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="font-medium text-zinc-100">{episode.name ?? "Untitled"}</span>
            <StatusBadge status={score.status} />
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
            {err && <span className="ml-2 text-rose-400">{err}</span>}
          </div>
        </div>
      </div>

      {/* Full-width, tappable on mobile; inline beside the row on sm+. */}
      <div className="grid grid-cols-3 gap-1.5 pl-12 sm:flex sm:shrink-0 sm:pl-0">
        <VoteButton label="Filler" active={score.myVote === "FILLER"} activeCls="bg-rose-600 text-white"
          disabled={!signedIn || busy} onClick={() => void vote("FILLER")} />
        <VoteButton label="Worth It" active={score.myVote === "WORTH_WATCHING"} activeCls="bg-sky-600 text-white"
          disabled={!signedIn || busy} onClick={() => void vote("WORTH_WATCHING")} />
        <VoteButton label="Canon" active={score.myVote === "CANON"} activeCls="bg-emerald-600 text-white"
          disabled={!signedIn || busy} onClick={() => void vote("CANON")} />
      </div>
    </div>
  );
}

/** At-a-glance stacked bar of the filler / worth-watching / canon vote ratio. */
function VoteBar({ filler, worth, canon }: { filler: number; worth: number; canon: number }) {
  const total = filler + worth + canon;
  if (total === 0) {
    return <div className="mt-1.5 h-1.5 w-full rounded-full bg-zinc-800" />;
  }
  const pct = (n: number) => `${(n / total) * 100}%`;
  return (
    <div className="mt-1.5 flex h-1.5 w-full overflow-hidden rounded-full bg-zinc-800" title={`${filler} filler / ${worth} worth it / ${canon} canon`}>
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
      title={disabled ? "Sign in to vote" : `Vote ${label}`}
      className={`whitespace-nowrap rounded-md px-3 py-2.5 text-sm font-medium ring-1 ring-inset ring-zinc-700 transition
        ${active ? activeCls : "text-zinc-300 hover:bg-zinc-800"}
        ${disabled ? "cursor-not-allowed opacity-50" : ""}`}
    >
      {label}
    </button>
  );
}
