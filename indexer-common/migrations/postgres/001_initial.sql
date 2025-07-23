CREATE TYPE CONTRACT_ACTION_VARIANT AS ENUM(
    'Deploy',
    'Call',
    'Update'
);

CREATE TYPE DUST_EVENT_TYPE AS ENUM(
    'DustInitialUtxo',
    'DustGenerationDtimeUpdate',
    'DustSpendProcessed'
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
    transaction_result JSONB NOT NULL,
    identifiers BYTEA[] NOT NULL,
    raw BYTEA NOT NULL,
    merkle_tree_root BYTEA NOT NULL,
    start_index BIGINT NOT NULL,
    end_index BIGINT NOT NULL,
    paid_fees BYTEA,
    estimated_fees BYTEA
);

CREATE INDEX ON transactions(block_id);

CREATE INDEX ON transactions(hash);

CREATE INDEX ON transactions(transaction_result);

CREATE INDEX ON transactions(start_index);

CREATE INDEX ON transactions(end_index);

CREATE INDEX ON transactions USING GIN(transaction_result);

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

CREATE TABLE unshielded_utxos(
    id BIGSERIAL PRIMARY KEY,
    creating_transaction_id BIGINT NOT NULL REFERENCES transactions(id),
    spending_transaction_id BIGINT REFERENCES transactions(id),
    owner BYTEA NOT NULL,
    token_type BYTEA NOT NULL,
    value BYTEA NOT NULL,
    output_index BIGINT NOT NULL,
    intent_hash BYTEA NOT NULL,
    UNIQUE (intent_hash, output_index)
);

CREATE INDEX ON unshielded_utxos(creating_transaction_id);

CREATE INDEX ON unshielded_utxos(spending_transaction_id);

CREATE INDEX ON unshielded_utxos(OWNER);

CREATE INDEX ON unshielded_utxos(token_type);

CREATE TABLE contract_balances(
    id BIGSERIAL PRIMARY KEY,
    contract_action_id BIGINT NOT NULL REFERENCES contract_actions(id),
    token_type BYTEA NOT NULL, -- Serialized TokenType (hex-encoded)
    amount BYTEA NOT NULL, -- u128 amount as bytes (for large number support)
    UNIQUE (contract_action_id, token_type)
);

CREATE INDEX ON contract_balances(contract_action_id);

CREATE INDEX ON contract_balances(token_type);

CREATE INDEX ON contract_balances(contract_action_id, token_type);

CREATE TABLE dust_generation_info (
    id BIGSERIAL PRIMARY KEY,
    night_utxo_hash BYTEA NOT NULL,
    value BYTEA NOT NULL,
    owner BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    ctime BIGINT NOT NULL,
    index BIGINT NOT NULL,
    dtime BIGINT
);

CREATE INDEX ON dust_generation_info(owner);
CREATE INDEX ON dust_generation_info(night_utxo_hash);

CREATE TABLE dust_utxos (
    id BIGSERIAL PRIMARY KEY,
    generation_info_id BIGINT NOT NULL REFERENCES dust_generation_info(id),
    spent_at_transaction_id BIGINT REFERENCES transactions(id),
    commitment BYTEA NOT NULL,
    initial_value BYTEA NOT NULL,
    owner BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    seq INTEGER NOT NULL,
    ctime BIGINT NOT NULL,
    nullifier BYTEA
);

CREATE INDEX ON dust_utxos(owner);
CREATE INDEX ON dust_utxos(generation_info_id);
CREATE INDEX ON dust_utxos(spent_at_transaction_id);
CREATE INDEX ON dust_utxos(substring(nullifier::text, 1, 8)) WHERE nullifier IS NOT NULL;

CREATE TABLE cnight_registrations (
    id BIGSERIAL PRIMARY KEY,
    cardano_address BYTEA NOT NULL,
    dust_address BYTEA NOT NULL,
    is_valid BOOLEAN NOT NULL,
    registered_at BIGINT NOT NULL,
    removed_at BIGINT,
    UNIQUE(cardano_address, dust_address)
);

CREATE INDEX ON cnight_registrations(cardano_address);
CREATE INDEX ON cnight_registrations(dust_address);

-- TODO: These tables are for future merkle tree storage once ledger integration is complete.
CREATE TABLE dust_commitment_tree (
    id BIGSERIAL PRIMARY KEY,
    block_height BIGINT NOT NULL,
    root BYTEA NOT NULL,
    tree_data BYTEA NOT NULL
);

CREATE TABLE dust_generation_tree (
    id BIGSERIAL PRIMARY KEY,
    block_height BIGINT NOT NULL,
    root BYTEA NOT NULL,
    tree_data BYTEA NOT NULL
);

CREATE TABLE dust_events (
    id BIGSERIAL PRIMARY KEY,
    transaction_id BIGINT NOT NULL REFERENCES transactions(id),
    transaction_hash BYTEA NOT NULL,
    logical_segment INTEGER NOT NULL,
    physical_segment INTEGER NOT NULL,
    event_type DUST_EVENT_TYPE NOT NULL,
    event_data JSONB NOT NULL
);

CREATE INDEX ON dust_events(transaction_id);
CREATE INDEX ON dust_events(event_type);

