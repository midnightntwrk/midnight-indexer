#!/usr/bin/env bash

set -eo pipefail

if [ -z "$1" ]; then
    echo "Error: node version parameter is required" >&2
    echo "Usage: $0 <node_version>" >&2
    exit 1
fi
node_version="$1"

if [ -d ./.node/$node_version ]; then
    rm -r ./.node/$node_version;
fi

docker run \
    -d \
    --name node \
    -p 9944:9944 \
    -e SHOW_CONFIG=false \
    -e CFG_PRESET=dev \
    -v ./.node/$node_version:/node \
    ghcr.io/midnight-ntwrk/midnight-node:$node_version

sleep 3

docker run \
    --rm \
    --network host \
    -v /tmp:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    generate-txs batches -n 3 -b 2

docker run \
    --rm \
    --network host \
    -v /tmp:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    generate-txs --dest-file /out/contract_tx_1_deploy.mn --to-bytes \
    contract-calls deploy \
    --rng-seed '0000000000000000000000000000000000000000000000000000000000000037'

docker run \
    --rm \
    --network host \
    -v /tmp:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    contract-address --network undeployed \
    --src-file /out/contract_tx_1_deploy.mn --dest-file /out/contract_address.mn

docker run \
    --rm \
    --network host \
    -v /tmp:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    generate-txs --src-files /out/contract_tx_1_deploy.mn --dest-url ws://127.0.0.1:9944 \
    send

docker run \
    --rm \
    --network host \
    -v /tmp:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    generate-txs contract-calls call \
    --rng-seed '0000000000000000000000000000000000000000000000000000000000000037' \
    --contract-address /out/contract_address.mn

docker run \
    --rm \
    --network host \
    -v /tmp:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    generate-txs contract-calls maintenance \
    --rng-seed '0000000000000000000000000000000000000000000000000000000000000037' \
    --contract-address /out/contract_address.mn

docker rm -f node
