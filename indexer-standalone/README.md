# Indexer

A unified indexer that combines `chain-indexer`, `wallet-indexer`, and `indexer-api` components with SQLite storage for local deployment.

## Quick Start

### Build Docker Image

```bash
just docker-indexer release
```

### Run Container

Start the indexer:

```bash
docker run \
  --name indexer \
  -v $(pwd):/data \
  -p 8088:8088 \
  -e RUST_LOG=info \
  -e APP__INFRA__SECRET=3031323334353637383930313233343536373839303132333435363738393031 \
  ghcr.io/midnight-ntwrk/indexer:latest
```

Key flags explained:
- `-e RUST_LOG=info`: set INFO log level for all targets
- `-v $(pwd):/data`: mount the SQLite storage file into the current directory
- `-p 8088:8088`: expose API port

## Configuration

Environment variables to configure the indexer:

| Variable | Description | Required |
|----------|-------------|----------|
| APP__NODE__URL | Blockchain node WebSocket URL | No (default: ws://localhost:9944) |
| RUST_LOG | Log level (debug, info, warn, error) | No (default: info) |
