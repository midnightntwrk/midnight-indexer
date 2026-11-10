-- Scope dust_generation_info rows to the incarnation of the DUST generation
-- tree they belong to.
--
-- The ledger 8 -> 9 state translation wipes dust state (midnight-node #2012,
-- backported as #2057): `first_free` returns to zero and the node replays only
-- cNIGHT's slice of the generating set. Rows written before that wipe are dead,
-- but nothing retires them -- the wipe happens inside the state translation, not
-- via a transaction, so no DustGenerationDtimeUpdate event fires. Left mixed
-- with the replayed rows they double-count NIGHT balances and carry
-- generation/commitment tree indices that now name different leaves.
--
-- `dust_epoch` separates them. It is bumped only by a fork that actually wipes
-- dust (see `LedgerVersion::dust_epoch`), so it stays stable across ledger
-- majors that leave dust alone.
--
-- Existing rows are backfilled from the protocol version of the transaction
-- that produced them (>= 2_000_000 is ledger 9, see `ProtocolVersion::
-- ledger_version`), rather than all defaulting to the pre-fork epoch. That
-- matters for a deployment that already crossed the boundary on an older build:
-- its post-fork rows are stamped epoch 1 here, so the corrected queries return
-- the right answer straight after the upgrade instead of needing a re-index.

--------------------------------------------------------------------------------
-- dust_generation_info
--------------------------------------------------------------------------------
ALTER TABLE dust_generation_info ADD COLUMN dust_epoch BIGINT NOT NULL DEFAULT 0;

-- Backfill: ledger-9 transactions wrote into the post-wipe tree.
UPDATE dust_generation_info
SET dust_epoch = 1
WHERE transaction_id IN (
    SELECT id FROM transactions WHERE protocol_version >= 2000000
);

-- Serves the per-owner epoch-scoped range scan behind the dustGenerations
-- subscription, which pages on generation_index within one epoch.
CREATE INDEX ON dust_generation_info (owner, dust_epoch, generation_index);

-- Serves the dtime-update join, which matches on night_utxo_hash and must not
-- pick up the same UTXO's row from a dead epoch.
CREATE INDEX ON dust_generation_info (night_utxo_hash, dust_epoch);
