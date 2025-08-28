#!/bin/bash
# PM-18678 Automated Investigation Runner
# Runs unattended for 3-4 weeks with comprehensive logging

set -e

# ============================================================================
# CONFIGURATION
# ============================================================================

INVESTIGATION_DIR="$HOME/midnight-investigation"
INDEXER_DIR="$INVESTIGATION_DIR/midnight-indexer"
LOG_DIR="$INVESTIGATION_DIR/logs/$(date +%Y%m%d_%H%M%S)"
BRANCH="investigation/PM-18678-hanging-root-cause"

# Test configuration
WALLET_COUNT=30
TEST_MODE="${1:-reproduce}"  # "reproduce" or "control"
NODE_URL="${NODE_URL:-ws://localhost:9944}"

# Create log directories
mkdir -p "$LOG_DIR"/{services,monitoring,issues,database}

# ============================================================================
# LOGGING FUNCTIONS
# ============================================================================

log_info() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] INFO: $1" | tee -a "$LOG_DIR/investigation.log"
}

log_error() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] ERROR: $1" | tee -a "$LOG_DIR/investigation.log" "$LOG_DIR/errors.log"
}

log_issue_detected() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] THE ISSUE DETECTED: $1" | tee -a "$LOG_DIR/issues/the-issue.log"
    # Also capture full system state when issue detected
    capture_diagnostics
}

capture_diagnostics() {
    local timestamp=$(date +%Y%m%d_%H%M%S)
    local diag_dir="$LOG_DIR/issues/diagnostics_$timestamp"
    mkdir -p "$diag_dir"
    
    # Capture database state
    psql -h localhost -U indexer -d indexer -c "\copy (
        SELECT 'wallets' as table_name, COUNT(*) as count FROM wallets
        UNION ALL
        SELECT 'transactions', COUNT(*) FROM transactions
        UNION ALL  
        SELECT 'relevant_transactions', COUNT(*) FROM relevant_transactions
    ) TO '$diag_dir/table_counts.csv' CSV HEADER" 2>/dev/null || true
    
    # Capture connection pool state
    psql -h localhost -U indexer -d indexer -c "\copy (
        SELECT * FROM pg_stat_activity WHERE datname = 'indexer'
    ) TO '$diag_dir/connections.csv' CSV HEADER" 2>/dev/null || true
    
    # Capture process info
    ps aux | grep -E 'indexer|midnight' > "$diag_dir/processes.txt"
    
    # Capture memory usage
    free -h > "$diag_dir/memory.txt"
    
    log_info "Diagnostics captured in $diag_dir"
}

# ============================================================================
# SETUP
# ============================================================================

log_info "Starting PM-18678 Investigation"
log_info "Mode: $TEST_MODE"
log_info "Log directory: $LOG_DIR"

cd "$INDEXER_DIR"

# Verify branch
CURRENT_BRANCH=$(git branch --show-current)
if [ "$CURRENT_BRANCH" != "$BRANCH" ]; then
    log_error "Wrong branch. Current: $CURRENT_BRANCH, Expected: $BRANCH"
    exit 1
fi

# Set environment
export APP__INFRA__STORAGE__PASSWORD=postgres
export APP__INFRA__PUB_SUB__PASSWORD=nats
export APP__INFRA__LEDGER_STATE_STORAGE__PASSWORD=nats
export APP__INFRA__SECRET=$(openssl rand -hex 32)
export APP__INFRA__NODE__URL="$NODE_URL"

# Set test mode
if [ "$TEST_MODE" = "reproduce" ]; then
    export PM18678_DISABLE_OPTIMIZATION=true
    log_info "PR #42 optimization DISABLED - attempting to reproduce issue"
else
    unset PM18678_DISABLE_OPTIMIZATION
    log_info "PR #42 optimization ENABLED - control test"
fi

# ============================================================================
# BUILD
# ============================================================================

log_info "Building indexer components..."

# Configure cargo to use sparse registry protocol to avoid git issues
export CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

# Check if we have GitHub token for private repos
if [ -n "$GITHUB_TOKEN" ]; then
    log_info "Using GitHub token for private dependencies"
    git config --global url."https://${GITHUB_TOKEN}@github.com/".insteadOf "https://github.com/"
else
    log_error "WARNING: No GITHUB_TOKEN set. Build may fail if private dependencies are required."
    log_error "To fix: export GITHUB_TOKEN=your_github_personal_access_token"
    log_error "Create a token at: https://github.com/settings/tokens with 'repo' scope"
    log_error ""
    log_error "Attempting build anyway..."
fi

cargo build --release --features cloud > "$LOG_DIR/build.log" 2>&1 || {
    log_error "Build failed. Check $LOG_DIR/build.log for details"
    log_error "If it's asking for GitHub credentials, you need to set GITHUB_TOKEN"
    tail -20 "$LOG_DIR/build.log"
    exit 1
}

# Build monitoring script
cd "$INDEXER_DIR/scripts/pm-18678-investigation"
cargo build --release >> "$LOG_DIR/build.log" 2>&1
cd "$INDEXER_DIR"

# Verify all binaries exist
log_info "Verifying built binaries..."
for binary in chain-indexer wallet-indexer indexer-api; do
    if [ ! -f "./target/release/$binary" ]; then
        log_error "Binary not found: ./target/release/$binary"
        log_error "Build may have failed. Check $LOG_DIR/build.log"
        exit 1
    fi
done
if [ ! -f "./scripts/pm-18678-investigation/target/release/pm18678-monitor" ]; then
    log_error "Monitor binary not found"
    exit 1
fi
log_info "All binaries built successfully"

# ============================================================================
# CHECK DEPENDENCIES
# ============================================================================

# Check for just (task runner)
if ! command -v just &> /dev/null; then
    log_info "Installing just task runner..."
    if command -v snap &> /dev/null; then
        sudo snap install just --classic
    elif command -v cargo &> /dev/null; then
        cargo install just
    else
        log_error "WARNING: Could not install just. You can still use shell scripts directly."
    fi
fi

# ============================================================================
# START INFRASTRUCTURE
# ============================================================================

log_info "Starting infrastructure services..."

# Use DOCKER_CMD from environment or default to "docker"
DOCKER_CMD="${DOCKER_CMD:-docker}"

# PostgreSQL
if ! $DOCKER_CMD ps | grep -q postgres; then
    $DOCKER_CMD run -d --name postgres \
        -e POSTGRES_USER=indexer \
        -e POSTGRES_PASSWORD=postgres \
        -e POSTGRES_DB=indexer \
        -p 5432:5432 \
        postgres:15
    sleep 10
fi

# NATS
if ! $DOCKER_CMD ps | grep -q nats; then
    $DOCKER_CMD run -d --name nats \
        -p 4222:4222 \
        nats:latest -js --user indexer --pass nats
    sleep 5
fi

# Midnight Node
if ! $DOCKER_CMD ps | grep -q node; then
    log_info "Starting Midnight node using just run-node..."
    
    # Remove any existing node container
    $DOCKER_CMD rm -f node 2>/dev/null || true
    
    # Clean up target/data directory for fresh start
    if [ -d "$INDEXER_DIR/target/data" ]; then
        log_info "Cleaning up target/data directory..."
        rm -rf "$INDEXER_DIR/target/data"
    fi
    mkdir -p "$INDEXER_DIR/target/data"
    
    # Use just run-node which is the reliable way
    cd "$INDEXER_DIR"
    if command -v just &> /dev/null; then
        # Run in background using nohup or as a docker command
        DOCKER_CMD="$DOCKER_CMD" just run-node &
        JUST_NODE_PID=$!
        sleep 15  # Give node time to start
        
        # Check if node started successfully
        if ! $DOCKER_CMD ps | grep -q node; then
            log_error "Failed to start node using just run-node"
            log_error "Please check Docker permissions and node configuration"
            exit 1
        fi
        log_info "Midnight node started successfully"
    else
        log_error "just command not found. Please install just or run node manually"
        log_error "To install: cargo install just"
        exit 1
    fi
    cd "$INDEXER_DIR/scripts/pm-18678-investigation"
else
    log_info "Midnight node already running"
fi

# Run migrations
log_info "Running database migrations..."
if ! cargo run --release -p chain-indexer --features cloud -- migrate > "$LOG_DIR/migrations.log" 2>&1; then
    log_error "Migration failed. Check $LOG_DIR/migrations.log for details"
    tail -20 "$LOG_DIR/migrations.log"
    exit 1
fi
log_info "Migrations completed successfully"

# ============================================================================
# SERVICE WRAPPER FUNCTION
# ============================================================================

run_service() {
    local service_name=$1
    local service_cmd=$2
    local log_file="$LOG_DIR/services/${service_name}.log"
    local error_file="$LOG_DIR/services/${service_name}.error.log"
    
    while true; do
        log_info "Starting $service_name..."
        
        # Run service with log splitting
        $service_cmd 2>&1 | while IFS= read -r line; do
            echo "$line" >> "$log_file"
            
            # Check for errors
            if echo "$line" | grep -E "ERROR|PANIC|panic" > /dev/null; then
                echo "$line" >> "$error_file"
            fi
            
            # Check for THE ISSUE
            if echo "$line" | grep "PM-18678 THE ISSUE DETECTED" > /dev/null; then
                log_issue_detected "$line"
            fi
            
            # Check for investigation logs
            if echo "$line" | grep "PM-18678" > /dev/null; then
                echo "$line" >> "$LOG_DIR/monitoring/pm18678.log"
            fi
        done
        
        log_error "$service_name crashed/stopped. Restarting in 10 seconds..."
        sleep 10
    done
}

# ============================================================================
# START SERVICES
# ============================================================================

log_info "Starting indexer services..."

# Chain Indexer
tmux new -d -s chain-indexer "
    export RUST_LOG=chain_indexer=debug,indexer_common=debug,info
    export CONFIG_FILE=chain-indexer/config.yaml
    $(declare -f log_info)
    $(declare -f log_error)
    $(declare -f log_issue_detected)
    $(declare -f capture_diagnostics)
    $(declare -f run_service)
    export LOG_DIR='$LOG_DIR'
    run_service 'chain-indexer' './target/release/chain-indexer'
"

# Wallet Indexer  
tmux new -d -s wallet-indexer "
    export RUST_LOG=wallet_indexer=debug,indexer_common=debug,info
    export CONFIG_FILE=wallet-indexer/config.yaml
    export PM18678_DISABLE_OPTIMIZATION='$PM18678_DISABLE_OPTIMIZATION'
    $(declare -f log_info)
    $(declare -f log_error)
    $(declare -f log_issue_detected)
    $(declare -f capture_diagnostics)
    $(declare -f run_service)
    export LOG_DIR='$LOG_DIR'
    run_service 'wallet-indexer' './target/release/wallet-indexer'
"

# API Replicas (3 instances)
for i in 0 1 2; do
    PORT=$((8080 + i))
    tmux new -d -s "api-$i" "
        export RUST_LOG=indexer_api=debug,indexer_common=debug,info
        export CONFIG_FILE=indexer-api/config.yaml
        export APP__INFRA__API__PORT=$PORT
        $(declare -f log_info)
        $(declare -f log_error)
        $(declare -f log_issue_detected)
        $(declare -f capture_diagnostics)
        $(declare -f run_service)
        export LOG_DIR='$LOG_DIR'
        run_service 'indexer-api-$PORT' './target/release/indexer-api'
    "
done

sleep 30  # Wait for services to start

# ============================================================================
# VERIFY SERVICES ARE READY
# ============================================================================

log_info "Verifying all services are ready..."

# Function to check if API endpoint is ready
check_api_ready() {
    local port=$1
    curl -s -X POST http://localhost:$port/graphql \
        -H "Content-Type: application/json" \
        -d '{"query": "{ __typename }"}' >/dev/null 2>&1
}

# Wait for all API services to be ready
max_attempts=60  # 5 minutes total
attempt=0
all_ready=false

while [ $attempt -lt $max_attempts ]; do
    services_ok=true
    
    # Check each API endpoint
    for port in 8080 8081 8082; do
        if ! check_api_ready $port; then
            services_ok=false
            break
        fi
    done
    
    if [ "$services_ok" = true ]; then
        log_info "All API services are ready!"
        all_ready=true
        break
    fi
    
    attempt=$((attempt + 1))
    if [ $((attempt % 10)) -eq 0 ]; then
        log_info "Waiting for services... (attempt $attempt/$max_attempts)"
    fi
    sleep 5
done

if [ "$all_ready" = false ]; then
    log_error "WARNING: Services not fully ready after $max_attempts attempts"
    log_error "Monitor will start anyway and retry wallet creation"
fi

# Add extra delay for stability
sleep 10

# ============================================================================
# START MONITORING
# ============================================================================

log_info "Starting monitoring script with enhanced retry logic..."

tmux new -d -s monitor "
    cd '$INDEXER_DIR/scripts/pm-18678-investigation'
    export RUST_LOG=pm_18678_investigation=debug,info
    $(declare -f log_info)
    $(declare -f log_error)
    $(declare -f log_issue_detected)
    $(declare -f capture_diagnostics)
    $(declare -f run_service)
    export LOG_DIR='$LOG_DIR'
    
    # Final readiness check in monitor session
    echo 'Checking service readiness from monitor session...'
    for i in 1 2 3 4 5; do
        if curl -s http://localhost:8080/graphql >/dev/null 2>&1; then
            echo 'Services confirmed ready!'
            break
        fi
        echo \"Attempt \$i/5: Services not ready yet, waiting...\"
        sleep 10
    done
    
    echo 'Starting PM-18678 monitor with 30 wallet subscriptions...'
    run_service 'monitor' './target/release/pm18678-monitor \
        --api-endpoints http://localhost:8080,http://localhost:8081,http://localhost:8082 \
        --database-url postgres://indexer:postgres@localhost:5432/indexer \
        --wallet-count $WALLET_COUNT \
        --network-id undeployed'
"

# ============================================================================
# PERIODIC TASKS
# ============================================================================

# Start periodic diagnostics collector
tmux new -d -s diagnostics "
    while true; do
        sleep 3600  # Every hour
        
        # Rotate logs if they get too big
        for log in '$LOG_DIR'/services/*.log; do
            if [ -f \"\$log\" ] && [ \$(stat -c%s \"\$log\" 2>/dev/null || stat -f%z \"\$log\" 2>/dev/null) -gt 1073741824 ]; then
                mv \"\$log\" \"\${log}.$(date +%Y%m%d_%H%M%S)\"
                touch \"\$log\"
            fi
        done
        
        # Capture periodic database stats
        psql -h localhost -U indexer -d indexer -c \"
            SELECT NOW() as timestamp,
                   (SELECT COUNT(*) FROM wallets) as wallets,
                   (SELECT COUNT(*) FROM transactions) as transactions,
                   (SELECT COUNT(*) FROM relevant_transactions) as relevant_transactions
        \" >> '$LOG_DIR/database/stats.log' 2>/dev/null || true
        
        # Check for THE ISSUE in logs
        if grep -q 'returned 0 rows' '$LOG_DIR'/services/*.log 2>/dev/null; then
            echo \"[$(date '+%Y-%m-%d %H:%M:%S')] Empty query detected in logs\" >> '$LOG_DIR/issues/summary.log'
        fi
    done
"

# ============================================================================
# LOG AGGREGATOR
# ============================================================================

# Start log analyzer that continuously scans for patterns
tmux new -d -s analyzer "
    while true; do
        sleep 60  # Every minute
        
        # Count investigation events
        PM_COUNT=\$(grep -c 'PM-18678' '$LOG_DIR'/services/*.log 2>/dev/null || echo 0)
        ISSUE_COUNT=\$(grep -c 'THE ISSUE' '$LOG_DIR'/services/*.log 2>/dev/null || echo 0)
        EMPTY_COUNT=\$(grep -c 'returned 0 rows' '$LOG_DIR'/services/*.log 2>/dev/null || echo 0)
        
        echo \"[$(date '+%Y-%m-%d %H:%M:%S')] PM-18678: \$PM_COUNT | Issues: \$ISSUE_COUNT | Empty: \$EMPTY_COUNT\" \
            >> '$LOG_DIR/monitoring/summary.log'
    done
"

# ============================================================================
# AUTOMATED ANALYSIS
# ============================================================================

# Run analyze-logs.sh periodically to check investigation status
tmux new -d -s auto-analysis "
    cd '$INDEXER_DIR/scripts/pm-18678-investigation'
    while true; do
        # Run analysis every 6 hours
        sleep 21600
        
        echo \"[$(date '+%Y-%m-%d %H:%M:%S')] Running automated analysis...\" >> '$LOG_DIR/monitoring/auto-analysis.log'
        
        # Run the analysis script and capture output
        ./analyze-logs.sh >> '$LOG_DIR/monitoring/auto-analysis.log' 2>&1
        
        # Check if THE ISSUE was detected
        if grep -q 'THE ISSUE™ DETECTED' '$LOG_DIR/monitoring/auto-analysis.log'; then
            echo \"\"
            echo \"============================================================================\"
            echo \"ALERT: THE ISSUE™ HAS BEEN DETECTED!\"
            echo \"Check: $LOG_DIR/monitoring/auto-analysis.log\"
            echo \"============================================================================\"
            echo \"\" >> '$LOG_DIR/issues/ALERT.txt'
            
            # Create a prominent alert file
            echo \"THE ISSUE™ DETECTED at $(date)\" > '$LOG_DIR/THE_ISSUE_DETECTED.txt'
        fi
    done
"

# ============================================================================
# COMPLETION
# ============================================================================

log_info "Investigation setup complete!"
log_info "Services running in tmux sessions:"
tmux ls

echo ""
echo "============================================================================"
echo "INVESTIGATION RUNNING"
echo "============================================================================"
echo "Mode: $TEST_MODE"
echo "Logs: $LOG_DIR"
echo ""
echo "Key log files:"
echo "  - Main: $LOG_DIR/investigation.log"
echo "  - Errors: $LOG_DIR/errors.log"  
echo "  - THE ISSUE: $LOG_DIR/issues/the-issue.log"
echo "  - PM-18678 events: $LOG_DIR/monitoring/pm18678.log"
echo "  - Auto-analysis: $LOG_DIR/monitoring/auto-analysis.log"
echo "  - Service logs: $LOG_DIR/services/*.log"
echo ""
echo "Automated analysis runs every 6 hours. Check:"
echo "  tail -f $LOG_DIR/monitoring/auto-analysis.log"
echo ""
echo "Monitor with:"
echo "  tail -f $LOG_DIR/issues/the-issue.log    # Watch for THE ISSUE"
echo "  tail -f $LOG_DIR/monitoring/summary.log  # Watch summary"
echo "  tmux attach -t monitor                   # Attach to monitor"
echo "  tmux attach -t auto-analysis             # Watch automated analysis"
echo ""
echo "Stop with: tmux kill-server"
echo "============================================================================"