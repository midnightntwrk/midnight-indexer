-- Stake snapshot per pool (latest values). This supports explorer stake distribution views.
-- Values are sourced from mainchain pool data (e.g., Blockfrost) and keyed by Cardano pool_id (56-hex string).

CREATE TABLE IF NOT EXISTS spo_stake_snapshot (
    pool_id         VARCHAR PRIMARY KEY REFERENCES pool_metadata_cache(pool_id) ON DELETE CASCADE,
    live_stake      NUMERIC,              -- current live stake (lovelace-like units) as big numeric
    active_stake    NUMERIC,              -- current active stake
    live_delegators INT,                  -- number of live delegators
    live_saturation DOUBLE PRECISION,     -- saturation ratio (0..1+)
    declared_pledge NUMERIC,              -- declared pledge
    live_pledge     NUMERIC,              -- current pledge
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Helpful indexes for ordering/filtering
CREATE INDEX IF NOT EXISTS spo_stake_snapshot_updated_at_idx ON spo_stake_snapshot (updated_at DESC);
CREATE INDEX IF NOT EXISTS spo_stake_snapshot_live_stake_idx ON spo_stake_snapshot ((COALESCE(live_stake, 0)) DESC);
