FROM rust:1.98.1-trixie@sha256:a8a5f0a1e5fe7dfe1d352591e4a1c7dd2c08fd70475cae872cf3458ba0df0546 AS toolchain
ARG RUST_VERSION
# cargo chef cook runs before rust-toolchain.toml is copied in, so the image's Rust must match it.
RUN test "$(rustc --version | cut -d ' ' -f 2)" = "${RUST_VERSION:?build arg required}" || \
    { echo "image has $(rustc --version), rust-toolchain.toml wants ${RUST_VERSION}" >&2; exit 1; }

FROM toolchain AS chef
ARG CARGO_CHEF_VERSION
RUN cargo install cargo-chef --locked --version "${CARGO_CHEF_VERSION:?build arg required}" && \
    rm -rf "$CARGO_HOME/registry"
WORKDIR /build

# Built from toolchain, not chef, so bumping either tool leaves the other's layer cached.
FROM toolchain AS auditable
ARG CARGO_AUDITABLE_VERSION
RUN cargo install cargo-auditable --locked --version "${CARGO_AUDITABLE_VERSION:?build arg required}" && \
    rm -rf "$CARGO_HOME/registry"

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json
# Only this fetch sees the token, and it runs no build script. Git asks the helper for the token
# when the ledger repository demands credentials; libgit2, cargo's default git client, ignores
# git config credentials.
RUN --mount=type=secret,id=git_token \
    CARGO_NET_GIT_FETCH_WITH_CLI=true \
    GIT_CONFIG_COUNT=1 \
    GIT_CONFIG_KEY_0=credential.https://github.com.helper \
    GIT_CONFIG_VALUE_0='!f() { echo username=x-access-token; echo "password=$(cat /run/secrets/git_token)"; }; f' \
    cargo fetch --locked

FROM chef AS builder
SHELL ["/bin/bash", "-c"]
ARG PROFILE=release
COPY --from=planner /build/recipe.json recipe.json
COPY --from=planner /usr/local/cargo/git /usr/local/cargo/git
COPY --from=planner /usr/local/cargo/registry /usr/local/cargo/registry
# Builds from the sources the planner fetched, so a missing one fails instead of fetching.
ENV CARGO_NET_OFFLINE=true
# Same package and features as the build below, so cargo reuses every dependency compiled here.
RUN cargo chef cook -p chain-indexer --locked --features cloud --profile $PROFILE --recipe-path recipe.json
COPY --from=auditable /usr/local/cargo/bin/cargo-auditable /usr/local/cargo/bin/
COPY . .
RUN cargo auditable build -p chain-indexer --locked --features cloud --profile $PROFILE && \
    if ! readelf -S "./target/${PROFILE/dev/debug}/chain-indexer" | grep -q '\.dep-v0'; then \
        echo "chain-indexer binary lacks the .dep-v0 section embedded by cargo auditable" >&2; exit 1; \
    fi && \
    mkdir -p /runtime/usr/local/bin && \
    mv "./target/${PROFILE/dev/debug}/chain-indexer" /runtime/usr/local/bin && \
    mv /build/chain-indexer/bin/entrypoint.sh /runtime/usr/local/bin && \
    mkdir -p /runtime/opt/chain-indexer && \
    mv /build/chain-indexer/config.yaml /runtime/opt/chain-indexer

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2 AS runtime
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates=20250419+deb13u1 && \
    rm -rf /var/lib/apt/lists/*
RUN useradd -u 10001 -d /nonexistent -s /usr/sbin/nologin -M -c "" appuser && \
    passwd -l appuser && \
    mkdir /var/run/chain-indexer && \
    chown appuser:appuser /var/run/chain-indexer
# Root-owned, so the service can run and read these but not replace them; it only writes
# to the directories created for it above.
COPY --from=builder /runtime /
# appuser, by UID: Kubernetes runAsNonRoot can only verify a numeric user.
USER 10001
WORKDIR /opt/chain-indexer
ENTRYPOINT ["entrypoint.sh"]
