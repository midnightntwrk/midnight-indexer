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

FROM chef AS builder
SHELL ["/bin/bash", "-c"]
ARG PROFILE=release
COPY --from=planner /build/recipe.json recipe.json
# Same package and features as the build below, so cargo reuses every dependency compiled here.
RUN cargo chef cook -p indexer-tests --locked --profile $PROFILE --recipe-path recipe.json
COPY --from=auditable /usr/local/cargo/bin/cargo-auditable /usr/local/cargo/bin/
COPY . .
RUN cargo auditable build -p indexer-tests --locked --profile $PROFILE && \
    if ! readelf -S "./target/${PROFILE/dev/debug}/indexer-tests" | grep -q '\.dep-v0'; then \
        echo "indexer-tests binary lacks the .dep-v0 section embedded by cargo auditable" >&2; exit 1; \
    fi && \
    mkdir -p /runtime/usr/local/bin && \
    mv "./target/${PROFILE/dev/debug}/indexer-tests" /runtime/usr/local/bin

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2 AS runtime
# The base image predates the trixie-security zlib1g fix for CVE-2026-85091 and CVE-2026-76844.
RUN apt-get update && \
    apt-get install -y --no-install-recommends zlib1g=1:1.3.dfsg+really1.3.1-1+deb13u1 && \
    rm -rf /var/lib/apt/lists/*
RUN useradd -u 10001 -d /nonexistent -s /usr/sbin/nologin -M -c "" appuser && \
    passwd -l appuser && \
    mkdir /var/run/indexer-tests && \
    chown appuser:appuser /var/run/indexer-tests
# Root-owned, so the service can run and read these but not replace them; it only writes
# to the directories created for it above.
COPY --from=builder /runtime /
# appuser, by UID: Kubernetes runAsNonRoot can only verify a numeric user.
USER 10001
WORKDIR /opt/indexer-tests
ENTRYPOINT ["indexer-tests"]
