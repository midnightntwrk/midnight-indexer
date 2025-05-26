# Running the Application

This document describes how to run the Midnight Indexer in both standalone and cloud modes.

## Prerequisites

- [Rust](https://www.rust-lang.org/) and cargo installed if you run from source.
- [Docker](https://www.docker.com/) for running services and containers.
- [just](https://github.com/casey/just) for managing commands (if running from source).

## Standalone Mode

In standalone mode all components (chain-indexer, wallet-indexer, and indexer-api) run in a single process with SQLite storage. This mode is ideal for development and testing on a single machine.

## Cloud Mode

In cloud mode, each component (chain-indexer, wallet-indexer, indexer-api) runs separately and connects to shared resources like PostgreSQL and NATS. This mode is suitable for more robust and scalable deployments.

## Starting using Docker Compose

There is a `docker-compose.yaml` file that already defines services for `chain-indexer`, `wallet-indexer`, `indexer-api`, `postgres`, `nats`, and `node` as well as `indexer-standalone`. The `docker-compose.yaml` sets up environment variables, health checks, and network configuration, making it easy to start everything at once.

## Manual Startup

If you prefer a more direct approach:

1. Start chain-indexer:
   ```bash
   just run-chain-indexer
   ```

3. Start wallet-indexer:
   ```bash
   just run-wallet-indexer
   ```

4. Start indexer-api:
   ```bash
   just run-indexer-api
   ```

## Configuration

All services support environment variables prefixed with `APP__` to override defaults. For example, to set a different database password:

```bash
APP__STORAGE__PASSWORD=mysecret docker-compose up -d
```
