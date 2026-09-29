#!/usr/bin/env bash

set -euxo pipefail

# Cleanup function to ensure node container and scratch build dir are removed.
token_issuer_build_dir=""
cleanup() {
    docker rm -f node >/dev/null 2>&1 || true
    # `[ -n ... ] && rm` as the trap's last command would make a successful run
    # exit 1 whenever the build dir was never set.
    if [ -n "$token_issuer_build_dir" ]; then
        rm -rf "$token_issuer_build_dir"
    fi
}

# Set up trap to cleanup on exit.
trap cleanup EXIT

if [ -z "$1" ]; then
    echo "Error: node version parameter is required" >&2
    echo "Usage: $0 <node_version> [toolkit_version]" >&2
    exit 1
fi
readonly node_version="$1"
# A node release names the toolkit release it ships with, and the two carry
# independent version numbers: node-1.0.2 ships toolkit-1.0.0.
readonly toolkit_version="${2:-$node_version}"
readonly toolkit_image="midnightntwrk/midnight-node-toolkit:$toolkit_version"
readonly rng_seed="0000000000000000000000000000000000000000000000000000000000000037"
readonly node_dir="$(pwd)/.node/$node_version"

# The compactc version to build contracts/token-issuer with is toolkit-js's
# compiler pin, not midnight-js's, and that pin ties to the WHOLE ledger stack
# a toolkit build targets, not just a compact-runtime number: each compactc
# release binds one onchain-runtime/ledger major via a fixed compact-runtime
# dependency, and compact-runtime's own version check demands an EXACT minor
# match under its 0.x versioning, so there is no forward- or
# backward-compatible choice here — one compactc version per ledger line,
# full stop.
#
#   ledger | node/toolkit line          | compactc    | compact-runtime
#   -------|-----------------------------|-------------|----------------
#   v8     | 1.x                         | 0.30.0      | 0.15.0
#   v9     | 2.1.0-beta.1+               | 0.33.0-rc.2 | 0.18.0-rc.1
#
# The default follows node_version per the table above; set COMPACTC_VERSION
# to override it. Ledger tokens token-issuer mints go to whichever wallet its
# mintUnshielded call names as recipient.
case "$node_version" in
    2.1.*) default_compactc_version="0.33.0-rc.2" ;;
    *) default_compactc_version="0.30.0" ;;
esac
readonly compactc_version="${COMPACTC_VERSION:-$default_compactc_version}"
readonly token_issuer_dir="$(pwd)/contracts/token-issuer"
readonly token_issuer_mint_seed="0000000000000000000000000000000000000000000000000000000000000001"
readonly token_issuer_mint_amount="100000000000"

# A GITHUB_TOKEN exported but empty (not merely unset) makes the `compact`
# CLI authenticate with a blank credential and fail every GitHub call outright
# ("Bad credentials") instead of falling back to an unauthenticated request.
if [ -z "${GITHUB_TOKEN:-}" ]; then
    unset GITHUB_TOKEN
fi

# The target triple compactc's release ASSETS are named with, for the
# direct-download fallback below.
compactc_asset_triple() {
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) echo "x86_64-unknown-linux-musl" ;;
        Linux-aarch64|Linux-arm64) echo "aarch64-unknown-linux-musl" ;;
        Darwin-x86_64) echo "x86_64-darwin" ;;
        Darwin-arm64|Darwin-aarch64) echo "aarch64-darwin" ;;
        *)
            echo "Error: no known compactc release asset for $(uname -s)-$(uname -m)" >&2
            exit 1
            ;;
    esac
}

# The directory name the `compact` CLI resolves a compiler under, i.e.
# versions/<version>/<triple>/compactc. It matches the asset triple everywhere
# except Intel macOS, where the asset ships as x86_64-darwin but the CLI's
# Target type renders x86_64-apple-darwin -- unpacking under the asset name
# there installs a compiler the CLI can never find.
compactc_install_triple() {
    case "$(uname -s)-$(uname -m)" in
        Darwin-x86_64) echo "x86_64-apple-darwin" ;;
        *) compactc_asset_triple ;;
    esac
}

# Make sure compactc $compactc_version is installed, fetching it if not.
ensure_compactc() {
    local version="$1"
    local install_triple dest
    install_triple="$(compactc_install_triple)"
    dest="$HOME/.compact/versions/$version/$install_triple"
    # Mirror the CLI's own definition of "installed" -- the compiler binary is
    # present -- rather than parsing `compact list --installed`, whose output
    # only indents entries when a default compiler is set and marks the current
    # one with a configurable icon, so a grep on it misses exactly the machines
    # this fallback has just provisioned.
    if [ -x "$dest/compactc" ]; then
        return
    fi
    if compact update "$version" --no-set-default >/dev/null 2>&1; then
        return
    fi
    # `compact update`/`list` only surface FINAL releases. Some versions (0.33.0
    # among them, needed for ledger v9) were never cut as a final release --
    # only as release-candidate tags (compactc-v0.33.0-rc.0/1/2) -- so pass the
    # exact upstream tag suffix as COMPACTC_VERSION (e.g. "0.33.0-rc.2") and
    # this fetches it straight from the compiler's upstream home,
    # LFDT-Minokawa/compact, which is where these tags are actually published
    # (midnightntwrk/compact exists, but only mirrors final releases).
    echo "compact CLI has no final release '$version'; fetching it from LFDT-Minokawa/compact directly" >&2
    if ! command -v gh >/dev/null 2>&1; then
        echo "Error: the 'gh' CLI is required to fetch compactc $version, which has no final release" >&2
        exit 1
    fi
    # `gh release download` hits the API as a user, so it fails on an
    # unauthenticated host even for a public repo.
    if ! gh auth status >/dev/null 2>&1; then
        echo "Error: 'gh' is not authenticated; run 'gh auth login' or export GH_TOKEN to fetch compactc $version" >&2
        exit 1
    fi
    if ! command -v unzip >/dev/null 2>&1; then
        echo "Error: 'unzip' is required to unpack the compactc $version release asset" >&2
        exit 1
    fi
    local asset_triple tmp_zip_dir tag found_tag
    asset_triple="$(compactc_asset_triple)"
    tmp_zip_dir="$(mktemp -d)"
    found_tag=""
    # Releases from 0.31.0 on are tagged compactc-v<version>; older ones
    # (0.30.0-rc.*) are tagged plain v<version>.
    for tag in "compactc-v$version" "v$version"; do
        if gh release download "$tag" --repo LFDT-Minokawa/compact \
            -p "*${asset_triple}.zip" -O "$tmp_zip_dir/compactc.zip" --clobber 2>/dev/null; then
            found_tag="$tag"
            break
        fi
    done
    if [ -z "$found_tag" ]; then
        rm -rf "$tmp_zip_dir"
        echo "Error: no compactc release for $asset_triple at LFDT-Minokawa/compact under tag compactc-v$version or v$version" >&2
        exit 1
    fi
    mkdir -p "$dest"
    unzip -oq "$tmp_zip_dir/compactc.zip" -d "$dest"
    chmod +x "$dest"/*
    rm -rf "$tmp_zip_dir"
}

# Set up fresh node data directory.
if [ -d $node_dir ]; then
    rm -r $node_dir;
fi
mkdir -p $node_dir

# Compile token-issuer on the host: toolkit-js's own config.ts imports
# @midnight-ntwrk/compact-js/effect, which only resolves under /toolkit-js
# inside the toolkit image, so the compiled contract and its config.ts are
# seeded into the toolkit_out volume and dual-mounted there for that step.
if ! command -v compact >/dev/null 2>&1; then
    echo "Error: the 'compact' CLI is required to build contracts/token-issuer (see https://docs.midnight.network for install instructions)" >&2
    exit 1
fi
ensure_compactc "$compactc_version"
token_issuer_build_dir="$(mktemp -d)"
compact compile "+$compactc_version" "$token_issuer_dir/token-issuer.compact" "$token_issuer_build_dir/out"

docker run \
    --rm \
    -v toolkit_out:/out \
    -v "$token_issuer_build_dir/out":/src/out:ro \
    -v "$token_issuer_dir/token-issuer.config.ts":/src/token-issuer.config.ts:ro \
    --entrypoint sh \
    $toolkit_image \
    -c 'rm -rf /out/out /out/token-issuer.config.ts && cp -r /src/out /out/out && cp /src/token-issuer.config.ts /out/token-issuer.config.ts'
rm -rf "$token_issuer_build_dir"

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
    midnightntwrk/midnight-node:$node_version

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
        }' | jq -r .result)
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
        }" | jq -r '.result.number')
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

# Deploy token-issuer and mint unshielded tokens to wallet 01 (the same funding
# wallet used elsewhere in this script), so the pre-populated
# chain also has a non-NIGHT unshielded token colour to query against.
docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    -v toolkit_out:/toolkit-js/token-issuer \
    -e TOOLKIT_JS_PATH=/toolkit-js \
    $toolkit_image \
    generate-intent deploy \
    --toolkit-js-path /toolkit-js \
    --config /toolkit-js/token-issuer/token-issuer.config.ts \
    --coin-public aa0d72bb77ea46f986a800c66d75c4e428a95bd7e1244f1ed059374e6266eb98 \
    --network undeployed \
    --output-intent /toolkit-js/token-issuer/deploy.intent \
    --output-private-state /toolkit-js/token-issuer/deploy.private \
    --output-zswap-state /toolkit-js/token-issuer/deploy.zswap

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs contract-custom \
    --funding-seed "0000000000000000000000000000000000000000000000000000000000000001" \
    --compiled-contract-dir /out/out \
    --intent-file /out/deploy.intent \
    --dest-file /out/token_issuer_deploy.mn

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs --src-file /out/token_issuer_deploy.mn \
    send

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    contract-address --src-file /out/token_issuer_deploy.mn > /tmp/token_issuer_address.mn

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    contract-state --contract-address $(cat /tmp/token_issuer_address.mn) \
    --dest-file /out/token_issuer_state.bin

token_issuer_recipient_address=$(docker run \
    --rm \
    $toolkit_image \
    show-address --network undeployed --seed $token_issuer_mint_seed \
    | jq -r .userAddress)

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    -v toolkit_out:/toolkit-js/token-issuer \
    -e TOOLKIT_JS_PATH=/toolkit-js \
    $toolkit_image \
    generate-intent circuit \
    --toolkit-js-path /toolkit-js \
    --config /toolkit-js/token-issuer/token-issuer.config.ts \
    --contract-address $(cat /tmp/token_issuer_address.mn) \
    --coin-public aa0d72bb77ea46f986a800c66d75c4e428a95bd7e1244f1ed059374e6266eb98 \
    --input-onchain-state /toolkit-js/token-issuer/token_issuer_state.bin \
    --input-private-state /toolkit-js/token-issuer/deploy.private \
    --output-intent /toolkit-js/token-issuer/mint.intent \
    --output-private-state /toolkit-js/token-issuer/mint.private \
    --output-zswap-state /toolkit-js/token-issuer/mint.zswap \
    mintUnshielded "{bytes:'0x$token_issuer_recipient_address'}" $token_issuer_mint_amount

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs contract-custom \
    --funding-seed "0000000000000000000000000000000000000000000000000000000000000001" \
    --compiled-contract-dir /out/out \
    --intent-file /out/mint.intent \
    --dest-file /out/token_issuer_mint.mn

docker run \
    --rm \
    --network host \
    -v toolkit_out:/out \
    $toolkit_image \
    generate-txs --src-file /out/token_issuer_mint.mn \
    send

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
        }' | jq -r .result)
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
        }" | jq -r '.result.number')
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
