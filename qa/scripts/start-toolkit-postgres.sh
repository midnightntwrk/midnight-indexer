#!/bin/bash
set -e

echo "Starting toolkit Postgres..."

docker rm -f toolkit-postgres 2>/dev/null || true

docker run -d \
  --name toolkit-postgres \
  -p 5434:5432 \
  -e POSTGRES_USER=toolkit \
  -e POSTGRES_PASSWORD=toolkit \
  -e POSTGRES_DB=toolkit \
  -v "$(pwd)/toolkit-postgres-data:/var/lib/postgresql/data" \
  postgres:16

echo "Waiting for toolkit Postgres to be ready..."

for i in {1..30}; do
  if docker exec toolkit-postgres pg_isready -U toolkit >/dev/null 2>&1; then
    echo "toolkit-postgres is ready"
    exit 0
  fi
  sleep 1
done

echo "ERROR: toolkit-postgres did not become ready"
exit 1
