//! FillerKiller API — Rust + Axum.
//!
//! Catalog read path: search + show detail (import-on-demand) + episodes.
//! Voting endpoints land next.

mod auth;
mod config;
mod db;
mod error;
mod import;
mod models;
mod oauth;
mod rate_limit;
mod response;
mod scoring;
mod tmdb;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::auth::{CurrentUser, OptionalUser};
use crate::config::{AuthConfig, Config};
use crate::error::AppError;
use crate::models::{AggregateView, SearchItem, SearchResponse, ShowDetail, VoteResponse};
use crate::rate_limit::{IpRateLimiter, UserRateLimiter};
use crate::response::{cacheable_json, private_json, TTL_AGGREGATE, TTL_CATALOG, TTL_SEARCH};
use crate::scoring::{build_skip_guide, ContestedHandling, ScoredEpisode, VoteValue};
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
    /// Per-IP vote rate limiter (in-memory, per-instance, best-effort).
    pub rate_limiter: Arc<IpRateLimiter>,
    /// Per-user vote rate limiter, keyed on the verified JWT user id (unspoofable).
    pub user_rate_limiter: Arc<UserRateLimiter>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    let config = Config::from_env()?;

    // Maintenance subcommands run an explicit step and exit, so ephemeral /
    // multi-instance deploys don't do schema work on every cold start.
    match std::env::args().nth(1).as_deref() {
        Some("migrate") => return run_migrate(&config).await,
        Some("recompute-scores") => return run_recompute_scores(&config).await,
        Some(other) => anyhow::bail!(
            "unknown subcommand {other:?}; expected `migrate` or `recompute-scores`"
        ),
        None => {}
    }

    // Lazy connect: the binary boots without a live DB, and serverless Postgres
    // can be cold. Keep the pool small — serverless Postgres caps connections.
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect_lazy(&config.database_url)?;

    // Migrations are an explicit deploy step by default (`migrate` subcommand).
    // A single-instance box can opt into boot migrations for a one-command deploy.
    if config.run_migrations_on_boot {
        match sqlx::migrate!("./migrations").run(&pool).await {
            Ok(_) => tracing::info!("migrations applied on boot"),
            Err(e) => tracing::warn!("boot migration skipped/failed (DB unreachable?): {e}"),
        }
    } else {
        tracing::info!("skipping boot migrations; run the `migrate` subcommand as a deploy step");
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
        rate_limiter: rate_limit::ip_limiter(config.vote_rate_per_minute),
        user_rate_limiter: rate_limit::user_limiter(config.vote_rate_per_minute),
    };

    // Evict idle per-IP buckets periodically so the keyed limiter doesn't grow
    // unbounded on a long-lived instance (a no-op on short-lived serverless ones).
    {
        let ip = state.rate_limiter.clone();
        let user = state.user_rate_limiter.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(300));
            loop {
                tick.tick().await;
                ip.retain_recent();
                user.retain_recent();
            }
        });
    }

    // Vote writes carry an extra per-IP rate-limit layer (defense in depth; the
    // edge CDN is the authoritative limiter — see `rate_limit`).
    let vote_routes = Router::new()
        .route("/api/episodes/{id}/vote", put(put_vote).delete(delete_vote))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit::limit_votes,
        ));

    let mut app = Router::new()
        .route("/health", get(health))
        .route("/health/db", get(health_db))
        .route("/api/search", get(search))
        .route("/api/shows/{id}", get(get_show))
        .route("/api/shows/{id}/episodes", get(get_show_episodes))
        .route("/api/shows/{id}/skip-guide", get(get_skip_guide))
        .merge(vote_routes)
        .route("/api/auth/{provider}/login", get(oauth_login))
        .route("/api/auth/{provider}/callback", get(oauth_callback))
        .route("/api/auth/logout", post(logout))
        .route("/api/me", get(me))
        // Unknown /api paths return our JSON 404 (not the SPA shell), so a
        // mistyped/removed endpoint can't return 200 HTML and break the client's
        // JSON parsing. Specific routes above take precedence over this catch-all.
        .route("/api/{*rest}", any(api_not_found));

    // Serve the built SPA same-origin as a fallback when configured (one service
    // on Cloud Run); otherwise expose API service-info at "/" (the box serves the
    // SPA via Caddy). ServeDir falls back to index.html so client-side routes
    // resolve to the app shell.
    app = match config.static_dir.as_deref() {
        Some(dir) if std::path::Path::new(dir).is_dir() => {
            let index = ServeFile::new(format!("{dir}/index.html"));
            tracing::info!("serving SPA from {dir}");
            app.fallback_service(ServeDir::new(dir).fallback(index))
        }
        Some(dir) => {
            tracing::warn!("STATIC_DIR {dir:?} not found; serving API only");
            app.route("/", get(root))
        }
        None => app.route("/", get(root)),
    };

    let app = app
        .layer(axum::middleware::from_fn(security_headers))
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

/// Connect eagerly for one-shot subcommands so they fail loudly if the DB is
/// unreachable (unlike the server's lazy pool).
async fn connect_pool(config: &Config) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .connect(&config.database_url)
        .await?;
    Ok(pool)
}

/// `migrate` subcommand: apply pending migrations, then exit. Run this as an
/// explicit deploy step for ephemeral/multi-instance compute.
async fn run_migrate(config: &Config) -> anyhow::Result<()> {
    let pool = connect_pool(config).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!("migrations applied");
    Ok(())
}

/// `recompute-scores` subcommand: rebuild `episode_score` from the source votes,
/// then exit. Drift correction / backfill; safe to run on a schedule.
async fn run_recompute_scores(config: &Config) -> anyhow::Result<()> {
    let pool = connect_pool(config).await?;
    let n = db::recompute_all_scores(&pool).await?;
    tracing::info!("recomputed episode_score for {n} episode(s)");
    Ok(())
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

/// Baseline security response headers on every response. The app is served
/// directly by Cloud Run (no edge proxy in front), so these must originate here.
/// CSP allows the SPA's own assets + inline styles (the vote-ratio bars use
/// inline `width`) and TMDB images; everything else is same-origin.
async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert(
        "referrer-policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    h.insert(
        "strict-transport-security",
        HeaderValue::from_static("max-age=63072000; includeSubDomains"),
    );
    h.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' https://image.tmdb.org data:; \
             style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; \
             frame-ancestors 'none'; base-uri 'self'; form-action 'self'",
        ),
    );
    res
}

async fn root() -> impl IntoResponse {
    Json(json!({
        "service": "fillerkiller-api",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// JSON 404 for unknown `/api/*` paths, so they return our error shape instead
/// of falling through to the SPA shell (which would be 200 HTML).
async fn api_not_found() -> AppError {
    AppError::NotFound("no such API endpoint".into())
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
    let imported: HashMap<i64, db::ImportedShow> = db::imported_show_ids(&state.pool, &tmdb_ids)
        .await?
        .into_iter()
        .map(|s| (s.tmdb_id, s))
        .collect();

    let results = found
        .results
        .into_iter()
        .map(|r| {
            let imp = imported.get(&r.id);
            SearchItem {
                show_id: imp.map(|s| s.id),
                slug: imp.map(|s| s.slug.clone()),
                tmdb_id: r.id,
                name: r.name,
                first_air_year: r.first_air_date.as_deref().and_then(import::parse_year),
                poster_path: r.poster_path,
                filler_coverage: None, // computed with the voting layer
            }
        })
        .collect();

    Ok(cacheable_json(&SearchResponse { results }, TTL_SEARCH))
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
        slug: core.slug,
        overview: core.overview,
        poster_path: core.poster_path,
        seasons,
    };
    Ok(cacheable_json(&detail, TTL_CATALOG))
}

#[derive(Debug, Deserialize)]
struct EpisodesParams {
    season: Option<i32>,
}

/// `GET /api/shows/{id}/episodes?season=` — episodes with aggregate scores, and
/// `myVote` when signed in. Anonymous responses are shared-cacheable briefly;
/// signed-in responses carry per-user data, so they are never cached.
async fn get_show_episodes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<EpisodesParams>,
    OptionalUser(user): OptionalUser,
) -> Result<Response, AppError> {
    let show_id = import::resolve_show_id(&state, &id).await?;
    let user_id = user.as_ref().map(|u| u.id);
    let episodes = db::episodes_with_scores(&state.pool, show_id, params.season, user_id).await?;
    let body = models::EpisodesResponse { episodes };
    Ok(match user_id {
        Some(_) => private_json(&body),
        None => cacheable_json(&body, TTL_AGGREGATE),
    })
}

#[derive(Debug, Deserialize)]
struct SkipGuideParams {
    /// How to place CONTESTED / NOT_ENOUGH_VOTES episodes: canon (default) | filler | show.
    contested: Option<String>,
    /// Include specials (season 0) in the guide. Default false.
    specials: Option<bool>,
}

/// `GET /api/shows/{id}/skip-guide` — the canon-only watch order (+ optional and
/// skipped lists) for a show. Derived from current votes, so cached briefly.
async fn get_skip_guide(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<SkipGuideParams>,
) -> Result<Response, AppError> {
    let contested = match params.contested.as_deref() {
        None | Some("canon") => ContestedHandling::Canon,
        Some("filler") => ContestedHandling::Filler,
        Some("show") => ContestedHandling::Show,
        Some(other) => {
            return Err(AppError::BadRequest(format!("invalid contested value: {other:?}")))
        }
    };

    let show_id = import::resolve_show_id(&state, &id).await?;
    let episodes = db::episodes_with_scores(&state.pool, show_id, None, None).await?;
    let scored: Vec<ScoredEpisode> = episodes
        .into_iter()
        .map(|e| ScoredEpisode {
            episode_id: e.id.to_string(),
            season_number: e.season_number,
            episode_number: e.episode_number,
            name: e.name,
            filler_votes: e.score.filler_votes,
            worth_watching_votes: e.score.worth_watching_votes,
            canon_votes: e.score.canon_votes,
        })
        .collect();

    let guide = build_skip_guide(&scored, contested, params.specials.unwrap_or(false));
    Ok(cacheable_json(&guide, TTL_AGGREGATE))
}

#[derive(Debug, Deserialize)]
struct VoteBody {
    value: VoteValue,
}

/// Build the vote response (caller's vote + fresh aggregate) for an episode.
async fn vote_response(
    state: &AppState,
    episode_id: Uuid,
    my_vote: Option<VoteValue>,
) -> Result<VoteResponse, AppError> {
    let (f, w, c) = db::episode_aggregate(&state.pool, episode_id).await?;
    Ok(VoteResponse {
        my_vote,
        score: AggregateView {
            filler_votes: f,
            worth_watching_votes: w,
            canon_votes: c,
            filler_score: scoring::filler_score(f, w, c),
            status: scoring::status(f, w, c),
        },
    })
}

/// `PUT /api/episodes/{id}/vote` — cast or change the caller's vote. Auth required.
async fn put_vote(
    State(state): State<AppState>,
    Path(id): Path<String>,
    user: CurrentUser,
    // `Result<..>` so a malformed/invalid body returns our JSON error shape
    // (400) instead of Axum's default plain-text 422.
    body: Result<Json<VoteBody>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, AppError> {
    rate_limit::check_user(&state.user_rate_limiter, user.id)?;
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.body_text()))?;
    let episode_id =
        Uuid::parse_str(&id).map_err(|_| AppError::BadRequest(format!("invalid episode id: {id:?}")))?;
    if !db::episode_exists(&state.pool, episode_id).await? {
        return Err(AppError::NotFound(format!("episode {episode_id} not found")));
    }

    db::upsert_vote(&state.pool, user.id, episode_id, body.value.as_db()).await?;
    let resp = vote_response(&state, episode_id, Some(body.value)).await?;
    Ok(private_json(&resp))
}

/// `DELETE /api/episodes/{id}/vote` — remove the caller's vote. Auth required.
async fn delete_vote(
    State(state): State<AppState>,
    Path(id): Path<String>,
    user: CurrentUser,
) -> Result<Response, AppError> {
    rate_limit::check_user(&state.user_rate_limiter, user.id)?;
    let episode_id =
        Uuid::parse_str(&id).map_err(|_| AppError::BadRequest(format!("invalid episode id: {id:?}")))?;
    if !db::episode_exists(&state.pool, episode_id).await? {
        return Err(AppError::NotFound(format!("episode {episode_id} not found")));
    }

    db::delete_vote(&state.pool, user.id, episode_id).await?;
    let resp = vote_response(&state, episode_id, None).await?;
    Ok(private_json(&resp))
}

// ---- Auth (OAuth -> stateless JWT in an httpOnly cookie) --------------------

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
