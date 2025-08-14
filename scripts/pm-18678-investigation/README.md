m# PM-18678: THE ISSUE™ Investigation

## Overview

This investigation tracks down a critical production bug where wallet subscriptions stop receiving `ViewingUpdate` events while `ProgressUpdate` events continue normally. The issue historically appeared 1-2 weeks after deployment and was temporarily resolved by PR #42's optimization, though the root cause remains unknown.

## Quick Start (EC2)

### Option 1: Fresh Setup (No repo cloned)
```bash
# SSH into EC2 instance
AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... \
  aws ssm start-session --target i-05f50sdfsdfsdb2 --region eu-central-1

# Start a better shell environment (SSM has limited terminal capabilities)
# Option 1: Use screen for scrollback buffer
screen -S main
# Screen commands:
# - Detach (keep running): Ctrl+A then D
# - Scroll mode: Ctrl+A then ESC, use arrows/PgUp/PgDn, press ESC to exit
# - Kill current session (while inside): Ctrl+A then K (confirm with 'y')
# - Or simply type: exit
# - To stop screen session from inside: Ctrl+D or type 'exit'

# Option 2: Use tmux for better terminal
tmux new -s main
# Scroll in tmux: Ctrl+B then [, use arrows/PgUp/PgDn, press q to exit scroll mode

# Option 3: Just use bash (no scrollback)
bash

# Navigate to home directory (SSM starts in /var/snap/amazon-ssm-agent/11797)
cd ~  # Goes to /home/ssm-user

# Set GitHub token for private dependencies
# IMPORTANT: Token must have 'read:packages' scope to pull Docker images
# Create token at: https://github.com/settings/tokens with 'read:packages' permission
export GITHUB_TOKEN=your_github_personal_access_token

# Authenticate Docker with GitHub Container Registry
echo $GITHUB_TOKEN | sudo docker login ghcr.io -u YOUR_GITHUB_USERNAME --password-stdin

# Download the launch script
curl -O https://raw.githubusercontent.com/midnightntwrk/midnight-indexer/investigation/PM-18678-hanging-root-cause/scripts/pm-18678-investigation/launch-ec2-investigation.sh
chmod +x launch-ec2-investigation.sh

# Run in screen to prevent SSM timeout (first build takes 10-20 minutes)
screen -S pm18678
./launch-ec2-investigation.sh reproduce
# Press Ctrl+A then D to detach from screen (keeps it running)
# To reattach: screen -r pm18678
# To kill the screen session: Press Ctrl+A then K (or exit the shell)
```

### Reconnecting After Disconnect

If your SSM session times out or disconnects:

```bash
# SSH back into EC2
AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... \
  aws ssm start-session --target i-05f50sdfsdfsdb2 --region eu-central-1

# Start bash shell
bash

# Navigate to home
cd ~

# Check running screen sessions
screen -ls

# Reattach to the pm18678 session
screen -r pm18678

# If screen is attached elsewhere, force reattach
screen -d -r pm18678
```

### Option 2: Repo Already Cloned
```bash
# Navigate to the investigation scripts directory
cd ~/midnight-investigation/midnight-indexer/scripts/pm-18678-investigation

# Run the investigation directly
./run-investigation.sh reproduce  # or 'control' for PR #42 enabled
```

The `run-investigation.sh` script will:
1. Start Docker containers (PostgreSQL, NATS, Midnight Node)
2. Build all services with `--features cloud`
3. Launch services in tmux sessions (chain-indexer, wallet-indexer, indexer-api)
4. Start the monitoring tool that creates 30+ wallet subscriptions
5. Run automated analysis every 6 hours (runs `analyze-logs.sh` automatically)
6. Run continuously for 3-4 weeks, monitoring for THE ISSUE™
7. Create alert files when THE ISSUE™ is detected

**Note**: The script attempts to start a local Midnight devnet node from GitHub Container Registry. 
- Requires GitHub token with `read:packages` scope and Docker authentication (see setup above)
- If you have access issues with the Docker image, you can:
  - Use an existing node by setting `APP__INFRA__NODE__URL=ws://your-node:9944`
  - Or manually start a node before running the investigation

## Investigation Status

- **Start Date**: August 7, 2025 (scheduled)
- **Current Status**: Not yet started
- **Critical Window**: Days 7-14 (historically when issue appears)
- **Issue Reproduced**: TBD

## Code Modifications Added

### 1. ViewingUpdate Detection (`indexer-api/src/infra/api/v1/subscription/shielded.rs`)
- Tracks transaction count for each query
- Logs when `get_relevant_transactions` returns 0 rows
- Identifies which replica experiences the issue
- Measures query duration

### 2. Query Instrumentation (`indexer-api/src/infra/storage/transaction.rs`)
- Unique query ID for tracking
- Detailed logging when empty results detected
- Automatic database state diagnostics
- Connection pool size monitoring

### 3. PR #42 Toggle (`wallet-indexer/src/application.rs`)
- Environment variable `PM18678_DISABLE_OPTIMIZATION=true` to disable optimization
- Allows testing with/without the fix that prevented the issue

### 4. Progress Update Tracking
- Logs all `ProgressUpdate` events
- Helps identify when ViewingUpdates stop but Progress continues

## Build Options

### Standard Build (PR #42 Optimization Enabled)
```bash
cargo build --release --features cloud
```

### Reproduction Build (PR #42 Optimization Disabled)
```bash
PM18678_DISABLE_OPTIMIZATION=true cargo build --release --features cloud
```

Or set the environment variable when running:
```bash
PM18678_DISABLE_OPTIMIZATION=true cargo run --release -p wallet-indexer --features cloud
```

## Automated Scripts

### Main Scripts
- `launch-ec2-investigation.sh` - One-command setup and launch for EC2
- `run-investigation.sh` - Main orchestrator that runs all services
- `analyze-logs.sh` - Log analysis and issue detection

### Using `just` Commands (Preferred)
```bash
just pm18678-build         # Build monitoring tool
just pm18678-run reproduce  # Run investigation
just pm18678-status        # Check status
just pm18678-analyze       # Analyze logs
just pm18678-stop          # Stop investigation
just pm18678-reset         # Reset environment (preserves logs)
just pm18678-purge         # Full cleanup including logs
```

## Managing the Investigation

### Check Running Services

```bash
# Check all tmux sessions (should show multiple services)
tmux ls

# Check Docker containers (use sudo if you get permission denied)
sudo docker ps
# Or: newgrp docker && docker ps

# Check specific service logs
tmux attach -t chain-indexer  # Attach to chain-indexer
tmux attach -t wallet-indexer # Attach to wallet-indexer
tmux attach -t monitor        # Attach to monitoring tool

# To detach from tmux (leave it running):
# Press Ctrl+B then D

# To stop/kill the specific tmux session:
# Press Ctrl+C to stop the process, then type 'exit'

# Quick status check
cd ~/midnight-investigation/midnight-indexer/scripts/pm-18678-investigation
./quick-check.sh
```

### Stop the Investigation

```bash
# One command to stop everything (tmux, screen, and docker)
tmux kill-server 2>/dev/null; pkill screen 2>/dev/null; sudo docker stop postgres nats midnight-node 2>/dev/null; sudo docker rm postgres nats midnight-node 2>/dev/null

# Or step by step:
tmux kill-server                        # Stop all tmux sessions
screen -X -S pm18678 quit               # Stop specific screen session
pkill screen                            # Kill ALL screen sessions (simple)
# OR: screen -ls | grep -o '[0-9]*\..*' | cut -d. -f1 | xargs -I{} screen -X -S {} quit  # Stop ALL screen sessions (proper)
sudo docker stop postgres nats midnight-node    # Stop Docker containers
sudo docker rm postgres nats midnight-node      # Remove Docker containers

# Or use just command (if in repo directory)
cd ~/midnight-investigation/midnight-indexer
just pm18678-stop
```

### Restart After Stopping

```bash
cd ~/midnight-investigation/midnight-indexer/scripts/pm-18678-investigation
./run-investigation.sh reproduce  # or 'control'
```

### Check Investigation Logs

```bash
# Find latest log directory
LATEST_LOG=$(ls -td ~/midnight-investigation/logs/*/ | head -1)

# OPTION 1: Use 'less' for scrollable viewing (recommended)
less +F $LATEST_LOG/investigation.log         # Press Ctrl+C to stop following, 'F' to resume
less $LATEST_LOG/errors.log                   # Use arrows/PgUp/PgDn to scroll, 'q' to quit
less $LATEST_LOG/issues/the-issue.log         # '/' to search, 'n' for next match

# OPTION 2: Use 'tail' for live following (no scrolling)
tail -f $LATEST_LOG/investigation.log         # Live updates but can't scroll
tail -f $LATEST_LOG/errors.log
tail -f $LATEST_LOG/issues/the-issue.log
tail -f $LATEST_LOG/build.log
tail -f $LATEST_LOG/services/chain-indexer.log
tail -f $LATEST_LOG/services/wallet-indexer.log
tail -f $LATEST_LOG/services/indexer-api-8080.log
tail -f $LATEST_LOG/services/indexer-api-8081.log
tail -f $LATEST_LOG/services/indexer-api-8082.log
tail -f $LATEST_LOG/services/monitor.log
tail -f $LATEST_LOG/monitoring/auto-analysis.log

# Check service logs (use less for scrollable viewing)
less +F $LATEST_LOG/services/chain-indexer.log
less +F $LATEST_LOG/services/wallet-indexer.log
less +F $LATEST_LOG/services/indexer-api-8080.log  # API instance 1
less +F $LATEST_LOG/services/indexer-api-8081.log  # API instance 2
less +F $LATEST_LOG/services/indexer-api-8082.log  # API instance 3
less +F $LATEST_LOG/services/monitor.log

# Check automated analysis results (runs every 6 hours)
less $LATEST_LOG/monitoring/auto-analysis.log

# View PM-18678 specific events
grep "PM-18678" $LATEST_LOG/services/*.log | less

# View last 100 lines of a log
tail -100 $LATEST_LOG/investigation.log | less

# Quick summary of all logs
ls -la $LATEST_LOG/
```

#### Less Commands Quick Reference
- `less +F file.log` - Start in follow mode (like tail -f)
- `Ctrl+C` - Stop following to scroll
- `Shift+F` - Resume following 
- `↑↓` or `j/k` - Scroll line by line
- `PgUp/PgDn` or `Space/b` - Page up/down
- `G` - Go to end of file
- `g` - Go to beginning
- `/pattern` - Search forward
- `?pattern` - Search backward
- `n/N` - Next/previous search match
- `q` - Quit

## Monitoring Script

The Rust monitoring script (`pm18678-monitor`) provides:
- Multiple wallet session management
- Replica comparison
- Database connection monitoring
- Automatic issue detection
- Diagnostic capture on first occurrence

## Automated Analysis

The `run-investigation.sh` script runs automated analysis every 6 hours:
- Executes `analyze-logs.sh` automatically in a tmux session
- Logs results to `~/midnight-investigation/logs/*/monitoring/auto-analysis.log`
- Creates alert files when THE ISSUE™ is detected
- Runs in tmux session `auto-analysis` (attach with `tmux attach -t auto-analysis`)

Manual analysis can still be run anytime:
```bash
cd ~/midnight-investigation/midnight-indexer/scripts/pm-18678-investigation
./analyze-logs.sh
```

### Running the Monitor
```bash
cd scripts/pm-18678-investigation
cargo run --release -- \
  --api-endpoints "http://localhost:8080,http://localhost:8081,http://localhost:8082" \
  --database-url "postgres://indexer:postgres@localhost:5432/indexer" \
  --wallet-count 30 \
  --check-interval 60 \
  --db-check-interval 120
```

## Log Analysis

### Find Investigation Logs
```bash
grep "PM-18678" *.log | sort -k2
```

### Find THE ISSUE™ Occurrences
```bash
grep "THE ISSUE" *.log
```

### Find Empty Query Results
```bash
grep "returned 0 rows" *.log
```

### Compare Replica Behaviors
```bash
for port in 8080 8081 8082; do
    echo "=== Replica on port $port ==="
    grep "port=$port" *.log | grep "ViewingUpdate"
done
```

## Database Monitoring Queries

### Check Connection State
```sql
SELECT pid, application_name, backend_start, state, 
       backend_xmin::text as xmin,
       EXTRACT(EPOCH FROM (NOW() - backend_start)) as connection_age_seconds
FROM pg_stat_activity 
WHERE datname = 'indexer'
ORDER BY backend_start;
```

### Find Long-Running Transactions
```sql
SELECT pid, NOW() - xact_start as transaction_duration, query
FROM pg_stat_activity 
WHERE datname = 'indexer' 
  AND xact_start IS NOT NULL
  AND NOW() - xact_start > interval '5 minutes';
```

### Check Wallet State
```sql
SELECT w.id, w.session_id, w.last_indexed_transaction_id,
       COUNT(rt.id) as relevant_count
FROM wallets w
LEFT JOIN relevant_transactions rt ON rt.wallet_id = w.id
WHERE w.session_id = $1
GROUP BY w.id, w.session_id, w.last_indexed_transaction_id;
```

## Test Timeline

### Week 1 (Days 1-7): Baseline
- 20-30 wallet sessions
- Establish normal metrics
- Document baseline behavior

### Week 2 (Days 8-14): Critical Window
- 75 wallet sessions
- Historical issue manifestation period
- Test with one replica using old pool settings

### Week 3 (Days 15-21): Stress Testing
- 150+ wallet sessions
- Force edge cases
- Test connection pool limits

### Week 4 (Days 22-28): Analysis
- Document findings
- Make go/no-go recommendation

## Success Criteria

### If Issue Reproduced
- Exact conditions captured
- Replica that failed first identified
- Database state at failure time documented
- Can reliably trigger the issue

### If Issue Not Reproduced
- Ran for full 3-4 weeks
- Tested up to 200+ concurrent wallets
- Stressed beyond production loads
- Can confidently say issue is resolved

## Key Findings So Far

1. **PR #42 Impact**: Reducing database queries by checking `last_indexed_transaction_id < max_transaction_id` prevented the issue
2. **Connection Pool**: Aggressive recycling (5min max_lifetime, 1min idle_timeout) also helps
3. **Pod-Specific**: Issue historically affected specific pods/replicas differently
4. **Query Behavior**: `get_relevant_transactions` returns 0 rows when it shouldn't

## Hypothesis

The issue appears related to PostgreSQL connection state, possibly:
- MVCC snapshot staleness
- Connection pool exhaustion
- Transaction isolation anomalies
- Long-running transactions blocking visibility

## Files Modified

- `indexer-api/src/infra/api/v1/subscription/shielded.rs` - ViewingUpdate tracking
- `indexer-api/src/infra/storage/transaction.rs` - Query instrumentation
- `wallet-indexer/src/application.rs` - PR #42 toggle (environment variable controlled)

## Contact

- **Lead**: Sean Kwak (Heiko supervising)
- **JIRA**: PM-18678
- **Slack**: #midnight-indexer-team

## Notes

- Test scheduled to start August 7, 2025 on EC2
- Will run for 3-4 weeks unattended
- Testing both with and without PR #42 optimization
- Focus on empirical evidence over theories