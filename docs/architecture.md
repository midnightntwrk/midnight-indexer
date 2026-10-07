# Architecture

How the pieces fit together and talk. For *what each component is* see the
[top-level README](../README.md#components) and the per-component READMEs; this is the data flow
and the parts that aren't obvious from the component list.

## Data flow

```
node ──subxt──▶ chain-indexer ──writes──▶ DB ◀─reads/writes─▶ indexer-api ──GraphQL──▶ clients / wallets
                     │                    ▲
                     └──event (IDs)──▶ NATS ──▶ wallet-indexer ──writes relevant txs──┘
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

## What the chain-indexer follows: the outcome on chain

The chain-indexer does not decide what happened in a block; the runtime does, and records it in
the block's events. The chain-indexer reads the block body and `System.Events` together
(`transactions` in `chain-indexer/src/infra/subxt_node/runtimes.rs`, over each runtime's calls
and events) and applies only what was applied on chain.

**Inclusion rule.** Every call in the body is matched by pallet and call, exhaustively (a runtime
that adds a pallet or a call does not compile until it is handled). Two calls carry
transactions; every other call is ignored whatever its outcome:

| Call | What the indexer does |
|---|---|
| `Midnight::send_mn_transaction` | A regular transaction. Applied if it was applied on chain, recorded as `FAILURE` (zero fees, no effects, ledger state untouched) if it was rejected at dispatch |
| `MidnightSystem::send_mn_system_transaction` (top level) | Never a transaction after genesis: it is `Root`-only, so in an executed block it can only have failed, and is ignored like any other failed call. One that applied would be taken from its `SystemTransactionApplied` event |

**Outcomes come from events that only the extrinsic's own dispatch emits**, under its phase
`ApplyExtrinsic(i)`: `Midnight::TxApplied` (applied), `Midnight::TxPartialSuccess` (partially
applied) and `System::ExtrinsicFailed` (rejected, for example `CallFiltered` in safe mode). Each
`Midnight` extrinsic gets exactly one. The hash in the `TxApplied` and `TxPartialSuccess` events,
and the result the indexer's own ledger computes, must agree with the outcome on chain. A
disagreement, a missing or repeated outcome, or an outcome for an extrinsic that is not a
`Midnight` call is a *divergence* and is logged at error level, naming the block.

FRAME ends every extrinsic's dispatch with `System::ExtrinsicSuccess` or `System::ExtrinsicFailed`.
A `Midnight` extrinsic that ends with `ExtrinsicSuccess` but emitted neither `TxApplied` nor
`TxPartialSuccess` is a divergence too; it is still indexed as applied, so that it is not dropped.

**System transactions** are built by the runtime inside other extrinsics, so no extrinsic carries
their bytes. The indexer takes them from `MidnightSystem::SystemTransactionApplied`, which is
emitted only on success and carries the exact bytes applied.

### Block execution model

How a block is executed, and the phase stamped on the events of each step:

| Step | Runs | Phase of its events | System transactions |
|---|---|---|---|
| 1 Initialize | `on_runtime_upgrade`, `on_initialize` | `Initialization` | none today |
| 2 Inherents | extrinsics `0 … k-1` (timestamp, cNight `process_tokens`, bridge `handle_transfers`, …) | `ApplyExtrinsic(i)` | cNight, bridge |
| 3 Post-inherents | post-inherents hook, then one multi-block-migration step or `on_poll` | `ApplyExtrinsic(k)`: FRAME has no phase for this step and stamps the *next* index, which may not exist | cNight migration step |
| 4 Extrinsics | user and signed extrinsics `k … n-1` | `ApplyExtrinsic(i)` | governance, inside `FederatedAuthority::motion_close` |
| 5 Finalize | `on_idle`, `on_finalize` | `Finalization` | none today |

The genesis block is not executed: it has no events, and its extrinsics only record how the genesis
state was built. At height 0 every `Midnight` and `MidnightSystem` extrinsic in the body is taken
as applied, in body order, with the hash the ledger computes: SHA-256 over the transaction's tagged
serialization, which is the bytes on chain. The genesis cNight registrations are read from
storage; they are part of the genesis state, so they are recorded under `Initialization`, with
their position in the storage read, which is ordered by hashed key.

**Attribution.**

- Every item carries the phase the chain records it under, as the indexer's own `Phase`, declared
  in execution order. A regular transaction is `ApplyExtrinsic(i)` with its body index; a system
  transaction, a registration or a bridge event takes the phase of its event, including the
  step-3 `k` and `Initialization`/`Finalization`. Registrations and bridge events also carry
  their index in `System.Events`.
- The events are in execution order, so the block's transactions are applied, stored and inserted
  in that order. A governance system transaction therefore stays among the user transactions it
  ran between, instead of being moved to the front.
- A phase says *where* on chain something happened, not always *who* caused it: step 3's events
  carry the next extrinsic's index. Phases are therefore never validated against the body, and an
  outcome is only ever taken from the dispatch-only events above, so the phase of a post-inherents
  event never changes the outcome of the extrinsic that shares its index.
- Phase and event index are carried only as far as `node::Block`; nothing about them is persisted.

## See also

- [Testing & node consistency](./testing.md) - the runtime root-match guard.
- Per-component detail: [chain-indexer](../chain-indexer/README.md),
  [wallet-indexer](../wallet-indexer/README.md), [indexer-api](../indexer-api/README.md).
