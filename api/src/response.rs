//! Response helpers. Catalog reads are cacheable and shared across all users,
//! so we set `Cache-Control` to let a CDN/edge cache absorb most reads — the
//! dominant cost lever.

use axum::http::header::CACHE_CONTROL;
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

// Deliberate shared-cache TTLs (seconds), tuned for edge-hit rate vs freshness.
// Catalog data is near-immutable, so it caches for a long time;
// vote-derived aggregates move as people vote, so they cache briefly and lean on
// stale-while-revalidate to stay cheap. Bump these to scale read traffic.

/// Show detail / seasons — catalog data, changes rarely.
pub const TTL_CATALOG: u32 = 3600;
/// Search proxy — TMDB results change slowly; spares the upstream.
pub const TTL_SEARCH: u32 = 600;
/// Episode lists and skip guides — derived from live votes.
pub const TTL_AGGREGATE: u32 = 30;
/// The home page's popular-shows list — vote-derived, but rank order moves
/// slowly and the endpoint is homepage-hot, so it caches longer.
pub const TTL_POPULAR: u32 = 600;

/// JSON response with a shared-cache `Cache-Control` for catalog data.
/// `s_maxage` is the CDN/edge TTL in seconds.
pub fn cacheable_json<T: Serialize>(value: &T, s_maxage: u32) -> Response {
    let mut res = Json(value).into_response();
    let header = format!("public, s-maxage={s_maxage}, stale-while-revalidate=60");
    if let Ok(value) = HeaderValue::from_str(&header) {
        res.headers_mut().insert(CACHE_CONTROL, value);
    }
    res
}

/// JSON response that must NOT be stored by any cache — for per-user data (e.g.
/// responses carrying `myVote`). A shared cache must never serve one user's
/// per-user data to another.
pub fn private_json<T: Serialize>(value: &T) -> Response {
    let mut res = Json(value).into_response();
    res.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    res
}
