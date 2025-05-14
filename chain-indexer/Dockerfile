# ---------- BUILD STAGE ---------- #

ARG RUST_VERSION
FROM rust:$RUST_VERSION-bookworm AS build_stage

ARG PROFILE=release

WORKDIR /build

RUN git config --global url."https://@github.com".insteadOf "ssh://git@github.com"

COPY ./Cargo.toml ./Cargo.lock ./rust-toolchain.toml ./
COPY ./.cargo/ ./.cargo/
COPY ./indexer-common/Cargo.toml ./indexer-common/
COPY ./chain-indexer/Cargo.toml ./chain-indexer/
RUN mkdir ./indexer-common/src && \
    echo "#[allow(unused)] fn lib() {}" > ./indexer-common/src/lib.rs && \
    mkdir ./chain-indexer/src && \
    echo "fn main() {}" > ./chain-indexer/src/main.rs && \
    sed -i "/\"wallet-indexer\"/s/^/# /" Cargo.toml && \
    sed -i "/\"indexer-api\"/s/^/# /" Cargo.toml && \
    sed -i "/\"indexer-standalone\"/s/^/# /" Cargo.toml && \
    sed -i "/\"indexer-tests\"/s/^/# /" Cargo.toml && \
    find . -path '*/src/*' | xargs touch -t 197001010000 -m
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo build -p chain-indexer --features cloud --profile $PROFILE && \
    find ./target | xargs touch -t 197001020000 -m

COPY ./indexer-common/ ./indexer-common/
COPY ./chain-indexer/ ./chain-indexer/
COPY ./.node/ ./.node/
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo build -p chain-indexer --locked --features cloud --profile $PROFILE && \
    mv ./target/$([ "$PROFILE" = "release" ] && echo "release" || echo "debug")/chain-indexer /

# ---------- IMAGE STAGE ---------- #

FROM debian:bookworm-slim AS image_stage

ARG CA_CERTIFICATES_VERSION=20230311
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates=${CA_CERTIFICATES_VERSION} && \
    rm -rf /var/lib/apt/lists/*

RUN adduser --disabled-password --gecos "" --home "/nonexistent" --shell "/sbin/nologin" --no-create-home --uid "10001" appuser && \
    mkdir /var/run/chain-indexer && \
    chown appuser:appuser /var/run/chain-indexer

COPY --from=build_stage --chown=appuser:appuser /build/chain-indexer/bin/entrypoint.sh /usr/local/bin/
COPY --from=build_stage --chown=appuser:appuser /chain-indexer /usr/local/bin/
COPY --from=build_stage --chown=appuser:appuser /build/chain-indexer/config.yaml /opt/chain-indexer/

USER appuser
WORKDIR /opt/chain-indexer
ENTRYPOINT ["entrypoint.sh"]
