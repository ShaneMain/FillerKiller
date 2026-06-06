/**
 * Filler scoring and skip-guide derivation.
 *
 * This is the single source of truth in code for the math described in
 * the design notes. The threshold constants below MUST match
 * that document — changing them is a spec change.
 *
 * Pure functions only: no I/O, no DB. This keeps the rules unit-testable and
 * swappable (e.g. a future Wilson lower-bound) without touching the API layer.
 */

/** Minimum total votes before we show a confident label at all. */
export const MIN_VOTES = 5;
/** fillerScore strictly below this → CANON. */
export const CANON_BELOW = 0.4;
/** fillerScore strictly above this → FILLER. */
export const FILLER_ABOVE = 0.6;

export type VoteValue = "FILLER" | "CANON";

export type EpisodeStatus =
  | "CANON"
  | "FILLER"
  | "CONTESTED"
  | "NOT_ENOUGH_VOTES";

/**
 * fillerVotes / totalVotes, or null when there are no votes.
 */
export function fillerScore(fillerVotes: number, canonVotes: number): number | null {
  const total = fillerVotes + canonVotes;
  if (total <= 0) return null;
  return fillerVotes / total;
}

/**
 * Derive the displayed status from raw vote counts, applying the confidence
 * floor (MIN_VOTES) before the score thresholds.
 */
export function status(fillerVotes: number, canonVotes: number): EpisodeStatus {
  const total = fillerVotes + canonVotes;
  if (total < MIN_VOTES) return "NOT_ENOUGH_VOTES";
  const s = fillerVotes / total;
  if (s < CANON_BELOW) return "CANON";
  if (s > FILLER_ABOVE) return "FILLER";
  return "CONTESTED";
}

/** How to treat CONTESTED / NOT_ENOUGH_VOTES episodes in the skip guide. */
export type ContestedHandling = "canon" | "filler" | "show";

export interface ScoredEpisode {
  episodeId: string;
  seasonNumber: number;
  episodeNumber: number;
  name: string | null;
  fillerVotes: number;
  canonVotes: number;
}

export interface SkipGuideEntry {
  episodeId: string;
  seasonNumber: number;
  episodeNumber: number;
  name: string | null;
  status: EpisodeStatus;
}

export interface SkipGuide {
  watch: SkipGuideEntry[];
  skipped: SkipGuideEntry[];
  thresholds: { canonBelow: number; fillerAbove: number; minVotes: number };
}

/**
 * Build a show's skip guide from its scored episodes.
 *
 * Safe default: when unsure (CONTESTED / NOT_ENOUGH_VOTES), keep the episode in
 * the watch list, because wrongly skipping canon is worse than wrongly watching
 * filler. `contested` overrides that bias. Specials (season 0) are excluded from
 * the watch order unless `includeSpecials` is set.
 */
export function buildSkipGuide(
  episodes: ScoredEpisode[],
  contested: ContestedHandling = "canon",
  includeSpecials = false,
): SkipGuide {
  const ordered = [...episodes].sort(
    (a, b) =>
      a.seasonNumber - b.seasonNumber || a.episodeNumber - b.episodeNumber,
  );

  const watch: SkipGuideEntry[] = [];
  const skipped: SkipGuideEntry[] = [];

  for (const ep of ordered) {
    if (ep.seasonNumber === 0 && !includeSpecials) continue;

    const st = status(ep.fillerVotes, ep.canonVotes);
    const entry: SkipGuideEntry = {
      episodeId: ep.episodeId,
      seasonNumber: ep.seasonNumber,
      episodeNumber: ep.episodeNumber,
      name: ep.name,
      status: st,
    };

    let skip: boolean;
    if (st === "FILLER") {
      skip = true;
    } else if (st === "CANON") {
      skip = false;
    } else {
      // CONTESTED or NOT_ENOUGH_VOTES
      skip = contested === "filler";
    }

    (skip ? skipped : watch).push(entry);
  }

  return {
    watch,
    skipped,
    thresholds: {
      canonBelow: CANON_BELOW,
      fillerAbove: FILLER_ABOVE,
      minVotes: MIN_VOTES,
    },
  };
}
