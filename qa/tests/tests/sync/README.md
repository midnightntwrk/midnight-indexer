# Sync Compatibility Tests

## Overview

Sync tests answer one question: **can this indexer version index this chain at all?**

They start a local indexer stack of a chosen image tag, point it at a deployed
chain's public RPC endpoint, and watch it index from genesis. The indexer
re-executes every block through the ledger library, so a version that disagrees
with the node derives a different state and bails out — at which point the block
height it bailed at is the answer, because it localises the divergence to a single
block.

This is a different question from the other projects': `smoke` and `integration`
check an *already deployed and synced* indexer's API, and treat syncing as a
precondition to wait for. Here the sync itself is the subject.

## Test Scope

- **Sync liveness**: the stack indexes blocks and no container exits.
- **State root agreement**: no `zswap state root mismatch` and no
  `ledger state root mismatch` in the container logs.
- **Migration and decode failures**: surface as a non-zero container exit.
- **Node compatibility**: an image that cannot talk to the chain's runtime fails here.
- **Progress reporting**: the rate/estimate maths is covered by fast unit-level cases
  that need no containers, so the project stays meaningful in CI.

### What it does not catch

Released images only compare the **zswap** merkle root per block. The full
ledger-state-root comparison is gated to genesis
(`chain-indexer/src/infra/subxt_node.rs`), so a *silent* full-state divergence
passes here. Landing a per-block ledger-state-root guard upstream makes this suite
strictly stronger with no change to the suite itself.

## When to Run

- Before promoting an indexer version into an environment.
- When a node runtime upgrade lands, against each supported indexer version.
- To bisect the first block at which a version diverges from a chain.

Not part of the aggregate `bun run test` script: a run takes minutes at best and
days if unbounded.

## Execution

```bash
# From qa/tests. SYNC_INDEXER_TAG names the image tag under test.
TARGET_ENV=qanet SYNC_INDEXER_TAG=4.3.7 bun run test:sync

# A larger budget, and the standalone topology instead of the deployed shape.
TARGET_ENV=qanet SYNC_INDEXER_TAG=4.3.7 MAX_BLOCKS=50000 MAX_DURATION_MS=7200000 \
  SYNC_TOPOLOGY=standalone bun run test:sync
```

`TARGET_ENV` selects the **chain** (its RPC endpoint and network id come from
`environment/model.ts`), not a deployed indexer — the indexer under test always runs
locally from `qa/docker/docker-compose-sync.yaml`. `mainnet` is rejected.

The harness generates its own container secrets, so no `APP__INFRA__*` or
`FUNDING_SEED*` setup is needed. The whole run is skipped when `SYNC_INDEXER_TAG`
is unset or `docker compose` is unavailable, so the unit-level cases still run.

## Environment Variables

| Variable | Required | Default | Description |
| --- | --- | --- | --- |
| `TARGET_ENV` | Yes | — | Chain to index: `undeployed`, `devnet`, `qanet`, `preview`, `preprod`, `stagenet`. `mainnet` is rejected. |
| `SYNC_INDEXER_TAG` | Yes | — | Indexer image tag under test. Deliberately not `INDEXER_TAG`, which must not be set for deployed environments. |
| `SYNC_TOPOLOGY` | No | `cloud` | `cloud` (chain-indexer + wallet-indexer + indexer-api + postgres + nats) or `standalone` (single container, SQLite). |
| `SYNC_IMAGE_REGISTRY` | No | `midnightntwrk` | Image registry. |
| `MAX_BLOCKS` | No | `2000` | Blocks to index before the run passes. **`0` means no block bound — sync to the chain tip.** It does not mean "index zero blocks". |
| `MAX_DURATION_MS` | No | `1800000` (30 min) | Wall-clock bound. The only bound when `MAX_BLOCKS=0`. |
| `SYNC_PROGRESS` | No | auto | `live` or `plain`. Auto-detected from `CI` and whether stdout is a terminal. |
| `SYNC_API_PORT` | No | `8188` | Host port for the local indexer API. |
| `SYNC_METRICS_PORT` | No | `9100` | Host port for the Prometheus endpoint the harness reads progress from. |
| `SYNC_POSTGRES_PORT` / `SYNC_NATS_PORT` | No | `5433` / `4422` | Host ports for the cloud profile's infrastructure. |

## Progress Reporting

Progress comes from the indexer's own Prometheus metrics
(`indexer_block_height`, `indexer_node_block_height`, `indexer_caught_up`), which
give height, chain tip and caught-up state in a single scrape and work even for a
bare `chain-indexer` with no API. None of them are published until the first block
batch is processed, so an absent metric is reported as "not known yet" and the chain
tip falls back to a direct JSON-RPC read.

Two output modes:

- **`live`** (a terminal): one carriage-return-rewritten line with a spinner, erased
  before any summary or failure output.
- **`plain`** (CI, or a piped stdout): one line immediately, one as soon as a height
  is known, then one every five minutes, then a final summary. A long sync must never
  go silent in a CI log.

Both report the same four fields:

```
blocks synced: 700/1000 (70%) | 2.2 blocks/s overall | 3.1 blocks/s (30s) | ETA: 3h 28m
```

A poll interval can carry a run past its budget, so the block count may exceed the
target; the percentage is capped at 100 rather than reading e.g. 105%.

## Runtime Expectations

Deployed chains are millions of blocks deep, which is why the default is bounded.
Measured against qanet on the `cloud` topology, indexing runs at roughly **3.4
blocks/s** over public RPC — so the 2000-block default takes about ten minutes, and
`MAX_BLOCKS=0` on a chain of 2.3 M blocks is a multi-day run. Size `MAX_BLOCKS` and
`MAX_DURATION_MS` accordingly.

The `standalone` topology is markedly slower — about **0.2 blocks/s** on the same
chain, some seventeen times slower than `cloud`. Its SPO task cannot be switched off
and, while it works through its epoch backlog, it contends with the chain indexer for
SQLite's single writer (`spo-indexer/src/application.rs` sleeps between polls only
once it has caught up). `cloud` is the default for that reason, and because it is the
shape actually deployed. Use `standalone` for what it uniquely covers — the
single-binary build — with a small `MAX_BLOCKS` and a generous `MAX_DURATION_MS`.
