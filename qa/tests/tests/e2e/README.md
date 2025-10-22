# End-to-End (E2E) Tests

## Overview

End-to-end tests provide comprehensive validation of the Midnight Indexer by actively interacting with the blockchain through the Midnight Node Toolkit. These tests create real blockchain transactions and contract interactions, then verify that the indexer correctly captures and reports these events through its GraphQL API.

## Be aware of
These tests are slower than the other tests as submitting transactions to a Midnight node can take time. Also these test use the Midnight Node Toolkit, which needs the full view of the blockchain to be able to operate with transactions. These test will be slow the first time because the Toolkit needs to pull all the blocks from the chain and this can take a lot of time and depends on the number of blocks. Hoewever, the good news is that after the first pull, everything should be cached and future execution should take much less time.

## Test Scope

### Toolkit Basics
- **Toolkit Functionality**: Validates that the Node Toolkit container is operational and can perform basic operations
- **Key Material Management**: Tests generation and handling of cryptographic keys and addresses

### Transaction Tests
- **Shielded Transactions**: Creates shielded token transfers between wallets and validates indexer reporting
- **Unshielded Transactions**: Performs unshielded STAR transfers and verifies balance updates and transaction events

### Contract Action Tests
- **Contract Deployment**: Deploys smart contracts and validates deployment events in the indexer
- **Contract Calls**: Invokes deployed contract methods and verifies the indexer captures these interactions
- **Contract Updates**: Tests contract upgrade scenarios and validates update events

## Cache Warmup

E2E tests include a global setup phase that warms up the toolkit cache. This process:
- Downloads and initializes the Node Toolkit container
- Prepares the toolkit environment for test execution
- Typically takes 30-60 seconds on first run (cached for subsequent runs)

## When to Run

E2E tests should be executed:
- As part of comprehensive validation of indexer functionality
- When testing the full integration between the node, toolkit, and indexer
- Before major releases or after significant changes
- When validating end-to-end workflows

## Execution

```bash
# Run only e2e tests (includes cache warmup)
TARGET_ENV=undeployed yarn test:e2e
```

These tests require:
- A running Midnight node and indexer
- Docker access for the Node Toolkit
- More time to execute due to real blockchain interactions

