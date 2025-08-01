#!/bin/bash

# Development entrypoint script that includes mock data loading
# This is a temporary solution until node and ledger produce new dependencies

set -e

trap 'rm -f /var/run/indexer-api/running' EXIT
trap 'kill -SIGINT $PID' INT
trap 'kill -SIGTERM $PID' TERM

echo "Starting indexer-api with development mock data..."

# Start indexer-api in background to let migrations run
touch /var/run/indexer-api/running
indexer-api &
PID=$!

# Check if the process started successfully
sleep 2
if ! kill -0 $PID 2>/dev/null; then
    echo "ERROR: indexer-api failed to start"
    echo "Trying to start indexer-api directly to see the error..."
    indexer-api
    exit 1
fi

# Wait for the service to be ready and migrations to complete
echo "Waiting for indexer-api to complete migrations..."
# Wait until the API is responding
for i in {1..30}; do
    if curl -s http://localhost:8088/health > /dev/null 2>&1; then
        echo "Service is ready after $i seconds"
        break
    fi
    echo "Waiting for service to start... ($i/30)"
    sleep 1
done

# Check if mock data should be loaded (only if tables are empty)
if [ -f "/opt/mock-data/check-and-load-mock-data.sh" ]; then
    echo "Checking if mock data needs to be loaded..."
    /opt/mock-data/check-and-load-mock-data.sh
else
    echo "Mock data scripts not found, skipping..."
fi

# Wait for the main process
wait $PID