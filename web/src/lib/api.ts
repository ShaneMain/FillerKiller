// Client for the FillerKiller API. Calls are same-origin (relative `/api/...`):
// in prod Caddy serves the SPA and API together; in dev the Vite proxy forwards
// `/api` + `/health` to the backend.

export type VoteValue = "FILLER" | "WORTH_WATCHING" | "CANON";

/** Reason tags that can be attached to a vote. Each tag is only valid for one
 *  vote value; the UI enforces this by only showing the relevant three chips. */
export type VoteReason =
  // FILLER reasons
  | "recap"
  | "side-story"
  | "fun-but-skippable"
  // WORTH_WATCHING reasons
  | "self-contained-gem"
  | "character-moment"
  | "worldbuilding"
  // CANON reasons
  | "major-plot"
  | "character-development"
  | "arc-setup";

/** Human-readable labels for each reason tag, matching the backend spec. */
export const REASON_LABELS: Record<VoteReason, string> = {
  "recap": "Recap episode",
  "side-story": "Side story, no plot",
  "fun-but-skippable": "Fun but skippable",
  "self-contained-gem": "Self-contained gem",
  "character-moment": "Great character moment",
  "worldbuilding": "Worldbuilding",
  "major-plot": "Major plot",
  "character-development": "Character development",
  "arc-setup": "Sets up a later arc",
};

/** The three reason tags valid for each vote value. */
export const VOTE_REASONS: Record<VoteValue, VoteReason[]> = {
  FILLER: ["recap", "side-story", "fun-but-skippable"],
  WORTH_WATCHING: ["self-contained-gem", "character-moment", "worldbuilding"],
  CANON: ["major-plot", "character-development", "arc-setup"],
};
export type GuideMode = "completionist" | "standard" | "canon-only" | "binge";
export type EpisodeStatus =
  | "CANON"
  | "WORTH_WATCHING"
  | "FILLER"
  | "CONTESTED"
  | "NOT_ENOUGH_VOTES";

export interface SearchItem {
  showId: string | null;
  slug: string | null;
  tmdbId: number;
  name: string;
  firstAirYear: number | null;
  posterPath: string | null;
  fillerCoverage: number | null;
}

export interface SeasonSummary {
  id: string;
  seasonNumber: number;
  name: string | null;
  episodeCount: number;
}

export interface ShowDetail {
  id: string;
  tmdbId: number;
  name: string;
  slug: string;
  overview: string | null;
  posterPath: string | null;
  /** TMDB's overall show rating (0–10) and vote count; null until imported. */
  tmdbRating: number | null;
  tmdbVoteCount: number | null;
  seasons: SeasonSummary[];
}

export interface EpisodeScore {
  fillerVotes: number;
  worthWatchingVotes: number;
  canonVotes: number;
  fillerScore: number | null;
  status: EpisodeStatus;
  myVote: VoteValue | null;
  /** The signed-in user's reason tag for their vote; null if none set. */
  myReason: VoteReason | null;
  /** Reason counts for the plurality value. Only reasons with count > 0 are
   *  present; the map is empty when the episode has no clear verdict. */
  reasonCounts: Partial<Record<VoteReason, number>>;
  /** Whether the signed-in user has marked this episode as watched. Always
   *  false when not signed in. */
  watched: boolean;
}

export interface Episode {
  id: string;
  seasonNumber: number;
  episodeNumber: number;
  name: string | null;
  airDate: string | null;
  stillPath: string | null;
  /** TMDB's own audience rating (0–10) and vote count; null until imported. */
  tmdbRating: number | null;
  tmdbVoteCount: number | null;
  /** Runtime in minutes from TMDB; null until imported. */
  runtimeMinutes: number | null;
  score: EpisodeScore;
}

export interface VoteResult {
  myVote: VoteValue | null;
  myReason: VoteReason | null;
  // The vote response's score carries no per-user `watched` flag (that state
  // is owned by the watched endpoints) — the client preserves its local value.
  score: Omit<EpisodeScore, "myVote" | "myReason" | "watched">;
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
  optional: SkipGuideEntry[];
  skipped: SkipGuideEntry[];
  thresholds: { minVotes: number; contestedMargin: number };
  mode: GuideMode;
  validModes: string[];
  minutesSkipped: number | null;
}

export interface Me {
  id: string;
  email: string;
  displayName: string | null;
}

export type Disposition = "WATCH" | "OPTIONAL" | "SKIP";

export interface GuideSummary {
  id: string;
  title: string;
  description: string | null;
  authorName: string | null;
  likeCount: number;
  watchCount: number;
  optionalCount: number;
  skipCount: number;
  isPublished: boolean;
  myLike: boolean;
  mine: boolean;
}

export interface GuideEntry {
  episodeId: string;
  seasonNumber: number;
  episodeNumber: number;
  name: string | null;
  disposition: Disposition;
}

export interface GuideDetail {
  id: string;
  showId: string;
  showSlug: string;
  showName: string;
  posterPath: string | null;
  title: string;
  description: string | null;
  authorName: string | null;
  likeCount: number;
  isPublished: boolean;
  myLike: boolean;
  mine: boolean;
  entries: GuideEntry[];
}

export interface GuideInput {
  title: string;
  description?: string | null;
  entries: { episodeId: string; disposition: Disposition }[];
  published: boolean;
}

export interface MyGuide {
  id: string;
  title: string;
  isPublished: boolean;
  likeCount: number;
  showSlug: string;
  showName: string;
}

export class ApiError extends Error {
  status: number;
  code: string;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

// Error codes whose server-sent `message` is curated and safe to show the user
// verbatim. Anything else (notably `internal`, or an unrecognized code) falls
// back to a generic message, so a verbose/unexpected server string can never be
// surfaced in the UI. The raw message is still kept on the console for debugging.
const SAFE_ERROR_CODES = new Set([
  "bad_request",
  "not_found",
  "unauthorized",
  "forbidden",
  "rate_limited",
  "upstream_error",
  "upstream_rate_limited",
]);

function userFacingMessage(code: string, rawMessage: string): string {
  if (SAFE_ERROR_CODES.has(code) && rawMessage) return rawMessage;
  return "Something went wrong. Please try again.";
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api${path}`, {
    credentials: "include",
    headers: { Accept: "application/json", ...(init?.body ? { "Content-Type": "application/json" } : {}) },
    ...init,
  });
  if (!res.ok) {
    let code = "error";
    let rawMessage = res.statusText;
    try {
      const body = await res.json();
      code = body?.error?.code ?? code;
      rawMessage = body?.error?.message ?? rawMessage;
    } catch {
      /* non-JSON error body */
    }
    const message = userFacingMessage(code, rawMessage);
    if (message !== rawMessage) {
      // Keep the real cause out of the UI but available to a developer.
      console.debug(`API ${res.status} ${code}: ${rawMessage}`);
    }
    throw new ApiError(res.status, code, message);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

export function searchShows(q: string): Promise<{ results: SearchItem[] }> {
  return request(`/search?q=${encodeURIComponent(q)}`);
}

export function getShow(id: string): Promise<ShowDetail> {
  return request(`/shows/${encodeURIComponent(id)}`);
}

export interface EpisodesResponse {
  episodes: Episode[];
  /** How many episodes the signed-in user has watched for the whole show. Null
   *  when not signed in. */
  watchedCount: number | null;
}

export function getEpisodes(id: string, season?: number): Promise<EpisodesResponse> {
  const q = season != null ? `?season=${season}` : "";
  return request(`/shows/${encodeURIComponent(id)}/episodes${q}`);
}

/** Mark an episode as watched (idempotent). Auth required. */
export function markWatched(episodeId: string): Promise<void> {
  return request(`/episodes/${episodeId}/watched`, { method: "PUT" });
}

/** Unmark an episode as watched (idempotent). Auth required. */
export function unmarkWatched(episodeId: string): Promise<void> {
  return request(`/episodes/${episodeId}/watched`, { method: "DELETE" });
}

export function getSkipGuide(
  id: string,
  mode: GuideMode = "standard",
): Promise<SkipGuide> {
  return request(`/shows/${encodeURIComponent(id)}/skip-guide?mode=${mode}`);
}

export function castVote(
  episodeId: string,
  value: VoteValue,
  reason?: VoteReason | null,
): Promise<VoteResult> {
  return request(`/episodes/${episodeId}/vote`, {
    method: "PUT",
    body: JSON.stringify({ value, reason: reason ?? null }),
  });
}

export function removeVote(episodeId: string): Promise<VoteResult> {
  return request(`/episodes/${episodeId}/vote`, { method: "DELETE" });
}

export function getMe(): Promise<Me | null> {
  return request(`/me`);
}

/** Set (or clear, with `null`) the user's screen name. Re-issues the session cookie. */
export function updateScreenName(name: string | null): Promise<Me> {
  return request(`/me`, { method: "PUT", body: JSON.stringify({ screenName: name }) });
}

/** The signed-in user's own guides, published and drafts. */
export function listMyGuides(): Promise<MyGuide[]> {
  return request(`/me/guides`);
}

// ---- User-authored skip guides ----

export function listGuides(showId: string): Promise<GuideSummary[]> {
  return request(`/shows/${encodeURIComponent(showId)}/guides`);
}

export function getGuide(guideId: string): Promise<GuideDetail> {
  return request(`/guides/${guideId}`);
}

export function createGuide(showId: string, input: GuideInput): Promise<{ id: string }> {
  return request(`/shows/${encodeURIComponent(showId)}/guides`, {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export function updateGuide(guideId: string, input: GuideInput): Promise<void> {
  return request(`/guides/${guideId}`, { method: "PUT", body: JSON.stringify(input) });
}

export function deleteGuide(guideId: string): Promise<void> {
  return request(`/guides/${guideId}`, { method: "DELETE" });
}

export function likeGuide(guideId: string): Promise<{ likeCount: number; myLike: boolean }> {
  return request(`/guides/${guideId}/like`, { method: "PUT" });
}

export function unlikeGuide(guideId: string): Promise<{ likeCount: number; myLike: boolean }> {
  return request(`/guides/${guideId}/like`, { method: "DELETE" });
}

export function logout(): Promise<void> {
  return request(`/auth/logout`, { method: "POST" });
}

/**
 * Permanently delete the current user's account and personal data. Their votes
 * are retained anonymously (dissociated from the user) so community totals stay
 * intact.
 */
export function deleteAccount(): Promise<void> {
  return request(`/me`, { method: "DELETE" });
}

/** Full-page navigation target to start an OAuth login. `next` is a site-relative
 *  path to return to after sign-in. */
export function loginUrl(provider: "google" | "github", next?: string): string {
  const q = next ? `?next=${encodeURIComponent(next)}` : "";
  return `/api/auth/${provider}/login${q}`;
}

const TMDB_IMG = "https://image.tmdb.org/t/p";

/** Build a TMDB image URL from a stored relative path (images come from TMDB's CDN). */
export function imageUrl(path: string | null, size = "w300"): string | null {
  return path ? `${TMDB_IMG}/${size}${path}` : null;
}
