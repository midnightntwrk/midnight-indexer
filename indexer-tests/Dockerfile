ARG RUST_VERSION
FROM lukemathwalker/cargo-chef:0.1.73-rust-$RUST_VERSION-bookworm AS chef
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

FROM debian:bookworm-slim@sha256:b1a741487078b369e78119849663d7f1a5341ef2768798f7b7406c4240f86aef AS runtime
RUN adduser --disabled-password --gecos "" --home "/nonexistent" --shell "/sbin/nologin" --no-create-home --uid "10001" appuser && \
    mkdir /var/run/indexer-tests && \
    chown appuser:appuser /var/run/indexer-tests
COPY --from=builder --chown=appuser:appuser /runtime /
USER appuser
WORKDIR /opt/indexer-tests
ENTRYPOINT ["indexer-tests"]
