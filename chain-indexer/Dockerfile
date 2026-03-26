ARG RUST_VERSION
FROM lukemathwalker/cargo-chef:0.1.77-rust-$RUST_VERSION-trixie AS chef
WORKDIR /build

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
SHELL ["/bin/bash", "-c"]
ARG PROFILE=release
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --profile $PROFILE --recipe-path recipe.json
COPY . .
RUN cargo build -p chain-indexer --locked --features cloud --profile $PROFILE && \
    mkdir -p /runtime/usr/local/bin && \
    mv "./target/${PROFILE/dev/debug}/chain-indexer" /runtime/usr/local/bin && \
    mv /build/chain-indexer/bin/entrypoint.sh /runtime/usr/local/bin && \
    mkdir -p /runtime/opt/chain-indexer && \
    mv /build/chain-indexer/config.yaml /runtime/opt/chain-indexer

FROM debian:trixie-slim@sha256:26f98ccd92fd0a44d6928ce8ff8f4921b4d2f535bfa07555ee5d18f61429cf0c AS runtime
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates=20250419 && \
    rm -rf /var/lib/apt/lists/*
RUN useradd -u 10001 -d /nonexistent -s /usr/sbin/nologin -M -c "" appuser && \
    passwd -l appuser && \
    mkdir /var/run/chain-indexer && \
    chown appuser:appuser /var/run/chain-indexer
COPY --from=builder --chown=appuser:appuser /runtime /
USER appuser
WORKDIR /opt/chain-indexer
ENTRYPOINT ["entrypoint.sh"]
