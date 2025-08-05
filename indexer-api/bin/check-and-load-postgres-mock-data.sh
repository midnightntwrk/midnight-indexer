#!/bin/bash

# Check if mock data needs to be loaded into PostgreSQL
# Only loads if database is empty (no blocks exist)

set -e

# Set default values if not provided
: ${APP__INFRA__STORAGE__HOST:=postgres}
: ${APP__INFRA__STORAGE__USER:=indexer}
: ${APP__INFRA__STORAGE__DATABASE:=indexer}

# Wait for PostgreSQL to be ready
until PGPASSWORD=$APP__INFRA__STORAGE__PASSWORD psql -h "$APP__INFRA__STORAGE__HOST" -U "$APP__INFRA__STORAGE__USER" -d "$APP__INFRA__STORAGE__DATABASE" -c '\q' 2>/dev/null; do
    echo "Waiting for PostgreSQL to be ready..."
    sleep 2
done

# Check if blocks table has data
BLOCK_COUNT=$(PGPASSWORD=$APP__INFRA__STORAGE__PASSWORD psql -h "$APP__INFRA__STORAGE__HOST" -U "$APP__INFRA__STORAGE__USER" -d "$APP__INFRA__STORAGE__DATABASE" -t -c "SELECT COUNT(*) FROM blocks;" 2>/dev/null || echo "0")

if [ "$BLOCK_COUNT" -eq "0" ]; then
    echo "Database is empty, loading mock data..."
    
    # Load all mock data SQL files in order
    for sql_file in /opt/mock-data/postgres/*.sql; do
        if [ -f "$sql_file" ]; then
            echo "Loading: $(basename $sql_file)"
            PGPASSWORD=$APP__INFRA__STORAGE__PASSWORD psql -h "$APP__INFRA__STORAGE__HOST" -U "$APP__INFRA__STORAGE__USER" -d "$APP__INFRA__STORAGE__DATABASE" -f "$sql_file"
        fi
    done
    
    echo "Mock data loaded successfully!"
else
    echo "Database already contains data (found $BLOCK_COUNT blocks), skipping mock data load."
fi