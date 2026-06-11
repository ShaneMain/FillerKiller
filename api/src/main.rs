//! FillerKiller API — Rust + Axum.
//!
//! Catalog read path: search + show detail (import-on-demand) + episodes.
//! Voting endpoints land next.

mod auth;
mod config;
mod db;
mod error;
mod guides;
mod import;
mod models;
mod oauth;
mod og;
mod rate_limit;
mod response;
mod scoring;
mod seo;
mod telemetry;
mod tmdb;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
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
use crate::models::{
    AggregateView, PopularShowItem, PopularShowsResponse, SearchItem, SearchResponse, ShowDetail,
    VoteResponse,
};
use crate::rate_limit::{DirectRateLimiter, IpRateLimiter, UserRateLimiter};
use crate::response::{
    cacheable_json, private_json, TTL_AGGREGATE, TTL_CATALOG, TTL_POPULAR, TTL_SEARCH,
};
use crate::scoring::{build_skip_guide, GuideMode, ScoredEpisode, VoteValue};
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
    /// Per-IP limiter on `/api/search`: every search proxies an outbound TMDB
    /// call on our token, so bound how fast any one client can burn that budget.
    pub search_limiter: Arc<IpRateLimiter>,
    /// Global (per-instance) limiter on TMDB import-on-demand. The import path is
    /// unauthenticated and fans out several upstream calls, so bound how often a
    /// fresh show can be imported to cap the outbound load any traffic can induce.
    pub import_limiter: Arc<DirectRateLimiter>,
    /// The built SPA's `index.html`, used as the template for server-rendered SEO
    /// pages. `None` when not serving static files (the box deploy serves the SPA
    /// via Caddy, so there is no server-side rendering to do).
    pub index_html: Option<Arc<String>>,
    /// Hours before a viewed RECENT show is re-synced from TMDB (background).
    pub refresh_ttl_hours: i32,
    /// Hours before a viewed ENDED show (no episode in ~2 years) is re-synced.
    pub refresh_ttl_hours_ended: i32,
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
        Some("refresh-catalog") => return run_refresh_catalog(&config).await,
        Some(other) => anyhow::bail!(
            "unknown subcommand {other:?}; expected `migrate`, `recompute-scores`, or `refresh-catalog`"
        ),
        None => {}
    }

    // Install the global metrics recorder before anything can emit. Long-running
    // serve only (the maintenance subcommands above return first).
    let metrics_handle = telemetry::install_recorder()?;

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

    // Bound outbound calls: a hung TMDB/OAuth endpoint must not hold a request
    // (and, during an import, its DB transaction) open indefinitely and exhaust
    // the pool. Both timeouts are generous relative to normal upstream latency.
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()?;
    let tmdb = TmdbClient::new(
        http.clone(),
        config.tmdb_token.clone(),
        config.tmdb_image_base_url.clone(),
    );

    // Load the SPA shell once for server-side SEO rendering (single-container
    // deploy only; absent on the Caddy box deploy where static_dir is unset).
    let index_html = config
        .static_dir
        .as_deref()
        .filter(|d| std::path::Path::new(d).is_dir())
        .and_then(|d| std::fs::read_to_string(format!("{d}/index.html")).ok())
        .map(Arc::new);

    let cors = build_cors(&config.cors_allowed_origin);
    let state = AppState {
        pool,
        tmdb,
        http,
        auth: Arc::new(config.auth.clone()),
        rate_limiter: rate_limit::ip_limiter(config.vote_rate_per_minute),
        user_rate_limiter: rate_limit::user_limiter(config.vote_rate_per_minute),
        search_limiter: rate_limit::ip_limiter(config.search_rate_per_minute),
        import_limiter: rate_limit::direct_limiter(config.import_rate_per_minute),
        index_html,
        refresh_ttl_hours: config.refresh_ttl_hours,
        refresh_ttl_hours_ended: config.refresh_ttl_hours_ended,
    };

    // Evict idle per-IP buckets periodically so the keyed limiter doesn't grow
    // unbounded on a long-lived instance (a no-op on short-lived serverless ones).
    {
        let ip = state.rate_limiter.clone();
        let user = state.user_rate_limiter.clone();
        let search = state.search_limiter.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(300));
            loop {
                tick.tick().await;
                ip.retain_recent();
                user.retain_recent();
                search.retain_recent();
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

    // Search proxies TMDB (one outbound call per request on our token), so it
    // carries its own per-IP limiter.
    let search_routes = Router::new()
        .route("/api/search", get(search))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit::limit_search,
        ));

    let mut app = Router::new()
        .route("/health", get(health))
        .route("/health/db", get(health_db))
        .merge(search_routes)
        .route("/api/shows", get(list_popular_shows))
        .route("/img/t/p/{size}/{file}", get(img_proxy))
        .route("/api/shows/{id}", get(get_show))
        .route("/api/shows/{id}/episodes", get(get_show_episodes))
        .route("/api/shows/{id}/skip-guide", get(get_skip_guide))
        .route("/api/shows/{id}/guides", get(list_guides).post(post_guide))
        .route(
            "/api/guides/{id}",
            get(get_guide_detail).put(put_guide).delete(delete_guide_handler),
        )
        .route(
            "/api/guides/{id}/like",
            put(like_guide_handler).delete(unlike_guide_handler),
        )
        .merge(vote_routes)
        .route(
            "/api/episodes/{id}/watched",
            put(put_watched).delete(delete_watched),
        )
        .route("/api/auth/{provider}/login", get(oauth_login))
        .route("/api/auth/{provider}/callback", get(oauth_callback))
        .route("/api/auth/logout", post(logout))
        .route("/api/me", get(me).put(update_me).delete(delete_me))
        .route("/api/me/guides", get(my_guides))
        // Unknown /api paths return our JSON 404 (not the SPA shell), so a
        // mistyped/removed endpoint can't return 200 HTML and break the client's
        // JSON parsing. Specific routes above take precedence over this catch-all.
        .route("/api/{*rest}", any(api_not_found))
        // Dynamic Open Graph card targeted by the show/skip-guide pages'
        // `og:image`. Matchit can't combine a param with a literal suffix in one
        // segment ("{slug}.png"), so the route captures the whole segment and
        // the handler strips the ".png" the published URLs carry.
        .route("/og/shows/{slug}", get(og_show_png));

    // Serve the built SPA same-origin as a fallback when configured (one service
    // on Cloud Run); otherwise expose API service-info at "/" (the box serves the
    // SPA via Caddy). ServeDir falls back to index.html so client-side routes
    // resolve to the app shell.
    app = match config.static_dir.as_deref() {
        Some(dir) if std::path::Path::new(dir).is_dir() => {
            let index = ServeFile::new(format!("{dir}/index.html"));
            tracing::info!("serving SPA from {dir}");
            // Server-render the SEO-critical routes (per-page meta + a crawlable
            // content snapshot); everything else falls through to the SPA shell.
            app.route("/shows/{slug}", get(show_html))
                .route("/shows/{slug}/skip-guide", get(skip_guide_html))
                .route("/shows/{slug}/guides/{guide_id}", get(guide_html))
                .route("/sitemap.xml", get(sitemap))
                .fallback_service(ServeDir::new(dir).fallback(index))
        }
        Some(dir) => {
            tracing::warn!("STATIC_DIR {dir:?} not found; serving API only");
            app.route("/", get(root))
        }
        None => app.route("/", get(root)),
    };

    let app = app
        // `route_layer` runs only for matched routes, so `MatchedPath` is set and
        // RED labels use the route template — and unmatched/static-fallback
        // requests don't generate metric series.
        .route_layer(axum::middleware::from_fn(telemetry::track_metrics))
        .layer(axum::middleware::from_fn(security_headers))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!("FillerKiller API listening on {}", config.bind_addr);
    let main_server = axum::serve(listener, app);

    // Serve `/metrics` on its own private listener (off the public ingress port)
    // when configured. Run it alongside the API; if either exits, so does the
    // process.
    match config.metrics_addr.clone() {
        Some(addr) => {
            let metrics_listener = tokio::net::TcpListener::bind(&addr).await?;
            tracing::info!("metrics listening on {addr} (/metrics, private)");
            let metrics_server = axum::serve(metrics_listener, telemetry::metrics_router(metrics_handle));
            tokio::try_join!(
                async { main_server.await.map_err(anyhow::Error::from) },
                async { metrics_server.await.map_err(anyhow::Error::from) },
            )?;
        }
        None => main_server.await?,
    }
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

/// `refresh-catalog` subcommand: re-import every show from TMDB to backfill cached
/// fields (e.g. TMDB ratings) on shows imported before those fields existed.
/// Best-effort — logs and continues past per-show failures. Preserves slugs and
/// never touches the vote/score layer. Run as a one-off ops step.
async fn run_refresh_catalog(config: &Config) -> anyhow::Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    const CONCURRENCY: usize = 8;

    let pool = PgPoolOptions::new()
        .max_connections(CONCURRENCY as u32)
        .acquire_timeout(Duration::from_secs(30))
        .connect(&config.database_url)
        .await?;
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()?;
    let tmdb = TmdbClient::new(
        http,
        config.tmdb_token.clone(),
        config.tmdb_image_base_url.clone(),
    );
    let ids = db::all_show_tmdb_ids(&pool).await?;
    let total = ids.len();
    tracing::info!("refreshing {total} shows from TMDB (concurrency {CONCURRENCY})");

    // Bound concurrency with a semaphore: each show is independent, so refresh up
    // to CONCURRENCY at once (gentle on TMDB, far faster than sequential).
    let sem = Arc::new(tokio::sync::Semaphore::new(CONCURRENCY));
    let done = Arc::new(AtomicUsize::new(0));
    let mut set = tokio::task::JoinSet::new();
    for id in ids {
        let permit = sem.clone().acquire_owned().await.expect("semaphore open");
        let (tmdb, pool, done) = (tmdb.clone(), pool.clone(), done.clone());
        set.spawn(async move {
            let _permit = permit;
            let ok = import::import_show(&tmdb, &pool, id).await.is_ok();
            if !ok {
                tracing::warn!("refresh show tmdb:{id} failed");
            }
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n % 25 == 0 || n == total {
                tracing::info!("refreshed {n}/{total}");
            }
            ok
        });
    }
    let mut ok = 0usize;
    while let Some(res) = set.join_next().await {
        if matches!(res, Ok(true)) {
            ok += 1;
        }
    }
    tracing::info!("catalog refresh done: {ok}/{total} shows updated");
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
            // The cloudflareinsights.com entries admit the Cloudflare Web
            // Analytics beacon (auto-injected at the edge): the script loads
            // from static.cloudflareinsights.com and reports to
            // cloudflareinsights.com.
            "default-src 'self'; img-src 'self' https://image.tmdb.org data:; \
             style-src 'self' 'unsafe-inline'; \
             script-src 'self' https://static.cloudflareinsights.com; \
             connect-src 'self' https://cloudflareinsights.com; \
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
    // Bound the query length — TMDB caps it anyway, and an unbounded `q` is just a
    // pointlessly large outbound request.
    if q.chars().count() > 200 {
        return Err(AppError::BadRequest("query parameter `q` is too long".into()));
    }

    let found = state.tmdb.search_shows(q).await?;
    let tmdb_ids: Vec<i64> = found.results.iter().map(|r| r.id).collect();
    let imported: HashMap<i64, db::ImportedShow> = db::imported_show_ids(&state.pool, &tmdb_ids)
        .await?
        .into_iter()
        .map(|s| (s.tmdb_id, s))
        .collect();

    // Fraction of each imported show's episodes the community has rated
    // confidently — a freshness signal in the result list.
    let imported_ids: Vec<Uuid> = imported.values().map(|s| s.id).collect();
    let coverage: HashMap<Uuid, f64> = if imported_ids.is_empty() {
        HashMap::new()
    } else {
        db::filler_coverage(&state.pool, &imported_ids, scoring::MIN_VOTES)
            .await?
            .into_iter()
            .collect()
    };

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
                // Imported shows report their coverage (0.0 when nothing's rated
                // yet); shows we haven't imported report null (no data).
                filler_coverage: imp.map(|s| coverage.get(&s.id).copied().unwrap_or(0.0)),
            }
        })
        .collect();

    Ok(cacheable_json(&SearchResponse { results }, TTL_SEARCH))
}

#[derive(Debug, Deserialize)]
struct PopularShowsParams {
    limit: Option<i64>,
}

/// Days an unpinned cached image survives without being re-pinned. Posters of
/// currently-popular shows are pinned and never expire; see `image_cache`.
const IMAGE_CACHE_TTL_DAYS: i32 = 14;

/// TMDB sizes the image proxy will serve — the ones the SPA and OG cards use.
/// Anything else is rejected rather than forwarded, so the proxy can't be used
/// to enumerate TMDB's CDN.
const IMAGE_SIZES: &[&str] = &["w92", "w154", "w185", "w300", "w342", "w500"];

/// Whether `{size}/{file}` is a well-formed TMDB image request: a whitelisted
/// size and a bare `{alnum}.jpg|jpeg|png` file name. Keeps the proxy from
/// fetching arbitrary upstream paths (no slashes, dots, or query syntax).
fn valid_image_request(size: &str, file: &str) -> bool {
    if !IMAGE_SIZES.contains(&size) {
        return false;
    }
    let Some((stem, ext)) = file.rsplit_once('.') else {
        return false;
    };
    matches!(ext, "jpg" | "jpeg" | "png")
        && !stem.is_empty()
        && stem.len() <= 64
        && stem.chars().all(|c| c.is_ascii_alphanumeric())
}

/// `GET /api/shows?limit=` — the most-voted shows, for the home page's browse
/// grid. Unauthenticated but deliberately NOT rate-limited (unlike search): it
/// spends no upstream quota, the query is bounded by the limit clamp, and the
/// edge cache (TTL_POPULAR) absorbs repeat traffic.
async fn list_popular_shows(
    State(state): State<AppState>,
    Query(params): Query<PopularShowsParams>,
) -> Result<Response, AppError> {
    let limit = params.limit.unwrap_or(12).clamp(1, 50);
    let popular = db::popular_shows(&state.pool, limit).await?;

    // Each card carries its OG-card stats ("X% filler — skip N of M"), derived
    // from the same scored-episode rows the card uses. One indexed query per
    // show, fanned out concurrently; the edge cache means this whole handler
    // runs at most once per TTL_POPULAR.
    let mut tasks = tokio::task::JoinSet::new();
    for (i, s) in popular.iter().enumerate() {
        let pool = state.pool.clone();
        let show_id = s.id;
        tasks.spawn(async move {
            (i, db::episodes_with_scores(&pool, show_id, None, None).await)
        });
    }
    let mut stats: Vec<og::OgStats> = vec![og::OgStats::default(); popular.len()];
    while let Some(joined) = tasks.join_next().await {
        let (i, episodes) = joined.map_err(|e| AppError::Internal(e.into()))?;
        match episodes {
            Ok(eps) => stats[i] = og::stats_from_episodes(&eps),
            // One show's stats failing shouldn't take the whole list down —
            // its card just renders without a verdict until the next refresh.
            Err(e) => tracing::warn!("popular stats query failed for show {i}: {e}"),
        }
    }

    let shows: Vec<PopularShowItem> = popular
        .into_iter()
        .zip(stats)
        .map(|(s, st)| PopularShowItem {
            slug: s.slug,
            tmdb_id: s.tmdb_id,
            name: s.name,
            first_air_year: s.first_air_year,
            poster_path: s.poster_path,
            episode_count: st.total(),
            filler_pct: st.filler_pct(),
            skip_count: st.filler,
            rated: st.total() > 0 && st.undecided < st.total(),
        })
        .collect();

    // Re-sync the image cache's pin set to this popular list (and prune expired
    // unpinned rows). Best-effort and off the response path: this read runs at
    // most once per edge-cache TTL, so it doubles as the cache's housekeeping
    // tick without needing a scheduler.
    let files: Vec<String> = shows
        .iter()
        .filter_map(|s| s.poster_path.as_deref())
        .map(|p| p.trim_start_matches('/').to_string())
        .collect();
    let pool = state.pool.clone();
    tokio::spawn(async move {
        match db::sync_pinned_images(&pool, &files, IMAGE_CACHE_TTL_DAYS).await {
            Ok(pruned) if pruned > 0 => tracing::info!("image cache pruned {pruned} rows"),
            Ok(_) => {}
            Err(e) => tracing::warn!("image cache pin sync failed: {e}"),
        }
    });

    Ok(cacheable_json(&PopularShowsResponse { shows }, TTL_POPULAR))
}

/// `GET /img/t/p/{size}/{file}` — same-origin TMDB image proxy backed by the
/// `image_cache` table. A hit serves straight from Postgres; a miss fetches
/// from TMDB's CDN and stores the body. Served `immutable` (TMDB image paths
/// are content-unique), so the Cloudflare edge absorbs nearly all traffic and
/// the DB sees roughly one read per image per edge eviction.
async fn img_proxy(
    State(state): State<AppState>,
    Path((size, file)): Path<(String, String)>,
) -> Result<Response, AppError> {
    if !valid_image_request(&size, &file) {
        return Err(AppError::BadRequest("not a valid image path".into()));
    }
    let key = format!("{size}/{file}");

    let cached = match db::get_cached_image(&state.pool, &key).await {
        Ok(hit) => hit,
        // A cache-read failure shouldn't take images down — fall through to TMDB.
        Err(e) => {
            tracing::warn!("image cache read failed for {key}: {e}");
            None
        }
    };
    let (content_type, body) = match cached {
        Some(img) => (img.content_type, img.body),
        None => {
            let url = state
                .tmdb
                .image_url(Some(&format!("/{file}")), &size)
                .expect("path is always Some");
            let res = state
                .http
                .get(&url)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
            if res.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(AppError::NotFound("no such image".into()));
            }
            if !res.status().is_success() {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "upstream image fetch returned {}",
                    res.status()
                )));
            }
            let content_type = res
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .filter(|v| v.starts_with("image/"))
                .unwrap_or("image/jpeg")
                .to_string();
            let body = res
                .bytes()
                .await
                .map_err(|e| AppError::Internal(e.into()))?
                .to_vec();
            // Posters at our sizes are tens of KB; refuse to cache or relay
            // anything implausibly large.
            const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
            if body.len() > MAX_IMAGE_BYTES {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "upstream image unreasonably large ({} bytes)",
                    body.len()
                )));
            }
            if let Err(e) = db::upsert_cached_image(&state.pool, &key, &content_type, &body).await {
                tracing::warn!("image cache write failed for {key}: {e}");
            }
            (content_type, body)
        }
    };

    let mut res = body.into_response();
    let headers = res.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&content_type) {
        headers.insert(axum::http::header::CONTENT_TYPE, v);
    }
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    Ok(res)
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
        tmdb_rating: core.tmdb_vote_average,
        tmdb_vote_count: core.tmdb_vote_count,
        seasons,
    };
    Ok(cacheable_json(&detail, TTL_CATALOG))
}

#[derive(Debug, Deserialize)]
struct EpisodesParams {
    season: Option<i32>,
}

/// `GET /api/shows/{id}/episodes?season=` — episodes with aggregate scores, and
/// `myVote` + `watched` when signed in. Anonymous responses are shared-cacheable
/// briefly; signed-in responses carry per-user data, so they are never cached.
async fn get_show_episodes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<EpisodesParams>,
    OptionalUser(user): OptionalUser,
) -> Result<Response, AppError> {
    let show_id = import::resolve_show_id(&state, &id).await?;
    let user_id = user.as_ref().map(|u| u.id);
    let (episodes, watched_count) = tokio::join!(
        db::episodes_with_scores(&state.pool, show_id, params.season, user_id),
        async {
            match user_id {
                Some(uid) => db::watch_count_for_show(&state.pool, uid, show_id).await.ok(),
                None => None,
            }
        },
    );
    let body = models::EpisodesResponse { episodes: episodes?, watched_count };
    Ok(match user_id {
        Some(_) => private_json(&body),
        None => cacheable_json(&body, TTL_AGGREGATE),
    })
}

#[derive(Debug, Deserialize)]
struct SkipGuideParams {
    /// Which skip-guide mode to use: completionist | standard (default) | canon-only | binge.
    mode: Option<String>,
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
    let mode = match params.mode.as_deref() {
        None | Some("standard") => GuideMode::Standard,
        Some("completionist") => GuideMode::Completionist,
        Some("canon-only") => GuideMode::CanonOnly,
        Some("binge") => GuideMode::Binge,
        Some(other) => {
            return Err(AppError::BadRequest(format!(
                "invalid mode: {other:?}; valid values: completionist, standard, canon-only, binge"
            )))
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
            tmdb_rating: e.tmdb_rating,
            runtime_minutes: e.runtime_minutes,
        })
        .collect();

    let guide = build_skip_guide(&scored, mode, params.specials.unwrap_or(false));
    Ok(cacheable_json(&guide, TTL_AGGREGATE))
}

// ---- Server-side SEO rendering (single-container deploy) --------------------

/// Shared-cacheable HTML response for a server-rendered SPA page. No per-user
/// data is embedded (the SPA fetches that client-side), so it's safe to cache.
fn html_response(html: String) -> Response {
    (
        [(axum::http::header::CACHE_CONTROL, "public, max-age=600")],
        Html(html),
    )
        .into_response()
}

/// Serve the SPA shell with a 404 status (and a short cache) for an SEO route
/// whose target doesn't exist — so crawlers see a true "not found" instead of a
/// soft 404, while the browser still renders the SPA's own not-found UI.
fn not_found_html(html: String) -> Response {
    (
        StatusCode::NOT_FOUND,
        [(axum::http::header::CACHE_CONTROL, "public, max-age=60")],
        Html(html),
    )
        .into_response()
}

/// Look up a show by slug only (never imports) for the server-rendered pages.
async fn show_core_by_slug(state: &AppState, slug: &str) -> Option<db::ShowCore> {
    match db::find_show_id_by_slug(&state.pool, slug).await {
        Ok(Some(id)) => db::find_show_core(&state.pool, id).await.ok().flatten(),
        _ => None,
    }
}

/// `GET /shows/{slug}` — show page with per-show SEO `<head>` + a crawlable
/// content snapshot injected into the shell. An unknown slug falls back to the
/// plain SPA shell, which resolves it (import / canonicalize) client-side.
async fn show_html(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    let Some(template) = state.index_html.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match show_core_by_slug(&state, &slug).await {
        Some(core) => {
            let seasons = db::seasons_with_counts(&state.pool, core.id)
                .await
                .unwrap_or_default();
            let image = state.tmdb.image_url(core.poster_path.as_deref(), "w500");
            html_response(seo::show_page(
                &template,
                &state.auth.base_url,
                &core,
                &seasons,
                image,
            ))
        }
        // Unknown show → real 404, not a soft 404; the SPA still renders its own
        // not-found page from the shell.
        None => not_found_html(template.as_str().to_string()),
    }
}

/// `GET /shows/{slug}/skip-guide` — server-rendered skip guide (the watch/skip
/// lists as real content) for crawlers; the SPA hydrates over it.
async fn skip_guide_html(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    let Some(template) = state.index_html.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match show_core_by_slug(&state, &slug).await {
        Some(core) => {
            let episodes = db::episodes_with_scores(&state.pool, core.id, None, None)
                .await
                .unwrap_or_default();
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
                    tmdb_rating: e.tmdb_rating,
                    runtime_minutes: e.runtime_minutes,
                })
                .collect();
            let guide = build_skip_guide(&scored, GuideMode::Standard, false);
            html_response(seo::skip_guide_page(
                &template,
                &state.auth.base_url,
                &core,
                &guide,
            ))
        }
        None => not_found_html(template.as_str().to_string()),
    }
}

/// `GET /shows/{slug}/guides/{guide_id}` — server-rendered share page for a
/// published user guide (per-guide head + the curated lists as content). Drafts,
/// unknown ids, and the SPA-only `/guides/new` route fall back to the plain shell.
async fn guide_html(
    State(state): State<AppState>,
    Path((_slug, guide_id)): Path<(String, String)>,
) -> Response {
    let Some(template) = state.index_html.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match Uuid::parse_str(&guide_id) {
        Ok(gid) => match guides::get_guide(&state.pool, gid, None).await {
            Ok(Some(guide)) if guide.is_published => {
                let image = state.tmdb.image_url(guide.poster_path.as_deref(), "w500");
                html_response(seo::guide_page(
                    &template,
                    &state.auth.base_url,
                    &guide,
                    image,
                ))
            }
            // A valid id that isn't a published guide (missing or a draft) → 404.
            _ => not_found_html(template.as_str().to_string()),
        },
        // Not a UUID (e.g. the SPA-only `/guides/new` route) → plain shell, 200.
        Err(_) => html_response(template.as_str().to_string()),
    }
}

/// `GET /sitemap.xml` — generated from the catalog: stable routes, every show
/// (+ its skip guide), and every published user guide, each with a `<lastmod>`.
async fn sitemap(State(state): State<AppState>) -> Response {
    let shows = db::sitemap_shows(&state.pool).await.unwrap_or_default();
    let guides = db::sitemap_guides(&state.pool).await.unwrap_or_default();
    let xml = seo::sitemap_xml(&state.auth.base_url, &shows, &guides);
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/xml; charset=utf-8",
            ),
            (axum::http::header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        xml,
    )
        .into_response()
}

/// `GET /og/shows/{slug}` (published as `/og/shows/{slug}.png`) — the dynamic
/// 1200×630 Open Graph card: poster + "X% filler — skip N of M episodes". The
/// stats are the same scored episodes the skip guide uses. The poster fetch is
/// best-effort (short timeout, degrades to a text-only card); only an unknown
/// slug or a render failure errors.
async fn og_show_png(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Response, AppError> {
    // Slugs never contain dots (see `db::slugify`), so a ".png" suffix is
    // unambiguously the published URL form.
    let slug = slug.strip_suffix(".png").unwrap_or(&slug);
    let core = show_core_by_slug(&state, slug)
        .await
        .ok_or_else(|| AppError::NotFound(format!("show {slug:?} not found")))?;
    // The episode query and the poster fetch are independent — overlap them.
    let (episodes, poster) = tokio::join!(
        db::episodes_with_scores(&state.pool, core.id, None, None),
        async {
            match state.tmdb.image_url(core.poster_path.as_deref(), "w342") {
                Some(url) => og::fetch_poster(&state.http, &url).await,
                None => None,
            }
        }
    );
    let stats = og::stats_from_episodes(&episodes?);

    let svg = og::og_svg(&core.name, &stats, poster.as_ref());
    // Rasterizing is CPU-bound (tens of ms); keep it off the async workers.
    let png = tokio::task::spawn_blocking(move || og::render_png(&svg))
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .map_err(AppError::Internal)?;

    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "image/png"),
            // Long edge TTL: the card only moves as votes accumulate, and the
            // Cloudflare edge absorbs the unfurler bursts a share produces.
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=3600, s-maxage=86400",
            ),
        ],
        png,
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
struct VoteBody {
    value: VoteValue,
    /// Optional reason tag for this vote. Must be one of the tags valid for
    /// `value`; omitting it (or explicitly `null`) clears any previous reason.
    reason: Option<scoring::VoteReason>,
}

/// Build the vote response (caller's vote + fresh aggregate) for an episode.
async fn vote_response(
    state: &AppState,
    episode_id: Uuid,
    my_vote: Option<VoteValue>,
    my_reason: Option<scoring::VoteReason>,
) -> Result<VoteResponse, AppError> {
    let (f, w, c) = db::episode_aggregate(&state.pool, episode_id).await?;
    let st = scoring::status(f, w, c);

    // Fetch reason counts only when the episode has a clear plurality verdict.
    let reason_counts = if let Some(pv) = scoring::plurality_value(st) {
        db::episode_reason_counts(&state.pool, episode_id, pv).await?
    } else {
        std::collections::HashMap::new()
    };

    Ok(VoteResponse {
        my_vote,
        my_reason,
        score: AggregateView {
            filler_votes: f,
            worth_watching_votes: w,
            canon_votes: c,
            filler_score: scoring::filler_score(f, w, c),
            status: st,
            reason_counts,
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

    // Validate that the supplied reason tag is valid for the vote value.
    if let Some(reason) = body.reason {
        if !reason.is_valid_for(body.value) {
            return Err(AppError::BadRequest(format!(
                "reason {:?} is not valid for a {:?} vote; valid reasons are: {}",
                reason.as_str(),
                body.value.as_db(),
                scoring::VoteReason::valid_for(body.value)
                    .iter()
                    .map(|r| r.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }

    let episode_id =
        Uuid::parse_str(&id).map_err(|_| AppError::BadRequest(format!("invalid episode id: {id:?}")))?;
    if !db::episode_exists(&state.pool, episode_id).await? {
        return Err(AppError::NotFound(format!("episode {episode_id} not found")));
    }

    db::upsert_vote(
        &state.pool,
        user.id,
        episode_id,
        body.value.as_db(),
        body.reason.map(|r| r.as_str()),
    )
    .await?;
    metrics::counter!("votes_total", "value" => body.value.as_db()).increment(1);
    let resp = vote_response(&state, episode_id, Some(body.value), body.reason).await?;
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
    let resp = vote_response(&state, episode_id, None, None).await?;
    Ok(private_json(&resp))
}

// ---- Watch progress ---------------------------------------------------------

/// `PUT /api/episodes/{id}/watched` — mark an episode as watched. Idempotent.
/// Auth required. Lower-stakes than voting, so no extra rate-limit layer.
async fn put_watched(
    State(state): State<AppState>,
    Path(id): Path<String>,
    user: CurrentUser,
) -> Result<Response, AppError> {
    let episode_id =
        Uuid::parse_str(&id).map_err(|_| AppError::BadRequest(format!("invalid episode id: {id:?}")))?;
    if !db::episode_exists(&state.pool, episode_id).await? {
        return Err(AppError::NotFound(format!("episode {episode_id} not found")));
    }
    db::upsert_watch(&state.pool, user.id, episode_id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `DELETE /api/episodes/{id}/watched` — unmark an episode as watched. Idempotent.
/// Auth required.
async fn delete_watched(
    State(state): State<AppState>,
    Path(id): Path<String>,
    user: CurrentUser,
) -> Result<Response, AppError> {
    let episode_id =
        Uuid::parse_str(&id).map_err(|_| AppError::BadRequest(format!("invalid episode id: {id:?}")))?;
    if !db::episode_exists(&state.pool, episode_id).await? {
        return Err(AppError::NotFound(format!("episode {episode_id} not found")));
    }
    db::delete_watch(&state.pool, user.id, episode_id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---- User-authored skip guides ----------------------------------------------

/// `GET /api/shows/{id}/guides` — published user guides for a show, most-liked
/// first. Carries the viewer's like/ownership flags when signed in.
async fn list_guides(
    State(state): State<AppState>,
    Path(id): Path<String>,
    OptionalUser(user): OptionalUser,
) -> Result<Response, AppError> {
    let show_id = import::resolve_show_id(&state, &id).await?;
    let list = guides::list_published(&state.pool, show_id, user.as_ref().map(|u| u.id)).await?;
    Ok(match user {
        Some(_) => private_json(&list),
        None => cacheable_json(&list, TTL_AGGREGATE),
    })
}

/// `POST /api/shows/{id}/guides` — create a guide for a show. Auth required.
async fn post_guide(
    State(state): State<AppState>,
    Path(id): Path<String>,
    user: CurrentUser,
    body: Result<Json<guides::GuideInput>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, AppError> {
    rate_limit::check_user(&state.user_rate_limiter, user.id)?;
    let Json(input) = body.map_err(|e| AppError::BadRequest(e.body_text()))?;
    let show_id = import::resolve_show_id(&state, &id).await?;
    let guide_id = guides::create_guide(&state.pool, show_id, user.id, &input).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": guide_id }))).into_response())
}

/// `GET /api/guides/{id}` — full guide detail. Drafts are visible only to their author.
async fn get_guide_detail(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    OptionalUser(user): OptionalUser,
) -> Result<Response, AppError> {
    let guide = guides::get_guide(&state.pool, id, user.as_ref().map(|u| u.id))
        .await?
        .filter(|g| g.is_published || g.mine)
        .ok_or_else(|| AppError::NotFound("guide not found".into()))?;
    Ok(match user {
        Some(_) => private_json(&guide),
        None => cacheable_json(&guide, TTL_AGGREGATE),
    })
}

/// `PUT /api/guides/{id}` — update a guide (author only).
async fn put_guide(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    user: CurrentUser,
    body: Result<Json<guides::GuideInput>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, AppError> {
    rate_limit::check_user(&state.user_rate_limiter, user.id)?;
    let Json(input) = body.map_err(|e| AppError::BadRequest(e.body_text()))?;
    guides::update_guide(&state.pool, id, user.id, &input).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `DELETE /api/guides/{id}` — delete a guide (author only).
async fn delete_guide_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    user: CurrentUser,
) -> Result<Response, AppError> {
    guides::delete_guide(&state.pool, id, user.id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `PUT /api/guides/{id}/like` — like a guide. Auth required.
async fn like_guide_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    user: CurrentUser,
) -> Result<Response, AppError> {
    rate_limit::check_user(&state.user_rate_limiter, user.id)?;
    guide_must_be_visible(&state, id, user.id).await?;
    let count = guides::like_guide(&state.pool, id, user.id).await?;
    Ok(private_json(&json!({ "likeCount": count, "myLike": true })))
}

/// `DELETE /api/guides/{id}/like` — remove a like. Auth required.
async fn unlike_guide_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    user: CurrentUser,
) -> Result<Response, AppError> {
    rate_limit::check_user(&state.user_rate_limiter, user.id)?;
    let count = guides::unlike_guide(&state.pool, id, user.id).await?;
    Ok(private_json(&json!({ "likeCount": count, "myLike": false })))
}

/// A guide is likeable only if it exists and is visible to the user (published,
/// or their own draft) — so a hidden draft can't be probed via the like endpoint.
async fn guide_must_be_visible(
    state: &AppState,
    guide_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    let meta = guides::guide_meta(&state.pool, guide_id)
        .await?
        .ok_or_else(|| AppError::NotFound("guide not found".into()))?;
    if !meta.is_published && meta.author_id != Some(user_id) {
        return Err(AppError::NotFound("guide not found".into()));
    }
    Ok(())
}

// ---- Auth (OAuth -> stateless JWT in an httpOnly cookie) --------------------

#[derive(Debug, Deserialize)]
struct LoginParams {
    /// Where to send the user after login — a site-relative path (e.g. the page
    /// they came from). Validated by `safe_next` to prevent an open redirect.
    next: Option<String>,
}

/// Validate a post-login `next` target: it must be a site-relative path (a single
/// leading `/`, not protocol-relative `//`, no backslashes or control chars), so
/// it can only ever redirect within our own origin. Otherwise `None`.
fn safe_next(next: &str) -> Option<String> {
    let ok = next.starts_with('/')
        && !next.starts_with("//")
        && !next.contains('\\')
        && !next.contains(['\n', '\r', '\t'])
        && next.len() <= 512;
    ok.then(|| next.to_string())
}

/// `GET /api/auth/{provider}/login` — redirect to the provider with a CSRF state,
/// remembering an optional `?next=` return path for after login.
async fn oauth_login(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(params): Query<LoginParams>,
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

    let mut jar = jar.add(auth::state_cookie(csrf, state.auth.cookie_secure));
    if let Some(next) = params.next.as_deref().and_then(safe_next) {
        jar = jar.add(auth::next_cookie(next, state.auth.cookie_secure));
    }
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
    let secure = state.auth.cookie_secure;
    if let Some(err) = params.error.as_deref() {
        tracing::info!("oauth callback error from {provider}: {err}");
        let jar = jar
            .add(auth::clear_cookie(auth::state_cookie_name(secure), secure))
            .add(auth::clear_cookie(auth::next_cookie_name(secure), secure));
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
        .get(auth::state_cookie_name(secure))
        .map(|c| c.value().to_string())
        .ok_or_else(|| AppError::BadRequest("missing OAuth state".into()))?;
    if expected != returned_state {
        return Err(AppError::BadRequest("OAuth state mismatch".into()));
    }

    let redirect_uri = format!("{}/api/auth/{}/callback", state.auth.base_url, provider);

    // Exchange the code and mint a session. A failure here (upstream hiccup, a
    // provider without a verified email) shouldn't dump a raw 502 JSON page on the
    // user mid-login: log the real cause and bounce back to the SPA with the same
    // generic failure flag the consent-denied path uses.
    let session = async {
        let access_token = p.exchange_code(&state.http, &code, &redirect_uri).await?;
        let user = p.fetch_user(&state.http, &access_token).await?;
        // Identity is keyed on the provider's stable subject id, never the
        // email alone — see `db::resolve_oauth_user`.
        let (user_id, ver) = db::resolve_oauth_user(
            &state.pool,
            p.kind.as_str(),
            &user.subject,
            &user.email,
            user.name.as_deref(),
        )
        .await?;
        // Honour a previously-set screen name over the OAuth profile name.
        let display = db::effective_display_name(&state.pool, user_id).await?;
        auth::issue_jwt(
            &state.auth.jwt_secret,
            user_id,
            &db::normalize_email(&user.email),
            display.as_deref(),
            ver,
        )
    }
    .await;

    let token = match session {
        Ok(token) => token,
        Err(e) => {
            tracing::warn!("oauth callback failed for {provider}: {e:?}");
            let jar = jar
                .add(auth::clear_cookie(auth::state_cookie_name(secure), secure))
                .add(auth::clear_cookie(auth::next_cookie_name(secure), secure));
            let url = format!("{}?auth_error=signin_failed", state.auth.web_post_login_url);
            return Ok((jar, Redirect::to(&url)).into_response());
        }
    };

    // Return the user to where they started (the validated `next` cookie), else home.
    let dest = match jar
        .get(auth::next_cookie_name(secure))
        .and_then(|c| safe_next(c.value()))
    {
        Some(path) => format!("{}{}", state.auth.web_post_login_url.trim_end_matches('/'), path),
        None => state.auth.web_post_login_url.clone(),
    };
    let jar = jar
        .add(auth::clear_cookie(auth::state_cookie_name(secure), secure))
        .add(auth::clear_cookie(auth::next_cookie_name(secure), secure))
        .add(auth::session_cookie(token, secure));
    Ok((jar, Redirect::to(&dest)).into_response())
}

/// `GET /api/me/guides` — the caller's own guides (published + drafts). Auth required.
async fn my_guides(State(state): State<AppState>, user: CurrentUser) -> Result<Response, AppError> {
    let list = guides::list_by_author(&state.pool, user.id).await?;
    Ok(private_json(&list))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMeBody {
    /// New screen name; trimmed, blank clears it (revert to the OAuth name).
    screen_name: Option<String>,
}

/// `PUT /api/me` — update the caller's profile (screen name). Re-issues the
/// session cookie so the new display name takes effect immediately. Auth required.
async fn update_me(
    State(state): State<AppState>,
    user: CurrentUser,
    jar: CookieJar,
    body: Result<Json<UpdateMeBody>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, AppError> {
    let Json(body) = body.map_err(|e| AppError::BadRequest(e.body_text()))?;
    let screen = body
        .screen_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(s) = screen {
        if s.chars().count() > 40 {
            return Err(AppError::BadRequest(
                "screen name must be at most 40 characters".into(),
            ));
        }
    }
    let display = db::set_screen_name(&state.pool, user.id, screen).await?;
    let token = auth::issue_jwt(
        &state.auth.jwt_secret,
        user.id,
        &user.email,
        display.as_deref(),
        user.token_version,
    )?;
    let jar = jar.add(auth::session_cookie(token, state.auth.cookie_secure));
    Ok((
        jar,
        private_json(&json!({ "id": user.id, "email": user.email, "displayName": display })),
    )
        .into_response())
}

/// `GET /api/me` — current user, decoded from the cookie (plus the extractor's
/// revocation check, so a deleted/logged-out-everywhere account reads as null).
async fn me(OptionalUser(user): OptionalUser) -> impl IntoResponse {
    match user {
        Some(u) => Json(json!({ "id": u.id, "email": u.email, "displayName": u.name })),
        None => Json(serde_json::Value::Null),
    }
}

/// `DELETE /api/me` — permanently delete the caller's account. Auth required.
/// The user's votes are retained but anonymized (the `vote.user_id` FK is
/// `ON DELETE SET NULL`), so community totals stay intact. Clears the session;
/// any *other* outstanding tokens die with the row (the extractor's
/// token_version lookup finds no user).
async fn delete_me(
    State(state): State<AppState>,
    user: CurrentUser,
    jar: CookieJar,
) -> Result<Response, AppError> {
    db::delete_user(&state.pool, user.id).await?;
    let jar = jar.add(auth::clear_session_cookie(state.auth.cookie_secure));
    Ok((jar, StatusCode::NO_CONTENT).into_response())
}

/// `POST /api/auth/logout` — clear the session cookie AND bump the user's
/// token_version, revoking every outstanding session (a stateless JWT can't be
/// revoked individually, and "I'm done / I suspect theft" both want all of them
/// dead). Still clears the cookie when the session is already invalid.
async fn logout(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    jar: CookieJar,
) -> Result<Response, AppError> {
    if let Some(u) = user {
        db::bump_token_version(&state.pool, u.id).await?;
    }
    let jar = jar.add(auth::clear_session_cookie(state.auth.cookie_secure));
    Ok((jar, StatusCode::NO_CONTENT).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_me_body_accepts_camelcase_screen_name() {
        // Crosses the actual wire contract: the SPA sends `screenName`.
        let set: UpdateMeBody = serde_json::from_str(r#"{"screenName":"Ann"}"#).unwrap();
        assert_eq!(set.screen_name.as_deref(), Some("Ann"));
        let cleared: UpdateMeBody = serde_json::from_str(r#"{"screenName":null}"#).unwrap();
        assert_eq!(cleared.screen_name, None);
    }

    #[test]
    fn image_request_validation_blocks_proxy_abuse() {
        assert!(valid_image_request("w154", "lP4zwr0F7hWTbAFltfoFTc2AxRG.jpg"));
        assert!(valid_image_request("w342", "a1B2.png"));
        assert!(valid_image_request("w92", "x.jpeg"));
        // Unknown size, traversal, nested paths, query smuggling, wrong/missing
        // extension, oversized stem — all rejected.
        assert!(!valid_image_request("original", "a.jpg"));
        assert!(!valid_image_request("w154", "..jpg"));
        assert!(!valid_image_request("w154", "a/b.jpg"));
        assert!(!valid_image_request("w154", "a.jpg?x=1"));
        assert!(!valid_image_request("w154", "a.svg"));
        assert!(!valid_image_request("w154", "a"));
        assert!(!valid_image_request("w154", &format!("{}.jpg", "a".repeat(65))));
    }

    #[test]
    fn safe_next_allows_relative_and_blocks_offsite() {
        assert_eq!(safe_next("/shows/24").as_deref(), Some("/shows/24"));
        assert_eq!(safe_next("/account?tab=guides").as_deref(), Some("/account?tab=guides"));
        // Off-site / protocol-relative / scheme / control-char / non-relative are rejected.
        assert!(safe_next("//evil.example").is_none());
        assert!(safe_next("https://evil.example").is_none());
        assert!(safe_next("/a\\b").is_none());
        assert!(safe_next("/a\nb").is_none());
        assert!(safe_next("not-relative").is_none());
        assert!(safe_next("").is_none());
    }
}
