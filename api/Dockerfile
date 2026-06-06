# Multi-stage build for the FillerKiller API. Produces a small image suitable for
# a self-hosted box / containers. Builds offline using the committed
# .sqlx query cache, so no database is needed at image-build time.
#
# Note: this compiles dependencies on each image build. If build speed becomes a
# concern, introduce cargo-chef for a cached dependency layer (the hand-rolled
# stub-main trick is omitted on purpose — it can silently ship a stub binary if
# cargo treats the second build as up-to-date).

FROM rust:1-slim AS builder
WORKDIR /app
# Use the committed sqlx offline cache instead of a live DB at build time.
ENV SQLX_OFFLINE=true
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 10001 app
COPY --from=builder /app/target/release/fillerkiller-api /usr/local/bin/fillerkiller-api
USER app
ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["fillerkiller-api"]
