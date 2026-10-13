# Indexer QA Test Execution and Report Summary — 4.3.800-rc.2

## Overview

| Field | Value |
|---|---|
| Date | 13 October 2026 |
| QA Contact | Oleksandr Romanov (oleksandr.romanov@shielded.io) |
| Indexer version | 4.3.800-rc.2 |
| Node version | 1.0.400 |
| QA Sign-off | ⚠️ Pending (1 integration failure under triage) |

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

One integration test failed:

- `contract queries › a contract query by address › should return non-empty
  unshieldedBalances for contracts known to hold tokens`
  (`tests/integration/basic/queries/contract-queries.test.ts:163`): `expected 0 to be greater than 0`.
  This test checks the fix for #1245. Before that fix, `unshieldedBalances` came
  back empty for every contract. The fix is #1246, and the test only passes once
  history has been backfilled or re-indexed. The configured qanet token-holding
  contract still returns `unshieldedBalances: []`.

## Test Report Details

The `qanet` folder has the full report (`indexer-test-report.md`) and the
source result files (`test-results.json` / `test-results.xml` for each suite)
that it was built from.

## Test Execution Guide

Run all commands from `qa/tests/`. Hosted environments such as `qanet` need
`FUNDING_SEED_<ENV>`. It must be a wallet that has made **both** shielded and
unshielded transactions.

```bash
# qanet — smoke
export TARGET_ENV=qanet
export FUNDING_SEED_QANET=<wallet seed with shielded + unshielded txs>
export XRAY_COMPONENT=indexer
export VITEST_MAX_WORKERS=4
bun run test:smoke

# qanet — integration
export TARGET_ENV=qanet
export FUNDING_SEED_QANET=<wallet seed with shielded + unshielded txs>
export XRAY_COMPONENT=indexer
export VITEST_MAX_WORKERS=4
bun run test:integration

# qanet — e2e
export TARGET_ENV=qanet
export FUNDING_SEED_QANET=<wallet seed with shielded + unshielded txs>
export XRAY_COMPONENT=indexer
export TX_BACKEND=moth
export VITEST_MAX_WORKERS=4
bun run test:e2e
```
