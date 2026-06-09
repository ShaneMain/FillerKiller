//! Application metrics.
//!
//! A Prometheus text exposition is served on a SEPARATE, instance-local listener
//! (see `Config::metrics_addr`) — never on the public ingress port. On Cloud Run a
//! Managed-Service-for-Prometheus collector sidecar scrapes `localhost:<port>/metrics`
//! over the instance-local network; on the single box it sits on the private Docker
//! network. Either way the endpoint is not reachable from the internet, so it needs
//! no auth.
//!
//! Signals:
//! - RED — `track_metrics` layers over every matched route and records
//!   `http_requests_total`, `http_request_duration_seconds` (histogram) and an
//!   `http_requests_in_flight` gauge, labelled by method / matched-route / status.
//! - Business — a few counters emitted straight from the handlers
//!   (`votes_total{value}`, `show_imports_total`).
//!
//! All emission goes through the `metrics` facade's global recorder, installed once
//! by `install_recorder`; if it is never installed every macro is a cheap no-op.

use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::http::header::CONTENT_TYPE;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

/// Request-latency histogram buckets, in seconds. Skewed toward the sub-second
/// range where a healthy API lives, with headroom for cold serverless-Postgres
/// spikes on the import path.
const LATENCY_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Install the process-global Prometheus recorder and return its render handle.
/// Call exactly once, at startup, before serving. The matched route template
/// keeps `http_request_duration_seconds` to a handful of series, so a fixed
/// bucket set is fine.
pub fn install_recorder() -> anyhow::Result<PrometheusHandle> {
    let handle = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("http_request_duration_seconds".to_owned()),
            LATENCY_BUCKETS,
        )?
        .install_recorder()?;
    Ok(handle)
}

/// The private metrics listener's router: just `GET /metrics`, rendering the
/// current exposition. Deliberately carries no other routes and no app state.
pub fn metrics_router(handle: PrometheusHandle) -> Router {
    Router::new().route(
        "/metrics",
        get(move || {
            let body = handle.render();
            async move {
                (
                    [(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
                    body,
                )
                    .into_response()
            }
        }),
    )
}

/// Layer (via `Router::route_layer`) recording RED signals for every matched
/// route. Labels use the route *template* (`/api/shows/{id}`), read from the
/// `MatchedPath` extension, so per-id URLs never blow up label cardinality.
pub async fn track_metrics(req: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().clone();
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_owned())
        .unwrap_or_else(|| "<unmatched>".to_owned());

    // Decrement via a drop guard so the gauge can't drift upward if a downstream
    // handler panics (axum doesn't catch panics, so a plain decrement after
    // `next.run` would be skipped on unwind).
    struct InFlight;
    impl Drop for InFlight {
        fn drop(&mut self) {
            metrics::gauge!("http_requests_in_flight").decrement(1.0);
        }
    }
    metrics::gauge!("http_requests_in_flight").increment(1.0);
    let _in_flight = InFlight;

    let response = next.run(req).await;

    let method = method.to_string();
    let status = response.status().as_u16().to_string();
    let elapsed = start.elapsed().as_secs_f64();

    metrics::counter!(
        "http_requests_total",
        "method" => method.clone(), "path" => path.clone(), "status" => status.clone()
    )
    .increment(1);
    metrics::histogram!(
        "http_request_duration_seconds",
        "method" => method, "path" => path, "status" => status
    )
    .record(elapsed);

    response
}
