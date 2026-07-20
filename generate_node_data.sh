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
readonly node_version="$1"

# Release images live on Docker Hub (midnightntwrk), pre-release builds on GHCR
# (ghcr.io/midnight-ntwrk). Resolve whichever registry has the tag, preferring a
# locally present image, then Docker Hub.
resolve_image() {
    local image="$1"
    local candidate
    for candidate in "midnightntwrk/$image" "ghcr.io/midnight-ntwrk/$image"; do
        if docker image inspect "$candidate" >/dev/null 2>&1 \
            || docker manifest inspect "$candidate" >/dev/null 2>&1; then
            echo "$candidate"
            return
        fi
    done
    echo "Error: $image not found on Docker Hub (midnightntwrk) or GHCR (midnight-ntwrk)" >&2
    return 1
}

toolkit_image=$(resolve_image "midnight-node-toolkit:$node_version")
node_image=$(resolve_image "midnight-node:$node_version")
readonly toolkit_image node_image
readonly rng_seed="0000000000000000000000000000000000000000000000000000000000000037"
readonly node_dir="$(pwd)/.node/$node_version"

# Set up fresh node data directory.
if [ -d $node_dir ]; then
    rm -r $node_dir;
fi
mkdir -p $node_dir

# Start the node container.
docker run \
    -d \
    --name node \
    -p 9944:9944 \
    -e SHOW_CONFIG=false \
    -e CFG_PRESET=dev \
    -e SIDECHAIN_BLOCK_BENEFICIARY="04bcf7ad3be7a5c790460be82a713af570f22e0f801f6659ab8e84a52be6969e" \
    -e THRESHOLD=0 \
    -v $node_dir:/node \
    $node_image

# Wait for node to be ready.
echo "Waiting for node to be ready..."
timeout=60
start_time=$(date +%s)
while true; do
    sleep 3

    if (( $(date +%s) - start_time > timeout )); then
        echo "Timeout after ${timeout}s waiting for node to be ready"
        exit 1
    fi

    finalized_hash=$(curl -s -X POST http://localhost:9944 \
        -H "Content-Type: application/json" \
        -d '{
            "jsonrpc":"2.0",
            "id":1,
            "method":"chain_getFinalizedHead",
            "params":[]
        }' | jq -r .result || true)
    if [[ -z "$finalized_hash" || "$finalized_hash" == "null" ]]; then
        echo "No finalized hash"
        continue
    fi

    finalized_number=$(curl -s -X POST http://localhost:9944 \
        -H "Content-Type: application/json" \
        -d "{
            \"jsonrpc\":\"2.0\",
            \"id\":2,
            \"method\":\"chain_getHeader\",
            \"params\":[\"$finalized_hash\"]
        }" | jq -r '.result.number' || true)
    if [[ -z "$finalized_number" || "$finalized_number" == "null" ]]; then
        echo "No finalized number"
        continue
    fi

    height=$((finalized_number))
    echo "finalized height: $height"
    if [[ $height -ge 1 ]]; then
        echo "Node ready - finalized height: $height"
        break
    fi
done

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs \
    batches -n 1 -b 1

# Send shielded and unshielded tokens from wallet 01 to ff.
docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs \
    single-tx \
    --shielded-amount 42 \
    --unshielded-amount 42 \
    --source-seed "0000000000000000000000000000000000000000000000000000000000000001" \
    --destination-address mn_shield-addr_undeployed157w7tlh2tjdcgnpm96ljf0n6srngrtdutw4zttpvpl78lskz3gnue9yumatpl54u4j9n3gknewvpw22qfexvww2gdrncgth4v58a2qcevfags \
    --destination-address mn_addr_undeployed1792ny9snf3hkzglcfs07agsela6v9dkkqs2m9xyvk4ryl3k99d2s8ea4ga

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs --dest-file /out/contract_tx_1_deploy.mn \
    contract-simple \
    deploy \
    --rng-seed $rng_seed

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    contract-address --src-file /out/contract_tx_1_deploy.mn > /tmp/contract_address.mn

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs --src-file /out/contract_tx_1_deploy.mn --dest-url ws://127.0.0.1:9944 \
    send

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs \
    contract-simple call \
    --call-key store \
    --rng-seed $rng_seed \
    --contract-address $(cat /tmp/contract_address.mn)

# Wait for the contract call to be finalized before running maintenance.
sleep 15
docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs \
    contract-simple maintenance \
    --rng-seed $rng_seed \
    --contract-address $(cat /tmp/contract_address.mn) \
    --new-authority-seed 1000000000000000000000000000000000000000000000000000000000000001

# Emit a standard contract event so the e2e chain exercises the `contractEvents` query and
# subscription (#1163; test_contract_event_query / test_contract_events_subscription in
# indexer-tests/src/e2e.rs). The emit contract source lives in indexer-tests/emit-contract; it is
# compiled with the compactc bundled in the toolkit image and driven through the toolkit's
# generate-intent / send-intent custom-contract pipeline (see util/toolkit README in midnight-node).
readonly emit_work=$(mktemp -d)
cp indexer-tests/emit-contract/emitcounter.compact indexer-tests/emit-contract/contract.config.ts "$emit_work"
chmod -R a+rX "$emit_work"

# Compile the emit contract with the bundled compactc (runs as root via the shell entrypoint so it
# can write into the bind mount).
docker run \
    --rm \
    -v "$emit_work":/toolkit-js/contract-emit \
    --entrypoint sh \
    $toolkit_image \
    -c 'cd /toolkit-js/contract-emit && /compact-home/compactc emitcounter.compact managed/emitcounter'

readonly emit_coin_public=$(docker run --rm $toolkit_image show-address \
    --network undeployed \
    --seed 0000000000000000000000000000000000000000000000000000000000000001 \
    --coin-public | tail -1)

# Deploy the emit contract: intent -> proven tx file -> send.
docker run \
    --rm \
    --network host \
    -v "$emit_work":/toolkit-js/contract-emit \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-intent deploy \
    -c /toolkit-js/contract-emit/contract.config.ts \
    --toolkit-js-path /toolkit-js \
    --coin-public "$emit_coin_public" \
    --output-intent /out/emit_deploy.bin \
    --output-private-state /out/emit_private_state.json \
    --output-zswap-state /out/emit_zswap.json \
    0
docker run \
    --rm \
    --network host \
    -v "$emit_work":/toolkit-js/contract-emit \
    -v toolkit_out:/out \
    $toolkit_image \
    send-intent \
    --intent-file /out/emit_deploy.bin \
    --compiled-contract-dir /toolkit-js/contract-emit/managed/emitcounter \
    --dest-file /out/emit_deploy_tx.mn
docker run \
    --rm \
    -v toolkit_out:/out \
    $toolkit_image \
    contract-address --src-file /out/emit_deploy_tx.mn > /tmp/emit_contract_address.mn
docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs --src-file /out/emit_deploy_tx.mn --dest-url ws://127.0.0.1:9944 send

# Wait for the deploy to be finalized before calling the emit circuit.
sleep 15
docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    contract-state \
    --contract-address $(cat /tmp/emit_contract_address.mn) \
    --dest-file /out/emit_onchain_state.mn

# Call the emit_unpaused circuit: intent -> proven tx file -> send.
docker run \
    --rm \
    --network host \
    -v "$emit_work":/toolkit-js/contract-emit \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-intent circuit \
    -c /toolkit-js/contract-emit/contract.config.ts \
    --toolkit-js-path /toolkit-js \
    --contract-address $(cat /tmp/emit_contract_address.mn) \
    --coin-public "$emit_coin_public" \
    --input-onchain-state /out/emit_onchain_state.mn \
    --input-private-state /out/emit_private_state.json \
    --output-intent /out/emit_call.bin \
    --output-private-state /out/emit_ps2.json \
    --output-zswap-state /out/emit_zswap2.json \
    emit_unpaused
docker run \
    --rm \
    --network host \
    -v "$emit_work":/toolkit-js/contract-emit \
    -v toolkit_out:/out \
    $toolkit_image \
    send-intent \
    --intent-file /out/emit_call.bin \
    --compiled-contract-dir /toolkit-js/contract-emit/managed/emitcounter \
    --dest-file /out/emit_call_tx.mn
docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs --src-file /out/emit_call_tx.mn --dest-url ws://127.0.0.1:9944 send

rm -rf "$emit_work"

# Wait for enough blocks to be finalized so that the pre-populated chain data
# contains sufficient blocks for e2e tests (MAX_HEIGHT = 32 in e2e.rs).
readonly min_finalized_height=40
echo "Waiting for finalized height >= $min_finalized_height..."
timeout=360
start_time=$(date +%s)
while true; do
    sleep 6

    if (( $(date +%s) - start_time > timeout )); then
        echo "Timeout after ${timeout}s waiting for finalized height >= $min_finalized_height"
        exit 1
    fi

    finalized_hash=$(curl -s -X POST http://localhost:9944 \
        -H "Content-Type: application/json" \
        -d '{
            "jsonrpc":"2.0",
            "id":1,
            "method":"chain_getFinalizedHead",
            "params":[]
        }' | jq -r .result || true)
    if [[ -z "$finalized_hash" || "$finalized_hash" == "null" ]]; then
        continue
    fi

    finalized_number=$(curl -s -X POST http://localhost:9944 \
        -H "Content-Type: application/json" \
        -d "{
            \"jsonrpc\":\"2.0\",
            \"id\":2,
            \"method\":\"chain_getHeader\",
            \"params\":[\"$finalized_hash\"]
        }" | jq -r '.result.number' || true)
    if [[ -z "$finalized_number" || "$finalized_number" == "null" ]]; then
        continue
    fi

    height=$((finalized_number))
    echo "finalized height: $height"
    if [[ $height -ge $min_finalized_height ]]; then
        echo "Reached target finalized height: $height"
        break
    fi
done

echo "Successfully generated node data"
