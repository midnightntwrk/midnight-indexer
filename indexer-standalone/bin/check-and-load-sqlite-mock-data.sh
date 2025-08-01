#!/bin/bash

# Check if mock data needs to be loaded into SQLite
# Only loads if database is empty (no blocks exist)

set -e

# Default SQLite database path - hardcoded to match the default config
DB_PATH="/data/indexer.sqlite"

# Wait for SQLite database to be created
while [ ! -f "$DB_PATH" ]; do
    echo "Waiting for SQLite database to be created at $DB_PATH..."
    sleep 2
done

# Check if blocks table has data
BLOCK_COUNT=$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM blocks;" 2>/dev/null || echo "0")

if [ "$BLOCK_COUNT" -eq "0" ]; then
    echo "Database is empty, loading mock data..."
    
    # Load the mock data script
    for sql_file in /opt/mock-data/sqlite/*.sh; do
        if [ -f "$sql_file" ]; then
            echo "Loading: $(basename $sql_file)"
            # Set the DATABASE_FILE environment variable for the script
            export DATABASE_FILE="$DB_PATH"
            # Execute the shell script which contains SQLite commands
            bash "$sql_file"
        fi
    done
    
    echo "Mock data loaded successfully!"
else
    echo "Database already contains data (found $BLOCK_COUNT blocks), skipping mock data load."
fi