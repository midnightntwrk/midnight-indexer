#!/bin/bash

# Shared helpers for the undeployed-stack startup scripts.
# Source from `startup-localenv-*.sh`:
#   . "$(dirname "$0")/_lib.sh"

# Derive Docker Compose project name the same way Docker Compose does
# (basename of cwd, lowercased, dots stripped, hyphens kept).
derive_docker_project_name() {
    local project_dir
    project_dir=$(basename "$(pwd)")
    echo "$project_dir" | tr '[:upper:]' '[:lower:]' | sed 's/\.//g'
}

# Tear down any prior compose stack and the associated node_data volume.
# Args: $1 = docker-compose project name (used to scope volume cleanup).
teardown_prior_stack() {
    local project_name="$1"

    echo "Tearing down any prior compose stack..."
    docker compose --profile cloud down --remove-orphans 2>&1 \
        || echo "[startup] No prior stack to remove (normal on first run; if docker daemon is down, subsequent steps will fail)."

    # Belt-and-suspenders: stop any container still holding the node_data volume.
    # `docker volume rm` has no force flag — the volume can only be removed once no
    # container references it.
    local volume_users
    volume_users=$(docker ps -a -q --filter volume="${project_name}_node_data" 2>/dev/null)
    if [ -n "$volume_users" ]; then
        echo "Removing containers still holding node_data volume..."
        docker rm -f $volume_users
    fi

    if docker volume ls | grep -q "${project_name}_node_data"; then
        local volumes
        volumes=$(docker volume ls | grep "${project_name}_node_data" | awk -F " " '{print $2}')
        for volume in $volumes; do
            docker volume rm $volume
        done
        echo "Named volumes removed."
    else
        echo "No named volumes to remove."
    fi
}

# Poll the indexer /ready endpoint until it responds or the 20s budget is spent.
# Exits non-zero on timeout after dumping container state and indexer-api logs.
wait_for_indexer_ready() {
    echo "Waiting for indexer API to become ready (20s budget)..."
    local ready=0 i
    for i in {1..10}; do
        if curl -sf http://localhost:8088/ready >/dev/null; then
            echo "Indexer API is ready"
            ready=1
            break
        fi
        echo "Not ready yet... ($i/10)"
        sleep 2
    done
    if [ "$ready" -ne 1 ]; then
        echo "ERROR: Indexer API did not become ready within 20s. Dumping container state:"
        docker compose --profile cloud ps
        echo "Last 50 lines of indexer-api logs:"
        docker compose --profile cloud logs --tail=50 indexer-api 2>&1 || true
        exit 1
    fi
}

# Clear the toolkit fetch cache and the block-scanner's per-env scan cursor + block cache.
# Stale entries would otherwise cause generate:data to skip the current chain's blocks
# and write outdated hashes into the test data files.
clear_block_scanner_cache() {
    echo "Deleting toolkit cache..."
    rm -rf qa/tests/.tmp/toolkit/.sync_cache-undeployed/

    echo "Clearing block-scanner cache for undeployed..."
    rm -f qa/tools/block-scanner/tmp_scan/undeployed_*.jsonl
    rm -f qa/tools/block-scanner/stats/undeployed_*.json
}
