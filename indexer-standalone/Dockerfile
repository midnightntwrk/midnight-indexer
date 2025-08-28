ARG RUST_VERSION
FROM lukemathwalker/cargo-chef:0.1.72-rust-$RUST_VERSION-bookworm AS chef
WORKDIR /build

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ARG PROFILE=release
RUN git config --global url."https://@github.com".insteadOf "ssh://git@github.com"
COPY --from=planner /build/recipe.json recipe.json
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo chef cook --profile $PROFILE --recipe-path recipe.json
COPY . .
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo build -p indexer-standalone --locked --features standalone --profile $PROFILE && \
    mv ./target/$([ "$PROFILE" = "release" ] && echo "release" || echo "debug")/indexer-standalone /

FROM debian:bookworm-slim AS runtime
ARG CA_CERTIFICATES_VERSION=20230311+deb12u1
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates=${CA_CERTIFICATES_VERSION} && \
    rm -rf /var/lib/apt/lists/*
RUN adduser --disabled-password --gecos "" --home "/nonexistent" --shell "/sbin/nologin" --no-create-home --uid "10001" appuser && \
    mkdir /var/run/indexer-standalone && \
    chown appuser:appuser /var/run/indexer-standalone
COPY --from=builder --chown=appuser:appuser /build/indexer-standalone/bin/entrypoint.sh /usr/local/bin/
COPY --from=builder --chown=appuser:appuser /indexer-standalone /usr/local/bin/
COPY --from=builder --chown=appuser:appuser /build/indexer-standalone/config.yaml /opt/indexer-standalone/
USER appuser
WORKDIR /opt/indexer-standalone
ENTRYPOINT ["entrypoint.sh"]
EXPOSE 8088
