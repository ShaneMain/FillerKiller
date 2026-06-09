//! Server-side TMDB client.
//!
//! The TMDB token lives only here, in the API process. The React SPA never
//! holds it — all catalog access is proxied through our endpoints.

// Foundation: the client and response types are defined ahead of the catalog
// endpoints that consume them. Remove once those endpoints are wired up.
#![allow(dead_code)]

use serde::Deserialize;

const TMDB_BASE: &str = "https://api.themoviedb.org/3";

/// A thin TMDB client holding the shared HTTP client and the bearer token.
#[derive(Clone)]
pub struct TmdbClient {
    http: reqwest::Client,
    token: String,
    image_base_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TmdbError {
    #[error("TMDB rate limit hit (429); back off and retry")]
    RateLimited,
    #[error("TMDB request failed: {status} for {path}")]
    Status {
        status: reqwest::StatusCode,
        path: String,
    },
    #[error("TMDB transport error: {0}")]
    Transport(#[from] reqwest::Error),
}

impl TmdbClient {
    pub fn new(http: reqwest::Client, token: String, image_base_url: String) -> Self {
        Self {
            http,
            token,
            image_base_url,
        }
    }

    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, TmdbError> {
        let res = self
            .http
            .get(format!("{TMDB_BASE}{path}"))
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await?;

        if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(TmdbError::RateLimited);
        }
        if !res.status().is_success() {
            return Err(TmdbError::Status {
                status: res.status(),
                path: path.to_string(),
            });
        }
        Ok(res.json::<T>().await?)
    }

    /// Search TV series by name.
    pub async fn search_shows(&self, query: &str) -> Result<TmdbSearchResponse, TmdbError> {
        self.get("/search/tv", &[("query", query)]).await
    }

    /// Full series detail including the season list.
    pub async fn get_show(&self, tmdb_id: i64) -> Result<TmdbShowDetail, TmdbError> {
        self.get(&format!("/tv/{tmdb_id}"), &[]).await
    }

    /// A season's episodes.
    pub async fn get_season(
        &self,
        tmdb_id: i64,
        season_number: i32,
    ) -> Result<TmdbSeasonDetail, TmdbError> {
        self.get(&format!("/tv/{tmdb_id}/season/{season_number}"), &[])
            .await
    }

    /// Build a full image URL from a TMDB-relative path.
    pub fn image_url(&self, path: Option<&str>, size: &str) -> Option<String> {
        path.map(|p| format!("{}/{}{}", self.image_base_url, size, p))
    }
}

// --- Minimal response shapes (expand as endpoints are wired up) ---

#[derive(Debug, Deserialize)]
pub struct TmdbSearchResult {
    pub id: i64,
    pub name: String,
    pub first_air_date: Option<String>,
    pub poster_path: Option<String>,
    pub overview: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TmdbSearchResponse {
    pub page: i32,
    pub results: Vec<TmdbSearchResult>,
    pub total_results: i32,
}

#[derive(Debug, Deserialize)]
pub struct TmdbSeasonSummary {
    pub season_number: i32,
    pub name: String,
    pub episode_count: i32,
}

#[derive(Debug, Deserialize)]
pub struct TmdbShowDetail {
    pub id: i64,
    pub name: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub first_air_date: Option<String>,
    pub vote_average: Option<f64>,
    pub vote_count: Option<i32>,
    pub seasons: Vec<TmdbSeasonSummary>,
}

#[derive(Debug, Deserialize)]
pub struct TmdbEpisode {
    pub id: i64,
    pub season_number: i32,
    pub episode_number: i32,
    pub name: String,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub still_path: Option<String>,
    /// TMDB's own audience rating (0–10) and the number of votes behind it.
    pub vote_average: Option<f64>,
    pub vote_count: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct TmdbSeasonDetail {
    pub season_number: i32,
    pub name: String,
    pub episodes: Vec<TmdbEpisode>,
}
