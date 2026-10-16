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

```
┌─────────────┬─────────────┬────────┬────────┬─────────┬──────┬───────┐
│ Environment │ Suite       │ Passed │ Failed │ Skipped │ Todo │ Total │
├─────────────┼─────────────┼────────┼────────┼─────────┼──────┼───────┤
│ qanet       │ smoke       │     18 │      0 │      12 │    0 │    30 │
│ qanet       │ integration │    189 │      1 │      93 │   20 │   303 │
│ qanet       │ e2e         │     53 │      0 │      11 │    1 │    65 │
├─────────────┼─────────────┼────────┼────────┼─────────┼──────┼───────┤
│ Total       │             │    260 │      1 │     116 │   21 │   398 │
└─────────────┴─────────────┴────────┴────────┴─────────┴──────┴───────┘
```

The smoke and e2e suites passed. The skipped and todo entries are cases the
suites skip on purpose under certain conditions. They are not errors.

One integration test failed on qanet (`contract-queries.test.ts`,
`unshieldedBalances` for a contract known to hold tokens). The qanet indexer
was not re-synced from genesis, so the fix was not applied there.
**Retested successfully on preview** against indexer 4.3.800-rc.2 synced
from block 0. Details: [indexer-test-report.md](./qanet/indexer-test-report.md)

## Test Report Details

The `qanet` folder has the full report (`indexer-test-report.md`) and the
source result files (`<suite>/junit-test-results.xml` for smoke, integration
and e2e) that it was built from.

## Test Execution Guide

Run all commands from `qa/tests/`. Hosted environments such as `qanet` need
`FUNDING_SEED_<ENV>`. It must be a wallet that has made **both** shielded and
unshielded transactions.

```bash
# qanet — smoke
export TARGET_ENV=qanet
export FUNDING_SEED_QANET=<wallet seed with shielded + unshielded txs>
export VITEST_MAX_WORKERS=4
bun run test:smoke

# qanet — integration
export TARGET_ENV=qanet
export FUNDING_SEED_QANET=<wallet seed with shielded + unshielded txs>
export VITEST_MAX_WORKERS=4
bun run test:integration

# qanet — e2e
export TARGET_ENV=qanet
export FUNDING_SEED_QANET=<wallet seed with shielded + unshielded txs>
export TX_BACKEND=moth
export VITEST_MAX_WORKERS=4
bun run test:e2e
```
