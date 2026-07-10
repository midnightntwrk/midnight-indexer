# Architecture

How the pieces fit together and talk. For *what each component is* see the
[top-level README](../README.md#components) and the per-component READMEs; this is the data flow
and the parts that aren't obvious from the component list.

## Data flow

```text
node ──subxt──▶ chain-indexer ──writes──▶ DB ◀─reads/writes─▶ indexer-api ──GraphQL──▶ clients / wallets
                     │                    ▲
                     └──event (IDs)──pg_notify──▶ wallet-indexer ──writes relevant txs──┘
```

- **chain-indexer** is the **single writer**. It subscribes to the node over subxt (a
  finalized-block subscription), applies each block to its **own** `LedgerState` - recomputing and
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

## The pub-sub layer is a signal bus, not a data bus

The pub-sub layer carries **small event messages - IDs only** (block ID, transaction ID, wallet);
the data itself stays in the DB. So the queue stays light regardless of chain size. In the cloud
deployment this is implemented with **Postgres LISTEN/NOTIFY** (`pg_notify`): notifications are
fired inside the same DB transaction as the write, so they are delivered if and only if the
transaction commits. There is no separate message broker to run or replicate - the pub-sub travels
over the same Postgres connection the indexer already depends on.

Deployed clusters typically run **2 wallet-indexer** replicas for redundancy, alongside the single
chain-indexer and the HPA'd indexer-api.

## Run modes

- **cloud** - the four services (chain-indexer, indexer-api, wallet-indexer, spo-indexer) +
  PostgreSQL, as separate images. This is what runs in Kubernetes.
- **standalone** - one `indexer-standalone` binary with SQLite and an **in-memory** pub/sub (tokio
  broadcast channels) in place of Postgres LISTEN/NOTIFY. For local dev / single-operator use.

The messaging seam is `indexer-common`'s `pub_sub` (Postgres LISTEN/NOTIFY for cloud, in-memory
channels for standalone); the SQL migrations also live in `indexer-common/migrations`.

## See also

- [Testing & node consistency](./testing.md) - the runtime root-match guard.
- Per-component detail: [chain-indexer](../chain-indexer/README.md),
  [wallet-indexer](../wallet-indexer/README.md), [indexer-api](../indexer-api/README.md).
