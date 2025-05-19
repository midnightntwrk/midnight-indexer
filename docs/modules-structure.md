# Modules Structure

The Rust version uses a workspace with multiple crates:

- `indexer-common`: Shared domain types, utilities, and traits.
- `indexer-common-macro`: Procedural macros supporting the common crate.
- `chain-indexer`: Fetches blocks and transactions from the node and stores them.
- `wallet-indexer`: Associates wallets with relevant transactions.
- `indexer-api`: Provides the GraphQL API.
- `indexer-tests`: Integration and end-to-end tests.
- `indexer`: A unified binary running chain-indexer, wallet-indexer, and api together (for standalone mode).

## Local Mode (indexer crate)

The `indexer` crate composes chain, wallet, and api into a single binary for quick standalone testing.

## Cloud Mode

Deploy chain-indexer, wallet-indexer, and indexer-api as separate processes or containers, each connecting to a shared database and pub/sub system.

## GraphQL Schema

`indexer-api/graphql/schema-v1.graphql` defines the API schema. The `indexer-api` crate provides queries, mutations, and subscriptions.

## Database and Pub/Sub

- Storage: Supports PostgreSQL and SQLite through `sqlx`.
- Pub/Sub: Uses NATS for message passing between components.
