//! Runtime configuration, loaded from the environment. See api/.env.example
//! and the design notes.

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
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            tmdb_token: required("TMDB_API_READ_TOKEN")?,
            tmdb_image_base_url: optional(
                "TMDB_IMAGE_BASE_URL",
                "https://image.tmdb.org/t/p",
            ),
            cors_allowed_origin: optional("CORS_ALLOWED_ORIGIN", "http://localhost:5173"),
            bind_addr: optional("BIND_ADDR", "0.0.0.0:8080"),
        })
    }
}

fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key)
        .map_err(|_| anyhow::anyhow!("missing required env var {key} (see api/.env.example)"))
}

fn optional(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
