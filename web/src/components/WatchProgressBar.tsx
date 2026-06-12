/** Slim progress bar for watch progress — used on the show page, the
 *  community skip guide, and user guide cards/detail pages. */
export function WatchProgressBar({
  watched,
  total,
  label = "watched",
  className = "mt-4",
}: {
  watched: number;
  total: number;
  /** Noun after the count, e.g. "watched" → "3 / 12 watched". */
  label?: string;
  className?: string;
}) {
  if (total === 0) return null;
  const pct = Math.min(100, Math.round((watched / total) * 100));
  return (
    <div className={`rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 ${className}`}>
      <div className="flex items-center justify-between text-xs text-zinc-400">
        <span>{watched} / {total} {label}</span>
        <span>{pct}%</span>
      </div>
      <div className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
        <div
          className="h-full rounded-full bg-emerald-600 transition-[width]"
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}
