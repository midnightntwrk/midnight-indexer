m# PM-18678: THE ISSUE™ Investigation

## Overview

This investigation tracks down a critical production bug where wallet subscriptions stop receiving `ViewingUpdate` events while `ProgressUpdate` events continue normally. The issue historically appeared 1-2 weeks after deployment and was temporarily resolved by PR #42's optimization, though the root cause remains unknown.

## Quick Start (EC2)

### Option 1: Fresh Setup (No repo cloned)
```bash
# SSH into EC2 instance
AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... \
  aws ssm start-session --target i-05f50sdfsdfsdb2 --region eu-central-1

# Download and run the launch script (it will clone the repo and set everything up)
curl -O https://raw.githubusercontent.com/input-output-hk/midnight-indexer/investigation/PM-18678-hanging-root-cause/scripts/pm-18678-investigation/launch-ec2-investigation.sh
chmod +x launch-ec2-investigation.sh
./launch-ec2-investigation.sh reproduce
```

### Option 2: Repo Already Cloned
```bash
# Navigate to the investigation scripts directory
cd ~/midnight-investigation/midnight-indexer/scripts/pm-18678-investigation

# Run the investigation directly
./run-investigation.sh reproduce  # or 'control' for PR #42 enabled
```

The `run-investigation.sh` script will:
1. Start Docker containers (PostgreSQL, NATS)
2. Build all services with `--features cloud`
3. Launch services in tmux sessions (chain-indexer, wallet-indexer, indexer-api)
4. Start the monitoring tool that creates 30+ wallet subscriptions
5. Run automated analysis every 6 hours (runs `analyze-logs.sh` automatically)
6. Run continuously for 3-4 weeks, monitoring for THE ISSUE™
7. Create alert files when THE ISSUE™ is detected

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