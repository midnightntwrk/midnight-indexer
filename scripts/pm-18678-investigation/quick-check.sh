#!/bin/bash
# Quick check script for PM-18678 investigation status

echo "============================================================================"
echo "PM-18678 QUICK STATUS CHECK"
echo "============================================================================"
echo ""

# Find the latest log directory
LOG_DIR=$(ls -td "$HOME/midnight-investigation/logs"/*/ 2>/dev/null | head -1)

if [ -z "$LOG_DIR" ]; then
    echo "No investigation logs found. Is the investigation running?"
    exit 1
fi

echo "Checking logs in: $LOG_DIR"
echo ""

# Check if THE ISSUE has been detected
if [ -f "$LOG_DIR/THE_ISSUE_DETECTED.txt" ]; then
    echo "🚨 THE ISSUE™ HAS BEEN DETECTED!"
    cat "$LOG_DIR/THE_ISSUE_DETECTED.txt"
    echo ""
fi

# Show recent auto-analysis
if [ -f "$LOG_DIR/monitoring/auto-analysis.log" ]; then
    echo "Latest automated analysis:"
    tail -5 "$LOG_DIR/monitoring/auto-analysis.log"
    echo ""
fi

# Show current statistics
if [ -f "$LOG_DIR/monitoring/summary.log" ]; then
    echo "Current statistics:"
    tail -1 "$LOG_DIR/monitoring/summary.log"
    echo ""
fi

# Check tmux sessions
echo "Active tmux sessions:"
tmux ls 2>/dev/null || echo "  No tmux sessions running"
echo ""

# Run full analysis
echo "Running full analysis..."
cd "$(dirname "$0")"
./analyze-logs.sh

echo ""
echo "============================================================================"
echo "For detailed monitoring:"
echo "  tail -f $LOG_DIR/monitoring/auto-analysis.log"
echo "  tmux attach -t monitor"
echo "============================================================================"