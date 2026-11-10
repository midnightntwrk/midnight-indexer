set shell := ["bash", "-uc"]

# Can be overridden on the command line: `just feature=standalone`.
feature := "cloud"
packages := "indexer-common chain-indexer wallet-indexer indexer-api spo-indexer indexer-standalone indexer-tests"
rust_version := `grep channel rust-toolchain.toml | sed -r 's/channel = "(.*)"/\1/'`
latest_node_version := `tail -n 1 NODE_VERSIONS`
cargo_auditable_version := `grep -E '^\s+CARGO_AUDITABLE_VERSION:' .github/workflows/build-indexer-images.yaml | sed -E 's/#.*//; s/^[^:]*:[[:space:]]*//; s/[[:space:]]+$//' | tr -d \"\'`
cargo_chef_version := `grep -E '^\s+CARGO_CHEF_VERSION:' .github/workflows/build-indexer-images.yaml | sed -E 's/#.*//; s/^[^:]*:[[:space:]]*//; s/[[:space:]]+$//' | tr -d \"\'`

check:
    for package in {{packages}}; do \
        cargo check -p "$package" --tests --features {{feature}}; \
    done

license-headers:
    ./license_headers.sh

fmt:
    cargo fmt

fmt-check:
    cargo fmt --check

fix:
    cargo fix --allow-dirty --allow-staged --features {{feature}} --tests

lint:
    for package in {{packages}}; do \
        cargo clippy -p "$package" --no-deps --tests --features {{feature}} -- -D warnings; \
    done

lint-fix:
    for package in {{packages}}; do \
        cargo clippy -p "$package" --no-deps --tests --fix --allow-dirty --allow-staged --features {{feature}}; \
    done

test:
    # We must build the executables needed by the e2e tests!
    # Selecting the whole workspace with tests resolves the same dependency features as nextest, so nextest and the schema check reuse these artifacts.
    # Naming the binaries keeps this build from overwriting the other feature's executables in target/debug. Cargo still rebuilds chain-indexer because its integration tests need its binary.
    cargo build --workspace --tests --features {{feature}} --bin indexer-api-cli \
        {{ if feature == "cloud" { "--bin chain-indexer --bin wallet-indexer --bin indexer-api" } else { "--bin indexer-standalone" } }}
    cargo nextest run --workspace --exclude indexer-standalone --features {{feature}}
    # Check indexer-api schema:
    "${CARGO_TARGET_DIR:-target}/debug/indexer-api-cli" print-api-schema-v4 > \
        indexer-api/graphql/schema-v4.graphql.check
    @if ! cmp -s indexer-api/graphql/schema-v4.graphql indexer-api/graphql/schema-v4.graphql.check; then \
        echo "schema-v4.graphql has changes!"; exit 1; \
    fi

# `doc_cfg` (feature badges) is unstable; RUSTC_BOOTSTRAP=1 lets the pinned stable toolchain accept it.
doc:
    RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo doc -p indexer-common --no-deps --features {{feature}}
    RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo doc -p chain-indexer  --no-deps --features {{feature}}
    RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo doc -p wallet-indexer --no-deps --features {{feature}}
    RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo doc -p indexer-api    --no-deps --features {{feature}}
    if [ "{{feature}}" = "standalone" ]; then \
        RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo doc -p indexer-standalone --no-deps --features standalone; \
    fi

bench:
    cargo bench -p indexer-common -p chain-indexer --features standalone

all: license-headers check fmt lint test doc

all-all:
    just feature=cloud all
    just feature=standalone all

coverage:
    ./coverage.sh

generate-indexer-api-schema:
    cargo run -p indexer-api --features {{feature}} --bin indexer-api-cli print-api-schema-v4 > \
        indexer-api/graphql/schema-v4.graphql

generate-spo-api-schema:
    cargo run -p spo-api --features cloud --bin spo-api-cli print-api-schema-v1 > \
        spo-api/graphql/schema-v1.graphql

build-docker-image package profile="dev":
    tag=$(git rev-parse --short=8 HEAD) && \
    docker build \
        --build-arg "RUST_VERSION={{rust_version}}" \
        --build-arg "PROFILE={{profile}}" \
        --build-arg "CARGO_AUDITABLE_VERSION={{cargo_auditable_version}}" \
        --build-arg "CARGO_CHEF_VERSION={{cargo_chef_version}}" \
        -t midnightntwrk/{{package}}:${tag} \
        -t midnightntwrk/{{package}}:latest \
        -f {{package}}/Dockerfile \
        .

run-chain-indexer node="ws://localhost:9944" network_id="undeployed":
    docker compose up -d --wait postgres nats
    RUST_LOG=chain_indexer=debug,indexer_common=debug,fastrace_opentelemetry=off,tracing::span=off,midnight_ledger=warn,midnight_zswap=warn,info \
        CONFIG_FILE=chain-indexer/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        APP__INFRA__NODE__URL={{node}} \
        cargo run -p chain-indexer --features {{feature}}

run-wallet-indexer network_id="undeployed":
    docker compose up -d --wait postgres nats
    RUST_LOG=wallet_indexer=debug,indexer_common=debug,fastrace_opentelemetry=off,info \
        CONFIG_FILE=wallet-indexer/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        cargo run -p wallet-indexer --features {{feature}}

run-indexer-api network_id="undeployed":
    docker compose up -d --wait postgres nats
    RUST_LOG=indexer_api=debug,indexer_common=debug,info \
        CONFIG_FILE=indexer-api/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        cargo run -p indexer-api --bin indexer-api --features {{feature}}

run-spo-indexer node="ws://localhost:9944" network_id="undeployed":
    docker compose up -d --wait postgres
    RUST_LOG=spo_indexer=debug,indexer_common=debug,fastrace_opentelemetry=off,info \
        CONFIG_FILE=spo-indexer/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        APP__INFRA__NODE__URL={{node}} \
        cargo run -p spo-indexer --features {{feature}}

run-indexer-standalone node="ws://localhost:9944" network_id="undeployed":
    mkdir -p target/data
    RUST_LOG=indexer=debug,chain_indexer=debug,wallet_indexer=debug,indexer_api=debug,indexer_common=debug,fastrace_opentelemetry=off,info \
        CONFIG_FILE=indexer-standalone/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        APP__INFRA__NODE__URL={{node}} \
        APP__INFRA__STORAGE__CNN_URL=target/data/indexer.sqlite \
        APP__INFRA__LEDGER_DB__CNN_URL=target/data/ledger-db.sqlite \
        cargo run -p indexer-standalone --features standalone

# generate-node-data wipes `.node/<node_version>`, so it has to run before
# get-node-metadata writes metadata.scale there.
update-node node_version=latest_node_version toolkit_version=node_version: (generate-node-data node_version toolkit_version) (get-node-metadata node_version)

generate-node-data node_version=latest_node_version toolkit_version=node_version:
    ./generate_node_data.sh {{node_version}} {{toolkit_version}}

get-node-metadata node_version=latest_node_version:
    ./get_node_metadata.sh {{node_version}}

generate-txs:
    ./generate_txs.sh {{latest_node_version}}

run-node node_version=latest_node_version:
    #!/usr/bin/env bash
    node_dir=$(mktemp -d)
    cp -r ./.node/{{node_version}}/ $node_dir
    # SIDECHAIN_BLOCK_BENEFICIARY specifies the wallet that receives block rewards and transaction fees (DUST).
    # This hex value is a public key that matches the one used in toolkit-e2e.sh.
    docker run \
        --name node \
        -p 9944:9944 \
        -e SHOW_CONFIG=false \
        -e CFG_PRESET=dev \
        -e SIDECHAIN_BLOCK_BENEFICIARY="04bcf7ad3be7a5c790460be82a713af570f22e0f801f6659ab8e84a52be6969e" \
        -v $node_dir:/node \
        midnightntwrk/midnight-node:{{node_version}}

# --- Forked-network integration (midnight-node local-environment) ------------
# Runs the cloud indexer stack against a forked well-known network (e.g. a
# Preview fork) brought up by midnight-node's `local-environment` tooling.
# See docs/running-against-a-fork.md. Set MIDNIGHT_NODE_DIR to an existing
# midnight-node checkout to skip the sparse clone.

midnight_node_dir := env_var_or_default("MIDNIGHT_NODE_DIR", ".midnight-node")
# A checkout supplied via MIDNIGHT_NODE_DIR is the user's own and is used as-is.
midnight_node_dir_is_managed := if env_var_or_default("MIDNIGHT_NODE_DIR", "") == "" { "true" } else { "false" }
midnight_node_repo := env_var_or_default("MIDNIGHT_NODE_REPO", "https://github.com/midnightntwrk/midnight-node")
# Forks need local-environment to convert the boot node too (midnight-node#2308),
# which no node release includes yet; use `node-<version>` once one does.
midnight_node_ref := env_var_or_default("MIDNIGHT_NODE_REF", "main")
# Released local-environment versions default to an older image whose fork
# never finalizes and whose fork block chain-indexer cannot decode.
mock_authorities_image := env_var_or_default("MOCK_AUTHORITIES_IMAGE", "ghcr.io/shieldedtech/mock-authorities:14cee4b")
# Keep in sync with the default in docker-compose.midnight-fork.yaml.
fork_indexer_tag := env_var_or_default("INDEXER_TAG", "4.4.101")

# Sparse-clone midnight-node (local-environment only) and check out MIDNIGHT_NODE_REF (branch, tag or SHA).
fork-clone:
    #!/usr/bin/env bash
    set -euo pipefail
    dir="{{midnight_node_dir}}"
    if [ "{{midnight_node_dir_is_managed}}" != true ]; then
        [ -d "$dir/local-environment" ] || { echo "MIDNIGHT_NODE_DIR=$dir has no local-environment/" >&2; exit 1; }
        echo "using midnight-node checkout $dir at $(git -C "$dir" rev-parse --short HEAD 2>/dev/null || echo '<not a git checkout>')"
        exit 0
    fi
    if [ ! -d "$dir/.git" ]; then
        git clone --quiet --filter=blob:none --sparse --no-checkout --depth 1 "{{midnight_node_repo}}" "$dir"
        git -C "$dir" sparse-checkout set local-environment
    fi
    # Fetched on every run so a changed ref, or a moved branch, is picked up.
    git -C "$dir" fetch --quiet --depth 1 origin "{{midnight_node_ref}}"
    git -C "$dir" checkout --quiet --detach FETCH_HEAD
    echo "midnight-node {{midnight_node_ref}} at $(git -C "$dir" rev-parse --short HEAD)"

# Bring up the fork, then attach the indexer stack (NODE_IMAGE required; pass `--from-snapshot <url>` on first run).
fork-up network="preview" *args="": fork-clone
    #!/usr/bin/env bash
    set -euo pipefail
    local_env="{{midnight_node_dir}}/local-environment"
    # Reinstall whenever the lockfile changes, not only on the first run.
    lock_sum=$(shasum "$local_env/package-lock.json" | cut -d' ' -f1)
    if [ "$(cat "$local_env/node_modules/.fork-lock-sum" 2>/dev/null)" != "$lock_sum" ]; then
        (cd "$local_env" && npm ci)
        echo "$lock_sum" > "$local_env/node_modules/.fork-lock-sum"
    fi
    (cd "$local_env" && MOCK_AUTHORITIES_IMAGE="{{mock_authorities_image}}" npm run "run:{{network}}" -- {{args}})
    manifest="$local_env/artifacts/{{network}}.manifest.env"
    [ -f "$manifest" ] || { echo "fork manifest not found: $manifest" >&2; exit 1; }
    # chain-indexer only follows node versions it was built with metadata for
    # (see docs/updating-node-version.md), so check against the image being run.
    indexer_tag="{{fork_indexer_tag}}"
    if supported=$(git show "v$indexer_tag:NODE_VERSIONS" 2>/dev/null) ||
        supported=$(curl -fsSL "https://raw.githubusercontent.com/midnightntwrk/midnight-indexer/v$indexer_tag/NODE_VERSIONS" 2>/dev/null); then
        source="indexer $indexer_tag"
    else
        supported=$(cat NODE_VERSIONS)
        source="this checkout's NODE_VERSIONS (INDEXER_TAG=$indexer_tag is not a release)"
    fi
    tag=$(sed -n 's/^MIDNIGHT_FORK_NODE_TAG=//p' "$manifest")
    problem=""
    if [ -z "$tag" ]; then
        problem="the fork's node version is unknown: NODE_IMAGE has no tag"
    elif [[ "$tag" =~ ^(sha256:)?[0-9a-f]{64}$ ]]; then
        problem="the fork's node version is unknown: NODE_IMAGE is pinned by digest"
    elif ! grep -qxF "$tag" <<<"$supported"; then
        problem="node $tag is not supported by $source"
    fi
    if [ -n "$problem" ]; then
        echo "$problem; chain-indexer may refuse the chain." >&2
        echo "Set FORK_SKIP_VERSION_CHECK=1 to proceed anyway." >&2
        [ "${FORK_SKIP_VERSION_CHECK:-0}" = "1" ] || exit 1
    fi
    INDEXER_TAG="$indexer_tag" docker compose --env-file "$manifest" -f docker-compose.midnight-fork.yaml up -d
    echo "indexer-api: http://localhost:${INDEXER_API_PORT:-8088} (GraphQL at /api/v4/graphql)"

# Tear down the indexer overlay, then the fork (overlay first so the fork can remove its network).
fork-down network="preview":
    #!/usr/bin/env bash
    set -euo pipefail
    local_env="{{midnight_node_dir}}/local-environment"
    manifest="$local_env/artifacts/{{network}}.manifest.env"
    if [ -f "$manifest" ]; then
        docker compose --env-file "$manifest" -f docker-compose.midnight-fork.yaml down --volumes
    else
        # Without a manifest the required variables only need to be set, not correct.
        MIDNIGHT_FORK_NETWORK_ID=unknown MIDNIGHT_FORK_NODE_WS=unknown MIDNIGHT_FORK_NETWORK=unknown \
            docker compose -f docker-compose.midnight-fork.yaml down --volumes
    fi
    if [ -d "$local_env" ]; then
        (cd "$local_env" && npm run "stop:{{network}}")
    else
        echo "no midnight-node checkout at {{midnight_node_dir}}; stop the fork from your own checkout" >&2
    fi
