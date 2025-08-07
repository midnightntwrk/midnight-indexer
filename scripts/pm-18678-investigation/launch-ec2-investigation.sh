#!/bin/bash
# PM-18678 EC2 Investigation Launcher
# One-command setup and launch for the investigation

set -e

echo "============================================================================"
echo "PM-18678 EC2 INVESTIGATION LAUNCHER"
echo "============================================================================"

# Configuration
INVESTIGATION_DIR="$HOME/midnight-investigation"
REPO_URL="https://github.com/midnightntwrk/midnight-indexer.git"
BRANCH="investigation/PM-18678-hanging-root-cause"
COMMIT_HASH="9b8cead"

# Parse arguments
MODE="${1:-reproduce}"  # Default to reproduce mode
WALLET_COUNT="${2:-30}"

if [ "$MODE" != "reproduce" ] && [ "$MODE" != "control" ]; then
    echo "Usage: $0 [reproduce|control] [wallet_count]"
    echo "  reproduce - Disable PR #42 optimization (default)"
    echo "  control   - Enable PR #42 optimization"
    exit 1
fi

echo ""
echo "Configuration:"
echo "  Mode: $MODE"
echo "  Wallets: $WALLET_COUNT"
echo "  Directory: $INVESTIGATION_DIR"
echo ""

# ============================================================================
# SETUP REPOSITORY
# ============================================================================

if [ ! -d "$INVESTIGATION_DIR/midnight-indexer" ]; then
    echo "Setting up repository..."
    mkdir -p "$INVESTIGATION_DIR"
    cd "$INVESTIGATION_DIR"
    
    git clone "$REPO_URL"
    cd midnight-indexer
    git checkout "$BRANCH"
    
    # Verify we have the investigation commit
    if ! git log --oneline | grep -q "$COMMIT_HASH"; then
        echo "Warning: Investigation commit $COMMIT_HASH not found"
        echo "Current HEAD: $(git rev-parse HEAD)"
    fi
else
    echo "Repository already exists. Updating..."
    cd "$INVESTIGATION_DIR/midnight-indexer"
    git fetch
    git checkout "$BRANCH"
    git pull
fi

# ============================================================================
# INSTALL DEPENDENCIES
# ============================================================================

echo "Checking dependencies..."

# Check for Rust
if ! command -v cargo &> /dev/null; then
    echo "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

# Check for Docker
if ! command -v docker &> /dev/null; then
    echo "Error: Docker is required but not installed"
    echo "Install Docker and try again"
    exit 1
fi

# Check for tmux
if ! command -v tmux &> /dev/null; then
    echo "Installing tmux..."
    if command -v apt-get &> /dev/null; then
        sudo apt-get update && sudo apt-get install -y tmux
    elif command -v yum &> /dev/null; then
        sudo yum install -y tmux
    else
        echo "Error: Cannot install tmux. Please install manually"
        exit 1
    fi
fi

# Check for PostgreSQL client
if ! command -v psql &> /dev/null; then
    echo "Installing PostgreSQL client..."
    if command -v apt-get &> /dev/null; then
        sudo apt-get update && sudo apt-get install -y postgresql-client
    elif command -v yum &> /dev/null; then
        sudo yum install -y postgresql
    fi
fi

# ============================================================================
# STOP ANY EXISTING INVESTIGATION
# ============================================================================

echo "Checking for existing investigation..."
if tmux ls 2>/dev/null | grep -q .; then
    echo "Stopping existing tmux sessions..."
    tmux kill-server 2>/dev/null || true
    sleep 2
fi

# Stop any existing Docker containers
echo "Cleaning up Docker containers..."
docker stop postgres nats 2>/dev/null || true
docker rm postgres nats 2>/dev/null || true

# ============================================================================
# LAUNCH INVESTIGATION
# ============================================================================

cd "$INVESTIGATION_DIR/midnight-indexer/scripts/pm-18678-investigation"

# Make scripts executable
chmod +x *.sh

echo ""
echo "Launching investigation in $MODE mode..."
echo ""

# Run the main investigation script
./run-investigation.sh "$MODE"

# ============================================================================
# POST-LAUNCH
# ============================================================================

echo ""
echo "============================================================================"
echo "INVESTIGATION LAUNCHED SUCCESSFULLY"
echo "============================================================================"
echo ""
echo "The investigation is now running in the background."
echo ""
echo "Monitor progress with:"
echo "  cd $INVESTIGATION_DIR/midnight-indexer/scripts/pm-18678-investigation"
echo "  ./analyze-logs.sh"
echo ""
echo "View live logs:"
echo "  tail -f $INVESTIGATION_DIR/logs/*/issues/the-issue.log"
echo ""
echo "Attach to services:"
echo "  tmux ls          # List all sessions"
echo "  tmux attach -t monitor  # Attach to monitoring"
echo ""
echo "Stop investigation:"
echo "  tmux kill-server"
echo ""
echo "============================================================================"

# Create convenience script
cat > "$INVESTIGATION_DIR/check-investigation.sh" << 'EOF'
#!/bin/bash
cd "$HOME/midnight-investigation/midnight-indexer/scripts/pm-18678-investigation"
./analyze-logs.sh
EOF
chmod +x "$INVESTIGATION_DIR/check-investigation.sh"

echo "Quick check script created: $INVESTIGATION_DIR/check-investigation.sh"
echo "============================================================================"