# Running the Application

This document describes how to run the Midnight Indexer in both standalone and cloud modes.

## Prerequisites

- [Rust](https://www.rust-lang.org/) and cargo installed if you run from source.
- [Docker](https://www.docker.com/) for running services and containers.
- [just](https://github.com/casey/just) for managing commands (if running from source).

## Local Mode

Local mode runs all components (chain-indexer, wallet-indexer, and indexer-api) in a single process with SQLite storage. This mode is ideal for development and testing on a single machine.

For more details about running in standalone mode (single binary), refer to the `indexer/README.md` in the repository. The `indexer` crate provides a unified binary that starts all services internally.

**Example:**

```bash
just run-indexer node="ws://localhost:9944"
```

This will:

- Start the indexer on `http://localhost:8088/api/v1/graphql`
- Use an SQLite database by default
- Connect to a Midnight node at `ws://localhost:9944` (adjust node URL as needed)

The details are in [indexer/README.md](../indexer/README.md)

## Cloud Mode

In cloud mode, each component (chain-indexer, wallet-indexer, indexer-api) runs separately and connects to shared resources like PostgreSQL and NATS. This mode is suitable for more robust and scalable deployments.

### Using docker-compose

You have a `docker-compose.yaml` file that already defines services for `chain-indexer`, `wallet-indexer`, `indexer-api`, `postgres`, `nats`, and `node`. The `docker-compose.yaml` sets up environment variables, health checks, and network configuration, making it easy to start everything at once.

**Example:**

```bash
docker-compose up -d
```

This command:

- Starts `postgres` and `nats` containers.
- Starts `chain-indexer`, `wallet-indexer`, and `indexer-api` containers.
- Starts a `node` container simulating the Midnight blockchain node.

Once all services are running, the indexer-api is accessible at:

```
http://localhost:8088/api/v1/graphql
```

You can now send GraphQL queries, mutations, and subscriptions to this endpoint.

### Manual Startup (Optional)

If you prefer a more manual approach (not usually needed if you have `docker-compose`):

1. Start dependencies:
   ```bash
   docker run -d --name postgres -p 5432:5432 -e POSTGRES_USER=indexer -e POSTGRES_PASSWORD=indexer -e POSTGRES_DB=indexer postgres:17.1-alpine
   docker run -d --name nats -p 4222:4222 nats:2.10.24 --user indexer --pass indexer -js
   docker run -d --name node -p 9944:9944 ghcr.io/midnight-ntwrk/midnight-node:0.9.0-rc2
   ```

2. Start chain-indexer:
   ```bash
   just run-chain-indexer node="ws://node:9944"
   ```

3. Start wallet-indexer:
   ```bash
   just run-wallet-indexer
   ```

4. Start indexer-api:
   ```bash
   just run-indexer-api
   ```

However, since a `docker-compose.yaml` is already provided and configured, using it directly is recommended over the manual approach.

## Configuration

All services support environment variables prefixed with `APP__` to override defaults. For example, to set a different database password:

```bash
APP__STORAGE__PASSWORD=mysecret docker-compose up -d
```

If running the indexer locally without Docker, specify configuration via environment variables or the `CONFIG_FILE` env var:

```bash
CONFIG_FILE=./config.yaml RUST_LOG=info just run-indexer
```

## Health Checks

- `GET /health`: Returns 200 OK if alive
- `GET /ready`: Returns 200 OK if the indexer is ready (fully indexed and synchronized)

Use these endpoints to ensure services are ready before sending queries or subscriptions.

## Summary

- **Local Mode**: One binary, simple, uses SQLite, good for dev.
- **Cloud Mode**: Multiple components, uses Docker and possibly Postgres/NATS for production scenarios.
- The provided `docker-compose.yaml` makes it easy to spin up a fully functioning cloud environment.

For more details, see other documentation files like `api-documentation.md`, `development-setup.md`, or `releasing.md`.