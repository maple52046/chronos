# syntax=docker/dockerfile:1.7
#
# Single combined image containing both Chronos binaries. There is no fixed
# ENTRYPOINT: consumers pick the binary via `command`/CMD. No HEALTHCHECK is
# baked in because the server (:8080) and gateway (:9090) listen on different
# ports; healthchecks are defined per service in Compose / the orchestrator.
#
# Build and tag with a UTC timestamp:
#   TS=$(date -u +%Y%m%d%H%M%S)
#   docker build -t "ghcr.io/maple52046/chronos:v1-${TS}" .

ARG RUST_IMAGE=rust:1-bookworm
ARG RUNTIME_IMAGE=debian:bookworm-slim

FROM ${RUST_IMAGE} AS chef
WORKDIR /app
RUN cargo install cargo-chef --locked

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
WORKDIR /app
# cmake and a C toolchain are required to build the aws-lc-rs rustls provider.
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config ca-certificates cmake clang \
    && rm -rf /var/lib/apt/lists/*
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --bin chronos-server
RUN cargo build --release --locked --bin chronos-gateway

FROM ${RUNTIME_IMAGE} AS runtime

# Build-time metadata for the per-build OCI labels below. Defaults keep `docker
# build` usable on its own; build-image.sh overrides them via --build-arg.
ARG VERSION="0.0.0-dev"
ARG REVISION="unknown"
ARG CREATED="2026-06-19T13:23:20Z"

LABEL org.opencontainers.image.title="Chronos" \
      org.opencontainers.image.description="HTTP-backend time synchronization gateway (chronos-server and chronos-gateway)." \
      org.opencontainers.image.source="https://github.com/maple52046/chronos" \
      org.opencontainers.image.url="https://github.com/maple52046/chronos" \
      org.opencontainers.image.documentation="https://github.com/maple52046/chronos/blob/main/README.md" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.vendor="maple52046" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.created="${CREATED}"

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates tzdata curl \
    && rm -rf /var/lib/apt/lists/*
RUN groupadd --system chronos \
    && useradd --system --no-create-home --gid chronos --shell /usr/sbin/nologin chronos
RUN mkdir -p /etc/chronos /run/chronos \
    && chown -R chronos:chronos /etc/chronos /run/chronos
COPY --from=builder /app/target/release/chronos-server /usr/local/bin/chronos-server
COPY --from=builder /app/target/release/chronos-gateway /usr/local/bin/chronos-gateway
ENV RUST_LOG=info
USER chronos:chronos
WORKDIR /
