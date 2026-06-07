//! Runtime configuration, loaded from the environment. See api/.env.example
//! and the design notes.

use crate::oauth::{ProviderConfig, ProviderKind};

#[derive(Debug, Clone)]
pub struct Config {
    /// Postgres connection string. Use the POOLED string for serverless
    /// Postgres (Neon/Supabase) — see the design notes.
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
    /// explicit `migrate` deploy step instead. A single-instance box
    /// can opt back in with RUN_MIGRATIONS_ON_BOOT=true for one-command deploys.
    pub run_migrations_on_boot: bool,
    /// Max vote writes per client IP per minute.
    pub vote_rate_per_minute: u32,
    /// Directory of the built SPA to serve as a fallback (same-origin SPA+API on
    /// one service, e.g. Cloud Run). Unset → don't serve static files (the box
    /// deploy serves the SPA via Caddy instead).
    pub static_dir: Option<String>,
    /// Auth settings (OAuth + JWT). See the design notes.
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
            static_dir: std::env::var("STATIC_DIR").ok().filter(|s| !s.trim().is_empty()),
            auth: AuthConfig {
                jwt_secret,
                base_url: optional("AUTH_BASE_URL", "http://localhost:8080"),
                web_post_login_url: optional("WEB_POST_LOGIN_URL", "http://localhost:5173"),
                cookie_secure: optional("AUTH_COOKIE_SECURE", "false") == "true",
                providers: load_providers(),
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

fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key)
        .map_err(|_| anyhow::anyhow!("missing required env var {key} (see api/.env.example)"))
}

fn optional(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
