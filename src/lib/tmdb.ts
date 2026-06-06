/**
 * Server-side TMDB client. See the design notes.
 *
 * IMPORTANT: this module must never be imported into client components. The
 * TMDB token is server-only; all catalog access is proxied through our API so
 * the token never reaches the browser. The `server-only` import below makes an
 * accidental client import fail at build time rather than relying on convention.
 */
import "server-only";

const TMDB_BASE = "https://api.themoviedb.org/3";

function token(): string {
  const t = process.env.TMDB_API_READ_TOKEN;
  if (!t) {
    throw new Error;
  }
  return t;
}

// Default cache TTL for catalog data (1h). Search overrides with a shorter TTL.
const DEFAULT_REVALIDATE = 60 * 60;

async function tmdb<T>(
  path: string,
  params: Record<string, string> = {},
  revalidate: number = DEFAULT_REVALIDATE,
): Promise<T> {
  const url = new URL(`${TMDB_BASE}${path}`);
  for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);

  const res = await fetch(url, {
    headers: { Authorization: `Bearer ${token()}`, Accept: "application/json" },
    // Catalog data is cacheable; tune per the design notes sync strategy.
    next: { revalidate },
  });

  if (res.status === 429) {
    throw new Error("TMDB rate limit hit (429). Back off and retry.");
  }
  if (!res.ok) {
    throw new Error(`TMDB request failed: ${res.status} ${res.statusText} for ${path}`);
  }
  return res.json() as Promise<T>;
}

// --- Minimal response shapes (expand as endpoints are wired up) ---

export interface TmdbSearchResult {
  id: number;
  name: string;
  first_air_date?: string;
  poster_path?: string | null;
  overview?: string;
}

export interface TmdbSearchResponse {
  page: number;
  results: TmdbSearchResult[];
  total_results: number;
}

/** Search TV series by name. Cached briefly (10 min) per the design notes sync strategy. */
export function searchShows(query: string): Promise<TmdbSearchResponse> {
  return tmdb<TmdbSearchResponse>("/search/tv", { query }, 60 * 10);
}

export interface TmdbSeasonSummary {
  season_number: number;
  name: string;
  episode_count: number;
}

export interface TmdbShowDetail {
  id: number;
  name: string;
  overview?: string;
  poster_path?: string | null;
  first_air_date?: string;
  seasons: TmdbSeasonSummary[];
}

/** Full series detail including the season list. */
export function getShow(tmdbId: number): Promise<TmdbShowDetail> {
  return tmdb<TmdbShowDetail>(`/tv/${tmdbId}`);
}

export interface TmdbEpisode {
  id: number;
  season_number: number;
  episode_number: number;
  name: string;
  overview?: string;
  air_date?: string;
  still_path?: string | null;
}

export interface TmdbSeasonDetail {
  season_number: number;
  name: string;
  episodes: TmdbEpisode[];
}

/** A season's episodes. */
export function getSeason(tmdbId: number, seasonNumber: number): Promise<TmdbSeasonDetail> {
  return tmdb<TmdbSeasonDetail>(`/tv/${tmdbId}/season/${seasonNumber}`);
}

/** Build a full image URL from a TMDB-relative path. */
export function imageUrl(path: string | null | undefined, size = "w500"): string | null {
  if (!path) return null;
  const base = process.env.TMDB_IMAGE_BASE_URL ?? "https://image.tmdb.org/t/p";
  return `${base}/${size}${path}`;
}
