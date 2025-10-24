#!/usr/bin/env bash

set -euxo pipefail

# Cleanup function to ensure node container is removed.
cleanup() {
    docker rm -f node >/dev/null 2>&1 || true
}

# Set up trap to cleanup on exit.
trap cleanup EXIT

if [ -z "$1" ]; then
    echo "Error: node version parameter is required" >&2
    echo "Usage: $0 <node_version>" >&2
    exit 1
fi
node_version="$1"

# Function to run all toolkit commands.
run_toolkit_commands() {
    docker run \
        --rm \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
        generate-txs \
        batches -n 3 -b 2

    # Send shielded and unshielded tokens from wallet 01 to ff.
    docker run \
        --rm \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
        generate-txs \
        single-tx \
        --shielded-amount 42 \
        --unshielded-amount 42 \
        --source-seed "0000000000000000000000000000000000000000000000000000000000000001" \
        --destination-address mn_shield-addr_undeployed14lthhq9xj62zdyeekyc3r6gfght8q8q6xp0h8npmq045fljhss8qxqxvjjwd74sl6272ezec5tfuhxqh99qyunx889yx3euy9m6k2r74qvd60zx5 \
        --destination-address mn_addr_undeployed1792ny9snf3hkzglcfs07agsela6v9dkkqs2m9xyvk4ryl3k99d2s8ea4ga

    docker run \
        --rm \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
        generate-txs --dest-file /out/contract_tx_1_deploy.mn --to-bytes \
        contract-simple \
        deploy \
        --rng-seed '0000000000000000000000000000000000000000000000000000000000000037'

    docker run \
        --rm \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
        contract-address --src-file /out/contract_tx_1_deploy.mn > /tmp/contract_address.mn

    docker run \
        --rm \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
        generate-txs --src-file /out/contract_tx_1_deploy.mn --dest-url ws://127.0.0.1:9944 \
        send

    # The 'store' function inserts data into a Merkle tree in the test contract
    # (see midnight-node MerkleTreeContract). We need this to generate contract
    # action events in the test data so the indexer can verify it properly tracks
    # and indexes contract state changes.
    docker run \
        --rm \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
        generate-txs \
        contract-simple call \
        --call-key store \
        --rng-seed '0000000000000000000000000000000000000000000000000000000000000037' \
        --contract-address $(cat /tmp/contract_address.mn)

    # Wait for the contract call to be finalized before running maintenance.
    sleep 15

    docker run \
        --rm \
        --network host \
        -v /tmp:/out \
        ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
        generate-txs \
        contract-simple maintenance \
        --rng-seed '0000000000000000000000000000000000000000000000000000000000000037' \
        --contract-address $(cat /tmp/contract_address.mn)
}

# Clean up any existing data.
if [ -d ./.node/$node_version ]; then
    rm -r ./.node/$node_version;
fi

mkdir -p ./.node/$node_version

# Start the node container.
docker run \
    -d \
    --name node \
    -p 9944:9944 \
    -e SHOW_CONFIG=false \
    -e CFG_PRESET=dev \
    -e SIDECHAIN_BLOCK_BENEFICIARY="04bcf7ad3be7a5c790460be82a713af570f22e0f801f6659ab8e84a52be6969e" \
    -v ./.node/$node_version:/node \
    ghcr.io/midnight-ntwrk/midnight-node:$node_version

# Wait for node to be ready (max 30 seconds).
echo "Waiting for node to be ready..."
for i in {1..30}; do
    if curl -f http://localhost:9944/health/readiness 2>/dev/null; then
        echo "Node is ready"
        sleep 2  # Give it a moment to fully initialize
        break
    fi
    if [ $i -eq 30 ]; then
        echo "Error: Node failed to start after 30 seconds" >&2
        docker logs node 2>&1 | tail -20
        exit 1
    fi
    sleep 1
done

# Retry the entire toolkit command sequence up to 3 times.
max_attempts=3
attempt=1

while [ $attempt -le $max_attempts ]; do
    echo "Running toolkit commands (attempt $attempt of $max_attempts)..."

    # Try to run all toolkit commands.
    if run_toolkit_commands; then
        echo "Successfully generated node data"
        exit 0
    fi

    echo "Toolkit commands failed on attempt $attempt" >&2

    # If this wasn't the last attempt, clean up and retry.
    if [ $attempt -lt $max_attempts ]; then
        echo "Cleaning up node data folder for retry..." >&2
        rm -rf ./.node/$node_version/*
        echo "Waiting before retry..." >&2
        sleep $((attempt * 5))
    fi

    attempt=$((attempt + 1))
done

echo "Failed to generate node data after $max_attempts attempts" >&2
# Clean up the folder on final failure.
rm -rf ./.node/$node_version
exit 1
