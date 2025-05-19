# Midnight Indexer

The Midnight Indexer is a set of components designed to optimize the flow of blockchain data from a Midnight node to end-user applications. It retrieves history of blocks, processes them, stores indexed data efficiently, and provides a GraphQL API for queries and subscriptions. This Rust-based implementation is the next-generation iteration of the previous Scala-based indexer, offering improved performance, modularity, and ease of deployment.
```
                                ┌─────────────────┐
                                │                 │
                                │                 │
                                │      Node       │
                                │                 │
                                │                 │
                                └─────────────────┘
                                         │
┌────────────────────────────────────────┼────────────────────────────────────────┐
│                                        │                                        │
│                                        │ fetch                                  │
│                                        │ blocks                                 │
│                                        ▼                                        │
│                               ┌─────────────────┐                               │
│                               │                 │                               │
│                               │                 │                               │
│                               │      Chain      │                               │
│             ┌─────────────────│     Indexer     │                               │
│             │                 │                 │                               │
│             │                 │                 │                               │
│             │                 └─────────────────┘                               │
│             │                          │                                        │
│             │                          │ save blocks                            │
│             │                          │ and transactions                       │
│             │    save relevant         ▼                                        │
│             │     transactions   .───────────.                                  │
│      notify │        ┌─────────▶(     DB      )───────────────────┐             │
│ transaction │        │           `───────────'                    │             │
│     indexed │        │                                            │ read data   │
│             ▼        │                                            ▼             │
│    ┌─────────────────┐                                   ┌─────────────────┐    │
│    │                 │                                   │                 │    │
│    │                 │                                   │                 │    │
│    │     Wallet      │                                   │     Indexer     │    │
│    │     Indexer     │◀──────────────────────────────────│       API       │    │
│    │                 │  notify                           │                 │    │
│    │                 │  wallet                           │                 │    │
│    └─────────────────┘  connected                        └─────────────────┘    │
│                                                                   ▲             │
│                                                           connect │             │
│                                                                   │             │
└───────────────────────────────────────────────────────────────────┼─────────────┘
                                                                    │
                                 ┌─────────────────┐                │
                                 │                 │                │
                                 │                 │                │
                                 │     Wallet      │────────────────┘
                                 │                 │
                                 │                 │
                                 └─────────────────┘
```

## Components

- [Chain Indexer](chain-indexer/README.md): Connects to the Midnight node, fetches blocks and transactions, and stores indexed data.
- [Wallet Indexer](wallet-indexer/README.md): Associates connected wallets with relevant transactions, enabling personalised data streams.
- [Indexer API](indexer-api/README.md): Exposes a GraphQL API for queries, mutations, and subscriptions. Integrates with both chain and wallet indexers.

## Features

- Fetch and query blocks, transactions, and contracts at various offsets.
- Real-time subscriptions to new blocks, contract updates, and wallet-related events.
- Secure wallet sessions enabling clients to track only their relevant transactions.
- Configurable for both local (single-binary) and cloud (microservices) deployments.
- Supports both PostgreSQL (cloud) and SQLite (local) storage backends.
- Extensively tested with integration tests using containers and end-to-end scenarios.

## Getting Started

1. **Installation**: Ensure you have [Rust](https://www.rust-lang.org/), [Cargo](https://doc.rust-lang.org/cargo/), and [just](https://github.com/casey/just) installed.
2. **Build**: 
   ```bash
   just all-features
   ```
This runs checks, lints, tests, and docs.

3. **Run Locally**:
   ```bash
   just run-indexer node="ws://localhost:9944"
   ```
   This starts the local unified indexer with SQLite storage.

4. **Run in Cloud Mode**:
    - Spin up separate components using Docker or on separate hosts.
    - For example, run `chain-indexer`, `wallet-indexer`, and `indexer-api` containers connected to a PostgreSQL instance and NATS for pub/sub.

## Documentation

- [API Documentation](./docs/api/v1/api-documentation.md): Detailed info on GraphQL queries, mutations, and subscriptions.
- [Development Setup](./docs/development-setup.md): Instructions for setting up your environment.
- [Running the Application](./docs/running.md): How to run locally or in Docker.
- [Integration Tests](./docs/integration-tests.md): Details on running the test suite.
- [Releasing](./docs/releasing.md): How to release new versions.
- [Contributing](./docs/contributing.md): Guidelines for development and contributions.
- [Modules Structure](./docs/modules-structure.md): Overview of the project layout.

## Status and Health Endpoints

- `GET /ready`: returns 200 OK if ready.
