//! API response shapes for the catalog endpoints. camelCase wire format.
//! These are serialized to the SPA; they are not DB rows.

use chrono::NaiveDate;
use serde::Serialize;
use uuid::Uuid;

use std::collections::HashMap;

use crate::scoring::{EpisodeStatus, VoteReason, VoteValue};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    /// Our internal id, or null if the show hasn't been imported yet.
    pub show_id: Option<Uuid>,
    /// URL slug, or null if the show hasn't been imported yet.
    pub slug: Option<String>,
    pub tmdb_id: i64,
    pub name: String,
    pub first_air_year: Option<i32>,
    pub poster_path: Option<String>,
    /// Fraction of episodes with enough votes to be confident. Null if the show
    /// isn't imported. (Not yet computed — populated with the voting layer.)
    pub filler_coverage: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchItem>,
}

/// An imported show in the home page's "popular" browse list. Unlike
/// `SearchItem`, these are always imported, so `slug` is non-null.
///
/// The stat fields carry the same numbers as the show's OG card ("X% filler —
/// skip N of M episodes"), so the front page shows the verdict, not just a
/// poster. Specials (season 0) are excluded, matching the card.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PopularShowItem {
    pub slug: String,
    pub tmdb_id: i64,
    pub name: String,
    pub first_air_year: Option<i32>,
    pub poster_path: Option<String>,
    /// Episodes tracked (excluding specials).
    pub episode_count: u32,
    /// Whole-number percentage of tracked episodes with a FILLER verdict.
    pub filler_pct: u32,
    /// Episodes a skip guide drops — the "skip N" in "skip N of M".
    pub skip_count: u32,
    /// False while every episode is still contested / short of votes — the
    /// client shows "Not yet rated" instead of a misleading 0%.
    pub rated: bool,
}

#[derive(Debug, Serialize)]
pub struct PopularShowsResponse {
    pub shows: Vec<PopularShowItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonSummary {
    pub id: Uuid,
    pub season_number: i32,
    pub name: Option<String>,
    pub episode_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowDetail {
    pub id: Uuid,
    pub tmdb_id: i64,
    pub name: String,
    pub slug: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    /// TMDB's overall show rating (0–10) and the vote count behind it.
    pub tmdb_rating: Option<f64>,
    pub tmdb_vote_count: Option<i32>,
    pub seasons: Vec<SeasonSummary>,
    /// Whole-number percentage of tracked episodes (specials excluded) with a
    /// FILLER verdict — the same number as the show's OG card and popular chip.
    pub filler_pct: u32,
    /// False while every episode is still contested / short of votes, so the
    /// client shows "Not yet rated" instead of a misleading 0%.
    pub rated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeScoreView {
    pub filler_votes: i64,
    pub worth_watching_votes: i64,
    pub canon_votes: i64,
    pub filler_score: Option<f64>,
    pub status: EpisodeStatus,
    /// The current user's vote on this episode; null when not signed in.
    pub my_vote: Option<VoteValue>,
    /// The current user's reason tag for their vote; null if no tag or not signed in.
    pub my_reason: Option<VoteReason>,
    /// Reason tag counts among votes for the plurality value, keyed by reason
    /// tag string. Only reasons with count > 0 are included; omitted entirely
    /// when the episode has no plurality verdict (status is CONTESTED or
    /// NOT_ENOUGH_VOTES). This lets the UI show e.g. "62% say recap episode".
    pub reason_counts: HashMap<String, i64>,
    /// Whether the signed-in user has marked this episode as watched. Always
    /// false when not signed in.
    pub watched: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeItem {
    pub id: Uuid,
    pub season_number: i32,
    pub episode_number: i32,
    pub name: Option<String>,
    pub air_date: Option<NaiveDate>,
    pub still_path: Option<String>,
    /// TMDB's own audience rating (0–10) and the vote count behind it. Null until
    /// the episode is (re)imported from TMDB. Distinct from our filler `score`.
    pub tmdb_rating: Option<f64>,
    pub tmdb_vote_count: Option<i32>,
    /// Episode runtime in minutes from TMDB; null until imported or re-synced.
    pub runtime_minutes: Option<i32>,
    pub score: EpisodeScoreView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodesResponse {
    pub episodes: Vec<EpisodeItem>,
    /// How many episodes in the show the signed-in user has watched. Null when
    /// anonymous (saving a query and keeping the anonymous response cacheable).
    pub watched_count: Option<i64>,
}

/// Aggregate for a single episode, returned by the vote endpoints (no `myVote`
/// inside — that's a sibling field on the vote response).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateView {
    pub filler_votes: i64,
    pub worth_watching_votes: i64,
    pub canon_votes: i64,
    pub filler_score: Option<f64>,
    pub status: EpisodeStatus,
    /// Reason tag counts for the plurality value (same semantics as
    /// `EpisodeScoreView::reason_counts`; omitted when no plurality verdict).
    pub reason_counts: HashMap<String, i64>,
}

/// Response to PUT/DELETE vote: the caller's current vote + the new aggregate.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoteResponse {
    pub my_vote: Option<VoteValue>,
    /// The caller's reason tag for their current vote; null if none set.
    pub my_reason: Option<VoteReason>,
    pub score: AggregateView,
}
