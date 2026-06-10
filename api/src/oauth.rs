//! OAuth 2.0 Authorization Code flow for Google and GitHub.
//!
//! We hand-roll the two HTTP steps (token exchange + userinfo) rather than pull
//! a provider framework: it's a couple of requests and the per-provider userinfo
//! differences are clearer explicit. Client secrets stay server-side.

use serde::Deserialize;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Google,
    Github,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::Google => "google",
            ProviderKind::Github => "github",
        }
    }

    /// (client_id env var, client_secret env var).
    pub fn env_vars(&self) -> (&'static str, &'static str) {
        match self {
            ProviderKind::Google => ("GOOGLE_CLIENT_ID", "GOOGLE_CLIENT_SECRET"),
            ProviderKind::Github => ("GITHUB_CLIENT_ID", "GITHUB_CLIENT_SECRET"),
        }
    }

    fn authorize_endpoint(&self) -> &'static str {
        match self {
            ProviderKind::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            ProviderKind::Github => "https://github.com/login/oauth/authorize",
        }
    }

    fn token_endpoint(&self) -> &'static str {
        match self {
            ProviderKind::Google => "https://oauth2.googleapis.com/token",
            ProviderKind::Github => "https://github.com/login/oauth/access_token",
        }
    }

    fn scope(&self) -> &'static str {
        match self {
            ProviderKind::Google => "openid email profile",
            ProviderKind::Github => "read:user user:email",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub client_id: String,
    pub client_secret: String,
}

/// The identity we care about from a provider. `subject` is the provider's
/// stable user id (Google `sub`, GitHub numeric `id`) — the durable identity
/// key. Email is profile data: verified, but changeable at the provider.
pub struct OAuthUser {
    pub subject: String,
    pub email: String,
    pub name: Option<String>,
}

/// Log the real upstream cause and return a generic error that leaks no upstream
/// status/body/parse detail to the client.
fn upstream_err(context: &str, detail: impl std::fmt::Display) -> AppError {
    tracing::warn!("oauth {context}: {detail}");
    AppError::Upstream("sign-in is temporarily unavailable; try again".into())
}

impl ProviderConfig {
    /// Build the provider authorize URL to redirect the browser to.
    pub fn authorize_url(&self, redirect_uri: &str, state: &str) -> String {
        let params = [
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", self.kind.scope()),
            ("state", state),
        ];
        let query = serde_urlencoded::to_string(params).unwrap_or_default();
        format!("{}?{}", self.kind.authorize_endpoint(), query)
    }

    /// Exchange an authorization code for an access token.
    pub async fn exchange_code(
        &self,
        http: &reqwest::Client,
        code: &str,
        redirect_uri: &str,
    ) -> Result<String, AppError> {
        let params = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ];
        let res = http
            .post(self.kind.token_endpoint())
            .header("Accept", "application/json")
            .form(&params)
            .send()
            .await
            .map_err(|e| upstream_err("token exchange transport", e))?;
        if !res.status().is_success() {
            return Err(upstream_err("token exchange status", res.status()));
        }
        let body: TokenResponse = res
            .json()
            .await
            .map_err(|e| upstream_err("token response decode", e))?;
        Ok(body.access_token)
    }

    /// Fetch the user's email and name using an access token.
    pub async fn fetch_user(
        &self,
        http: &reqwest::Client,
        access_token: &str,
    ) -> Result<OAuthUser, AppError> {
        match self.kind {
            ProviderKind::Google => self.fetch_google_user(http, access_token).await,
            ProviderKind::Github => self.fetch_github_user(http, access_token).await,
        }
    }

    async fn fetch_google_user(
        &self,
        http: &reqwest::Client,
        access_token: &str,
    ) -> Result<OAuthUser, AppError> {
        let info: GoogleUserinfo = http
            .get("https://www.googleapis.com/oauth2/v3/userinfo")
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| upstream_err("google userinfo transport", e))?
            .json()
            .await
            .map_err(|e| upstream_err("google userinfo decode", e))?;
        match info.email {
            Some(email) if info.email_verified.unwrap_or(false) => Ok(OAuthUser {
                subject: info.sub,
                email,
                name: info.name,
            }),
            _ => Err(AppError::Upstream(
                "your Google account has no verified email".into(),
            )),
        }
    }

    async fn fetch_github_user(
        &self,
        http: &reqwest::Client,
        access_token: &str,
    ) -> Result<OAuthUser, AppError> {
        // GitHub requires a User-Agent and returns email separately.
        let user: GithubUser = http
            .get("https://api.github.com/user")
            .bearer_auth(access_token)
            .header("User-Agent", "FillerKiller")
            .send()
            .await
            .map_err(|e| upstream_err("github user transport", e))?
            .json()
            .await
            .map_err(|e| upstream_err("github user decode", e))?;

        let emails: Vec<GithubEmail> = http
            .get("https://api.github.com/user/emails")
            .bearer_auth(access_token)
            .header("User-Agent", "FillerKiller")
            .send()
            .await
            .map_err(|e| upstream_err("github emails transport", e))?
            .json()
            .await
            .map_err(|e| upstream_err("github emails decode", e))?;

        let email = emails
            .into_iter()
            .find(|e| e.primary && e.verified)
            .map(|e| e.email)
            .ok_or_else(|| AppError::Upstream("your GitHub account has no verified email".into()))?;

        Ok(OAuthUser {
            subject: user.id.to_string(),
            email,
            name: user.name,
        })
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct GoogleUserinfo {
    /// Google's stable account id — the OIDC subject.
    sub: String,
    email: Option<String>,
    email_verified: Option<bool>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubUser {
    /// GitHub's stable numeric account id (logins can be renamed; this can't).
    id: i64,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubEmail {
    email: String,
    primary: bool,
    verified: bool,
}
