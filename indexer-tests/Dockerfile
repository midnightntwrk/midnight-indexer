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
COPY ./indexer-api/Cargo.toml ./indexer-api/
COPY ./indexer-tests/Cargo.toml ./indexer-tests/
RUN mkdir ./indexer-common/src && \
    echo "#[allow(unused)] fn lib() {}" > ./indexer-common/src/lib.rs && \
    mkdir ./chain-indexer/src && \
    echo "fn main() {}" > ./chain-indexer/src/main.rs && \
    mkdir ./indexer-api/src && \
    echo "fn main() {}" > ./indexer-api/src/main.rs && \
    mkdir ./indexer-tests/src && \
    echo "fn main() {}" > ./indexer-tests/src/main.rs && \
    sed -i "/\"wallet-indexer\"/s/^/# /" Cargo.toml && \
    sed -i "/\"indexer-standalone\"/s/^/# /" Cargo.toml && \
    find . -path '*/src/*' | xargs touch -t 197001010000 -m
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo build -p indexer-tests --profile $PROFILE && \
    find ./target | xargs touch -t 197001020000 -m

COPY ./indexer-common/ ./indexer-common/
COPY ./indexer-api/ ./indexer-api/
COPY ./indexer-tests/ ./indexer-tests/
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo build -p indexer-tests --locked --profile $PROFILE && \
    mv ./target/$([ "$PROFILE" = "release" ] && echo "release" || echo "debug")/indexer-tests /

# ---------- IMAGE STAGE ---------- #

FROM debian:bookworm-slim AS image_stage

RUN adduser --disabled-password --gecos "" --home "/nonexistent" --shell "/sbin/nologin" --no-create-home --uid "10001" appuser && \
    mkdir /var/run/indexer-tests && \
    chown appuser:appuser /var/run/indexer-tests

COPY --from=build_stage --chown=appuser:appuser /indexer-tests /usr/local/bin/

USER appuser
WORKDIR /opt/indexer-tests
ENTRYPOINT ["indexer-tests"]
