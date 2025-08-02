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
COPY ./wallet-indexer/Cargo.toml ./wallet-indexer/
COPY ./indexer-api/Cargo.toml ./indexer-api/
COPY ./indexer-standalone/Cargo.toml ./indexer-standalone/
RUN mkdir ./indexer-common/src && \
    echo "#[allow(unused)] fn lib() {}" > ./indexer-common/src/lib.rs && \
    mkdir ./chain-indexer/src && \
    echo "fn main() {}" > ./chain-indexer/src/main.rs && \
    mkdir ./wallet-indexer/src && \
    echo "fn main() {}" > ./wallet-indexer/src/main.rs && \
    mkdir ./indexer-api/src && \
    echo "fn main() {}" > ./indexer-api/src/main.rs && \
    mkdir ./indexer-standalone/src && \
    echo "fn main() {}" > ./indexer-standalone/src/main.rs && \
    sed -i "/\"indexer-tests\"/s/^/# /" Cargo.toml && \
    find . -path '*/src/*' | xargs touch -t 197001010000 -m
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo build -p indexer-standalone --features standalone --profile $PROFILE && \
    find ./target | xargs touch -t 197001020000 -m

COPY ./indexer-common/ ./indexer-common/
COPY ./chain-indexer/ ./chain-indexer/
COPY ./wallet-indexer/ ./wallet-indexer/
COPY ./indexer-api/ ./indexer-api/
COPY ./indexer-standalone/ ./indexer-standalone/
COPY ./.node/ ./.node/
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    cargo build -p indexer-standalone --locked --features standalone --profile $PROFILE && \
    mv ./target/$([ "$PROFILE" = "release" ] && echo "release" || echo "debug")/indexer-standalone /

# ---------- IMAGE STAGE ---------- #

FROM debian:bookworm-slim AS image_stage

ARG CA_CERTIFICATES_VERSION=20230311
# Install SQLite for mock data loading (development only)
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates=${CA_CERTIFICATES_VERSION} sqlite3 && \
    rm -rf /var/lib/apt/lists/*

RUN adduser --disabled-password --gecos "" --home "/nonexistent" --shell "/sbin/nologin" --no-create-home --uid "10001" appuser && \
    mkdir /var/run/indexer-standalone && \
    chown appuser:appuser /var/run/indexer-standalone && \
    mkdir /data && \
    chown appuser:appuser /data && \
    mkdir -p /opt/mock-data/sqlite && \
    chown -R appuser:appuser /opt/mock-data
    
COPY --from=build_stage --chown=appuser:appuser /build/indexer-standalone/bin/entrypoint.sh /usr/local/bin/
COPY --from=build_stage --chown=appuser:appuser /build/indexer-standalone/bin/entrypoint-with-mock-data.sh /usr/local/bin/
COPY --from=build_stage --chown=appuser:appuser /build/indexer-standalone/bin/check-and-load-sqlite-mock-data.sh /opt/mock-data/check-and-load-mock-data.sh
COPY --from=build_stage --chown=appuser:appuser /indexer-standalone /usr/local/bin/
COPY --from=build_stage --chown=appuser:appuser /build/indexer-standalone/config.yaml /opt/indexer-standalone/

# Copy SQLite mock data for development (temporary until node/ledger integration)
COPY --from=build_stage --chown=appuser:appuser /build/indexer-standalone/test-data/sqlite/*.sh /opt/mock-data/sqlite/

VOLUME ["/data"]
EXPOSE 8088
USER appuser
WORKDIR /opt/indexer-standalone

# Use development entrypoint with mock data loading
# TODO: Switch back to entrypoint.sh when node/ledger integration is ready
ENTRYPOINT ["entrypoint-with-mock-data.sh"]
