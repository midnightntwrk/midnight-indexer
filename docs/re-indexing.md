# Re-indexing from genesis

Some changes cannot be migrated: the data a new version needs was never stored,
and no SQL migration can invent it. The indexer then has to be re-indexed from
genesis, which means **wiping both stores together** and letting chain-indexer
rebuild them from the node.

This is the only such change so far — see [When a re-index is
required](#when-a-re-index-is-required) — but the procedure applies to any future
one.

## Why both stores, always together

The indexer keeps its data in two places, and they reference each other:

| Store           | cloud                      | standalone                                        |
| --------------- | -------------------------- | ------------------------------------------------- |
| Indexer DB      | PostgreSQL                 | `indexer.sqlite` (`APP__INFRA__STORAGE__CNN_URL`) |
| Ledger DB       | the *same* PostgreSQL, in `ledger_db_nodes` and `ledger_db_roots` | `ledger-db.sqlite` (`APP__INFRA__LEDGER_DB__CNN_URL`) |

The ledger DB is a content-addressed arena of ledger nodes. Rows in the indexer
DB point into it: `blocks.ledger_state_key`, and — since contract states became
arena keys — `contract_actions.state_key` and `contract_actions.zswap_state_key`.

Wiping only one of them leaves dangling references in whichever survives. Keys
pointing at nodes that are gone do not read back as empty or as an error: they
**panic inside storage-core** when the pointer is used. In cloud both stores live
in one database, so a drop takes care of itself; in standalone they are two files
and it is easy to delete one and keep the other. Don't.

## When a re-index is required

- **Contract states stored as arena keys** (v4.4.0). `contract_actions.state` and
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

### cloud

1. Stop chain-indexer, wallet-indexer and indexer-api. Leaving indexer-api up
   against a half-wiped database is what turns a maintenance window into an
   incident.
2. Drop and recreate the database (or drop the schema), so that the indexer
   tables and `ledger_db_nodes` / `ledger_db_roots` all go together.
3. Start chain-indexer with `run_migrations: true`. It creates the schema and
   begins at genesis.
4. Start wallet-indexer and indexer-api once chain-indexer reports
   `caught_up`. Until then indexer-api's `/ready` returns 503 by design.

### standalone

1. Stop the indexer.
2. Delete **both** files, and their SQLite sidecars:

   ```bash
   rm -f /data/indexer.sqlite* /data/ledger-db.sqlite*
   ```

   The glob matters: `-wal` and `-shm` files carry committed pages, so removing
   only the main file can resurrect part of the old database.

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

## See also

- [Upgrading the ledger](./upgrading-ledger.md) — a ledger bump does *not*
  normally require a re-index; that guide assumes both stores survive.
- [Creating a release](./releasing.md)
