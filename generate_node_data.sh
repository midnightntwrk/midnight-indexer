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
    # Specifies the wallet that receives block rewards and transaction fees (DUST)
    # Required after fees were enabled in 0.16.0-da0b6c69
    # This hex value is a public key that matches the one used in toolkit-e2e.sh
    -e SIDECHAIN_BLOCK_BENEFICIARY="04bcf7ad3be7a5c790460be82a713af570f22e0f801f6659ab8e84a52be6969e" \
    -v ./.node/$node_version:/node \
    ghcr.io/midnight-ntwrk/midnight-node:$node_version

sleep 10

# Generate batches
# Note: Reduced from -n 3 -b 2 to -n 1 -b 1 to minimize DUST requirements
# after fees were enabled in node 0.16.0-da0b6c69. Larger batch sizes fail with:
# "Balancing TX failed: Insufficient DUST (trying to spend X, need Y more)"
# This matches the approach used in midnight-node's toolkit-e2e.sh CI tests.
docker run \
    --rm \
    --network host \
    -v /tmp:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    generate-txs batches -n 1 -b 1

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

# Add delay to work around PM-19168 (ctime > tblock validation issue)
sleep 2

docker run \
    --rm \
    --network host \
    -v /tmp:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    generate-txs --src-files /out/contract_tx_1_deploy.mn --dest-url ws://127.0.0.1:9944 \
    send

# Add delay to work around PM-19168
sleep 2

docker run \
    --rm \
    --network host \
    -v /tmp:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    generate-txs contract-calls call \
    # The 'store' function inserts data into a Merkle tree in the test contract
    # (see midnight-node MerkleTreeContract). We need this to generate contract
    # action events in the test data so the indexer can verify it properly tracks
    # and indexes contract state changes.
    --call-key store \
    --rng-seed '0000000000000000000000000000000000000000000000000000000000000037' \
    --contract-address /out/contract_address.mn

# Wait for the contract call to be finalized before running maintenance
sleep 15

# Add longer delay for maintenance to work around PM-19168
sleep 5

docker run \
    --rm \
    --network host \
    -v /tmp:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    generate-txs contract-calls maintenance \
    --rng-seed '0000000000000000000000000000000000000000000000000000000000000037' \
    --contract-address /out/contract_address.mn

docker rm -f node
