#!/bin/bash

set -euo pipefail

getUnshieldedAddress() {
    local env=$1
    local seed=$2
    docker run --rm -e RUST_BACKTRACE=1 "$TOOLKIT_IMAGE" \
        show-address \
        --network "$env" \
        --seed "$seed" | jq .unshielded | tr -d '"'
}

setEnvData() {
    local env=$1
    # Get script directory to locate environments.json
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    ENVIRONMENTS_JSON="${SCRIPT_DIR}/environments.json"

    case "$env" in
        "undeployed")
            TARGET_URL="ws://localhost:9944"
            DESTINATION_SEED_ENV="FUNDING_SEED_UNDEPLOYED"
            ;;
        "devnet")
            TARGET_URL="wss://rpc.devnet.midnight.network"
            DESTINATION_SEED_ENV="FUNDING_SEED_DEVNET"
            ;;
        "preview")
            TARGET_URL="wss://rpc.preview.midnight.network"
            DESTINATION_SEED_ENV="FUNDING_SEED_PREVIEW"
            ;;
        "testnet02")
            TARGET_URL="wss://rpc.testnet02.midnight.network"
            DESTINATION_SEED_ENV="FUNDING_SEED_TESTNET02"
            ;;
        "qanet")
            TARGET_URL="wss://rpc.qanet.dev.midnight.network"
            DESTINATION_SEED_ENV="FUNDING_SEED_QANET"
            ;;
        "nodedev01")
            TARGET_URL="wss://rpc.node-dev-01.dev.midnight.network"
            DESTINATION_SEED_ENV="FUNDING_SEED_NODE_DEV_01"
            ;;
    esac
}

getNodeToolkit() {
    local env=$1
    NODE_TOOLKIT_TAG=${NODE_TOOLKIT_TAG:-latest-main}
    TOOLKIT_IMAGE="ghcr.io/midnight-ntwrk/midnight-node-toolkit:$NODE_TOOLKIT_TAG"
    docker pull $TOOLKIT_IMAGE
    HOST_CACHE_DIR="/tmp/toolkit/.sync_cache-${env}"
    echo "NODE_TOOLKIT_TAG=$NODE_TOOLKIT_TAG"
}

setDestinationSeed() {
    DESTINATION_SEED="${FUNDING_SEED:-0000000000000000000000000000000000000000000000000000000000000001}"
    # Get destination seed - either directly from JSON or from environment variable
    #DESTINATION_SEED_ENV=$(jq -r ".environments.\"${ENVIRONMENT}\".source_seed_env // empty" "$ENVIRONMENTS_JSON")
    if [ -n "$DESTINATION_SEED_ENV" ]; then
        # Use environment variable
        DESTINATION_SEED_VAR_NAME="$DESTINATION_SEED_ENV"
        DESTINATION_SEED="${!DESTINATION_SEED_VAR_NAME}"
    fi
}

ENVIRONMENT=$1
if [ -z "$ENVIRONMENT" ]; then
    echo "Please provide an environment"
    echo "Usage: $0 <environment>"
    exit 1
fi


getNodeToolkit "$ENVIRONMENT"
setEnvData "$ENVIRONMENT"

SOURCE_SEED="0000000000000000000000000000000000000000000000000000000987654321"

setDestinationSeed

DESTINATION_ADDRESS=$(getUnshieldedAddress "$ENVIRONMENT" "$DESTINATION_SEED")

SOURCE_ADDRESS=$(getUnshieldedAddress "$ENVIRONMENT" "$SOURCE_SEED")

mkdir -p $HOST_CACHE_DIR
ls -ls $HOST_CACHE_DIR

echo "SOURCE_SEED: $SOURCE_SEED"
echo "DESTINATION_ADDRESS: $DESTINATION_ADDRESS"

docker run --rm -e RUST_BACKTRACE=1 -v $HOST_CACHE_DIR:/.cache/sync "$TOOLKIT_IMAGE" \
    generate-txs \
    --src-url $TARGET_URL \
    --dest-url $TARGET_URL \
    single-tx \
    --source-seed "$SOURCE_SEED" \
    --unshielded-amount 1000000 \
    --destination-address "$DESTINATION_ADDRESS"

ls -ls $HOST_CACHE_DIR

echo "SOURCE_SEED   : $SOURCE_SEED"
echo "SOURCE_ADDRESS: $SOURCE_ADDRESS"
echo "DESTINATION_SEED   : $DESTINATION_SEED"
echo "DESTINATION_ADDRESS: $DESTINATION_ADDRESS"
