# ---------- BUILD STAGE ---------- #

ARG RUST_VERSION
FROM rust:$RUST_VERSION-bookworm AS build_stage

ARG PROFILE=release

WORKDIR /build

RUN git config --global url."https://@github.com".insteadOf "ssh://git@github.com"

COPY --from=ghcr.io/midnight-ntwrk/proof-server:3.1.0-rc.1 /nix/store/m2vp9cxky1ryckkmgqd5x7d2q2mi7chw-midnight-testing-public-parameters-0.1.0/kzg ./target/proof-server/public-parameters/kzg
COPY --from=ghcr.io/midnight-ntwrk/proof-server:3.1.0-rc.1 /nix/store/m2vp9cxky1ryckkmgqd5x7d2q2mi7chw-midnight-testing-public-parameters-0.1.0/kzg.vp ./target/proof-server/public-parameters/kzg.vp
COPY --from=ghcr.io/midnight-ntwrk/proof-server:3.1.0-rc.1 /nix/store/6vjbkbvb8ha75gbsimf1ksn980pmsm5i-midnight-zswap-artifacts-3.1.0-rc.1/zswap/ ./target/proof-server/zswap/

COPY ./Cargo.toml ./Cargo.lock ./rust-toolchain.toml ./
COPY ./.cargo/ ./.cargo/
COPY ./indexer-common/Cargo.toml ./indexer-common/
COPY ./wallet-indexer/Cargo.toml ./wallet-indexer/
RUN mkdir ./indexer-common/src && \
    echo "#[allow(unused)] fn lib() {}" > ./indexer-common/src/lib.rs && \
    mkdir ./wallet-indexer/src && \
    echo "fn main() {}" > ./wallet-indexer/src/main.rs && \
    sed -i "/\"chain-indexer\"/s/^/# /" Cargo.toml && \
    sed -i "/\"indexer-api\"/s/^/# /" Cargo.toml && \
    sed -i "/\"indexer-standalone\"/s/^/# /" Cargo.toml && \
    sed -i "/\"indexer-tests\"/s/^/# /" Cargo.toml && \
    find . -path '*/src/*' | xargs touch -t 197001010000 -m
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    MIDNIGHT_LEDGER_STATIC_DIR=/build/target/proof-server \
    ZKIR_PP=/build/target/proof-server/public-parameters \
    cargo build -p wallet-indexer --features cloud --profile $PROFILE && \
    find ./target | xargs touch -t 197001020000 -m

COPY ./indexer-common/ ./indexer-common/
COPY ./wallet-indexer/ ./wallet-indexer/
RUN --mount=type=secret,id=netrc,target=/root/.netrc \
    MIDNIGHT_LEDGER_STATIC_DIR=/build/target/proof-server \
    ZKIR_PP=/build/target/proof-server/public-parameters \
    cargo build -p wallet-indexer --locked --features cloud --profile $PROFILE && \
    mv ./target/$([ "$PROFILE" = "release" ] && echo "release" || echo "debug")/wallet-indexer /

# ---------- IMAGE STAGE ---------- #

FROM debian:bookworm-slim AS image_stage

RUN adduser --disabled-password --gecos "" --home "/nonexistent" --shell "/sbin/nologin" --no-create-home --uid "10001" appuser && \
    mkdir /var/run/wallet-indexer && \
    chown appuser:appuser /var/run/wallet-indexer

COPY --from=build_stage --chown=appuser:appuser /build/wallet-indexer/bin/entrypoint.sh /usr/local/bin/
COPY --from=build_stage --chown=appuser:appuser /wallet-indexer /usr/local/bin/
COPY --from=build_stage --chown=appuser:appuser /build/wallet-indexer/config.yaml /opt/wallet-indexer/

USER appuser
WORKDIR /opt/wallet-indexer
ENTRYPOINT ["entrypoint.sh"]
