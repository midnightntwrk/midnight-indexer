#!/bin/bash

# Development entrypoint script that includes mock data loading
# This is a temporary solution until node and ledger produce new dependencies

set -e

trap 'rm -f /var/run/indexer-standalone/running' EXIT
trap 'kill -SIGINT $PID' INT
trap 'kill -SIGTERM $PID' TERM

echo "Starting indexer-standalone with development mock data..."

# Start indexer-standalone in background to let migrations run
touch /var/run/indexer-standalone/running
indexer-standalone &
PID=$!

# Wait for the service to be ready and migrations to complete
echo "Waiting for indexer-standalone to complete migrations..."
# SQLite migrations are fast, but let's ensure the database is created
sleep 5

# Check if the process is still running
if ! kill -0 $PID 2>/dev/null; then
    echo "ERROR: indexer-standalone failed to start"
    echo "Trying to start indexer-standalone directly to see the error..."
    indexer-standalone
    exit 1
fi

# Check if mock data should be loaded (only if tables are empty)
if [ -f "/opt/mock-data/check-and-load-mock-data.sh" ]; then
    echo "Checking if mock data needs to be loaded..."
    /opt/mock-data/check-and-load-mock-data.sh
else
    echo "Mock data scripts not found, skipping..."
fi

# Check if the process is still running after mock data load
if ! kill -0 $PID 2>/dev/null; then
    echo "ERROR: indexer-standalone exited during mock data loading"
    exit 1
fi

echo "Service is running, entering wait loop..."
# Wait for the main process
wait $PID