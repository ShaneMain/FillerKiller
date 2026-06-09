//! Runtime configuration, loaded from the environment. See api/.env.example.

use crate::oauth::{ProviderConfig, ProviderKind};

#[derive(Debug, Clone)]
pub struct Config {
    /// Postgres connection string. Use the POOLED string for serverless
    /// Postgres (Neon/Supabase).
    pub database_url: String,
    /// TMDB v4 read token (server-side only; never sent to the client).
    pub tmdb_token: String,
    /// TMDB image CDN base, no trailing slash.
    pub tmdb_image_base_url: String,
    /// Origin allowed by CORS — the SPA's origin.
    pub cors_allowed_origin: String,
    /// Address to bind, e.g. "0.0.0.0:8080".
    pub bind_addr: String,
    /// Apply migrations on boot. Off by default: under ephemeral / multi-instance
    /// compute every cold start would race to migrate, so migrations run as an
    /// explicit `migrate` deploy step instead. A single-instance box can opt
    /// back in with RUN_MIGRATIONS_ON_BOOT=true for one-command deploys.
    pub run_migrations_on_boot: bool,
    /// Max vote writes per client IP per minute (app-level defense-in-depth; the
    /// authoritative limiter is the CDN edge).
    pub vote_rate_per_minute: u32,
    /// Max TMDB import-on-demand fan-outs per instance per minute (caps outbound
    /// load from the unauthenticated import path).
    pub import_rate_per_minute: u32,
    /// Refresh cadence for a RECENT show (latest episode within ~2 years): how
    /// long its cache is fresh before a viewed show is re-synced in the
    /// background. The refresh itself is incremental (only new/grown seasons).
    pub refresh_ttl_hours: i32,
    /// Refresh cadence for an ENDED show (no episode in ~2 years): such shows
    /// rarely change, so they refresh far less often (default ~semi-monthly).
    pub refresh_ttl_hours_ended: i32,
    /// Directory of the built SPA to serve as a fallback (same-origin SPA+API on
    /// one service, e.g. Cloud Run). Unset → don't serve static files (the box
    /// deploy serves the SPA via Caddy instead).
    pub static_dir: Option<String>,
    /// Auth settings (OAuth + JWT).
    pub auth: AuthConfig,
}

/// Auth configuration: JWT signing, OAuth redirect/return URLs, and the set of
/// enabled providers. A provider is enabled only if both its id and secret are
/// present in the environment.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    /// Public base URL of this API, used to build OAuth redirect URIs.
    pub base_url: String,
    /// Where to send the browser after a successful login.
    pub web_post_login_url: String,
    /// Whether to mark cookies `Secure` (true in prod/HTTPS).
    pub cookie_secure: bool,
    pub providers: Vec<ProviderConfig>,
}

impl AuthConfig {
    pub fn provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.kind.as_str() == name)
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let jwt_secret = required("AUTH_JWT_SECRET")?;
        if jwt_secret.trim().len() < 32 {
            anyhow::bail!(
                "AUTH_JWT_SECRET must be at least 32 bytes; generate with: openssl rand -base64 32"
            );
        }

        Ok(Self {
            database_url: required("DATABASE_URL")?,
            tmdb_token: required("TMDB_API_READ_TOKEN")?,
            tmdb_image_base_url: optional(
                "TMDB_IMAGE_BASE_URL",
                "https://image.tmdb.org/t/p",
            ),
            cors_allowed_origin: optional("CORS_ALLOWED_ORIGIN", "http://localhost:5173"),
            bind_addr: bind_addr_from_env(),
            run_migrations_on_boot: optional("RUN_MIGRATIONS_ON_BOOT", "false") == "true",
            vote_rate_per_minute: optional("VOTE_RATE_PER_MINUTE", "30")
                .parse()
                .unwrap_or(30),
            import_rate_per_minute: optional("IMPORT_RATE_PER_MINUTE", "20")
                .parse()
                .unwrap_or(20),
            refresh_ttl_hours: optional("REFRESH_TTL_HOURS", "72").parse().unwrap_or(72),
            refresh_ttl_hours_ended: optional("REFRESH_TTL_HOURS_ENDED", "360")
                .parse()
                .unwrap_or(360),
            static_dir: std::env::var("STATIC_DIR").ok().filter(|s| !s.trim().is_empty()),
            auth: {
                let base_url = optional("AUTH_BASE_URL", "http://localhost:8080");
                AuthConfig {
                    jwt_secret,
                    cookie_secure: cookie_secure(&base_url),
                    web_post_login_url: optional("WEB_POST_LOGIN_URL", "http://localhost:5173"),
                    base_url,
                    providers: load_providers(),
                }
            },
        })
    }
}

/// Build the list of enabled OAuth providers from the environment.
fn load_providers() -> Vec<ProviderConfig> {
    let mut providers = Vec::new();
    for kind in [ProviderKind::Google, ProviderKind::Github] {
        let (id_var, secret_var) = kind.env_vars();
        if let (Ok(client_id), Ok(client_secret)) =
            (std::env::var(id_var), std::env::var(secret_var))
        {
            if !client_id.is_empty() && !client_secret.is_empty() {
                providers.push(ProviderConfig {
                    kind,
                    client_id,
                    client_secret,
                });
            }
        }
    }
    if providers.is_empty() {
        tracing::warn!("no OAuth providers configured; sign-in is disabled");
    }
    providers
}

/// Resolve the bind address. Cloud Run (and most serverless container hosts)
/// inject the port to listen on via `$PORT`; honour it when present, otherwise
/// fall back to `BIND_ADDR` (default `0.0.0.0:8080`).
fn bind_addr_from_env() -> String {
    match std::env::var("PORT") {
        Ok(port) if !port.trim().is_empty() => format!("0.0.0.0:{}", port.trim()),
        _ => optional("BIND_ADDR", "0.0.0.0:8080"),
    }
}

/// Whether session cookies are marked `Secure`. Defaults to fail-closed: derived
/// from the public base URL's scheme (`https://` → secure), so a prod deploy can't
/// accidentally ship cookies over plain HTTP by omitting a flag. An explicit
/// `AUTH_COOKIE_SECURE` overrides (e.g. to test HTTPS behaviour locally).
fn cookie_secure(base_url: &str) -> bool {
    match std::env::var("AUTH_COOKIE_SECURE") {
        Ok(v) if !v.trim().is_empty() => v.trim() == "true",
        _ => base_url.starts_with("https://"),
    }
}

fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key)
        .map_err(|_| anyhow::anyhow!("missing required env var {key} (see api/.env.example)"))
}

fn optional(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
