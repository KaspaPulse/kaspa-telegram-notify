# syntax=docker/dockerfile:1

# Keep the human-readable tag for Dependabot while pinning the immutable image digest.
FROM rust:1.98.1-slim-trixie@sha256:ce84a5edd80c5f91e05c5533b1e53eb1da54028f33734dc06aa6b49fa190462d AS builder

RUN apt-get update \
    && apt-get install --yes --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
ARG TARGETARCH

# Keep Cargo downloads and compiled dependencies reusable across source/version bumps.
# Target caches are architecture-scoped; the final binary is copied to /out because
# BuildKit cache-mount contents are intentionally not committed to image layers.
COPY Cargo.toml Cargo.lock ./
COPY xtask/Cargo.toml ./xtask/Cargo.toml
COPY opqual-fixture/Cargo.toml ./opqual-fixture/Cargo.toml
RUN --mount=type=cache,id=kaspa-pulse-cargo-registry-${TARGETARCH},target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=kaspa-pulse-cargo-git-${TARGETARCH},target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=kaspa-pulse-target-${TARGETARCH},target=/app/target,sharing=locked \
    mkdir -p src xtask/src opqual-fixture/src \
    && printf 'fn main() {}\n' > src/main.rs \
    && touch src/lib.rs \
    && printf 'fn main() {}\n' > xtask/src/main.rs \
    && printf 'fn main() {}\n' > opqual-fixture/src/main.rs \
    && cargo build --locked --release --all-features \
    && rm -rf src xtask/src opqual-fixture/src

COPY . .
ARG SOURCE_REVISION=unknown
ENV KASPA_PULSE_SOURCE_REVISION=$SOURCE_REVISION
ENV SQLX_OFFLINE=true
RUN --mount=type=cache,id=kaspa-pulse-cargo-registry-${TARGETARCH},target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=kaspa-pulse-cargo-git-${TARGETARCH},target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=kaspa-pulse-target-${TARGETARCH},target=/app/target,sharing=locked \
    touch src/main.rs \
    && cargo build --locked --release --all-features \
    && install -D -m 0755 target/release/kaspa-pulse /out/kaspa-pulse

FROM debian:trixie-slim@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a AS runtime

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
COPY --from=builder --chown=kaspa:kaspa /out/kaspa-pulse /usr/local/bin/kaspa-pulse

ENV PANIC_EVENT_MARKER_PATH=/var/lib/kaspa-pulse/panic_event_pending.json

USER 10001:10001
ENTRYPOINT ["/usr/local/bin/kaspa-pulse"]
