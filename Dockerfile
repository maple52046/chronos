# syntax=docker/dockerfile:1.7
#
# Single combined image containing both Chronos binaries, built as fully static
# musl executables and shipped on distroless/static. There is no fixed
# ENTRYPOINT: consumers pick the binary via `command`/CMD. No HEALTHCHECK is
# baked in because the server (:8080) and gateway (:9090) listen on different
# ports; define a per-service healthcheck using the binary's own subcommand
# (e.g. `chronos-server healthcheck`), which needs no curl.
#
# Build and tag with the crate version and a UTC timestamp:
#   TS=$(date -u +%Y%m%d%H%M%S)
#   docker build -t "ghcr.io/maple52046/chronos:1.0.0-${TS}" .

ARG RUST_IMAGE=rust:1-bookworm
ARG RUNTIME_IMAGE=gcr.io/distroless/static-debian13
ARG TARGET=x86_64-unknown-linux-musl

FROM ${RUST_IMAGE} AS chef
ARG TARGET
WORKDIR /app
# Pin the toolchain first (rust-toolchain.toml selects "stable") so the musl
# target is added to the exact toolchain the later build resolves to; otherwise
# `cargo build` re-resolves the channel without the target. musl-tools provides
# musl-gcc, the C compiler the `ring` crate uses for the static musl target. No
# cmake/clang are needed (the rustls backend is ring, not aws-lc-rs).
COPY rust-toolchain.toml ./
RUN apt-get update && apt-get install -y --no-install-recommends musl-tools \
    && rm -rf /var/lib/apt/lists/* \
    && rustup show \
    && rustup target add "${TARGET}"
ENV CC_x86_64_unknown_linux_musl=musl-gcc
RUN cargo install cargo-chef --locked

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ARG TARGET
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --target "${TARGET}" --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --target "${TARGET}" --bin chronos-server
RUN cargo build --release --locked --target "${TARGET}" --bin chronos-gateway

FROM ${RUNTIME_IMAGE} AS runtime

# Build-time metadata for the per-build OCI labels below. Defaults keep `docker
# build` usable on its own; build-image.sh overrides them via --build-arg.
ARG VERSION="0.0.0-dev"
ARG REVISION="unknown"
ARG CREATED="2026-06-19T13:23:20Z"
ARG TARGET

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

# Static binaries need no runtime libraries; distroless/static ships only CA
# certificates, tzdata, and the nonroot user. /etc/chronos (config) and the
# chrony runtime dir (/run/chrony, for the gateway's SOCK refclock) are provided
# by runtime bind mounts (the image has no shell to create them).
COPY --from=builder /app/target/${TARGET}/release/chronos-server /usr/local/bin/chronos-server
COPY --from=builder /app/target/${TARGET}/release/chronos-gateway /usr/local/bin/chronos-gateway
ENV RUST_LOG=info
USER nonroot:nonroot
WORKDIR /
