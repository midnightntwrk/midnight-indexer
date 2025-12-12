#!/bin/bash

set -euo pipefail

ENVIRONMENT=$1
if [ -z "$ENVIRONMENT" ]; then
    echo "Please provide an environment"
    echo "Usage: $0 <environment>"
    exit 1
fi

#NODE_TOOLKIT_TAG="0.18.0-rc.5"
NODE_TOOLKIT_TAG=latest-main
TOOLKIT_IMAGE="ghcr.io/midnight-ntwrk/midnight-node-toolkit:$NODE_TOOLKIT_TAG"
SOURCE_SEED="${FUNDING_SEED:-0000000000000000000000000000000000000000000000000000000000000001}"
DESTINATION_SEED="0000000000000000000000000000000000000000000000000000000987654321"
HOST_CACHE_DIR="/tmp/toolkit/.sync_cache-${ENVIRONMENT}"

echo "NODE_TOOLKIT_TAG=$NODE_TOOLKIT_TAG"

# Get script directory to locate environments.json
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENVIRONMENTS_JSON="${SCRIPT_DIR}/environments.json"

# Load environment configuration from JSON
TARGET_URL=$(jq -r ".environments.\"${ENVIRONMENT}\".target_url" "$ENVIRONMENTS_JSON")

# Get source seed - either directly from JSON or from environment variable
SOURCE_SEED_ENV=$(jq -r ".environments.\"${ENVIRONMENT}\".source_seed_env // empty" "$ENVIRONMENTS_JSON")
if [ -n "$SOURCE_SEED_ENV" ]; then
    # Use environment variable
    SOURCE_SEED_VAR_NAME="$SOURCE_SEED_ENV"
    SOURCE_SEED="${!SOURCE_SEED_VAR_NAME}"
else
    # Use literal value from JSON
    SOURCE_SEED=$(jq -r ".environments.\"${ENVIRONMENT}\".source_seed" "$ENVIRONMENTS_JSON")
fi

DESTINATION_ADDRESS_SHIELDED=$(docker run --rm -e RUST_BACKTRACE=1 "$TOOLKIT_IMAGE" \
    show-address \
    --network $ENVIRONMENT \
    --seed "$DESTINATION_SEED" | jq .shielded | tr -d '"')

DESTINATION_ADDRESS_UNSHIELDED=$(docker run --rm -e RUST_BACKTRACE=1 "$TOOLKIT_IMAGE" \
    show-address \
    --network $ENVIRONMENT \
    --seed "$DESTINATION_SEED" | jq .unshielded | tr -d '"')

ls -ls $HOST_CACHE_DIR

echo "SOURCE_SEED: $SOURCE_SEED"
echo "DESTINATION_ADDRESS_SHIELDED: $DESTINATION_ADDRESS_SHIELDED"
echo "DESTINATION_ADDRESS_UNSHIELDED: $DESTINATION_ADDRESS_UNSHIELDED"

docker run --rm -e RUST_BACKTRACE=1 -v $HOST_CACHE_DIR:/.cache/sync "$TOOLKIT_IMAGE" \
    generate-txs \
    --src-url $TARGET_URL \
    --dest-url $TARGET_URL \
    single-tx \
    --source-seed "$SOURCE_SEED" \
    --unshielded-amount 10 \
    --shielded-amount 10 \
    --destination-address "$DESTINATION_ADDRESS_SHIELDED" \
    --destination-address "$DESTINATION_ADDRESS_UNSHIELDED"

ls -ls $HOST_CACHE_DIR