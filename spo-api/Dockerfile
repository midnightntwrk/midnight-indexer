ARG RUST_VERSION=1.89
FROM rust:${RUST_VERSION}-bookworm AS chef
WORKDIR /build
RUN cargo install cargo-chef --version 0.1.72

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
SHELL ["/bin/bash", "-c"]
ARG PROFILE=release
RUN git config --global url."https://@github.com".insteadOf "ssh://git@github.com"
COPY --from=planner /build/recipe.json recipe.json
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo chef cook --profile $PROFILE --recipe-path recipe.json
COPY . .
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo build -p spo-api --locked --features cloud --profile $PROFILE && \
    mkdir -p /runtime/usr/local/bin /runtime/opt/spo-api && \
    mv "./target/${PROFILE/dev/debug}/spo-api" /runtime/usr/local/bin/ && \
    install -Dm755 spo-api/bin/entrypoint.sh /runtime/usr/local/bin/entrypoint.sh && \
    install -Dm644 spo-api/config.yaml /runtime/opt/spo-api/config.yaml

FROM debian:bookworm-slim@sha256:b1a741487078b369e78119849663d7f1a5341ef2768798f7b7406c4240f86aef AS runtime
RUN adduser --disabled-password --gecos "" --home "/nonexistent" --shell "/sbin/nologin" --no-create-home --uid "10001" appuser && \
    mkdir /var/run/spo-api && \
    chown appuser:appuser /var/run/spo-api
COPY --from=builder --chown=appuser:appuser /runtime /
USER appuser
WORKDIR /opt/spo-api
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
EXPOSE 8090
