ARG RUST_VERSION
FROM lukemathwalker/cargo-chef:0.1.77-rust-$RUST_VERSION-trixie AS chef
WORKDIR /build

FROM chef AS auditable
ARG CARGO_AUDITABLE_VERSION
RUN cargo install cargo-auditable --locked --version "${CARGO_AUDITABLE_VERSION:?build arg required}"

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
SHELL ["/bin/bash", "-c"]
ARG PROFILE=release
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --profile $PROFILE --recipe-path recipe.json
COPY --from=auditable /usr/local/cargo/bin/cargo-auditable /usr/local/cargo/bin/
COPY . .
RUN cargo auditable build -p wallet-indexer --locked --features cloud --profile $PROFILE && \
    if ! readelf -S "./target/${PROFILE/dev/debug}/wallet-indexer" | grep -q '\.dep-v0'; then \
        echo "wallet-indexer binary lacks the .dep-v0 section embedded by cargo auditable" >&2; exit 1; \
    fi && \
    mkdir -p /runtime/usr/local/bin && \
    mv "./target/${PROFILE/dev/debug}/wallet-indexer" /runtime/usr/local/bin && \
    mv /build/wallet-indexer/bin/entrypoint.sh /runtime/usr/local/bin && \
    mkdir -p /runtime/opt/wallet-indexer && \
    mv /build/wallet-indexer/config.yaml /runtime/opt/wallet-indexer

FROM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad AS runtime
RUN useradd -u 10001 -d /nonexistent -s /usr/sbin/nologin -M -c "" appuser && \
    passwd -l appuser && \
    mkdir /var/run/wallet-indexer && \
    chown appuser:appuser /var/run/wallet-indexer
COPY --from=builder --chown=appuser:appuser /runtime /
USER appuser
WORKDIR /opt/wallet-indexer
ENTRYPOINT ["entrypoint.sh"]
