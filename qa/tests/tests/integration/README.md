# Integration Tests

## Overview

Integration tests validate the Midnight Indexer's GraphQL API functionality in detail, testing both queries and subscriptions against a running indexer instance. These tests assume the indexer has been pre-seeded with test data and verify that the API returns correct results.

## Test Scope

### Query Tests
- **Block Queries**: Validates retrieval of blocks by hash, height, and without parameters
- **Transaction Queries**: Tests transaction lookup by hash and identifier
- **Contract Queries**: Verifies contract action queries by address and offset
- **Genesis Data**: Validates correct handling of genesis block and initial transactions

### Subscription Tests
- **Block Subscriptions**: Tests real-time streaming of blocks with various filters
- **Transaction Subscriptions**: Validates streaming of shielded and unshielded transaction events
- **Contract Subscriptions**: Tests real-time updates for contract actions
- **Session Management**: Verifies proper handling of viewing keys and session lifecycle

## When to Run

Integration tests should be executed:
- After smoke tests pass successfully
- When validating API functionality against a seeded environment
- As part of regression testing
- When verifying GraphQL schema changes

## Execution

```bash
# Run only integration tests
TARGET_ENV=undeployed yarn test:integration
```

These tests run without the toolkit cache warmup and can start immediately. They require a running indexer with pre-seeded test data.

