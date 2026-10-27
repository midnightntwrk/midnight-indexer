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
RUN cargo auditable build -p indexer-api --locked --features cloud --profile $PROFILE && \
    if ! readelf -S "./target/${PROFILE/dev/debug}/indexer-api" | grep -q '\.dep-v0'; then \
        echo "indexer-api binary lacks the .dep-v0 section embedded by cargo auditable" >&2; exit 1; \
    fi && \
    mkdir -p /runtime/usr/local/bin && \
    mv "./target/${PROFILE/dev/debug}/indexer-api" /runtime/usr/local/bin && \
    mv /build/indexer-api/bin/entrypoint.sh /runtime/usr/local/bin && \
    mkdir -p /runtime/opt/indexer-api && \
    mv /build/indexer-api/config.yaml /runtime/opt/indexer-api

FROM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad AS runtime
RUN useradd -u 10001 -d /nonexistent -s /usr/sbin/nologin -M -c "" appuser && \
    passwd -l appuser && \
    mkdir /var/run/indexer-api && \
    chown appuser:appuser /var/run/indexer-api
COPY --from=builder --chown=appuser:appuser /runtime /
USER appuser
WORKDIR /opt/indexer-api
ENTRYPOINT ["entrypoint.sh"]
EXPOSE 8088
