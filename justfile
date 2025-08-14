set shell := ["bash", "-uc"]

# Can be overridden on the command line: `just feature=standalone`.
feature := "cloud"
packages := "indexer-common chain-indexer wallet-indexer indexer-api indexer-standalone indexer-tests"
rust_version := `grep channel rust-toolchain.toml | sed -r 's/channel = "(.*)"/\1/'`
nightly := "nightly-2025-07-01"
node_version := "0.13.2-rc.2"

check:
    for package in {{packages}}; do \
        cargo check -p "$package" --tests; \
        cargo check -p "$package" --tests --features {{feature}}; \
    done

license-headers:
    ./license_headers.sh

fmt:
    cargo +{{nightly}} fmt

fmt-check:
    cargo +{{nightly}} fmt --check

fix:
    cargo fix --allow-dirty --allow-staged --features {{feature}}

lint:
    for package in {{packages}}; do \
        cargo clippy -p "$package" --no-deps --tests                        -- -D warnings; \
        cargo clippy -p "$package" --no-deps --tests --features {{feature}} -- -D warnings; \
    done

lint-fix:
    for package in {{packages}}; do \
        cargo clippy -p "$package" --no-deps --tests --fix --allow-dirty --allow-staged                       ; \
        cargo clippy -p "$package" --no-deps --tests --fix --allow-dirty --allow-staged --features {{feature}}; \
    done

test:
    # We must build the executables needed by the e2e tests!
    if [ "{{feature}}" = "cloud" ]; then \
        cargo build -p chain-indexer -p wallet-indexer -p indexer-api --features cloud; \
    fi
    if [ "{{feature}}" = "standalone" ]; then \
        cargo build -p indexer-standalone --features standalone; \
    fi
    cargo nextest run --workspace --exclude indexer-standalone --features {{feature}}
    # Check indexer-api schema:
    cargo run -p indexer-api --bin indexer-api-cli print-api-schema-v1 > \
        indexer-api/graphql/schema-v1.graphql.check
    @if ! cmp -s indexer-api/graphql/schema-v1.graphql indexer-api/graphql/schema-v1.graphql.check; then \
        echo "schema-v1.graphql has changes!"; exit 1; \
    fi

doc:
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +{{nightly}} doc -p indexer-common --no-deps --all-features
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +{{nightly}} doc -p chain-indexer  --no-deps --features {{feature}}
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +{{nightly}} doc -p wallet-indexer --no-deps --features {{feature}}
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +{{nightly}} doc -p indexer-api    --no-deps --features {{feature}}
    if [ "{{feature}}" = "standalone" ]; then \
        RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo +{{nightly}} doc -p indexer-standalone --no-deps --features standalone; \
    fi

all: license-headers check fmt lint test doc

all-all:
    just feature=cloud all
    just feature=standalone all

coverage:
    ./coverage.sh {{nightly}}

generate-indexer-api-schema:
    cargo run -p indexer-api --bin indexer-api-cli print-api-schema-v1 > \
        indexer-api/graphql/schema-v1.graphql

build-docker-image package profile="dev":
    tag=$(git rev-parse --short=8 HEAD) && \
    docker build \
        --build-arg "RUST_VERSION={{rust_version}}" \
        --build-arg "PROFILE={{profile}}" \
        --secret id=netrc,src=$NETRC \
        -t ghcr.io/midnight-ntwrk/{{package}}:${tag} \
        -t ghcr.io/midnight-ntwrk/{{package}}:latest \
        -f {{package}}/Dockerfile \
        .

run-chain-indexer node="ws://localhost:9944" network_id="Undeployed":
    docker compose up -d --wait postgres nats
    RUST_LOG=chain_indexer=debug,indexer_common=debug,fastrace_opentelemetry=off,info \
        CONFIG_FILE=chain-indexer/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        APP__INFRA__NODE__URL={{node}} \
        cargo run -p chain-indexer --features {{feature}}

run-wallet-indexer network_id="Undeployed":
    docker compose up -d --wait postgres nats
    RUST_LOG=wallet_indexer=debug,indexer_common=debug,fastrace_opentelemetry=off,info \
        CONFIG_FILE=wallet-indexer/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        cargo run -p wallet-indexer --features {{feature}}

run-indexer-api network_id="Undeployed":
    docker compose up -d --wait postgres nats
    RUST_LOG=indexer_api=debug,indexer_common=debug,info \
        CONFIG_FILE=indexer-api/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        cargo run -p indexer-api --bin indexer-api --features {{feature}}

run-indexer-standalone node="ws://localhost:9944" network_id="Undeployed":
    mkdir -p target/data
    RUST_LOG=indexer=debug,chain_indexer=debug,wallet_indexer=debug,indexer_api=debug,indexer_common=debug,fastrace_opentelemetry=off,info \
        CONFIG_FILE=indexer-standalone/config.yaml \
        APP__APPLICATION__NETWORK_ID={{network_id}} \
        APP__INFRA__NODE__URL={{node}} \
        APP__INFRA__STORAGE__CNN_URL=target/data/indexer.sqlite \
        cargo run -p indexer-standalone --features standalone

generate-node-data:
    ./generate_node_data.sh {{node_version}}

run-node:
    #!/usr/bin/env bash
    node_dir=$(mktemp -d)
    cp -r ./.node/{{node_version}}/ $node_dir
    docker run \
        --name node \
        -p 9944:9944 \
        -e SHOW_CONFIG=false \
        -e CFG_PRESET=dev \
        -v $node_dir:/node \
        ghcr.io/midnight-ntwrk/midnight-node:{{node_version}}

get-node-metadata:
    subxt metadata \
        -f bytes \
        --url ws://localhost:9944 > \
        ./.node/{{node_version}}/metadata.scale

# PM-18678 Investigation Commands
# Run these to investigate the production bug where wallet subscriptions hang

# Build the PM-18678 monitoring tool
pm18678-build:
    cd scripts/pm-18678-investigation && \
    cargo build --release

# Launch full investigation on EC2 (one-command setup and run)
# Usage: just pm18678-launch-ec2 [reproduce|control] [wallet_count]
pm18678-launch-ec2 mode="reproduce" wallets="30":
    #!/usr/bin/env bash
    set -e
    cd scripts/pm-18678-investigation
    chmod +x launch-ec2-investigation.sh
    ./launch-ec2-investigation.sh {{mode}} {{wallets}}

# Run investigation locally (assumes dependencies are installed)
# Usage: just pm18678-run [reproduce|control]
pm18678-run mode="reproduce":
    #!/usr/bin/env bash
    set -e
    # Build with cloud feature for PM-18678 instrumentation
    cargo build --release --features cloud
    cd scripts/pm-18678-investigation
    cargo build --release
    chmod +x run-investigation.sh
    ./run-investigation.sh {{mode}}

# Analyze PM-18678 investigation logs
# Usage: just pm18678-analyze [log_directory]
pm18678-analyze log_dir="$HOME/midnight-investigation/logs":
    #!/usr/bin/env bash
    cd scripts/pm-18678-investigation
    chmod +x analyze-logs.sh
    ./analyze-logs.sh {{log_dir}}

# Quick check of PM-18678 investigation status
pm18678-check:
    #!/usr/bin/env bash
    cd scripts/pm-18678-investigation
    chmod +x quick-check.sh
    ./quick-check.sh

# Quick status check for running investigation
pm18678-status:
    #!/usr/bin/env bash
    echo "=== PM-18678 Investigation Status ==="
    echo ""
    echo "Active tmux sessions:"
    tmux ls 2>/dev/null || echo "  No sessions running"
    echo ""
    echo "Docker containers:"
    docker ps | grep -E "postgres|nats" || echo "  No containers running"
    echo ""
    if [ -d "$HOME/midnight-investigation/logs" ]; then
        LATEST_LOG=$(ls -dt "$HOME/midnight-investigation/logs"/2* 2>/dev/null | head -1)
        if [ -n "$LATEST_LOG" ]; then
            echo "Latest log directory: $LATEST_LOG"
            if [ -f "$LATEST_LOG/issues/the-issue.log" ]; then
                ISSUE_COUNT=$(wc -l < "$LATEST_LOG/issues/the-issue.log")
                echo "THE ISSUE™ detected: $ISSUE_COUNT times"
            else
                echo "THE ISSUE™ detected: 0 times"
            fi
        fi
    else
        echo "No investigation logs found"
    fi

# Stop PM-18678 investigation
pm18678-stop:
    #!/usr/bin/env bash
    echo "Stopping PM-18678 investigation..."
    tmux kill-server 2>/dev/null || true
    docker stop postgres nats midnight-node 2>/dev/null || true
    docker rm postgres nats midnight-node 2>/dev/null || true
    echo "Investigation stopped"

# Test PM-18678 setup without starting services
pm18678-test:
    #!/usr/bin/env bash
    echo "=== Testing PM-18678 Investigation Setup ==="
    echo ""
    
    # Test 1: Build monitoring tool
    echo "1. Building monitoring tool..."
    cd scripts/pm-18678-investigation && \
    if cargo build --release 2>&1 | grep -q "Finished"; then
        echo "   ✓ Monitoring tool built"
    else
        echo "   ✗ Build failed"
        exit 1
    fi
    cd ../..
    
    # Test 2: Check script is executable
    echo "2. Checking run script..."
    if [ -x scripts/pm-18678-investigation/run-investigation.sh ]; then
        echo "   ✓ run-investigation.sh is executable"
    else
        chmod +x scripts/pm-18678-investigation/run-investigation.sh
        echo "   ✓ Made run-investigation.sh executable"
    fi
    
    # Test 3: Verify monitoring binary
    echo "3. Testing monitoring binary..."
    if scripts/pm-18678-investigation/target/release/pm18678-monitor --version &>/dev/null; then
        echo "   ✓ Monitoring binary works"
    else
        echo "   ✗ Monitoring binary not working"
        exit 1
    fi
    
    # Test 4: Check Docker
    echo "4. Checking Docker services..."
    if docker ps | grep -q postgres; then
        echo "   ✓ PostgreSQL running"
    else
        echo "   ⚠ PostgreSQL not running (will be started)"
    fi
    if docker ps | grep -q nats; then
        echo "   ✓ NATS running"
    else
        echo "   ⚠ NATS not running (will be started)"
    fi
    
    # Test 5: Check environment
    echo "5. Checking environment..."
    if command -v tmux &>/dev/null; then
        echo "   ✓ tmux available"
    else
        echo "   ✗ tmux not found"
        exit 1
    fi
    
    echo ""
    echo "=== All checks passed! ==="
    echo "Ready to run: just pm18678-run [reproduce|control]"

# Clean PM-18678 investigation data (preserves logs)
pm18678-clean:
    #!/usr/bin/env bash
    read -p "This will remove investigation binaries but preserve logs. Continue? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        rm -f scripts/pm-18678-investigation/target/release/pm18678-monitor
        echo "Cleaned investigation binaries"
        echo "Logs preserved at: $HOME/midnight-investigation/logs"
    fi

# Reset PM-18678 environment for fresh start (removes containers, data, preserves logs)
pm18678-reset:
    #!/usr/bin/env bash
    echo "=== PM-18678 Investigation Reset ==="
    echo ""
    echo "This will:"
    echo "  • Stop all tmux sessions"
    echo "  • Remove Docker containers (postgres, nats)"
    echo "  • Clear target/data directory"
    echo "  • Remove investigation binaries"
    echo "  • Preserve logs for analysis"
    echo ""
    read -p "Continue with reset? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo ""
        # Stop tmux sessions
        echo "1. Stopping tmux sessions..."
        tmux kill-server 2>/dev/null || true
        echo "   ✓ Tmux sessions stopped"
        
        # Stop and remove Docker containers
        echo "2. Cleaning Docker containers..."
        # Stop named containers (from run-investigation.sh)
        docker stop postgres nats midnight-node 2>/dev/null || true
        docker rm postgres nats midnight-node 2>/dev/null || true
        # Stop docker-compose containers (from development)
        docker stop midnight-indexer-postgres-1 midnight-indexer-nats-1 2>/dev/null || true
        docker rm midnight-indexer-postgres-1 midnight-indexer-nats-1 2>/dev/null || true
        docker compose down 2>/dev/null || true
        echo "   ✓ Docker containers removed"
        
        # Clear target/data directory (SQLite database for standalone mode)
        echo "3. Removing target/data..."
        if [ -d "target/data" ]; then
            rm -rf target/data
            echo "   ✓ target/data removed"
        else
            echo "   ✓ target/data not found (already clean)"
        fi
        
        # Clean investigation binaries
        echo "4. Removing investigation binaries..."
        rm -f scripts/pm-18678-investigation/target/release/pm18678-monitor
        echo "   ✓ Investigation binaries removed"
        
        # Show log preservation
        echo ""
        echo "=== Reset Complete ==="
        if [ -d "$HOME/midnight-investigation/logs" ]; then
            LOG_COUNT=$(find "$HOME/midnight-investigation/logs" -type d -name "2*" | wc -l)
            echo "Logs preserved: $LOG_COUNT investigation sessions"
            echo "Location: $HOME/midnight-investigation/logs"
        else
            echo "No logs to preserve"
        fi
        echo ""
        echo "Ready for fresh investigation session!"
        echo "Run: just pm18678-test  # To verify setup"
    else
        echo "Reset cancelled"
    fi

# Full clean including logs (CAUTION: removes all investigation data)
pm18678-purge:
    #!/usr/bin/env bash
    echo "=== PM-18678 FULL PURGE WARNING ==="
    echo ""
    echo "This will PERMANENTLY remove:"
    echo "  ✗ All investigation logs"
    echo "  ✗ All Docker containers and volumes"
    echo "  ✗ All investigation binaries"
    echo "  ✗ All target/data"
    echo ""
    echo "THIS CANNOT BE UNDONE!"
    echo ""
    read -p "Type 'PURGE' to confirm: " -r
    echo
    if [[ $REPLY == "PURGE" ]]; then
        echo ""
        # Stop everything
        echo "1. Stopping all services..."
        tmux kill-server 2>/dev/null || true
        
        # Remove Docker containers and volumes
        echo "2. Removing Docker containers and volumes..."
        docker stop postgres nats midnight-node 2>/dev/null || true
        docker rm -v postgres nats midnight-node 2>/dev/null || true
        docker compose down -v 2>/dev/null || true
        
        # Remove all data
        echo "3. Removing all investigation data..."
        rm -rf target/data
        rm -rf scripts/pm-18678-investigation/target
        rm -rf "$HOME/midnight-investigation"
        
        echo ""
        echo "=== PURGE COMPLETE ==="
        echo "All PM-18678 investigation data has been removed"
    else
        echo "Purge cancelled (you typed: '$REPLY')"
    fi
