# syntax=docker/dockerfile:1.7
#
# HydroCore companion server: builds straight from GitHub, so this file (plus
# docker-compose.yml) is all you need on the host: no Rust, no database, no source tree.
#
#   docker compose up -d --build
#
# The runtime image is `distroless/static` (no shell, no libc, ~2 MB) containing only the
# statically linked binary. Alpine's native musl toolchain gives a fully static binary on
# both amd64 and arm64 without cross-compilation.

# ---------- Stage 1: build ----------------------------------------------------------
FROM rust:1-alpine AS builder

ARG HYDROCORE_REPO=https://github.com/mustafabayramguven/HydroCore.git
ARG HYDROCORE_REF=main

# git: fetch the sources. build-base/musl-dev: C toolchain for the bundled SQLite.
RUN apk add --no-cache git build-base musl-dev

WORKDIR /src
# Re-run with `docker compose build --no-cache` (or change HYDROCORE_REF) to pick up new commits:
# Docker cannot know the remote branch moved.
RUN git clone --depth 1 --branch "${HYDROCORE_REF}" "${HYDROCORE_REPO}" hydrocore

WORKDIR /src/hydrocore/server
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --locked \
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
