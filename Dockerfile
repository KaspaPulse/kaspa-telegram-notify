# syntax=docker/dockerfile:1

# Keep the human-readable tag for Dependabot while pinning the immutable image digest.
FROM rust:1.98.0-slim-trixie@sha256:17d1ba895198f9934c6314ec5346a0d5115372f3243390c3d731e242f35c2f27 AS builder

RUN apt-get update \
    && apt-get install --yes --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && printf 'fn main() {}\n' > src/main.rs \
    && touch src/lib.rs \
    && cargo build --locked --release --all-features \
    && rm -rf src

COPY . .
ENV SQLX_OFFLINE=true
RUN touch src/main.rs \
    && cargo build --locked --release --all-features

FROM debian:trixie-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132 AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && groupadd --system --gid 10001 kaspa \
    && useradd --system --uid 10001 --gid kaspa --home-dir /nonexistent --shell /usr/sbin/nologin kaspa \
    && install -d --owner=kaspa --group=kaspa --mode=0750 /var/lib/kaspa-pulse \
    && rm -rf /var/lib/apt/lists/*

LABEL org.opencontainers.image.source="https://github.com/KaspaPulse/kaspa-telegram-notify" \
      org.opencontainers.image.licenses="MIT"

WORKDIR /app
COPY --from=builder --chown=kaspa:kaspa /app/target/release/kaspa-pulse /usr/local/bin/kaspa-pulse

ENV PANIC_EVENT_MARKER_PATH=/var/lib/kaspa-pulse/panic_event_pending.json

USER 10001:10001
ENTRYPOINT ["/usr/local/bin/kaspa-pulse"]
