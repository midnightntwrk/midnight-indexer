set shell := ["bash", "-uc"]

rust_version := `grep channel rust-toolchain.toml | sed -r 's/channel = "(.*)"/\1/'`

# Can be overridden on the command line: `just feature=standalone`
feature := "cloud"

check:
    cargo check -p indexer-common       --tests
    cargo check -p indexer-common       --tests --features {{feature}}
    cargo check -p chain-indexer        --tests
    cargo check -p chain-indexer        --tests --features {{feature}}
    cargo check -p wallet-indexer       --tests
    cargo check -p wallet-indexer       --tests --features {{feature}}
    cargo check -p indexer-api          --tests
    cargo check -p indexer-api          --tests --features {{feature}}
    cargo check -p indexer-tests        --tests --features {{feature}}
    if [ "{{feature}}" = "standalone" ]; then cargo check -p indexer-standalone --tests --features standalone; fi

fmt:
    cargo +nightly-2025-07-01 fmt

fmt-check:
    cargo +nightly-2025-07-01 fmt --check

lint:
    cargo clippy -p indexer-common       --no-deps --tests                        -- -D warnings
    cargo clippy -p indexer-common       --no-deps --tests --features {{feature}} -- -D warnings
    cargo clippy -p chain-indexer        --no-deps --tests                        -- -D warnings
    cargo clippy -p chain-indexer        --no-deps --tests --features {{feature}} -- -D warnings
    cargo clippy -p wallet-indexer       --no-deps --tests                        -- -D warnings
    cargo clippy -p wallet-indexer       --no-deps --tests --features {{feature}} -- -D warnings
    cargo clippy -p indexer-api          --no-deps --tests                        -- -D warnings
    cargo clippy -p indexer-api          --no-deps --tests --features {{feature}} -- -D warnings
    cargo clippy -p indexer-tests        --no-deps --tests --features {{feature}} -- -D warnings
    if [ "{{feature}}" = "standalone" ]; then cargo clippy -p indexer-standalone --no-deps --tests --features standalone -- -D warnings; fi

test:
    # We must build the executables needed by the e2e tests!
    if [ "{{feature}}" = "cloud" ];      then cargo build -p chain-indexer      --features cloud;      fi
    if [ "{{feature}}" = "cloud" ];      then cargo build -p wallet-indexer     --features cloud;      fi
    if [ "{{feature}}" = "cloud" ];      then cargo build -p indexer-api        --features cloud;      fi
    if [ "{{feature}}" = "standalone" ]; then cargo build -p indexer-standalone --features standalone; fi
    cargo nextest run --workspace --exclude indexer-standalone --features {{feature}}
    # Check indexer-api schema:
    cargo run -p indexer-api --bin indexer-api-cli print-api-schema-v1 > indexer-api/graphql/schema-v1.graphql.check
    @if ! cmp -s indexer-api/graphql/schema-v1.graphql indexer-api/graphql/schema-v1.graphql.check; then echo "schema-v1.graphql has changes!"; exit 1; fi

generate-indexer-api-schema:
    cargo run -p indexer-api --bin indexer-api-cli print-api-schema-v1 > indexer-api/graphql/schema-v1.graphql

fix:
    cargo fix --allow-dirty --allow-staged --features {{feature}}

lint-fix:
    cargo clippy -p indexer-common       --no-deps --tests                        --fix --allow-dirty --allow-staged
    cargo clippy -p indexer-common       --no-deps --tests --features {{feature}} --fix --allow-dirty --allow-staged
    cargo clippy -p chain-indexer        --no-deps --tests                        --fix --allow-dirty --allow-staged
    cargo clippy -p chain-indexer        --no-deps --tests --features {{feature}} --fix --allow-dirty --allow-staged
    cargo clippy -p wallet-indexer       --no-deps --tests                        --fix --allow-dirty --allow-staged
    cargo clippy -p wallet-indexer       --no-deps --tests --features {{feature}} --fix --allow-dirty --allow-staged
    cargo clippy -p indexer-api          --no-deps --tests                        --fix --allow-dirty --allow-staged
    cargo clippy -p indexer-api          --no-deps --tests --features {{feature}} --fix --allow-dirty --allow-staged
    cargo clippy -p indexer-tests        --no-deps --tests --features {{feature}} --fix --allow-dirty --allow-staged
    if [ "{{feature}}" = "standalone" ]; then cargo clippy -p indexer-standalone --no-deps --tests --features standalone --fix --allow-dirty --allow-staged; fi

doc:
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +nightly-2025-07-01 doc -p indexer-common       --no-deps --all-features
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +nightly-2025-07-01 doc -p chain-indexer        --no-deps --features {{feature}}
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +nightly-2025-07-01 doc -p wallet-indexer       --no-deps --features {{feature}}
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +nightly-2025-07-01 doc -p indexer-api          --no-deps --features {{feature}}
    if [ "{{feature}}" = "standalone" ]; then RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +nightly-2025-07-01 doc -p indexer-standalone --no-deps --features standalone; fi

all: check fmt lint test doc

all-features:
    just all
    just feature=standalone all

coverage:
    #!/usr/bin/env bash
    set -euxo pipefail
    rustup component add llvm-tools-preview --toolchain nightly-2025-07-01
    cargo llvm-cov clean --workspace
    # First build tests without instrumentation.
    cloud_tests=$(cargo test -p indexer-tests --test native_e2e --features cloud --no-run --message-format=json | jq -r 'select(.profile.test == true and .target.name == "native_e2e") | .executable')
    standalone_tests=$(cargo test -p indexer-tests --test native_e2e --features standalone --no-run --message-format=json | jq -r 'select(.profile.test == true and .target.name == "native_e2e") | .executable')
    # Then setup for coverage instrumentation and build the executables which are spawned in the tests.
    source <(cargo +nightly-2025-07-01 llvm-cov show-env --export-prefix)
    cargo +nightly-2025-07-01 build -p chain-indexer      --features cloud
    cargo +nightly-2025-07-01 build -p wallet-indexer     --features cloud
    cargo +nightly-2025-07-01 build -p indexer-api        --features cloud
    cargo +nightly-2025-07-01 build -p indexer-standalone --features standalone
    # Finally execute tests and create coverage report.
    "$cloud_tests"
    "$standalone_tests"
    cargo llvm-cov report --html

docker-chain-indexer profile="dev":
    tag=$(git rev-parse --short=8 HEAD) && \
    docker build \
        --build-arg "RUST_VERSION={{rust_version}}" \
        --build-arg "PROFILE={{profile}}" \
        --secret id=netrc,src=$NETRC \
        -t ghcr.io/midnight-ntwrk/chain-indexer:${tag} \
        -t ghcr.io/midnight-ntwrk/chain-indexer:latest \
        -f chain-indexer/Dockerfile \
        .

docker-wallet-indexer profile="dev":
    tag=$(git rev-parse --short=8 HEAD) && \
    docker build \
        --build-arg "RUST_VERSION={{rust_version}}" \
        --build-arg "PROFILE={{profile}}" \
        --secret id=netrc,src=$NETRC \
        -t ghcr.io/midnight-ntwrk/wallet-indexer:${tag} \
        -t ghcr.io/midnight-ntwrk/wallet-indexer:latest \
        -f wallet-indexer/Dockerfile \
        .

docker-indexer-api profile="dev":
    tag=$(git rev-parse --short=8 HEAD) && \
    docker build \
        --build-arg "RUST_VERSION={{rust_version}}" \
        --build-arg "PROFILE={{profile}}" \
        --secret id=netrc,src=$NETRC \
        -t ghcr.io/midnight-ntwrk/indexer-api:${tag} \
        -t ghcr.io/midnight-ntwrk/indexer-api:latest \
        -f indexer-api/Dockerfile \
        .

docker-indexer-standalone profile="dev":
    tag=$(git rev-parse --short=8 HEAD) && \
    docker build \
        --build-arg "RUST_VERSION={{rust_version}}" \
        --build-arg "PROFILE={{profile}}" \
        --secret id=netrc,src=$NETRC \
        -t ghcr.io/midnight-ntwrk/indexer-standalone:${tag} \
        -t ghcr.io/midnight-ntwrk/indexer-standalone:latest \
        -f indexer-standalone/Dockerfile \
        .

docker-indexer-tests profile="dev":
    tag=$(git rev-parse --short=8 HEAD) && \
    docker build \
        --build-arg "RUST_VERSION={{rust_version}}" \
        --build-arg "PROFILE={{profile}}" \
        --secret id=netrc,src=$NETRC \
        -t ghcr.io/midnight-ntwrk/indexer-tests:${tag} \
        -t ghcr.io/midnight-ntwrk/indexer-tests:latest \
        -f indexer-tests/Dockerfile \
        .

run-chain-indexer node="ws://localhost:9944" network_id="Undeployed":
    docker compose up -d postgres nats
    RUST_LOG=chain_indexer=debug,indexer_common=debug,fastrace_opentelemetry=off,info \
        CONFIG_FILE=chain-indexer/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        APP__INFRA__NODE__URL={{node}} \
        cargo run -p chain-indexer --features {{feature}}

run-wallet-indexer network_id="Undeployed":
    docker compose up -d postgres nats
    RUST_LOG=wallet_indexer=debug,indexer_common=debug,fastrace_opentelemetry=off,info \
        CONFIG_FILE=wallet-indexer/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        cargo run -p wallet-indexer --features {{feature}}

run-indexer-api network_id="Undeployed":
    docker compose up -d postgres nats
    RUST_LOG=indexer_api=debug,indexer_common=debug,info \
        CONFIG_FILE=indexer-api/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        cargo run -p indexer-api --bin indexer-api --features {{feature}}

run-indexer-standalone node="ws://localhost:9944" network_id="Undeployed":
    mkdir -p target/data
    RUST_LOG=indexer=debug,chain_indexer=debug,wallet_indexer=debug,indexer_api=debug,indexer_common=debug,fastrace_opentelemetry=off,info \
        CONFIG_FILE=indexer-standalone/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        APP__INFRA__NODE__URL={{node}} \
        APP__INFRA__STORAGE__CNN_URL=target/data/indexer.sqlite \
        cargo run -p indexer-standalone --features standalone

node_version := "0.13.2-rc.2"
generator_version := "0.13.2-rc.2"

generate-node-data:
    if [ -d ./.node/{{node_version}} ]; then rm -r ./.node/{{node_version}}; fi
    docker run \
        -d \
        --name node \
        -p 9944:9944 \
        -e SHOW_CONFIG=false \
        -e CFG_PRESET=dev \
        -v ./.node/{{node_version}}:/node \
        ghcr.io/midnight-ntwrk/midnight-node:{{node_version}}
    sleep 3
    docker run \
        --rm \
        --name generator-generate-txs \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:{{generator_version}} \
        generate-txs batches -n 3 -b 2
    docker run \
        --rm \
        --name generator-generate-contract-deploy \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:{{generator_version}} \
        generate-txs --dest-file /out/contract_tx_1_deploy.mn --to-bytes \
        contract-calls deploy \
        --rng-seed '0000000000000000000000000000000000000000000000000000000000000037'
    docker run \
        --rm \
        --name generator-generate-contract-address \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:{{generator_version}} \
        contract-address --network undeployed \
        --src-file /out/contract_tx_1_deploy.mn --dest-file /out/contract_address.mn
    docker run \
        --rm \
        --name generator-send-contract-deploy \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:{{generator_version}} \
        generate-txs --src-files /out/contract_tx_1_deploy.mn --dest-url ws://127.0.0.1:9944 \
        send
    docker run \
        --rm \
        --name generator-generate-contract-call \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:{{generator_version}} \
        generate-txs contract-calls call \
        --rng-seed '0000000000000000000000000000000000000000000000000000000000000037' \
        --contract-address /out/contract_address.mn
    docker run \
        --rm \
        --name generator-generate-contract-maintenance \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:{{generator_version}} \
        generate-txs contract-calls maintenance \
        --rng-seed '0000000000000000000000000000000000000000000000000000000000000037' \
        --contract-address /out/contract_address.mn
    docker rm -f node

run-node:
    #!/usr/bin/env bash
    node_dir=$(mktemp -d)
    cp -r ./.node/{{node_version}}/ $node_dir
    docker run \
        --name node \
        -p 9944:9944 \
        -e SHOW_CONFIG=false \
        -e CFG_PRESET=dev \
        -v $node_dir:/node \
        ghcr.io/midnight-ntwrk/midnight-node:{{node_version}}

get-node-metadata:
    subxt metadata \
        -f bytes \
        --url ws://localhost:9944 \
        > ./.node/{{node_version}}/metadata.scale
