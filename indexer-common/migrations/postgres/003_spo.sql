--------------------------------------------------------------------------------
-- SPO (Stake Pool Operator) tables for block explorer
--------------------------------------------------------------------------------

-- Epochs table
CREATE TABLE epochs (
    epoch_no BIGINT PRIMARY KEY,
    starts_at TIMESTAMPTZ NOT NULL,
    ends_at TIMESTAMPTZ NOT NULL
);

-- Pool metadata cache
CREATE TABLE pool_metadata_cache (
    pool_id VARCHAR PRIMARY KEY,
    hex_id VARCHAR UNIQUE,
    name TEXT,
    ticker TEXT,
    homepage_url TEXT,
    updated_at TIMESTAMPTZ,
    url TEXT
);

-- SPO identity
CREATE TABLE spo_identity (
    spo_sk VARCHAR PRIMARY KEY,
    sidechain_pubkey VARCHAR UNIQUE,

    pool_id VARCHAR REFERENCES pool_metadata_cache(pool_id),
    mainchain_pubkey VARCHAR UNIQUE,
    aura_pubkey VARCHAR UNIQUE
);

-- Committee membership
CREATE TABLE committee_membership (
    spo_sk VARCHAR,
    sidechain_pubkey VARCHAR,

    epoch_no BIGINT NOT NULL,
    position INT NOT NULL,
    expected_slots INT NOT NULL,
    PRIMARY KEY (epoch_no, position)
);

-- SPO epoch performance
CREATE TABLE spo_epoch_performance (
    spo_sk VARCHAR REFERENCES spo_identity(spo_sk),
    identity_label VARCHAR,
    epoch_no BIGINT NOT NULL,
    expected_blocks INT NOT NULL,
    produced_blocks INT NOT NULL,
    PRIMARY KEY (epoch_no, spo_sk)
);

-- SPO history
CREATE TABLE spo_history (
    spo_hist_sk BIGSERIAL PRIMARY KEY,
    spo_sk VARCHAR REFERENCES spo_identity(spo_sk),
    epoch_no BIGINT NOT NULL,
    status TEXT NOT NULL,
    valid_from BIGINT NOT NULL,
    valid_to BIGINT NOT NULL,
    UNIQUE (spo_sk, epoch_no)
);

-- Update "updated_at" field each time the record is updated
CREATE OR REPLACE FUNCTION set_updated_at_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER update_pool_metadata_cache_updated_at
BEFORE UPDATE ON pool_metadata_cache
FOR EACH ROW
EXECUTE FUNCTION set_updated_at_timestamp();

--------------------------------------------------------------------------------
-- Stake snapshot per pool (latest values)
-- Values are sourced from mainchain pool data (e.g., Blockfrost)
--------------------------------------------------------------------------------

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

--------------------------------------------------------------------------------
-- Stake history table and refresh state cursor
--------------------------------------------------------------------------------

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

-- Single-row state table to track paging cursor for stake refresh
CREATE TABLE IF NOT EXISTS spo_stake_refresh_state (
    id           BOOLEAN PRIMARY KEY DEFAULT TRUE,
    last_pool_id VARCHAR,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO spo_stake_refresh_state (id)
VALUES (TRUE)
ON CONFLICT (id) DO NOTHING;

--------------------------------------------------------------------------------
-- Indexes
--------------------------------------------------------------------------------

CREATE INDEX IF NOT EXISTS spo_identity_pk ON spo_identity (pool_id, sidechain_pubkey, aura_pubkey);
CREATE INDEX IF NOT EXISTS spo_history_epoch_no_idx ON spo_history (epoch_no);
CREATE INDEX IF NOT EXISTS committee_membership_epoch_no_idx ON committee_membership (epoch_no);
CREATE INDEX IF NOT EXISTS spo_epoch_performance_identity_pk ON spo_epoch_performance (epoch_no, identity_label);
CREATE INDEX IF NOT EXISTS spo_epoch_performance_epoch_no_idx ON spo_epoch_performance (epoch_no);
CREATE INDEX IF NOT EXISTS spo_stake_snapshot_updated_at_idx ON spo_stake_snapshot (updated_at DESC);
CREATE INDEX IF NOT EXISTS spo_stake_snapshot_live_stake_idx ON spo_stake_snapshot ((COALESCE(live_stake, 0)) DESC);
CREATE INDEX IF NOT EXISTS spo_stake_history_pool_time_idx ON spo_stake_history (pool_id, recorded_at DESC);
CREATE INDEX IF NOT EXISTS spo_stake_history_epoch_idx ON spo_stake_history (mainchain_epoch);
