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
        alpine sh -c "rm -rf /project/target"
    echo "Data directories cleaned"
fi

mkdir -p target/data
mkdir -p target/data/postgres
mkdir -p target/data/nats
mkdir -p target/debug

tree target/data

export NODE_TAG=${NODE_TAG:-`cat NODE_VERSION`}
export NODE_TOOLKIT_TAG=${NODE_TOOLKIT_TAG:-`echo $NODE_TAG`}

# Use the derived Docker Compose project name to create volume name
DOCKER_VOLUME_NAME="${DOCKER_PROJECT_NAME}_node_data"

# Create the named volume (empty) for Docker Compose to use BEFORE starting containers
echo "Creating empty node data volume..."
echo "Using Docker Compose project name: $DOCKER_PROJECT_NAME"
echo "Volume name: $DOCKER_VOLUME_NAME"
docker volume rm $DOCKER_VOLUME_NAME 2>/dev/null || true
docker volume create $DOCKER_VOLUME_NAME

echo "Empty node data volume created successfully"
echo "NOTE: Any docker-compose warning about 'volume already exists' is harmless and expected"
echo "      We explicitly manage the node volume externally to ensure it exists before docker compose"

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

echo "Waiting for services to start..."
sleep 5

docker compose --profile cloud logs |grep "Highest known block"


docker ps --format "table {{.Image}}\t{{.Names}}\t{{.Status}}"


echo "Plase make sure all the services are running and healthy"

echo "Deleting toolkit cache..."
rm -rf qa/tests/.tmp/toolkit/.sync_cache-undeployed/

echo "Regenarating new test data... "
pushd qa/tools/block-scanner
bun run generate:data
popd