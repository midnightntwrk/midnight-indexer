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
cargo build --release --features cloud > "$LOG_DIR/build.log" 2>&1

# Build monitoring script
cd "$INDEXER_DIR/scripts/pm-18678-investigation"
cargo build --release >> "$LOG_DIR/build.log" 2>&1
cd "$INDEXER_DIR"

# ============================================================================
# START INFRASTRUCTURE
# ============================================================================

log_info "Starting infrastructure services..."

# PostgreSQL
if ! docker ps | grep -q postgres; then
    docker run -d --name postgres \
        -e POSTGRES_USER=indexer \
        -e POSTGRES_PASSWORD=postgres \
        -e POSTGRES_DB=indexer \
        -p 5432:5432 \
        postgres:15
    sleep 10
fi

# NATS
if ! docker ps | grep -q nats; then
    docker run -d --name nats \
        -p 4222:4222 \
        nats:latest -js --user indexer --pass nats
    sleep 5
fi

# Run migrations
log_info "Running database migrations..."
cargo run -p chain-indexer -- migrate > "$LOG_DIR/migrations.log" 2>&1

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
    run_service 'chain-indexer' 'cargo run --release -p chain-indexer --features cloud'
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
    run_service 'wallet-indexer' 'cargo run --release -p wallet-indexer --features cloud'
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
        run_service 'indexer-api-$PORT' 'cargo run --release -p indexer-api --bin indexer-api --features cloud'
    "
done

sleep 30  # Wait for services to start

# ============================================================================
# START MONITORING
# ============================================================================

log_info "Starting monitoring script..."

tmux new -d -s monitor "
    cd '$INDEXER_DIR/scripts/pm-18678-investigation'
    export RUST_LOG=pm_18678_investigation=debug,info
    $(declare -f log_info)
    $(declare -f log_error)
    $(declare -f log_issue_detected)
    $(declare -f capture_diagnostics)
    $(declare -f run_service)
    export LOG_DIR='$LOG_DIR'
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
echo "  - Service logs: $LOG_DIR/services/*.log"
echo ""
echo "Monitor with:"
echo "  tail -f $LOG_DIR/issues/the-issue.log    # Watch for THE ISSUE"
echo "  tail -f $LOG_DIR/monitoring/summary.log  # Watch summary"
echo "  tmux attach -t monitor                   # Attach to monitor"
echo ""
echo "Stop with: tmux kill-server"
echo "============================================================================"