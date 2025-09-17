#!/bin/bash

if [ -n "$(docker ps -a|grep -e ghcr.io -e nats -e postgres|awk -F " " '{print $1}'
)" ]; then
    docker rm -f $(docker ps -a|grep -e ghcr.io -e nats -e postgres|awk -F " " '{print $1}')
    echo "All Midnight containers removed."
else
    echo "No Midnight containers to remove."
    echo "Everything is clear!"
fi

sudo rm -rf target/data/postgres
sudo rm -rf target/data/nats
sudo rm -rf target/data/node
#sudo rm -rf target/debug


mkdir -p target/data
mkdir -p target/data/postgres
mkdir -p target/data/nats
mkdir -p target/data/node
mkdir -p target/debug

tree target/data

copy-node-data-to-target() {
    local SRC_DIR=$1
    local DEST_DIR="target/data/node"

    sudo cp -r $SRC_DIR $DEST_DIR

    tree $SRC_DIR

    tree $DEST_DIR
}

export NODE_TAG=${NODE_TAG:-`cat NODE_VERSION`} 

copy-node-data-to-target ".node/$NODE_TAG"


# I need a git command to get the latest sha1 on the current branch with 8 characters
TMP_INDEXER_TAG="3.0.0-$(git rev-parse --short=8 HEAD)"

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

export INDEXER_TAG=${INDEXER_TAG:-$TMP_INDEXER_TAG} 

echo "Using the following tags:"
echo " NODE_TAG: $NODE_TAG"
echo " INDEXER_TAG: $INDEXER_TAG"



docker compose --profile cloud up -d

echo "Waiting for 5 seconds..."
sleep 5

docker compose --profile cloud logs |grep "Highest known block"

# docker stop midnight-indexer-node-1
# docker rm midnight-indexer-node-1

# docker stop midnight-indexer-chain-indexer-1
# docker rm midnight-indexer-chain-indexer-1


docker ps --format "table {{.Image}}\t{{.Names}}\t{{.Status}}"


