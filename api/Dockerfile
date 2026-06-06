# Multi-stage build for the FillerKiller API. Produces a small image suitable for
# a self-hosted box / containers. Builds offline using the committed
# .sqlx query cache, so no database is needed at image-build time.

FROM rust:1-slim AS builder
WORKDIR /app
# Use the committed sqlx offline cache instead of a live DB at build time.
ENV SQLX_OFFLINE=true

# Cache dependencies: build against a stub main, then the real sources.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release || true
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/fillerkiller-api /usr/local/bin/fillerkiller-api
ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["fillerkiller-api"]
