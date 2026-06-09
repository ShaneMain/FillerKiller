// Shared presentation helpers for user-authored skip guides.
import type { Disposition, EpisodeStatus } from "./api";

export const DISPOSITIONS: Disposition[] = ["WATCH", "OPTIONAL", "SKIP"];

export const DISPOSITION_META: Record<
  Disposition,
  { label: string; dot: string; text: string; activeBtn: string }
> = {
  WATCH: { label: "Watch", dot: "bg-emerald-500", text: "text-emerald-300", activeBtn: "bg-emerald-600 text-white" },
  OPTIONAL: { label: "Optional", dot: "bg-sky-500", text: "text-sky-300", activeBtn: "bg-sky-600 text-white" },
  SKIP: { label: "Skip", dot: "bg-rose-500", text: "text-rose-300", activeBtn: "bg-rose-600 text-white" },
};

/** The community verdict mapped to a sensible default disposition for the editor. */
export function statusToDisposition(status: EpisodeStatus): Disposition {
  switch (status) {
    case "CANON":
      return "WATCH";
    case "WORTH_WATCHING":
      return "OPTIONAL";
    case "FILLER":
      return "SKIP";
    default:
      return "WATCH"; // CONTESTED / NOT_ENOUGH_VOTES → safe default
  }
}
