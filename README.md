# Midnight Indexer

The Midnight Indexer (midnight-indexer) is a set of components designed to optimize the flow of blockchain data from a Midnight Node to end-user applications. It retrieves the history of blocks, processes them, stores indexed data efficiently, and provides a GraphQL API for queries and subscriptions. This Rust-based implementation is the next-generation iteration of the previous Scala-based indexer, offering improved performance, modularity, and ease of deployment.
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

### Components

- [Chain Indexer](chain-indexer/README.md): Connects to the Midnight Node, fetches blocks and transactions, and stores indexed data.
- [Wallet Indexer](wallet-indexer/README.md): Associates connected wallets with relevant transactions, enabling personalized queries and subscriptions.
- [Indexer API](indexer-api/README.md): Exposes a GraphQL API for queries, mutations, and subscriptions.

### Features

- Fetch and query blocks, transactions and contract actions at specific block hashes, heights, transaction identifiers or contract addresses.
- Real-time subscriptions to new blocks, contract actions and wallet-related events through WebSocket connections.
- Secure wallet sessions enabling clients to track only their relevant transactions.
- Configurable for both cloud (microservices) and standalone (single binary) deployments.
- Supports both PostgreSQL (cloud) and SQLite (standalone) storage backends.
- Extensively tested with integration tests and end-to-end scenarios.

### Running

To run the Midnight Indexer Docker images are provided under the [`midnightntwrk`](https://hub.docker.com/r/midnightntwrk) organization. It is supposed that users are familiar with running Docker images, e.g. via Docker Compose or Kubernetes.

#### Standalone Mode

The standalone Indexer combines the Chain Indexer, Indexer API and Wallet Indexer components in a single executable alongside an in-process SQLite database. Therefore the only Docker image to be run is [`indexer-standalone`](https://hub.docker.com/r/midnightntwrk/indexer-standalone).

By default it connects to a local Midnight Node at `ws://localhost:9944` and exposes its GraphQL API at `0.0.0.0:8088`. All configuration has defaults except for the secret used to encrypt stored sensitive data which must be provided via the `APP__INFRA__SECRET` environment variable as valid hex-encoded 32 bytes.

`indexer-standalone` be configured by the following environment variables:

| Env Var | Meaning | Default |
|---|---|---|
| APP__APPLICATION__NETWORK_ID | Network ID: MainNet, TestNet, DevNet or Undeployed | `Undeployed` |
| APP__INFRA__STORAGE__CNN_URL | SQlite connection URL | `/data/indexer.sqlite` |
| APP__INFRA__NODE__URL | WebSocket Endpoint of Midnight Node | `ws://localhost:9944` |
| APP__INFRA__API__PORT | Port of the GraphQL API | `8088` |
| APP__INFRA__SECRET | Hex-encoded 32-byte secret to encrypt stored sensitive data | - |

For the full set of configuration options see [config.yaml](indexer-standalone/config.yaml).

#### Cloud Mode

The Chain Indexer, Indexer API and Wallet Indexer can be run as separate executables, interacting with a PostgreSQL database and a NATS messaging system. Running PostgreSQL and NATS is out of scope of this document. The respective Docker images are:
- [`chain-indexer`](https://hub.docker.com/r/midnightntwrk/chain-indexer)
- [`indexer-api`](https://hub.docker.com/r/midnightntwrk/indexer-api)
- [`wallet-indexer`](https://hub.docker.com/r/midnightntwrk/wallet-indexer)

##### `chain-indexer` Configuration

| Env Var | Meaning | Default |
|---|---|---|
| APP__APPLICATION__NETWORK_ID | Network ID: MainNet, TestNet, DevNet or Undeployed | `Undeployed` |
| APP__INFRA__STORAGE__HOST | PostgreSQL host | `localhost` |
| APP__INFRA__STORAGE__PORT | PostgreSQL port | `5432` |
| APP__INFRA__STORAGE__DBNAME | PostgreSQL database name | `indexer` |
| APP__INFRA__STORAGE__USER | PostgreSQL database user | `indexer` |
| APP__INFRA__PUB_SUB__URL | NATS URL | `localhost:4222` |
| APP__INFRA__PUB_SUB__USERNAME | NATS username | `indexer` |
| APP__INFRA__ZSWAP_STATE_STORAGE__URL | NATS URL | `localhost:4222` |
| APP__INFRA__ZSWAP_STATE_STORAGE__USERNAME | NATS username | `indexer` |
| APP__INFRA__NODE__URL | WebSocket Endpoint of Midnight Node | `ws://localhost:9944` |

For the full set of configuration options see [config.yaml](chain-indexer/config.yaml).

##### `indexer-api` Configuration

| Env Var | Meaning | Default |
|---|---|---|
| APP__APPLICATION__NETWORK_ID | Network ID: MainNet, TestNet, DevNet or Undeployed | `Undeployed` |
| APP__INFRA__STORAGE__HOST | PostgreSQL host | `localhost` |
| APP__INFRA__STORAGE__PORT | PostgreSQL port | `5432` |
| APP__INFRA__STORAGE__DBNAME | PostgreSQL database name | `indexer` |
| APP__INFRA__STORAGE__USER | PostgreSQL database user | `indexer` |
| APP__INFRA__PUB_SUB__URL | NATS URL | `localhost:4222` |
| APP__INFRA__PUB_SUB__USERNAME | NATS username | `indexer` |
| APP__INFRA__ZSWAP_STATE_STORAGE__URL | NATS URL | `localhost:4222` |
| APP__INFRA__ZSWAP_STATE_STORAGE__USERNAME | NATS username | `indexer` |
| APP__INFRA__API__PORT | Port of the GraphQL API | `8088` |
| APP__INFRA__SECRET | Hex-encoded 32-byte secret to encrypt stored sensitive data | - |

For the full set of configuration options see [config.yaml](indexer-api/config.yaml).

##### `wallet-indexer` Configuration

| Env Var | Meaning | Default |
|---|---|---|
| APP__APPLICATION__NETWORK_ID | Network ID: MainNet, TestNet, DevNet or Undeployed | `Undeployed` |
| APP__INFRA__STORAGE__HOST | PostgreSQL host | `localhost` |
| APP__INFRA__STORAGE__PORT | PostgreSQL port | `5432` |
| APP__INFRA__STORAGE__DBNAME | PostgreSQL database name | `indexer` |
| APP__INFRA__STORAGE__USER | PostgreSQL database user | `indexer` |
| APP__INFRA__PUB_SUB__URL | NATS URL | `localhost:4222` |
| APP__INFRA__PUB_SUB__USERNAME | NATS username | `indexer` |
| APP__INFRA__SECRET | Hex-encoded 32-byte secret to encrypt stored sensitive data | - |

For the full set of configuration options see [config.yaml](wallet-indexer/config.yaml).

### LICENSE

Apache 2.0.

### SECURITY.md

Provides a brief description of the Midnight Foundation's security policy and how to properly disclose security issues.

### CONTRIBUTING.md

Provides guidelines for how people can contribute to the Midnight project.

### CODEOWNERS

Defines repository ownership rules.

### CLA Assistant

The Midnight Foundation appreciates contributions, and like many other open source projects asks contributors to sign a contributor License Agreement before accepting contributions. We use CLA assistant (https://github.com/cla-assistant/cla-assistant) to streamline the CLA signing process, enabling contributors to sign our CLAs directly within a GitHub pull request.

### Dependabot

The Midnight Foundation uses GitHub Dependabot feature to keep our projects dependencies up-to-date and address potential security vulnerabilities. 

### Checkmarx

The Midnight Foundation uses Checkmarx for application security (AppSec) to identify and fix security vulnerabilities. All repositories are scanned with Checkmarx's suite of tools including: Static Application Security Testing (SAST), Infrastructure as Code (IaC), Software Composition Analysis (SCA), API Security, Container Security and Supply Chain Scans (SCS).

### Unito

Facilitates two-way data synchronization, automated workflows and streamline processes between: Jira, GitHub issues and Github project Kanban board. 
