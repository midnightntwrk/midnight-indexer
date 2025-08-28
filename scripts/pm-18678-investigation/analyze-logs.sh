#!/bin/bash
# PM-18678 Log Analysis Script
# Analyzes logs to identify patterns and THE ISSUE™

LOG_DIR="${1:-$HOME/midnight-investigation/logs}"

if [ ! -d "$LOG_DIR" ]; then
    echo "Error: Log directory not found: $LOG_DIR"
    echo "Usage: $0 [log_directory]"
    exit 1
fi

echo "============================================================================"
echo "PM-18678 LOG ANALYSIS"
echo "Log Directory: $LOG_DIR"
echo "============================================================================"
echo ""

# Find the most recent log directory if multiple exist
if [ -d "$LOG_DIR" ] && ls -d "$LOG_DIR"/2* >/dev/null 2>&1; then
    LATEST_LOG=$(ls -dt "$LOG_DIR"/2* | head -1)
    echo "Analyzing: $LATEST_LOG"
    LOG_DIR="$LATEST_LOG"
fi

echo ""
echo "=== THE ISSUE™ DETECTION ==="
echo "----------------------------"
if [ -f "$LOG_DIR/issues/the-issue.log" ]; then
    ISSUE_COUNT=$(wc -l < "$LOG_DIR/issues/the-issue.log")
    echo "THE ISSUE detected $ISSUE_COUNT times"
    echo ""
    echo "First occurrence:"
    head -1 "$LOG_DIR/issues/the-issue.log"
    echo ""
    echo "Last occurrence:"
    tail -1 "$LOG_DIR/issues/the-issue.log"
else
    echo "THE ISSUE not detected yet"
fi

echo ""
echo "=== EMPTY QUERY RESULTS ==="
echo "---------------------------"
EMPTY_QUERIES=$(grep -h "returned 0 rows" "$LOG_DIR"/services/*.log 2>/dev/null | wc -l)
echo "Empty query results: $EMPTY_QUERIES"
if [ "$EMPTY_QUERIES" -gt 0 ]; then
    echo ""
    echo "Sessions affected:"
    grep -h "returned 0 rows" "$LOG_DIR"/services/*.log 2>/dev/null | \
        grep -oE "session_id: [a-f0-9]+" | sort -u | head -5
fi

echo ""
echo "=== VIEWING UPDATE TRACKING ==="
echo "--------------------------------"
VIEWING_UPDATES=$(grep -h "Sending ViewingUpdate" "$LOG_DIR"/services/*.log 2>/dev/null | wc -l)
LIVE_VIEWING=$(grep -h "Sending live ViewingUpdate" "$LOG_DIR"/services/*.log 2>/dev/null | wc -l)
echo "Total ViewingUpdates sent: $VIEWING_UPDATES"
echo "Live ViewingUpdates: $LIVE_VIEWING"
echo "Initial ViewingUpdates: $((VIEWING_UPDATES - LIVE_VIEWING))"

echo ""
echo "=== PROGRESS UPDATE TRACKING ==="
echo "---------------------------------"
PROGRESS_UPDATES=$(grep -h "Sending ProgressUpdate" "$LOG_DIR"/services/*.log 2>/dev/null | wc -l)
echo "Total ProgressUpdates sent: $PROGRESS_UPDATES"

echo ""
echo "=== PR #42 OPTIMIZATION STATUS ==="
echo "-----------------------------------"
OPTIMIZATION_DISABLED=$(grep -h "PR #42 optimization DISABLED" "$LOG_DIR"/services/*.log 2>/dev/null | head -1)
if [ -n "$OPTIMIZATION_DISABLED" ]; then
    echo "PR #42 optimization: DISABLED (reproducing issue)"
    DISABLED_COUNT=$(grep -c "PR #42 optimization DISABLED" "$LOG_DIR"/services/*.log 2>/dev/null || echo 0)
    echo "Optimization bypass count: $DISABLED_COUNT"
else
    echo "PR #42 optimization: ENABLED (control test)"
fi

echo ""
echo "=== ERROR SUMMARY ==="
echo "---------------------"
if [ -f "$LOG_DIR/errors.log" ]; then
    ERROR_COUNT=$(wc -l < "$LOG_DIR/errors.log")
    echo "Total errors: $ERROR_COUNT"
    if [ "$ERROR_COUNT" -gt 0 ]; then
        echo ""
        echo "Error types:"
        grep -oE "ERROR.*?:" "$LOG_DIR/errors.log" | sort | uniq -c | sort -rn | head -5
    fi
else
    echo "No errors logged"
fi

echo ""
echo "=== SERVICE HEALTH ==="
echo "----------------------"
for service in chain-indexer wallet-indexer indexer-api-8080 indexer-api-8081 indexer-api-8082 monitor; do
    if [ -f "$LOG_DIR/services/${service}.log" ]; then
        SIZE=$(du -h "$LOG_DIR/services/${service}.log" | cut -f1)
        LINES=$(wc -l < "$LOG_DIR/services/${service}.log")
        LAST_LOG=$(tail -1 "$LOG_DIR/services/${service}.log" | grep -oE '\[.*?\]' | head -1)
        echo "$service: $LINES lines ($SIZE) - Last: $LAST_LOG"
        
        # Check for recent activity
        if [ -n "$LAST_LOG" ]; then
            LAST_TIMESTAMP=$(echo "$LAST_LOG" | tr -d '[]')
            if [ -n "$LAST_TIMESTAMP" ]; then
                LAST_EPOCH=$(date -j -f "%Y-%m-%d %H:%M:%S" "$LAST_TIMESTAMP" +%s 2>/dev/null || date -d "$LAST_TIMESTAMP" +%s 2>/dev/null || echo 0)
                NOW_EPOCH=$(date +%s)
                if [ "$LAST_EPOCH" -gt 0 ]; then
                    AGE=$((NOW_EPOCH - LAST_EPOCH))
                    if [ "$AGE" -gt 300 ]; then
                        echo "  WARNING: No activity for $((AGE/60)) minutes"
                    fi
                fi
            fi
        fi
    else
        echo "$service: NOT RUNNING or no logs"
    fi
done

echo ""
echo "=== DATABASE STATISTICS ==="
echo "---------------------------"
if [ -f "$LOG_DIR/database/stats.log" ]; then
    echo "Latest database state:"
    tail -1 "$LOG_DIR/database/stats.log"
else
    echo "No database statistics available"
fi

echo ""
echo "=== REPLICA DIVERGENCE ==="
echo "--------------------------"
# Check if different replicas show different behavior
for port in 8080 8081 8082; do
    if [ -f "$LOG_DIR/services/indexer-api-$port.log" ]; then
        REPLICA_EMPTY=$(grep -c "returned 0 rows" "$LOG_DIR/services/indexer-api-$port.log" 2>/dev/null || echo 0)
        REPLICA_VIEWING=$(grep -c "Sending ViewingUpdate" "$LOG_DIR/services/indexer-api-$port.log" 2>/dev/null || echo 0)
        echo "Replica $port: Empty queries=$REPLICA_EMPTY, ViewingUpdates=$REPLICA_VIEWING"
    fi
done

echo ""
echo "=== INVESTIGATION TIMELINE ==="
echo "------------------------------"
if [ -f "$LOG_DIR/monitoring/summary.log" ]; then
    echo "Recent activity (last 5 entries):"
    tail -5 "$LOG_DIR/monitoring/summary.log"
else
    echo "No timeline data available"
fi

echo ""
echo "=== RECOMMENDATIONS ==="
echo "-----------------------"
if [ "$ISSUE_COUNT" -gt 0 ]; then
    echo "⚠️  THE ISSUE™ has been detected!"
    echo "   - Check $LOG_DIR/issues/ for diagnostic captures"
    echo "   - Review replica-specific logs to identify which failed first"
    echo "   - Examine database connection states at time of failure"
elif [ "$EMPTY_QUERIES" -gt 0 ]; then
    echo "⚠️  Empty queries detected but not confirmed as THE ISSUE™"
    echo "   - Monitor if ViewingUpdates continue despite empty queries"
    echo "   - Check if this is affecting specific sessions only"
else
    echo "✓ System appears to be running normally"
    echo "   - Continue monitoring for 3-4 weeks"
    echo "   - Check logs daily for any anomalies"
fi

echo ""
echo "============================================================================"
echo "Run this script periodically to track investigation progress"
echo "For real-time monitoring: tail -f $LOG_DIR/issues/the-issue.log"
echo "============================================================================"