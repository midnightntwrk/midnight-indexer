-- Stake history table and refresh state cursor

CREATE TABLE IF NOT EXISTS spo_stake_history (
    id              BIGSERIAL PRIMARY KEY,
    pool_id         VARCHAR NOT NULL REFERENCES pool_metadata_cache(pool_id) ON DELETE CASCADE,
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    mainchain_epoch INTEGER,

    live_stake       NUMERIC,
    active_stake     NUMERIC,
    live_delegators  INTEGER,
    live_saturation  DOUBLE PRECISION,
    declared_pledge  NUMERIC,
    live_pledge      NUMERIC
);

CREATE INDEX IF NOT EXISTS spo_stake_history_pool_time_idx ON spo_stake_history (pool_id, recorded_at DESC);
CREATE INDEX IF NOT EXISTS spo_stake_history_epoch_idx ON spo_stake_history (mainchain_epoch);

-- Single-row state table to track paging cursor for stake refresh
CREATE TABLE IF NOT EXISTS spo_stake_refresh_state (
    id           BOOLEAN PRIMARY KEY DEFAULT TRUE,
    last_pool_id VARCHAR,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO spo_stake_refresh_state (id)
VALUES (TRUE)
ON CONFLICT (id) DO NOTHING;
