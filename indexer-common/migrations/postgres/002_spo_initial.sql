CREATE TABLE epochs (
    epoch_no BIGINT PRIMARY KEY,
    starts_at TIMESTAMPTZ NOT NULL,
    ends_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE pool_metadata_cache (
    pool_id VARCHAR PRIMARY KEY,
    hex_id VARCHAR UNIQUE,
    name TEXT,
    ticker TEXT,
    homepage_url TEXT,
    updated_at TIMESTAMPTZ,
    url TEXT
);

CREATE TABLE spo_identity (
    spo_sk VARCHAR PRIMARY KEY,
    sidechain_pubkey VARCHAR UNIQUE,

    pool_id VARCHAR REFERENCES pool_metadata_cache(pool_id),
    mainchain_pubkey VARCHAR UNIQUE,
    aura_pubkey VARCHAR UNIQUE
);

CREATE TABLE stg_committee (
    epoch_no BIGINT NOT NULL,
    position INT NOT NULL,
    sidechain_pubkey VARCHAR NOT NULL,
    arrived_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE committee_membership (
    spo_sk VARCHAR,
    sidechain_pubkey VARCHAR,

    epoch_no BIGINT NOT NULL,
    position INT NOT NULL,
    expected_slots INT NOT NULL,
    PRIMARY KEY (epoch_no, position)
);

CREATE TABLE spo_epoch_performance (
    spo_sk VARCHAR REFERENCES spo_identity(spo_sk),
    identity_label VARCHAR,
    epoch_no BIGINT NOT NULL,
    expected_blocks INT NOT NULL,
    produced_blocks INT NOT NULL,
    PRIMARY KEY (epoch_no, spo_sk)
);

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

-- indexes
CREATE INDEX IF NOT EXISTS spo_identity_pk ON spo_identity (pool_id, sidechain_pubkey, aura_pubkey);

CREATE INDEX IF NOT EXISTS spo_history_epoch_no_idx ON spo_history (epoch_no);

CREATE INDEX IF NOT EXISTS committee_membership_epoch_no_idx ON committee_membership (epoch_no);

CREATE INDEX IF NOT EXISTS spo_epoch_performance_identity_pk ON spo_epoch_performance (epoch_no, identity_label);
CREATE INDEX IF NOT EXISTS spo_epoch_performance_epoch_no_idx ON spo_epoch_performance (epoch_no);
