//! API error type. Renders the JSON error shape used by every endpoint:
//! `{ "error": { "code": string, "message": string } }`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::tmdb::TmdbError;

#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    /// Missing or invalid session.
    Unauthorized,
    /// Authenticated, but not allowed to act on this resource.
    Forbidden,
    /// Client exceeded the vote rate limit.
    RateLimited,
    /// Upstream (TMDB) failure.
    Upstream(String),
    /// Upstream asked us to back off.
    UpstreamRateLimited,
    /// Anything unexpected (DB, etc.). Detail is logged, not exposed.
    Internal(anyhow::Error),
}

impl AppError {
    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, "not_found", m.clone()),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, "bad_request", m.clone()),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "authentication required".to_string(),
            ),
            AppError::Forbidden => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "you don't have permission to do that".to_string(),
            ),
            AppError::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "too many requests; slow down".to_string(),
            ),
            AppError::Upstream(m) => (StatusCode::BAD_GATEWAY, "upstream_error", m.clone()),
            AppError::UpstreamRateLimited => (
                StatusCode::SERVICE_UNAVAILABLE,
                "upstream_rate_limited",
                "upstream is rate limiting; try again shortly".to_string(),
            ),
            AppError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error".to_string(),
            ),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        if let AppError::Internal(ref e) = self {
            // Log the real cause; never leak it to the client.
            tracing::error!("internal error: {e:#}");
        }
        (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Internal(e.into())
    }
}

impl From<TmdbError> for AppError {
    fn from(e: TmdbError) -> Self {
        match e {
            TmdbError::RateLimited => AppError::UpstreamRateLimited,
            // A missing show/season is a clean 404 for the client — don't leak the
            // upstream TMDB path/status into the response.
            TmdbError::Status { status, .. } if status.as_u16() == 404 => {
                AppError::NotFound("we couldn't find that show".to_string())
            }
            other => {
                tracing::warn!("TMDB upstream error: {other}");
                AppError::Upstream("the TV database is unavailable; try again shortly".to_string())
            }
        }
    }
}
