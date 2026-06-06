//! Response helpers. Catalog reads are cacheable and shared across all users,
//! so we set `Cache-Control` to let a CDN/edge cache absorb most reads — the
//! dominant cost lever per the design notes.

use axum::http::header::CACHE_CONTROL;
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

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
