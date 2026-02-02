ARG RUST_VERSION
FROM lukemathwalker/cargo-chef:0.1.73-rust-$RUST_VERSION-trixie AS chef
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
RUN cargo build -p spo-indexer --locked --features cloud --profile $PROFILE && \
    mkdir -p /runtime/usr/local/bin && \
    mv "./target/${PROFILE/dev/debug}/spo-indexer" /runtime/usr/local/bin && \
    mv /build/spo-indexer/bin/entrypoint.sh /runtime/usr/local/bin && \
    mkdir -p /runtime/opt/spo-indexer && \
    mv /build/spo-indexer/config.yaml /runtime/opt/spo-indexer

FROM debian:trixie-slim@sha256:66b37a5078a77098bfc80175fb5eb881a3196809242fd295b25502854e12cbec AS runtime
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates=20250419 && \
    rm -rf /var/lib/apt/lists/*
RUN useradd -u 10001 -d /nonexistent -s /usr/sbin/nologin -M -c "" appuser && \
    passwd -l appuser && \
    mkdir /var/run/spo-indexer && \
    chown appuser:appuser /var/run/spo-indexer
COPY --from=builder --chown=appuser:appuser /runtime /
USER appuser
WORKDIR /opt/spo-indexer
ENTRYPOINT ["entrypoint.sh"]
