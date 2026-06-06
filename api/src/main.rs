//! FillerKiller API — Rust + Axum.
//!
//! Catalog read path: search + show detail (import-on-demand) + episodes, per
//! the design notes. Voting endpoints land next.

mod auth;
mod config;
mod db;
mod error;
mod import;
mod models;
mod oauth;
mod response;
mod scoring;
mod tmdb;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::auth::OptionalUser;
use crate::config::{AuthConfig, Config};
use crate::error::AppError;
use crate::models::{SearchItem, SearchResponse, ShowDetail};
use crate::response::cacheable_json;
use crate::tmdb::TmdbClient;

/// Shared application state. All fields are cheaply cloneable (pool, clients, and
/// the Arc'd config are reference-counted), so the state is cloned per request.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub tmdb: TmdbClient,
    /// Shared HTTP client for outbound calls (OAuth token/userinfo).
    pub http: reqwest::Client,
    pub auth: Arc<AuthConfig>,
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

    let http = reqwest::Client::new();
    let tmdb = TmdbClient::new(
        http.clone(),
        config.tmdb_token.clone(),
        config.tmdb_image_base_url.clone(),
    );

    let cors = build_cors(&config.cors_allowed_origin);
    let state = AppState {
        pool,
        tmdb,
        http,
        auth: Arc::new(config.auth.clone()),
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/health/db", get(health_db))
        .route("/api/search", get(search))
        .route("/api/shows/{id}", get(get_show))
        .route("/api/shows/{id}/episodes", get(get_show_episodes))
        .route("/api/auth/{provider}/login", get(oauth_login))
        .route("/api/auth/{provider}/callback", get(oauth_callback))
        .route("/api/auth/logout", post(logout))
        .route("/api/me", get(me))
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

// ---- Auth -------------------------

/// `GET /api/auth/{provider}/login` — redirect to the provider with a CSRF state.
async fn oauth_login(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    let p = state
        .auth
        .provider(&provider)
        .ok_or_else(|| AppError::NotFound(format!("unknown or disabled provider: {provider}")))?;

    // A random, unguessable CSRF state echoed back by the provider.
    let csrf = format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let redirect_uri = format!("{}/api/auth/{}/callback", state.auth.base_url, provider);
    let url = p.authorize_url(&redirect_uri, &csrf);

    let jar = jar.add(auth::state_cookie(csrf, state.auth.cookie_secure));
    Ok((jar, Redirect::to(&url)).into_response())
}

#[derive(Debug, Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    /// Set by the provider when the user denies consent or an error occurs.
    error: Option<String>,
}

/// `GET /api/auth/{provider}/callback` — verify state, exchange code, upsert the
/// user, set the session cookie, and return the browser to the SPA.
async fn oauth_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(params): Query<CallbackParams>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    let p = state
        .auth
        .provider(&provider)
        .ok_or_else(|| AppError::NotFound(format!("unknown or disabled provider: {provider}")))?;

    // The provider can bounce back with ?error= (e.g. user denied consent) and
    // no code. Send the browser back to the SPA with a generic failure flag
    // (the real reason is logged, not reflected, to avoid URL injection).
    if let Some(err) = params.error.as_deref() {
        tracing::info!("oauth callback error from {provider}: {err}");
        let jar = jar.remove(auth::STATE_COOKIE);
        let url = format!("{}?auth_error=signin_failed", state.auth.web_post_login_url);
        return Ok((jar, Redirect::to(&url)).into_response());
    }

    let code = params
        .code
        .ok_or_else(|| AppError::BadRequest("missing authorization code".into()))?;
    let returned_state = params
        .state
        .ok_or_else(|| AppError::BadRequest("missing OAuth state".into()))?;

    // CSRF: the state cookie we set at login must match the returned state.
    let expected = jar
        .get(auth::STATE_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(|| AppError::BadRequest("missing OAuth state".into()))?;
    if expected != returned_state {
        return Err(AppError::BadRequest("OAuth state mismatch".into()));
    }

    let redirect_uri = format!("{}/api/auth/{}/callback", state.auth.base_url, provider);
    let access_token = p.exchange_code(&state.http, &code, &redirect_uri).await?;
    let user = p.fetch_user(&state.http, &access_token).await?;

    let user_id = db::upsert_user_by_email(&state.pool, &user.email, user.name.as_deref()).await?;
    let token = auth::issue_jwt(&state.auth.jwt_secret, user_id, &user.email, user.name.as_deref())?;

    let jar = jar
        .remove(auth::STATE_COOKIE)
        .add(auth::session_cookie(token, state.auth.cookie_secure));
    Ok((jar, Redirect::to(&state.auth.web_post_login_url)).into_response())
}

/// `GET /api/me` — current user, decoded from the cookie (no DB read).
async fn me(OptionalUser(user): OptionalUser) -> impl IntoResponse {
    match user {
        Some(u) => Json(json!({ "id": u.id, "email": u.email, "displayName": u.name })),
        None => Json(serde_json::Value::Null),
    }
}

/// `POST /api/auth/logout` — clear the session cookie.
async fn logout(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    let jar = jar.add(auth::clear_session_cookie(state.auth.cookie_secure));
    (jar, StatusCode::NO_CONTENT)
}
