# Architecture

How the pieces fit together and talk. For *what each component is* see the
[top-level README](../README.md#components) and the per-component READMEs; this is the data flow
and the parts that aren't obvious from the component list.

## Data flow

```
node ──archive JSON-RPC──▶ chain-indexer ──writes──▶ DB ◀─reads/writes─▶ indexer-api ──GraphQL──▶ clients / wallets
                              │                    ▲
                              └──event (IDs)──▶ NATS ──▶ wallet-indexer ──writes relevant txs──┘
```

- **chain-indexer** is the **single writer**. It sources finalized blocks from the node through
  the new JSON-RPC spec (see [Block sourcing](#block-sourcing)), applies each block to its **own**
  `LedgerState` - recomputing and
  [guarding the merkle roots](./testing.md) - and writes blocks, transactions and ledger state to
  the DB. Only one may run per environment (two would race the DB). It publishes small indexing
  events (`BlockIndexed`, `UnshieldedUtxoIndexed`).
- **wallet-indexer** does the per-wallet work **asynchronously in the background** - the
  least-obvious component. It subscribes to `BlockIndexed` (the new-data signal) and polls the
  active wallet set (`active_wallet_ids`), trial-decrypts each new transaction against each active
  wallet's viewing key, materialises the relevant transactions into the DB, and emits
  `WalletIndexed`.
- **indexer-api** serves GraphQL queries and subscriptions (reads) **and owns the wallet-lifecycle
  writes** - it is read-heavy, not read-only. `connect` upserts the wallet into the `wallets` table
  (the encrypted viewing key, a fresh `session_id`, and the scan start index) and returns the
  session ID; `disconnect` nulls the session; and the shielded subscription periodically writes a
  `keep_wallet_active` heartbeat. A newly connected wallet is picked up by wallet-indexer **polling
  the active wallet set**, not via a connect event; subscriptions then stream that wallet's
  relevant transactions.
- **spo-indexer** indexes stake-pool data via Blockfrost.

## Block sourcing

chain-indexer gets its blocks from a pipeline that runs ahead of indexing:

```
Finalized ─▶ Chunk ─▶ Resolve ─▶ Source ─▶ Verify ─▶ Emit ─▶ Decode (CPU pool) ─▶ indexing
```

- **Finalized** follows the node's finalized blocks with one `chainHead_v1_follow` subscription.
- **Chunk** splits the heights still to index into chunks of up to `source_chunk_size` blocks;
  **Resolve** maps their heights to hashes with `archive_v1_hashByHeight`.
- **Source** fetches each block's header, body, ledger and zswap state roots and storage
  (events, authority-set and system-parameter hashes) with batched `archive_v1_*` calls, up to
  `source_chunks_ahead` chunks at once. Values that rarely change (authority sets, system
  parameters, runtime metadata) are fetched only where their storage hash or the runtime changes.
- **Verify** checks that every block links to its parent, and that blocks near the finalized tip
  link to it; anything that doesn't is re-sourced by walking parent hashes back from the tip.
- **Emit** hands verified chunks on in height order; **Decode** turns them into blocks on a
  dedicated pool of `decode_cpu_threads` threads, chunks in parallel, blocks in order.
- Indexing then applies blocks one at a time.

The node must run with **`--state-pruning archive`**: the `archive_v1_*` methods exist only on
archive nodes, and chain-indexer refuses to start without them, naming the missing methods. All
calls go over one WebSocket connection, batched up to `rpc_batch_size` calls with up to
`rpc_batches_in_flight` batches in flight.

**Memory** is bounded in blocks: at most `source_chunks_ahead` chunks being sourced, as many
waiting for decode, the chunks decoding at once (two when `source_chunk_size` is at least the
number of decode threads) and the one chunk being indexed, each of at most `source_chunk_size`
blocks: (8 + 8 + 2 + 1) × 64 ≈ 1,200 blocks with the defaults. For comparison, the previous
concurrent fetch (#1453) used about 6 GiB in standalone at `fetch_concurrency` 32.

How fast blocks can be sourced depends mostly on the node: it serves the ledger and zswap state
root calls one at a time. `just source-throughput <url>` measures the pipeline alone against a
node, with per-interval rates and per-block sizes.

## NATS is a signal bus, not a data bus

NATS carries **small event messages - IDs only** (block ID, transaction ID, wallet); the data
itself stays in the DB. So the queue stays light regardless of chain size. Ledger state used to
live in NATS and was **moved to the DB** (more reliable across abrupt restarts) - NATS remains for
messaging only.

Deployed clusters run a **NATS quorum of 3** and typically **2 wallet-indexer** replicas for
redundancy, alongside the single chain-indexer and the HPA'd indexer-api.

## Run modes

- **cloud** - the four services (chain-indexer, indexer-api, wallet-indexer, spo-indexer) +
  PostgreSQL + NATS, as separate images. This is what runs in Kubernetes.
- **standalone** - one `indexer-standalone` binary with SQLite and an **in-memory** pub/sub in
  place of NATS. For local dev / single-operator use.

The messaging seam is `indexer-common`'s `pub_sub` (NATS for cloud, in-memory channels for
standalone); the SQL migrations also live in `indexer-common/migrations`.

## See also

- [Testing & node consistency](./testing.md) - the runtime root-match guard.
- Per-component detail: [chain-indexer](../chain-indexer/README.md),
  [wallet-indexer](../wallet-indexer/README.md), [indexer-api](../indexer-api/README.md).
