//! FillerKiller API — Rust + Axum.
//!
//! Foundation: config, DB pool,
//! server-side TMDB client, the scoring module, and health routes. Feature
//! endpoints (search, shows, episodes, vote, skip-guide) are wired up next,
//! against the contract in the design notes.

mod config;
mod scoring;
mod tmdb;

use std::time::Duration;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::tmdb::TmdbClient;

/// Shared application state. All fields are cheaply cloneable (pool + client are
/// reference-counted), so the state is cloned per request.
#[derive(Clone)]
struct AppState {
    pool: PgPool,
    #[allow(dead_code)] // used once catalog/TMDB endpoints land
    tmdb: TmdbClient,
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
async fn health_db(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query("SELECT 1").execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok", "db": "up" }))),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "down", "error": e.to_string() })),
        ),
    }
}
