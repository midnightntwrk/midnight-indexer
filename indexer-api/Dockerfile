# ---------- BUILD STAGE ---------- #

ARG RUST_VERSION
FROM rust:$RUST_VERSION-bookworm AS build_stage

ARG PROFILE=release

WORKDIR /build

RUN git config --global url."https://@github.com".insteadOf "ssh://git@github.com"

RUN apt-get update && apt-get install -y protobuf-compiler

COPY ./Cargo.toml ./Cargo.lock ./rust-toolchain.toml ./
COPY ./.cargo/ ./.cargo/
COPY ./indexer-common/Cargo.toml ./indexer-common/
COPY ./indexer-api/Cargo.toml ./indexer-api/
RUN mkdir ./indexer-common/src && \
    echo "#[allow(unused)] fn lib() {}" > ./indexer-common/src/lib.rs && \
    mkdir ./indexer-api/src && \
    echo "fn main() {}" > ./indexer-api/src/main.rs && \
    sed -i "/\"chain-indexer\"/s/^/# /" Cargo.toml && \
    sed -i "/\"wallet-indexer\"/s/^/# /" Cargo.toml && \
    sed -i "/\"indexer-standalone\"/s/^/# /" Cargo.toml && \
    sed -i "/\"indexer-tests\"/s/^/# /" Cargo.toml && \
    find . -path '*/src/*' | xargs touch -t 197001010000 -m
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo build -p indexer-api --features cloud --profile $PROFILE && \
    find ./target | xargs touch -t 197001020000 -m

COPY ./indexer-common/ ./indexer-common/
COPY ./indexer-api/ ./indexer-api/
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo build -p indexer-api --bin indexer-api --locked --features cloud --profile $PROFILE && \
    mv ./target/$([ "$PROFILE" = "release" ] && echo "release" || echo "debug")/indexer-api /

# ---------- IMAGE STAGE ---------- #

FROM debian:bookworm-slim AS image_stage

RUN adduser --disabled-password --gecos "" --home "/nonexistent" --shell "/sbin/nologin" --no-create-home --uid "10001" appuser && \
    mkdir /var/run/indexer-api && \
    chown appuser:appuser /var/run/indexer-api

COPY --from=build_stage --chown=appuser:appuser /build/indexer-api/bin/entrypoint.sh /usr/local/bin/
COPY --from=build_stage --chown=appuser:appuser /indexer-api /usr/local/bin/
COPY --from=build_stage --chown=appuser:appuser /build/indexer-api/config.yaml /opt/indexer-api/

EXPOSE 8088
USER appuser
WORKDIR /opt/indexer-api
ENTRYPOINT ["entrypoint.sh"]
