//! Per-IP rate limiting for mutating endpoints (vote writes).
//!
//! This is application-level *defense in depth*, not the primary control. The
//! authoritative, global per-IP limiter belongs at the CDN edge: it sees every request before our compute and
//! shares state across instances. This in-process limiter is per-instance — each
//! ephemeral instance keeps its own buckets — so under scale-out it bounds abuse
//! per instance, not globally. Ballot integrity does not depend on it either: the
//! `UNIQUE (user_id, episode_id)` constraint already caps one vote per user per
//! episode.

use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use governor::{clock::DefaultClock, state::keyed::DefaultKeyedStateStore, Quota, RateLimiter};

use crate::error::AppError;
use crate::AppState;

/// Keyed (per-IP) in-memory token-bucket limiter.
pub type IpRateLimiter = RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;

/// Build a limiter allowing `per_minute` requests per IP (burst = `per_minute`).
pub fn ip_limiter(per_minute: u32) -> Arc<IpRateLimiter> {
    let quota = Quota::per_minute(NonZeroU32::new(per_minute.max(1)).expect("non-zero"));
    Arc::new(RateLimiter::keyed(quota))
}

/// Axum middleware: reject vote writes from an IP over its quota with 429.
pub async fn limit_votes(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let ip = client_ip(req.headers());
    match state.rate_limiter.check_key(&ip) {
        Ok(()) => next.run(req).await,
        Err(_) => AppError::RateLimited.into_response(),
    }
}

/// Best-effort client IP from proxy headers. Behind Caddy/Cloudflare the socket
/// peer is the proxy, so we trust `CF-Connecting-IP` (set by Cloudflare) then the
/// first `X-Forwarded-For` hop. Unknown → a shared sentinel bucket, so requests
/// without a forwarded IP still share one limit rather than bypassing it.
///
/// This trust is sound only because the container is reachable *solely* through
/// the proxy (it exposes no public port — Caddy `expose`/Cloud Run ingress). If
/// it were ever exposed directly these headers become attacker-controlled and
/// the limiter degrades to best-effort — which is why the authoritative limiter
/// lives at the CDN edge.
fn client_ip(headers: &HeaderMap) -> IpAddr {
    header_ip(headers, "cf-connecting-ip")
        .or_else(|| {
            headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.trim().parse().ok())
        })
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

fn header_ip(headers: &HeaderMap, name: &str) -> Option<IpAddr> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse().ok())
}
