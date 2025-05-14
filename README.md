# Midnight Indexer

The Midnight Indexer (midnight-indexer) is a set of components designed to optimize the flow of blockchain data from a Midnight node to end-user applications. It retrieves the history of blocks, processes them, stores indexed data efficiently, and provides a GraphQL API for queries and subscriptions. This Rust-based implementation is the next-generation iteration of the previous Scala-based indexer, offering improved performance, modularity, and ease of deployment.
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

- [Chain Indexer](chain-indexer/README.md): Connects to the Midnight node, fetches blocks and transactions, and stores indexed data.
- [Wallet Indexer](wallet-indexer/README.md): Associates connected wallets with relevant transactions, enabling personalized queries and subscriptions.
- [Indexer API](indexer-api/README.md): Exposes a GraphQL API for queries, mutations, and subscriptions.

### Features

- Fetch and query blocks, transactions and contract actions at specific block hashes, heights, transaction identifiers or contract addresses.
- Real-time subscriptions to new blocks, contract actions and wallet-related events through WebSocket connections.
- Secure wallet sessions enabling clients to track only their relevant transactions.
- Configurable for both cloud (microservices) and standalone (single binary) deployments.
- Supports both PostgreSQL (cloud) and SQLite (standalone) storage backends.
- Extensively tested with integration tests and end-to-end scenarios.

### LICENSE

Apache 2.0.

### README.md

Provides a brief description for users and developers who want to understand the purpose, setup, and usage of the repository.

### SECURITY.md

Provides a brief description of the Midnight Foundation's security policy and how to properly disclose security issues.

### CONTRIBUTING.md

Provides guidelines for how people can contribute to the Midnight project.

### CODEOWNERS

Defines repository ownership rules.

### ISSUE_TEMPLATE

Provides templates for reporting various types of issues, such as: bug report, documentation improvement and feature request.

### PULL_REQUEST_TEMPLATE

Provides a template for a pull request.

### CLA Assistant

The Midnight Foundation appreciates contributions, and like many other open source projects asks contributors to sign a contributor License Agreement before accepting contributions. We use CLA assistant (https://github.com/cla-assistant/cla-assistant) to streamline the CLA signing process, enabling contributors to sign our CLAs directly within a GitHub pull request.

### Dependabot

The Midnight Foundation uses GitHub Dependabot feature to keep our projects dependencies up-to-date and address potential security vulnerabilities. 

### Checkmarx

The Midnight Foundation uses Checkmarx for application security (AppSec) to identify and fix security vulnerabilities. All repositories are scanned with Checkmarx's suite of tools including: Static Application Security Testing (SAST), Infrastructure as Code (IaC), Software Composition Analysis (SCA), API Security, Container Security and Supply Chain Scans (SCS).

### Unito

Facilitates two-way data synchronization, automated workflows and streamline processes between: Jira, GitHub issues and Github project Kanban board. 
