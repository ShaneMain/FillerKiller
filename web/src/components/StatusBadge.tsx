import type { EpisodeStatus } from "../lib/api";

const STYLES: Record<EpisodeStatus, { label: string; cls: string }> = {
  CANON: { label: "Canon", cls: "bg-emerald-500/15 text-emerald-300 ring-emerald-500/30" },
  WORTH_WATCHING: { label: "Worth It", cls: "bg-sky-500/15 text-sky-300 ring-sky-500/30" },
  FILLER: { label: "Filler", cls: "bg-rose-500/15 text-rose-300 ring-rose-500/30" },
  CONTESTED: { label: "Contested", cls: "bg-amber-500/15 text-amber-300 ring-amber-500/30" },
  NOT_ENOUGH_VOTES: { label: "Not enough votes", cls: "bg-zinc-500/15 text-zinc-400 ring-zinc-500/30" },
};

export function StatusBadge({ status }: { status: EpisodeStatus }) {
  const s = STYLES[status];
  return (
    <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ring-1 ring-inset ${s.cls}`}>
      {s.label}
    </span>
  );
}
