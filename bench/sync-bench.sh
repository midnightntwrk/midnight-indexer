#!/usr/bin/env bash
# This file is part of midnight-indexer.
# Copyright (C) Midnight Foundation
# SPDX-License-Identifier: Apache-2.0
# Licensed under the Apache License, Version 2.0 (the "License");
# You may not use this file except in compliance with the License.
# You may obtain a copy of the License at
# http://www.apache.org/licenses/LICENSE-2.0
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Measure chain-indexer (cloud) sync speed from genesis to a target height.
#
# Usage: bench/sync-bench.sh <indexer-worktree> <label> <target-height>
#
# NODE_URL must be an archive node, since the indexer reads historical state. For numbers that
# resemble production, run it next to the indexer (a local node), not over a public RPC.
#
# Env: NODE_URL (default ws://localhost:9944), NETWORK_ID (default undeployed),
#      BENCH_DIR (default <repo>/target/bench), RUST_LOG (default info),
#      WRAP (command prefix for the indexer, e.g. "env APP__APPLICATION__GC_INTERVAL=500" or
#            "samply record --save-only -o profile.json.gz --"),
#      PG_ARGS (extra Postgres server flags, e.g. "-c synchronous_commit=off"),
#      STALL_S (seconds without progress before giving up, default 300).
#
# Starts its own Postgres and NATS on non-default ports, so it never touches the dev compose stack.
# Output in $BENCH_DIR/runs/<label>/: heights.csv (sampled every 5 s), summary.txt, digest.txt and
# indexer.log. Runs of behavior-preserving changes must produce identical digests.
set -euo pipefail

worktree=$(realpath "$1")
label=$2
target=$3
node_url=${NODE_URL:-ws://localhost:9944}
network_id=${NETWORK_ID:-undeployed}
bench_dir=${BENCH_DIR:-$(dirname "$(realpath "$0")")/../target/bench}
stall_s=${STALL_S:-300}
run_dir=$bench_dir/runs/$label
pg_port=55432
nats_port=54222
metrics_port=59000
# Throwaway credentials for containers this script creates and destroys.
pg_password=bench
nats_password=bench

# Config must come from this script only, not from a developer's direnv.
unset $(compgen -e | grep "^APP__") || true

rm -rf "$run_dir"
mkdir -p "$run_dir"

cleanup() {
    if [[ -n ${indexer_pid:-} ]]; then
        # With WRAP the indexer is the wrapper's child; stopping it lets e.g. samply save.
        pkill -TERM -P "$indexer_pid" || kill "$indexer_pid" 2>/dev/null || true
        wait "$indexer_pid" 2>/dev/null || true
    fi
    docker rm -f indexer-bench-pg indexer-bench-nats >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "building chain-indexer in $worktree"
(cd "$worktree" && cargo build --release -p chain-indexer --features cloud)
binary=$worktree/target/release/chain-indexer

docker rm -f indexer-bench-pg indexer-bench-nats >/dev/null 2>&1 || true
docker run -d --name indexer-bench-pg -p 127.0.0.1:$pg_port:5432 \
    -e POSTGRES_USER=indexer -e POSTGRES_DB=indexer -e POSTGRES_PASSWORD=$pg_password \
    postgres:17.1-alpine ${PG_ARGS:-} >/dev/null
docker run -d --name indexer-bench-nats -p 127.0.0.1:$nats_port:4222 \
    nats:2.12.3 --user indexer --pass $nats_password -js >/dev/null
psql_bench() { docker exec indexer-bench-pg psql -U indexer -At -F ' ' -c "$1"; }

# Over TCP: the image's init phase answers on the unix socket only, then restarts.
until docker exec indexer-bench-pg pg_isready -h 127.0.0.1 -U indexer >/dev/null 2>&1; do sleep 1; done

CONFIG_FILE=$worktree/chain-indexer/config.yaml \
    RUST_LOG=${RUST_LOG:-info} \
    APP__APPLICATION__NETWORK_ID=$network_id \
    APP__INFRA__NODE__URL=$node_url \
    APP__INFRA__STORAGE__PORT=$pg_port \
    APP__INFRA__STORAGE__PASSWORD=$pg_password \
    APP__INFRA__PUB_SUB__URL=localhost:$nats_port \
    APP__INFRA__PUB_SUB__PASSWORD=$nats_password \
    APP__TELEMETRY__METRICS__ENABLED=true \
    APP__TELEMETRY__METRICS__PORT=$metrics_port \
    ${WRAP:-} "$binary" >"$run_dir/indexer.log" 2>&1 &
indexer_pid=$!

start=$(date +%s.%N)
echo "elapsed_s,height,gc_s,ledger_nodes,ledger_nodes_bytes,db_bytes" >"$run_dir/heights.csv"
height=0
last_progress=$SECONDS
last_height=0
while ((height < target)); do
    sleep 5
    kill -0 "$indexer_pid" 2>/dev/null || { echo "chain-indexer exited, see $run_dir/indexer.log"; exit 1; }
    # A hung runtime also hangs its metrics endpoint, hence the timeout.
    metrics=$(curl -s -m 5 "localhost:$metrics_port/metrics" || true)
    height=$(awk '/^indexer_block_height /{print int($2)}' <<<"$metrics")
    gc_s=$(awk '/^indexer_gc_duration_seconds_sum /{print $2}' <<<"$metrics")
    height=${height:-$last_height}
    if ((height > last_height)); then
        last_height=$height
        last_progress=$SECONDS
    elif ((SECONDS - last_progress > stall_s)); then
        echo "no progress for ${stall_s}s at height $height, see $run_dir/stall-backtrace.txt"
        pid=$(pgrep -P "$indexer_pid" chain-indexer || echo "$indexer_pid")
        gdb -p "$pid" -batch -ex "thread apply all bt" >"$run_dir/stall-backtrace.txt" 2>&1 || true
        exit 1
    fi
    elapsed=$(echo "$(date +%s.%N) - $start" | bc)
    # n_live_tup (inserts minus deletes) is a cheap stand-in for count(*), which is a full scan.
    disk=$(psql_bench "SELECT n_live_tup, pg_total_relation_size('ledger_db_nodes'), pg_database_size('indexer')
        FROM pg_stat_user_tables WHERE relname = 'ledger_db_nodes'" | tr ' ' , || true)
    echo "$elapsed,$height,${gc_s:-0},${disk:-,,}" >>"$run_dir/heights.csv"
    printf '\r%s: height %d / %d, %.0fs' "$label" "$height" "$target" "$elapsed"
done
echo

# Content digest up to the target height. Row ids are excluded as they depend on insert order only.
# Rows are hashed one by one: concatenating whole rows exceeds Postgres' 1 GB value limit on mainnet.
psql_bench "
    SELECT 'blocks', count(*), md5(string_agg(md5((to_jsonb(b) - 'id')::text), '' ORDER BY b.height))
    FROM blocks b WHERE b.height <= $target
    UNION ALL
    SELECT 'transactions', count(*),
        md5(coalesce(string_agg(md5((to_jsonb(t) - 'id' - 'block_id')::text || b.height), ''
            ORDER BY b.height, t.id), ''))
    FROM transactions t JOIN blocks b ON b.id = t.block_id WHERE b.height <= $target
    UNION ALL
    SELECT 'system_parameters_d', count(*),
        md5(coalesce(string_agg(md5((to_jsonb(s) - 'id')::text), '' ORDER BY s.block_height, s.id), ''))
    FROM system_parameters_d s WHERE s.block_height <= $target
    UNION ALL
    SELECT 'system_parameters_terms_and_conditions', count(*),
        md5(coalesce(string_agg(md5((to_jsonb(s) - 'id')::text), '' ORDER BY s.block_height, s.id), ''))
    FROM system_parameters_terms_and_conditions s WHERE s.block_height <= $target
" >"$run_dir/digest.txt"

# Exact counts after timing stopped. Rows beyond the live set are garbage gc has not collected yet.
ledger_rows=$(psql_bench "SELECT (SELECT count(*) FROM ledger_db_nodes), (SELECT count(*) FROM ledger_db_roots),
    pg_total_relation_size('ledger_db_nodes'), pg_database_size('indexer')")

gc_seconds=$(curl -s "localhost:$metrics_port/metrics" | awk '/^indexer_gc_duration_seconds_sum /{print $2}')
{
    echo "label:        $label"
    echo "commit:       $(git -C "$worktree" rev-parse --short HEAD) $(git -C "$worktree" status --porcelain | grep -q . && echo dirty)"
    echo "node:         $node_url"
    echo "blocks:       $height"
    echo "seconds:      $elapsed"
    echo "blocks/s:     $(echo "scale=1; $height / $elapsed" | bc)"
    echo "gc seconds:   ${gc_seconds:-n/a}"
    read -r nodes roots nodes_bytes db_bytes <<<"$ledger_rows"
    echo "ledger nodes: $nodes ($(numfmt --to=iec "$nodes_bytes")), roots: $roots, db: $(numfmt --to=iec "$db_bytes")"
    sed "s/^/digest:       /" "$run_dir/digest.txt"
} | tee "$run_dir/summary.txt"
