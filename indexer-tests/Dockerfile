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
RUN cargo build -p indexer-tests --locked --profile $PROFILE && \
    mkdir -p /runtime/usr/local/bin && \
    mv "./target/${PROFILE/dev/debug}/indexer-tests" /runtime/usr/local/bin

FROM debian:trixie-slim@sha256:f6e2cfac5cf956ea044b4bd75e6397b4372ad88fe00908045e9a0d21712ae3ba AS runtime
RUN useradd -u 10001 -d /nonexistent -s /usr/sbin/nologin -M -c "" appuser && \
    passwd -l appuser && \
    mkdir /var/run/indexer-tests && \
    chown appuser:appuser /var/run/indexer-tests
COPY --from=builder --chown=appuser:appuser /runtime /
USER appuser
WORKDIR /opt/indexer-tests
ENTRYPOINT ["indexer-tests"]
