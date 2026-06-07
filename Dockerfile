# Root multi-stage build for Cloud Run: builds the SPA and the Rust
# API and ships ONE image that serves both same-origin — API on /api + /health,
# static SPA on everything else (so the SameSite=Lax cookie just works).
# Deploy with: gcloud run deploy --source .
#
# The single-box deploy uses api/Dockerfile + Caddy instead; this file
# is only for the single-service Cloud Run deploy.

# --- Stage 1: build the SPA --------------------------------------------------
FROM node:22-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build          # -> /web/dist

# --- Stage 2: build the Rust API (offline via the committed .sqlx cache) ------
FROM rust:1-slim AS api
WORKDIR /app
ENV SQLX_OFFLINE=true
COPY api/ ./
RUN cargo build --release

# --- Stage 3: runtime --------------------------------------------------------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 10001 app
COPY --from=api /app/target/release/fillerkiller-api /usr/local/bin/fillerkiller-api
COPY --from=web /web/dist /app/web
USER app
ENV STATIC_DIR=/app/web
ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["fillerkiller-api"]
