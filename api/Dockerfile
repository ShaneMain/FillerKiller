# Multi-stage build for the FillerKiller API. Produces a small static-ish image
# suitable for Fly Machines / containers. For AWS Lambda, build with
# cargo-lambda instead of this Dockerfile.

FROM rust:1-slim AS builder
WORKDIR /app

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
