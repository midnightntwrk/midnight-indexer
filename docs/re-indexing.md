# Re-indexing from genesis

Some changes cannot be migrated: the data a new version needs was never stored,
and no SQL migration can invent it. The indexer then has to be re-indexed from
genesis, which means **wiping both stores together** and letting chain-indexer
rebuild them from the node.

> [!CAUTION]
> **Despite living in `migrations/`, `008`/`010_contract_state_keys.sql` converts
> nothing.** It drops the contract-state blob columns and adds empty key columns.
> Your existing contract states are not carried forward and cannot be recovered
> afterwards, by any version. Do not read "migration" here as "upgrade in place".

This is the only such change so far — see [When a re-index is
required](#when-a-re-index-is-required) — but the procedure applies to any future
one.

The indexer refuses to apply that migration to a database that still
holds contract states as blobs, so an accidental in-place upgrade fails safe and
leaves the database readable by the version that wrote it. Every component that
migrates on startup — chain-indexer, wallet-indexer, indexer-api, spo-indexer —
performs the check, so it does not matter which one starts first.

## Why both stores, always together

The indexer keeps its data in two places, and they reference each other:

| Store           | cloud                      | standalone                                        |
| --------------- | -------------------------- | ------------------------------------------------- |
| Indexer DB      | PostgreSQL                 | `indexer.sqlite` (`APP__INFRA__STORAGE__CNN_URL`) |
| Ledger DB       | the *same* PostgreSQL, in `ledger_db_nodes` and `ledger_db_roots` | `ledger-db.sqlite` (`APP__INFRA__LEDGER_DB__CNN_URL`) |

The ledger DB is a content-addressed arena of ledger nodes. Rows in the indexer
DB point into it: `blocks.ledger_state_key`, and — since contract states became
arena keys — `contract_actions.state_key` and `contract_actions.zswap_state_key`.

Wiping only one of them leaves dangling references in whichever survives, and
nothing good follows. On chain-indexer's startup path the missing ledger state is
caught and reported — `no persisted ledger state root found within the retention
window; the ledger DB cannot be resumed` — but a key whose node is gone is only
checked for loadability where the code does so explicitly; elsewhere using the
pointer **panics inside storage-core** rather than reading back empty or
returning an error. In cloud both stores live in one database, so a drop takes
care of itself; in standalone they are two files and it is easy to delete one and
keep the other. Don't.

## When a re-index is required

- **Contract states stored as arena keys** (the release that introduces
  `008`/`010_contract_state_keys.sql`). `contract_actions.state` and
  `contract_actions.zswap_state` held a full serialized state per action, one copy
  per action even when the state had not changed — on preprod 301 GB of a 292 GB
  database, with individual states reported at around 860 KB. Both columns are
  replaced by keys into the ledger arena, which stores the same states
  deduplicated and structurally shared. Measured on stagenet, whose states run
  1.5-18 KB, 14 actions on one contract collapsed from 257,530 bytes of
  byte-identical blobs to 18,956 bytes in 15 shared nodes.

  The old blobs cannot be converted: the arena nodes they would have to point at
  were garbage collected long ago, and a migration cannot replay the chain to
  recreate them. chain-indexer therefore refuses to start against a database
  whose `contract_actions` have no keys, rather than serving empty states:

  ```text
  found contract actions with no contract state key; they were indexed by a
  version that stored contract states as blobs, which cannot be converted.
  ```

## Procedure

Re-indexing takes as long as indexing the chain from genesis did. Plan for a
window, and note that during a long re-index `gc_bound: "0s"` is **not** a safe
shortcut: retention-window unpersists keep producing garbage, so with gc off the
ledger DB grows without bound.

### cloud — sync a new instance and cut over (preferred)

Every deployed environment runs two indexer instances behind
`indexer.<env>.midnight.network`, and new versions go to the secondary first (see
`qa/tests/environment/model.ts`). Use that: the re-index happens on the secondary
while the primary keeps serving, so the downtime is zero and the rollback is
"don't promote".

1. **Recreate the secondary's database empty.** Drop and recreate it (or drop the
   schema) so the indexer tables and `ledger_db_nodes` / `ledger_db_roots` go
   together.

   > [!WARNING]
   > Do **not** clone, restore or replicate the primary's database onto the
   > secondary to "get a head start". A copy carries the primary's blob-era
   > contract actions, the migration is refused, and the sync window is wasted.
   > Empty is the point.

2. Deploy the new version to the secondary with `run_migrations: true`. It creates
   the schema and begins at genesis.
3. Watch it converge: `indexer_uncaptured_contract_state_count` flat at zero, gc
   passes keeping up, and the ledger DB's node count settling rather than
   trending.
4. Verify before promoting, ideally against the primary — see
   [Verifying](#verifying).
5. Promote the secondary to primary.

### cloud — in place (single instance only)

Only when there is no second instance to sync. This costs a full re-index of
downtime and has no rollback: once the schema is migrated, the previous version
cannot read the database either.

1. **Back up the database first.** It is the only way back.
2. Stop chain-indexer, wallet-indexer, indexer-api and spo-indexer. Leaving any of
   them up against a half-wiped database is what turns a maintenance window into
   an incident.
3. Drop and recreate the database (or drop the schema), so that the indexer tables
   and `ledger_db_nodes` / `ledger_db_roots` all go together.
4. Start chain-indexer with `run_migrations: true`. It creates the schema and
   begins at genesis.
5. Start the others once chain-indexer reports `caught_up`. Until then
   indexer-api's `/ready` returns 503 by design.

### standalone

Standalone has no second instance, so this is in place by necessity. Back up both
files first if the data matters.

1. Stop the indexer.
2. Delete **both** files, and their SQLite sidecars:

   ```bash
   rm -f /data/indexer.sqlite* /data/ledger-db.sqlite*
   ```

   The glob is not a nicety. On a freshly stopped indexer almost the whole ledger
   DB can still be in the WAL — a 4 KB `ledger-db.sqlite` next to a 2.8 MB
   `ledger-db.sqlite-wal` is normal — so deleting only the main file leaves most
   of the old database behind, not merely part of it.

3. Start the indexer. It creates both files and begins at genesis.

## Verifying

- `contract_actions` has rows and none of them has both keys null:

  ```sql
  SELECT count(*) FROM contract_actions WHERE state_key IS NULL AND zswap_state_key IS NULL;
  ```

  Anything other than `0` means rows from a pre-key version survived — which the
  startup guard should already have refused.

- A `state`-selecting query returns bytes rather than the empty string:

  ```graphql
  query { contractAction(address: "<hex address>") { address state zswapState } }
  ```

- `indexer_uncaptured_contract_state_count` stays flat. It counts contract
  addresses per block for which no state could be captured; a rising rate means
  states are silently not being captured and `state` is reading back empty.

- **The strongest check, and the only one that uses real states: compare what the
  two instances serve.** While the primary still runs the old version, the same
  contract action can be read from both, and the bytes must match exactly — the
  arena round trip is supposed to be lossless, and this is where that claim meets
  production-sized states rather than test ones.

  ```bash
  Q='{"query":"query{ contractAction(address:\"<hex address>\"){ address state zswapState } }"}'
  curl -s -X POST https://indexer.<env>.midnight.network/api/v4/graphql \
      -H 'Content-Type: application/json' -d "$Q" > primary.json
  curl -s -X POST https://indexer-<colour>.<env>.midnight.network/api/v4/graphql \
      -H 'Content-Type: application/json' -d "$Q" > secondary.json
  diff primary.json secondary.json && echo "identical"
  ```

  Run it over a set of addresses, favouring contracts with many actions and large
  states. A difference is a promotion blocker: it means a state does not survive
  the round trip, which no amount of re-indexing will fix.

## See also

- [Upgrading the ledger](./upgrading-ledger.md) — a ledger bump does *not*
  normally require a re-index; that guide assumes both stores survive.
- [Creating a release](./releasing.md)
