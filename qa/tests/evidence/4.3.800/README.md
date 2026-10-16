# Indexer QA Test Execution and Report Summary — 4.3.800-rc.2

## Overview

| Field | Value |
|---|---|
| Date | 16 October 2026 |
| QA Contact | Oleksandr Romanov (oleksandr.romanov@shielded.io) |
| Indexer version | 4.3.800-rc.2 |
| Toolkit version | 1.0.400 |
| Node version | 1.0.400 |
| QA Sign-off | ✅ |

## Test Execution Summary

### undeployed

| Suite | Passed | Failed | Skipped | Todo | Total |
|---|---:|---:|---:|---:|---:|
| smoke | 18 | 0 | 1 | 0 | 19 |
| integration | 148 | 0 | 49 | 0 | 197 |
| e2e | 49 | 0 | 15 | 1 | 65 |
| **Total** | **215** | **0** | **65** | **1** | **281** |

### qanet

| Suite | Passed | Failed | Skipped | Todo | Total |
|---|---:|---:|---:|---:|---:|
| smoke | 18 | 0 | 12 | 0 | 30 |
| integration | 189 | 1 | 93 | 20 | 303 |
| e2e | 53 | 0 | 11 | 1 | 65 |
| **Total** | **260** | **1** | **116** | **21** | **398** |

All undeployed suites passed. On qanet, the smoke and e2e suites passed.
The skipped and todo entries are cases the suites skip on purpose under certain conditions. They are not errors.

One integration test failed on qanet (`contract-queries.test.ts`,
`unshieldedBalances` for a contract known to hold tokens). The qanet indexer
was not re-synced from genesis, so the fix was not applied there.
**Retested successfully on preview** against indexer 4.3.800-rc.2 synced
from block 0. Details: [indexer-test-report.md](./qanet/indexer-test-report.md)

## Test Report Details

The `undeployed` and `qanet` folders each have the full report (`indexer-test-report.md`) and the
source result files (`<suite>/junit-test-results.xml` for smoke, integration
and e2e) that it was built from.

## Test Execution Guide

These steps start from scratch and pin the test code to commit `5e64bcab`
(`release/4.3` HEAD on 16 October 2026), so a rerun uses the same tests. `release/4.3` moves over time, so always
check out the commit ID, not the branch.

The **undeployed** run is the one anyone can reproduce: it starts a fresh
local chain and uses known, shared seeds, so the results should match. The
**qanet** run targets the hosted environment and needs `FUNDING_SEED_QANET`,
which must be a wallet that has made **both** shielded and unshielded
transactions. Results there depend on that wallet and on the live chain
state.

Requirements: git, Docker, Node.js v22+ and Bun v1.3.x (not the snap build).

Run these from the folder you want to work in. They create a
`midnight-indexer-4.3.800-rc.2-b2bf8fd4` folder there.

```bash
# Get the exact test code this run used (release/4.3 commit 5e64bcab)
git clone https://github.com/midnightntwrk/midnight-indexer midnight-indexer-4.3.800-rc.2-b2bf8fd4
cd midnight-indexer-4.3.800-rc.2-b2bf8fd4
git checkout --detach 5e64bcab66a451396b97c096ab71ddd84ae3784b
cd qa/tests
bun install --frozen-lockfile
export XRAY_COMPONENT=indexer

# undeployed — no funding seed needed
# Local-only credentials for the throwaway Docker stack (postgres, nats, ledger state)
export APP__INFRA__STORAGE__PASSWORD=indexer
export APP__INFRA__PUB_SUB__PASSWORD=indexer
export APP__INFRA__LEDGER_STATE_STORAGE__PASSWORD=indexer
export APP__INFRA__SECRET=303132333435363738393031323334353637383930313233343536373839303132
export TARGET_ENV=undeployed
export INDEXER_TAG=4.3.800-rc.2-b2bf8fd4
export IMAGE_REGISTRY=ghcr.io/midnightntwrk
export NODE_TAG=1.0.400
export NODE_TOOLKIT_TAG=1.0.400
VITEST_MAX_WORKERS=4 bun run test:smoke
VITEST_MAX_WORKERS=4 bun run test:integration
bun run test:e2e   # do not set VITEST_MAX_WORKERS for e2e

# qanet — needs your own funding seed
export TARGET_ENV=qanet
export FUNDING_SEED_QANET=<wallet seed with shielded + unshielded txs>
VITEST_MAX_WORKERS=4 bun run test:smoke
VITEST_MAX_WORKERS=4 bun run test:integration
TX_BACKEND=moth VITEST_MAX_WORKERS=4 bun run test:e2e
```

Each suite writes its results to `qa/tests/reports/`, and the next suite
overwrites them. Copy the results out after each suite if you want to keep
them.
