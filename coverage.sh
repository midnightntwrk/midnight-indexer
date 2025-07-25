#!/usr/bin/env bash

set -euxo pipefail

# Install tooling and clean workspace.
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
