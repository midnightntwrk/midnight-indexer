-- Store contract state as ledger-arena keys instead of blobs.
--
-- `state` held a full serialized contract state per action and `zswap_state` a full serialized
-- filtered commitment tree. Both are already in the ledger arena (`ledger_db_nodes`),
-- content-addressed and structurally shared, so an action only needs to reference the node rather
-- than carry a copy of it. Every action stored a full copy even when the state had not changed, so
-- the columns grew with the number of actions per contract, and quadratically for contracts whose
-- state grows as it is called; on preprod they are reported at 301 GB of a 292 GB database.
--
-- BREAKING: the old blobs cannot be converted here. Their arena nodes were garbage collected long
-- ago and a SQL migration cannot replay the chain to recreate them, so both stores must be wiped
-- and re-indexed together. See docs/re-indexing.md.
--
-- The key columns are nullable because a failed action has no contract state to reference; the API
-- resolves NULL to the empty string, which is exactly what an empty `state` blob resolves to today.
-- They are variable-width rather than a fixed 33 bytes because an arena key is either a reference
-- or, for a small enough payload, a direct encoding — the same reason `blocks.ledger_state_key` is
-- stored variable-width.
--
-- The three indexes on `contract_actions` are on `transaction_id`, `address` and `(id, address)`,
-- none of which involve the dropped columns, so they carry over untouched.

ALTER TABLE contract_actions
  DROP COLUMN state,
  DROP COLUMN zswap_state,
  ADD COLUMN state_key BYTEA,
  ADD COLUMN zswap_state_key BYTEA;
