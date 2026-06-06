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
    <div className="flex items-center gap-3 border-b border-zinc-800 py-3">
      <div className="w-12 shrink-0 text-center text-sm text-zinc-500">E{episode.episodeNumber}</div>

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium text-zinc-100">{episode.name ?? "Untitled"}</span>
          <StatusBadge status={score.status} />
        </div>

        <VoteBar
          filler={score.fillerVotes}
          worth={score.worthWatchingVotes}
          canon={score.canonVotes}
        />

        <div className="mt-1 text-xs text-zinc-500">
          {total === 0
            ? "No votes yet"
            : `${score.fillerVotes} filler · ${score.worthWatchingVotes} worth it · ${score.canonVotes} canon`}
          {err && <span className="ml-2 text-rose-400">{err}</span>}
        </div>
      </div>

      <div className="flex shrink-0 gap-1.5">
        <VoteButton label="Filler" active={score.myVote === "FILLER"} activeCls="bg-rose-600 text-white"
          disabled={!signedIn || busy} onClick={() => void vote("FILLER")} />
        <VoteButton label="Worth it" active={score.myVote === "WORTH_WATCHING"} activeCls="bg-sky-600 text-white"
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
    <div className="mt-1.5 flex h-1.5 w-full overflow-hidden rounded-full bg-zinc-800" title={`${filler} filler / ${worth} worth watching / ${canon} canon`}>
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
      className={`rounded-md px-2.5 py-1 text-xs font-medium ring-1 ring-inset ring-zinc-700 transition
        ${active ? activeCls : "text-zinc-300 hover:bg-zinc-800"}
        ${disabled ? "cursor-not-allowed opacity-50" : ""}`}
    >
      {label}
    </button>
  );
}
