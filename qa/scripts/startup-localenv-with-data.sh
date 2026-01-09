#!/bin/bash

if [ -n "$(docker ps -a|grep -e ghcr.io -e nats -e postgres|awk -F " " '{print $1}'
)" ]; then
    docker rm -f $(docker ps -a|grep -e ghcr.io -e nats -e postgres|awk -F " " '{print $1}')
    echo "All Midnight containers removed."
else
    echo "No Midnight containers to remove."
    echo "Everything is clear!"
fi

# Derive Docker Compose project name from directory basename (same way Docker Compose does)
# Docker Compose normalizes: lowercase, remove dots, keep hyphens
PROJECT_DIR=$(basename "$(pwd)")
DOCKER_PROJECT_NAME=$(echo "$PROJECT_DIR" | tr '[:upper:]' '[:lower:]' | sed 's/\.//g')

# Remove the named volume to ensure fresh node data
if docker volume ls | grep -q "${DOCKER_PROJECT_NAME}_node_data"; then
    volumes=$(docker volume ls | grep "${DOCKER_PROJECT_NAME}_node_data" | awk -F " " '{print $2}')
    for volume in $volumes; do
        docker volume rm $volume
    done
    echo "Named volumes removed."
else
    echo "No named volumes to remove."
fi

# Use docker to clean postgres and nats data (avoids sudo issues)
if [ -d "target/data/postgres" ] || [ -d "target/data/nats" ]; then
    echo "Cleaning postgres and nats data directories..."
    docker run --rm \
        -v "$(pwd):/project" \
        alpine sh -c "rm -rf /project/target/data /project/target/postgres /project/target/nats"
    echo "Data directories cleaned"
fi

docker run --rm \
    -v "$(pwd):/project" \
    alpine sh -c "mkdir -p /project/target/data /project/target/postgres /project/target/nats"

export NODE_TAG=${NODE_TAG:-`cat NODE_VERSION`}
if [ -n "$NODE_TOOLKIT_TAG" ]; then
  echo "Using explicit NODE_TOOLKIT_TAG: $NODE_TOOLKIT_TAG"
else
  export NODE_TOOLKIT_TAG=latest-main
  echo "NODE_TOOLKIT_TAG not set; defaulting to 'latest-main'"
fi

# Use the derived Docker Compose project name to create volume name
DOCKER_VOLUME_NAME="${DOCKER_PROJECT_NAME}_node_data"

# Create the named volume and populate it with fresh node data BEFORE starting containers
echo "Creating and populating node data volume..."
echo "Using Docker Compose project name: $DOCKER_PROJECT_NAME"
echo "Volume name: $DOCKER_VOLUME_NAME"
docker volume rm $DOCKER_VOLUME_NAME 2>/dev/null || true
docker volume create $DOCKER_VOLUME_NAME

# Use a temporary container to copy data into the volume
echo "Copying fresh node data from .node/$NODE_TAG/ into volume..."
docker run --rm \
  -v "$(pwd)/.node/$NODE_TAG:/source:ro" \
  -v $DOCKER_VOLUME_NAME:/node \
  alpine sh -c "cp -r /source/. /node/ && chmod -R 777 /node/chain"

echo "Node data volume populated successfully"
echo "NOTE: Any docker-compose warning about 'volume already exists' is harmless and expected"
echo "      We explicitly manage the node volume externally to inject fresh test data"

# To workout the default indexer tag, find the latest 8-digit sha1 of the commit where
# NODE_VERSION file was updated with the $NODE_TAG value
if [ -z "${INDEXER_TAG:-}" ]; then
    # Find the commit where NODE_VERSION was set to the current NODE_TAG
    COMMIT_SHA=$(git log --all --format=%H --max-count=1 -S"$NODE_TAG" -- NODE_VERSION)
    
    if [ -n "$COMMIT_SHA" ]; then
        TMP_INDEXER_TAG="3.0.0-$(git rev-parse --short=8 $COMMIT_SHA)"
        echo "Found NODE_VERSION=$NODE_TAG in commit $COMMIT_SHA"
    else
        # Fallback to current HEAD if not found
        TMP_INDEXER_TAG="3.0.0-$(git rev-parse --short=8 HEAD)"
        echo "Could not find commit for NODE_VERSION=$NODE_TAG, using HEAD"
    fi

    docker pull ghcr.io/midnight-ntwrk/wallet-indexer:$TMP_INDEXER_TAG

    if [ $? -ne 0 ]; then
        echo "Failed to pull indexer image $TMP_INDEXER_TAG trying with the latest known one"
        export TMP_INDEXER_TAG="3.0.0-d850c371"
        docker pull ghcr.io/midnight-ntwrk/wallet-indexer:$TMP_INDEXER_TAG
        if [ $? -ne 0 ]; then
            echo "Failed again even with 3.0.0-d850c371"
            exit 1
        fi
    fi
    export INDEXER_TAG=$TMP_INDEXER_TAG
else
    echo "Using externally defined INDEXER_TAG: $INDEXER_TAG"
fi

echo "Using the following tags:"
echo " NODE_TAG: $NODE_TAG"
echo " INDEXER_TAG: $INDEXER_TAG"
echo " NODE_TOOLKIT_TAG: $NODE_TOOLKIT_TAG" 

docker compose --profile cloud up -d

echo "Waiting for indexer API to become ready..."
for i in {1..30}; do
  if curl -sf http://localhost:8088/ready >/dev/null; then
    echo "Indexer API is ready"
    break
  fi
  echo "Not ready yet... ($i)"
  sleep 2
done

echo "Chain startup info:"
docker compose --profile cloud logs | grep "Highest known block"

docker ps --format "table {{.Image}}\t{{.Names}}\t{{.Status}}"


echo "Plase make sure all the services are running and healthy"

echo "Deleting toolkit cache..."
rm -rf qa/tests/.tmp/toolkit/.sync_cache-undeployed/

echo "Regenarating new test data... "
pushd qa/tools/block-scanner
bun run generate:data
popd