CREATE TYPE APPLY_STAGE AS ENUM(
    'Success',
    'PartialSuccess',
    'Failure'
);

CREATE TYPE CONTRACT_ACTION_VARIANT AS ENUM(
    'Deploy',
    'Call',
    'Update'
);

CREATE TABLE blocks(
    id BIGSERIAL PRIMARY KEY,
    hash BYTEA NOT NULL UNIQUE,
    height BIGINT NOT NULL UNIQUE,
    protocol_version BIGINT NOT NULL,
    parent_hash BYTEA NOT NULL,
    author BYTEA,
    timestamp BIGINT NOT NULL
);

CREATE TABLE transactions(
    id BIGSERIAL PRIMARY KEY,
    block_id BIGINT NOT NULL REFERENCES blocks(id),
    hash BYTEA NOT NULL,
    protocol_version BIGINT NOT NULL,
    apply_stage APPLY_STAGE NOT NULL,
    identifiers BYTEA[] NOT NULL,
    raw BYTEA NOT NULL,
    merkle_tree_root BYTEA NOT NULL,
    start_index BIGINT NOT NULL,
    end_index BIGINT NOT NULL
);

CREATE INDEX ON transactions(block_id);

CREATE INDEX ON transactions(hash);

CREATE INDEX ON transactions(apply_stage);

CREATE INDEX ON transactions(end_index);

CREATE TABLE contract_actions(
    id BIGSERIAL PRIMARY KEY,
    transaction_id BIGINT NOT NULL REFERENCES transactions(id),
    address BYTEA NOT NULL,
    state BYTEA NOT NULL,
    zswap_state BYTEA NOT NULL,
    variant CONTRACT_ACTION_VARIANT NOT NULL,
    attributes JSONB NOT NULL
);

CREATE INDEX ON contract_actions(transaction_id);

CREATE INDEX ON contract_actions(address);

CREATE INDEX ON contract_actions(id, address);

CREATE TABLE wallets(
    id UUID PRIMARY KEY,
    session_id BYTEA NOT NULL UNIQUE,
    viewing_key BYTEA NOT NULL, -- Ciphertext with nonce, no longer unique!
    last_indexed_transaction_id BIGINT NOT NULL DEFAULT 0,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    last_active TIMESTAMPTZ NOT NULL
);

CREATE INDEX ON wallets(session_id);

CREATE INDEX ON wallets(last_indexed_transaction_id DESC);

CREATE TABLE relevant_transactions(
    id BIGSERIAL PRIMARY KEY,
    wallet_id UUID NOT NULL REFERENCES wallets(id),
    transaction_id BIGINT NOT NULL REFERENCES transactions(id),
    UNIQUE (wallet_id, transaction_id)
);

