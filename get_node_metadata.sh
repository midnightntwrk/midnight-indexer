#!/usr/bin/env bash

set -euxo pipefail

if [ -z "$1" ]; then
    echo "Error: node version parameter is required" >&2
    echo "Usage: $0 <node_version>" >&2
    exit 1
fi
node_version="$1"

mkdir -p ./.node/$node_version

# SIDECHAIN_BLOCK_BENEFICIARY specifies the wallet that receives block rewards and transaction fees (DUST).
# Required after fees were enabled in 0.16.0-da0b6c69.
# This hex value is a public key that matches the one used in toolkit-e2e.sh.
docker run \
    -d \
    --name node \
    -p 9944:9944 \
    -e SHOW_CONFIG=false \
    -e CFG_PRESET=dev \
    -e SIDECHAIN_BLOCK_BENEFICIARY="04bcf7ad3be7a5c790460be82a713af570f22e0f801f6659ab8e84a52be6969e" \
    ghcr.io/midnight-ntwrk/midnight-node:$node_version

sleep 10

subxt metadata \
    -f bytes \
    --url ws://localhost:9944 > \
    ./.node/$node_version/metadata.scale

docker rm -f node
