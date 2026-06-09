//! API response shapes for the catalog endpoints. camelCase wire format.
//! These are serialized to the SPA; they are not DB rows.

use chrono::NaiveDate;
use serde::Serialize;
use uuid::Uuid;

use crate::scoring::{EpisodeStatus, VoteValue};

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
    pub seasons: Vec<SeasonSummary>,
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
    pub score: EpisodeScoreView,
}

#[derive(Debug, Serialize)]
pub struct EpisodesResponse {
    pub episodes: Vec<EpisodeItem>,
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
}

/// Response to PUT/DELETE vote: the caller's current vote + the new aggregate.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoteResponse {
    pub my_vote: Option<VoteValue>,
    pub score: AggregateView,
}
