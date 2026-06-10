//! Session auth: a JWT in an httpOnly cookie. Signature/expiry verification is
//! local; authenticated requests additionally make one indexed DB read to check
//! the user's `token_version` — the revocation signal that lets logout and
//! account deletion kill outstanding sessions.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use uuid::Uuid;

use crate::db;
use crate::error::AppError;
use crate::AppState;

const SESSION_TTL_DAYS: i64 = 7;

/// Pin the token to this app: a JWT signed with the same secret for any other
/// purpose (a future service reusing the key) won't validate as a session.
const JWT_ISSUER: &str = "fillerkiller";
const JWT_AUDIENCE: &str = "fillerkiller-session";

/// Cookie names. With `Secure` (prod HTTPS) the `__Host-` prefix makes the
/// browser enforce host-locking: the cookie must be Secure, Path=/, and carry
/// no Domain — so a sibling subdomain can never set or shadow it (session
/// fixation). The prefix is browser-rejected without `Secure`, so plain-HTTP
/// dev keeps the bare names.
pub fn session_cookie_name(secure: bool) -> &'static str {
    if secure { "__Host-fk_session" } else { "fk_session" }
}
pub fn state_cookie_name(secure: bool) -> &'static str {
    if secure { "__Host-fk_oauth_state" } else { "fk_oauth_state" }
}
pub fn next_cookie_name(secure: bool) -> &'static str {
    if secure { "__Host-fk_oauth_next" } else { "fk_oauth_next" }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // our app_user.id
    pub email: String,
    pub name: Option<String>,
    /// The user's token_version at issue time; a mismatch on verify means the
    /// session was revoked (logout / account deletion).
    pub ver: i32,
    pub iss: String,
    pub aud: String,
    pub iat: usize,
    pub exp: usize,
}

fn unix_now() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as usize)
        .unwrap_or(0)
}

/// Issue a session JWT for a user. `ver` is the user's current token_version.
pub fn issue_jwt(
    secret: &str,
    user_id: Uuid,
    email: &str,
    name: Option<&str>,
    ver: i32,
) -> Result<String, AppError> {
    let now = unix_now();
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        name: name.map(str::to_string),
        ver,
        iss: JWT_ISSUER.to_string(),
        aud: JWT_AUDIENCE.to_string(),
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
/// expired, malformed, wrong issuer/audience). The token_version check happens
/// separately, in the `CurrentUser` extractor — this function stays DB-free.
pub fn verify_jwt(secret: &str, token: &str) -> Option<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[JWT_ISSUER]);
    validation.set_audience(&[JWT_AUDIENCE]);
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .ok()
    .map(|data| data.claims)
}

/// The session cookie carrying the JWT. `SameSite=Strict`: the cookie is never
/// sent on cross-site requests, including top-level navigations — fine here
/// because no page render depends on it server-side (the SPA fetches `/api/me`
/// same-origin after load), and it removes the residual CSRF surface Lax leaves
/// on top-level GETs. The OAuth round-trip cookies below stay Lax, since the
/// provider's redirect back to our callback must carry them.
pub fn session_cookie(token: String, secure: bool) -> Cookie<'static> {
    Cookie::build((session_cookie_name(secure), token))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(secure)
        .path("/")
        .max_age(time::Duration::days(SESSION_TTL_DAYS))
        .build()
}

/// An expired empty cookie that overwrites `name`, to clear it. The removal
/// must repeat the original Path/Secure attributes: browsers only replace a
/// cookie when the path matches, and reject `__Host-`-prefixed Set-Cookies
/// (including removals) that aren't `Secure; Path=/`.
pub fn clear_cookie(name: &'static str, secure: bool) -> Cookie<'static> {
    Cookie::build((name, ""))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(secure)
        .path("/")
        .max_age(time::Duration::ZERO)
        .build()
}

/// An expired empty session cookie, to clear the session on logout.
pub fn clear_session_cookie(secure: bool) -> Cookie<'static> {
    clear_cookie(session_cookie_name(secure), secure)
}

/// Short-lived CSRF `state` cookie for the OAuth round-trip.
pub fn state_cookie(state: String, secure: bool) -> Cookie<'static> {
    Cookie::build((state_cookie_name(secure), state))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .path("/")
        .max_age(time::Duration::minutes(10))
        .build()
}

/// Short-lived cookie carrying the post-login return path (a site-relative path)
/// across the OAuth round-trip, so the user lands back where they started.
pub fn next_cookie(path: String, secure: bool) -> Cookie<'static> {
    Cookie::build((next_cookie_name(secure), path))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .path("/")
        .max_age(time::Duration::minutes(10))
        .build()
}

/// The authenticated user, extracted from the session cookie. Use as a handler
/// argument to require auth; missing/invalid/revoked → 401.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    /// The user's current token_version (already validated against the claim);
    /// handlers re-issuing the session cookie reuse it.
    pub token_version: i32,
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
        let token = jar
            .get(session_cookie_name(state.auth.cookie_secure))
            .ok_or(AppError::Unauthorized)?
            .value();
        let claims = verify_jwt(&state.auth.jwt_secret, token).ok_or(AppError::Unauthorized)?;
        let id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized)?;
        // Revocation check — one indexed PK read. A deleted account (no row) or
        // a bumped token_version (logout-everywhere) invalidates the session
        // even though the signature is still good.
        let current = db::user_token_version(&state.pool, id)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        if current != Some(claims.ver) {
            return Err(AppError::Unauthorized);
        }
        Ok(CurrentUser {
            id,
            email: claims.email,
            name: claims.name,
            token_version: claims.ver,
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
        let token = issue_jwt(secret, id, "a@b.com", Some("Ann"), 3).unwrap();
        let claims = verify_jwt(secret, &token).expect("valid token");
        assert_eq!(claims.sub, id.to_string());
        assert_eq!(claims.email, "a@b.com");
        assert_eq!(claims.name.as_deref(), Some("Ann"));
        assert_eq!(claims.ver, 3);
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn jwt_rejects_wrong_secret() {
        let token = issue_jwt("right", Uuid::new_v4(), "a@b.com", None, 0).unwrap();
        assert!(verify_jwt("wrong", &token).is_none());
    }

    #[test]
    fn jwt_rejects_garbage() {
        assert!(verify_jwt("s", "not.a.jwt").is_none());
        assert!(verify_jwt("s", "").is_none());
    }

    #[test]
    fn jwt_rejects_foreign_issuer_or_audience() {
        // A token signed with OUR secret but minted for another purpose (different
        // iss/aud) must not validate as a session.
        let secret = "test-secret";
        let now = unix_now();
        let foreign = Claims {
            sub: Uuid::new_v4().to_string(),
            email: "a@b.com".into(),
            name: None,
            ver: 0,
            iss: "some-other-service".into(),
            aud: "some-other-purpose".into(),
            iat: now,
            exp: now + 3600,
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &foreign,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();
        assert!(verify_jwt(secret, &token).is_none());
    }

    #[test]
    fn cookie_names_are_host_prefixed_only_when_secure() {
        assert_eq!(session_cookie_name(true), "__Host-fk_session");
        assert_eq!(session_cookie_name(false), "fk_session");
        // __Host- requires Secure; the builders must agree with the names.
        assert!(session_cookie("t".into(), true).secure().unwrap_or(false));
    }
}
