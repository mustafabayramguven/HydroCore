# syntax=docker/dockerfile:1.7
#
# HydroCore companion server multi-stage build.
# Built in GitHub Actions (`.github/workflows/server-docker.yml`) and published to:
#   ghcr.io/mustafabayramguven/hydrocore-server:latest
#
# The runtime image is `distroless/static` (no shell, no libc, ~2 MB) containing only the
# statically linked musl binary.

# ---------- Stage 1: build ----------------------------------------------------------
FROM rust:1-alpine AS builder

# C toolchain for compiling the bundled SQLite (`rusqlite` with `bundled` feature).
RUN apk add --no-cache build-base musl-dev

WORKDIR /src

# Pre-build third-party dependencies in a separate layer so Buildx GHA cache (`type=gha`)
# reuses compiled crates (`tokio`, `axum`, bundled `libsqlite3-sys`, etc.) across commits.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src static \
 && echo "pub fn _cache() {}" > src/lib.rs \
 && echo "fn main() {}" > src/main.rs \
 && cargo build --release --locked \
 && rm -rf src static

# Copy server source code and embedded web dashboard assets from the repository context.
COPY . .

RUN touch src/lib.rs src/main.rs \
 && cargo build --release --locked --bin hydrocore-server \
 && cp target/release/hydrocore-server /hydrocore-server \
 # An empty data dir owned by the distroless "nonroot" user (uid 65532). Docker copies it
 # (with ownership) into a fresh named volume mounted at /data.
 && mkdir -p /data-skeleton

# ---------- Stage 2: runtime --------------------------------------------------------
FROM gcr.io/distroless/static-debian12:nonroot

COPY --from=builder /hydrocore-server /hydrocore-server
COPY --from=builder --chown=65532:65532 /data-skeleton /data

ENV HYDROCORE_BIND=0.0.0.0:8080 \
    HYDROCORE_DB=/data/hydrocore.db

VOLUME ["/data"]
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["/hydrocore-server", "healthcheck"]

USER nonroot:nonroot
ENTRYPOINT ["/hydrocore-server"]
