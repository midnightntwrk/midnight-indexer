--------------------------------------------------------------------------------
-- transactions variant index
--------------------------------------------------------------------------------
-- Add composite index on (variant, id) to optimize wallet-indexer queries
-- that filter on variant and scan from a starting transaction id.
-- Query pattern: WHERE id >= ? AND variant = 'Regular' ORDER BY id LIMIT ?
-- See: https://github.com/midnightntwrk/midnight-indexer/issues/646
CREATE INDEX transactions_variant_id_idx ON transactions (variant, id);
