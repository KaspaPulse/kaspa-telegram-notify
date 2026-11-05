# syntax=docker/dockerfile:1
# Immutable multi-architecture manifest for official Rust 1.99.1 (verified 2026-11-05).
FROM rust:1.99.1-slim-trixie@sha256:59f72d5265cfcf281fcfa4d990e75f09c53f911def9913c47a4a89dca690a887 AS builder

RUN apt-get update \
    && apt-get install --yes --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
ARG TARGETARCH
ARG SOURCE_REVISION=unknown
ENV SQLX_OFFLINE=true
ENV CARGO_INCREMENTAL=0

# Vendored teloxide-core and the complete source must be present before
# Cargo resolves [patch.crates-io]. A dummy first build would fail here.
COPY . .

# Architecture-scoped caches are not committed to image layers.
RUN --mount=type=cache,id=kaspa-pulse-registry-${TARGETARCH},target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=kaspa-pulse-git-${TARGETARCH},target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=kaspa-pulse-target-${TARGETARCH},target=/app/target,sharing=locked \
    cargo build --locked --release --bin kaspa-block-alert-bot \
    && install -D -m 0755 target/release/kaspa-block-alert-bot /out/kaspa-block-alert-bot

FROM debian:trixie-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132 AS runtime

ARG SOURCE_REVISION=unknown
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && groupadd --system --gid 10001 kaspa \
    && useradd --system --uid 10001 --gid kaspa --home-dir /nonexistent --shell /usr/sbin/nologin kaspa \
    && install -d --owner=kaspa --group=kaspa --mode=0750 /var/lib/kaspa-pulse \
    && rm -rf /var/lib/apt/lists/*

LABEL org.opencontainers.image.source="https://github.com/KaspaPulse/kaspa-telegram-notify" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.revision="$SOURCE_REVISION"

WORKDIR /app
COPY --from=builder --chown=kaspa:kaspa /out/kaspa-block-alert-bot /usr/local/bin/kaspa-block-alert-bot

ENV PANIC_EVENT_MARKER_PATH=/var/lib/kaspa-pulse/panic_event_pending.json
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/kaspa-block-alert-bot"]
