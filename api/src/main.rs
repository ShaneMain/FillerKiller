//! FillerKiller API — Rust + Axum.
//!
//! Catalog read path: search + show detail (import-on-demand) + episodes, per
//! the design notes. Voting endpoints land next.

mod config;
mod db;
mod error;
mod import;
mod models;
mod response;
mod scoring;
mod tmdb;

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::config::Config;
use crate::error::AppError;
use crate::models::{SearchItem, SearchResponse, ShowDetail};
use crate::response::cacheable_json;
use crate::tmdb::TmdbClient;

/// Shared application state. All fields are cheaply cloneable (pool + client are
/// reference-counted), so the state is cloned per request.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub tmdb: TmdbClient,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    let config = Config::from_env()?;

    // Lazy connect: the binary boots without a live DB, and serverless Postgres
    // can be cold. Keep the pool small — serverless Postgres caps connections.
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect_lazy(&config.database_url)?;

    // Best-effort migrations on boot. If the DB is unreachable we still serve
    // liveness; readiness (/health/db) will report the problem.
    match sqlx::migrate!("./migrations").run(&pool).await {
        Ok(_) => tracing::info!("migrations applied"),
        Err(e) => tracing::warn!("migrations skipped/failed (DB unreachable?): {e}"),
    }

    let tmdb = TmdbClient::new(
        reqwest::Client::new(),
        config.tmdb_token.clone(),
        config.tmdb_image_base_url.clone(),
    );

    let cors = build_cors(&config.cors_allowed_origin);
    let state = AppState { pool, tmdb };

    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/health/db", get(health_db))
        .route("/api/search", get(search))
        .route("/api/shows/{id}", get(get_show))
        .route("/api/shows/{id}/episodes", get(get_show_episodes))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!("FillerKiller API listening on {}", config.bind_addr);
    axum::serve(listener, app).await?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer())
        .init();
}

fn build_cors(origin: &str) -> CorsLayer {
    use axum::http::{HeaderValue, Method};
    match origin.parse::<HeaderValue>() {
        Ok(value) => CorsLayer::new()
            .allow_origin(value)
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_credentials(true)
            .allow_headers([axum::http::header::CONTENT_TYPE]),
        Err(_) => {
            tracing::warn!("invalid CORS_ALLOWED_ORIGIN {origin:?}; CORS left disabled");
            CorsLayer::new()
        }
    }
}

async fn root() -> impl IntoResponse {
    Json(json!({
        "service": "fillerkiller-api",
        "version": env!("CARGO_PKG_VERSION"),
        "docs": "see internal docs",
    }))
}

/// Liveness: process is up. Does not touch the database.
async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

/// Readiness: can we reach Postgres? May be slow on a cold serverless DB.
/// The error detail is logged, never returned (it can include connection info).
async fn health_db(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query("SELECT 1").execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok", "db": "up" }))),
        Err(e) => {
            tracing::error!("DB readiness check failed: {e:#}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "degraded", "db": "down" })),
            )
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchParams {
    q: String,
}

/// `GET /api/search?q=` — proxy TMDB search, annotating results with our show id
/// when we've already imported them. Cached briefly.
async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Response, AppError> {
    let q = params.q.trim();
    if q.is_empty() {
        return Err(AppError::BadRequest("query parameter `q` is required".into()));
    }

    let found = state.tmdb.search_shows(q).await?;
    let tmdb_ids: Vec<i64> = found.results.iter().map(|r| r.id).collect();
    let imported: HashMap<i64, Uuid> = db::imported_show_ids(&state.pool, &tmdb_ids)
        .await?
        .into_iter()
        .collect();

    let results = found
        .results
        .into_iter()
        .map(|r| SearchItem {
            show_id: imported.get(&r.id).copied(),
            tmdb_id: r.id,
            name: r.name,
            first_air_year: r.first_air_date.as_deref().and_then(import::parse_year),
            poster_path: r.poster_path,
            filler_coverage: None, // computed with the voting layer
        })
        .collect();

    Ok(cacheable_json(&SearchResponse { results }, 600))
}

/// `GET /api/shows/{id}` — show detail with seasons. `{id}` is our uuid, or
/// `tmdb:<n>` to import-on-demand. Catalog data → longer cache.
async fn get_show(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let show_id = import::resolve_show_id(&state, &id).await?;
    let core = db::find_show_core(&state.pool, show_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("show {show_id} not found")))?;
    let seasons = db::seasons_with_counts(&state.pool, show_id).await?;

    let detail = ShowDetail {
        id: core.id,
        tmdb_id: core.tmdb_id,
        name: core.name,
        overview: core.overview,
        poster_path: core.poster_path,
        seasons,
    };
    Ok(cacheable_json(&detail, 3600))
}

#[derive(Debug, Deserialize)]
struct EpisodesParams {
    season: Option<i32>,
}

/// `GET /api/shows/{id}/episodes?season=` — episodes with aggregate scores.
/// Scores change with votes, so cache only briefly.
async fn get_show_episodes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<EpisodesParams>,
) -> Result<Response, AppError> {
    let show_id = import::resolve_show_id(&state, &id).await?;
    let episodes = db::episodes_with_scores(&state.pool, show_id, params.season).await?;
    Ok(cacheable_json(&models::EpisodesResponse { episodes }, 60))
}
