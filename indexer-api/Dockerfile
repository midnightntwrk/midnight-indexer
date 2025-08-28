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
    cargo build -p indexer-api --locked --features cloud --profile $PROFILE && \
    mv ./target/$([ "$PROFILE" = "release" ] && echo "release" || echo "debug")/indexer-api /

FROM debian:bookworm-slim AS runtime
RUN adduser --disabled-password --gecos "" --home "/nonexistent" --shell "/sbin/nologin" --no-create-home --uid "10001" appuser && \
    mkdir /var/run/indexer-api && \
    chown appuser:appuser /var/run/indexer-api
COPY --from=builder --chown=appuser:appuser /build/indexer-api/bin/entrypoint.sh /usr/local/bin/
COPY --from=builder --chown=appuser:appuser /indexer-api /usr/local/bin/
COPY --from=builder --chown=appuser:appuser /build/indexer-api/config.yaml /opt/indexer-api/
USER appuser
WORKDIR /opt/indexer-api
ENTRYPOINT ["entrypoint.sh"]
EXPOSE 8088
