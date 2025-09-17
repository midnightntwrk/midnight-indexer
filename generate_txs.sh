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

# Start the node container.
docker run \
    -d \
    --name node \
    -p 9944:9944 \
    -e SHOW_CONFIG=false \
    -e CFG_PRESET=dev \
    -e SIDECHAIN_BLOCK_BENEFICIARY="04bcf7ad3be7a5c790460be82a713af570f22e0f801f6659ab8e84a52be6969e" \
    ghcr.io/midnight-ntwrk/midnight-node:$node_version

# Wait for port to be available (max 30 seconds)
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

# 1 to 2/2.
docker run \
    --rm \
    --network host \
    -v ./target:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    generate-txs \
    --dest-file /out/tx_1_2_2.mn \
    --to-bytes \
    single-tx \
    --shielded-amount 10 \
    --unshielded-amount 10 \
    --source-seed "0000000000000000000000000000000000000000000000000000000000000001" \
    --destination-address mn_shield-addr_undeployed1tffkxdesnqz86wvds2aprwuprpvzvag5t3mkveddr33hr7xyhlhqxqzfqqxy54an7cyznaxnzs7p8tduku7fuje5mwqx9auvdn9e8x03kvvy5r6z \
    --destination-address mn_addr_undeployed1gkasr3z3vwyscy2jpp53nzr37v7n4r3lsfgj6v5g584dakjzt0xqun4d4r
docker run \
    --rm \
    --network host \
    -v ./target:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    get-tx-from-context \
    --src-file /out/tx_1_2_2.mn \
    --network undeployed \
    --dest-file /out/tx_1_2_2.raw \
    --from-bytes

# 1 to 2/3.
docker run \
    --rm \
    --network host \
    -v ./target:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    generate-txs \
    --dest-file /out/tx_1_2_3.mn \
    --to-bytes \
    single-tx \
    --shielded-amount 10 \
    --unshielded-amount 10 \
    --source-seed "0000000000000000000000000000000000000000000000000000000000000001" \
    --destination-address mn_shield-addr_undeployed1tffkxdesnqz86wvds2aprwuprpvzvag5t3mkveddr33hr7xyhlhqxqzfqqxy54an7cyznaxnzs7p8tduku7fuje5mwqx9auvdn9e8x03kvvy5r6z \
    --destination-address mn_addr_undeployed1g9nr3mvjcey7ca8shcs5d4yjndcnmczf90rhv4nju7qqqlfg4ygs0t4ngm
docker run \
    --rm \
    --network host \
    -v ./target:/out \
    ghcr.io/midnight-ntwrk/midnight-node-toolkit:$node_version \
    get-tx-from-context \
    --src-file /out/tx_1_2_3.mn \
    --network undeployed \
    --dest-file /out/tx_1_2_3.raw \
    --from-bytes

mv target/*.raw indexer-common/tests
