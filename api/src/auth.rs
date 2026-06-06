//! Session auth: a stateless JWT in an httpOnly cookie. Verification
//! is local — no DB read per request.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;

pub const SESSION_COOKIE: &str = "fk_session";
pub const STATE_COOKIE: &str = "fk_oauth_state";
const SESSION_TTL_DAYS: i64 = 7;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // our app_user.id
    pub email: String,
    pub name: Option<String>,
    pub iat: usize,
    pub exp: usize,
}

fn unix_now() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as usize)
        .unwrap_or(0)
}

/// Issue a session JWT for a user.
pub fn issue_jwt(
    secret: &str,
    user_id: Uuid,
    email: &str,
    name: Option<&str>,
) -> Result<String, AppError> {
    let now = unix_now();
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        name: name.map(str::to_string),
        iat: now,
        exp: now + (SESSION_TTL_DAYS as usize) * 24 * 3600,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(e.into()))
}

/// Verify and decode a session JWT. Returns None on any failure (bad signature,
/// expired, malformed).
pub fn verify_jwt(secret: &str, token: &str) -> Option<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .ok()
    .map(|data| data.claims)
}

/// The session cookie carrying the JWT.
pub fn session_cookie(token: String, secure: bool) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .path("/")
        .max_age(time::Duration::days(SESSION_TTL_DAYS))
        .build()
}

/// An expired empty session cookie, to clear the session on logout.
pub fn clear_session_cookie(secure: bool) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .path("/")
        .max_age(time::Duration::ZERO)
        .build()
}

/// Short-lived CSRF `state` cookie for the OAuth round-trip.
pub fn state_cookie(state: String, secure: bool) -> Cookie<'static> {
    Cookie::build((STATE_COOKIE, state))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .path("/")
        .max_age(time::Duration::minutes(10))
        .build()
}

/// The authenticated user, extracted from the session cookie. Use as a handler
/// argument to require auth; missing/invalid → 401.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .unwrap_or_default();
        let token = jar.get(SESSION_COOKIE).ok_or(AppError::Unauthorized)?.value();
        let claims = verify_jwt(&state.auth.jwt_secret, token).ok_or(AppError::Unauthorized)?;
        let id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized)?;
        Ok(CurrentUser {
            id,
            email: claims.email,
            name: claims.name,
        })
    }
}

/// Like `CurrentUser` but optional — never rejects. Use where auth is optional
/// (e.g. to fill `myVote`).
pub struct OptionalUser(pub Option<CurrentUser>);

impl FromRequestParts<AppState> for OptionalUser {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(OptionalUser(
            CurrentUser::from_request_parts(parts, state).await.ok(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_round_trips() {
        let secret = "test-secret";
        let id = Uuid::new_v4();
        let token = issue_jwt(secret, id, "a@b.com", Some("Ann")).unwrap();
        let claims = verify_jwt(secret, &token).expect("valid token");
        assert_eq!(claims.sub, id.to_string());
        assert_eq!(claims.email, "a@b.com");
        assert_eq!(claims.name.as_deref(), Some("Ann"));
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn jwt_rejects_wrong_secret() {
        let token = issue_jwt("right", Uuid::new_v4(), "a@b.com", None).unwrap();
        assert!(verify_jwt("wrong", &token).is_none());
    }

    #[test]
    fn jwt_rejects_garbage() {
        assert!(verify_jwt("s", "not.a.jwt").is_none());
        assert!(verify_jwt("s", "").is_none());
    }
}
