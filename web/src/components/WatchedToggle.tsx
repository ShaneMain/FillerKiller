/** The watched-episode circle toggle — empty ring when unwatched, filled
 *  emerald check when watched. Shared by episode rows and guide checklists. */
export function WatchedToggle({
  watched,
  busy = false,
  onToggle,
  className = "h-5 w-5",
}: {
  watched: boolean;
  busy?: boolean;
  onToggle: () => void;
  className?: string;
}) {
  return (
    <button
      onClick={onToggle}
      disabled={busy}
      aria-pressed={watched}
      aria-label={watched ? "Mark as unwatched" : "Mark as watched"}
      title={watched ? "Mark as unwatched" : "Mark as watched"}
      className={`shrink-0 rounded-full border-2 transition
        ${watched
          ? "border-emerald-500 bg-emerald-500 text-zinc-900"
          : "border-zinc-600 bg-transparent text-transparent hover:border-zinc-400"}
        ${busy ? "opacity-50" : ""}
        ${className}`}
    >
      {watched && (
        <svg viewBox="0 0 12 12" fill="currentColor" className="h-full w-full p-0.5">
          <path d="M10 3L5 8.5 2 5.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" fill="none" />
        </svg>
      )}
    </button>
  );
}
