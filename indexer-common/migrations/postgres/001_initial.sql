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

CREATE TYPE TRANSACTION_VARIANT AS ENUM(
    'Regular',
    'System'
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
    variant TRANSACTION_VARIANT NOT NULL,
    hash BYTEA NOT NULL,
    protocol_version BIGINT NOT NULL,
    raw BYTEA NOT NULL
);

CREATE INDEX ON transactions(block_id);

CREATE INDEX ON transactions(hash);

CREATE TABLE regular_transactions(
    id BIGINT PRIMARY KEY REFERENCES transactions(id),
    transaction_result JSONB NOT NULL,
    merkle_tree_root BYTEA NOT NULL,
    start_index BIGINT NOT NULL,
    end_index BIGINT NOT NULL,
    paid_fees BYTEA,
    estimated_fees BYTEA,
    identifiers BYTEA[] NOT NULL
);

CREATE INDEX ON regular_transactions(transaction_result);

CREATE INDEX ON regular_transactions USING GIN(transaction_result);

CREATE INDEX ON regular_transactions(start_index);

CREATE INDEX ON regular_transactions(end_index);

CREATE TABLE contract_actions(
    id BIGSERIAL PRIMARY KEY,
    transaction_id BIGINT NOT NULL REFERENCES transactions(id),
    variant CONTRACT_ACTION_VARIANT NOT NULL,
    address BYTEA NOT NULL,
    state BYTEA NOT NULL,
    chain_state BYTEA NOT NULL,
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

CREATE TABLE dust_generation_info(
    id BIGSERIAL PRIMARY KEY,
    night_utxo_hash BYTEA NOT NULL,
    value BYTEA NOT NULL,
    owner BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    ctime BIGINT NOT NULL,
    merkle_index BIGINT NOT NULL,
    dtime BIGINT
);

CREATE INDEX ON dust_generation_info(OWNER);

CREATE INDEX ON dust_generation_info(night_utxo_hash);

CREATE TABLE dust_utxos(
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

CREATE INDEX ON dust_utxos(OWNER);

CREATE INDEX ON dust_utxos(generation_info_id);

CREATE INDEX ON dust_utxos(spent_at_transaction_id);

CREATE INDEX ON dust_utxos(substring(nullifier::TEXT, 1, 8))
WHERE
    nullifier IS NOT NULL;

CREATE TABLE cnight_registrations(
    id BIGSERIAL PRIMARY KEY,
    cardano_address BYTEA NOT NULL,
    dust_address BYTEA NOT NULL,
    is_valid BOOLEAN NOT NULL,
    registered_at BIGINT NOT NULL,
    removed_at BIGINT,
    block_id BIGINT REFERENCES blocks(id),
    UNIQUE (cardano_address, dust_address)
);

CREATE INDEX ON cnight_registrations(cardano_address);

CREATE INDEX ON cnight_registrations(dust_address);

CREATE INDEX ON cnight_registrations(block_id);

-- Create dust_utxo_mappings table for tracking UTXO-to-registration mappings
CREATE TABLE IF NOT EXISTS dust_utxo_mappings(
    id BIGSERIAL PRIMARY KEY,
    cardano_address BYTEA NOT NULL,
    dust_address BYTEA NOT NULL,
    utxo_id BYTEA NOT NULL,
    added_at BIGINT NOT NULL,
    removed_at BIGINT,
    block_id BIGINT REFERENCES blocks(id),
    UNIQUE (utxo_id)
);

CREATE INDEX ON dust_utxo_mappings(cardano_address);
CREATE INDEX ON dust_utxo_mappings(dust_address);
CREATE INDEX ON dust_utxo_mappings(block_id);

-- TODO: These tables are prepared for merkle tree storage but not populated yet.
-- The node needs to expose merkle tree update events before we can populate them.
-- The ledger has internal MerkleTree<DustGenerationInfo> and MerkleTree<()> for commitments,
-- but doesn't expose tree updates as events yet.
CREATE TABLE dust_commitment_tree(
    id BIGSERIAL PRIMARY KEY,
    block_height BIGINT NOT NULL,
    merkle_index BIGINT NOT NULL,
    root BYTEA NOT NULL,
    tree_data BYTEA NOT NULL
);

CREATE TABLE dust_generation_tree(
    id BIGSERIAL PRIMARY KEY,
    block_height BIGINT NOT NULL,
    merkle_index BIGINT NOT NULL,
    root BYTEA NOT NULL,
    tree_data BYTEA NOT NULL
);

CREATE INDEX ON dust_commitment_tree(merkle_index);
CREATE INDEX ON dust_generation_tree(merkle_index);

CREATE TABLE dust_events(
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

-- Reserve distribution tracking
-- Tracks when and how much NIGHT is distributed from the reserve pool
CREATE TABLE IF NOT EXISTS reserve_distributions (
    id BIGSERIAL PRIMARY KEY,
    transaction_id BIGINT NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    amount BYTEA NOT NULL, -- u128 as 16 bytes
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX ON reserve_distributions(transaction_id);
CREATE INDEX ON reserve_distributions(created_at);

-- Parameter updates tracking
-- Tracks changes to ledger parameters for audit trail
CREATE TABLE IF NOT EXISTS parameter_updates (
    id BIGSERIAL PRIMARY KEY,
    transaction_id BIGINT NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    parameters JSONB NOT NULL, -- Serialized LedgerParameters
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX ON parameter_updates(transaction_id);
CREATE INDEX ON parameter_updates(created_at);

-- NIGHT distribution tracking
-- Tracks NIGHT token distributions (claims)
CREATE TABLE IF NOT EXISTS night_distributions (
    id BIGSERIAL PRIMARY KEY,
    transaction_id BIGINT NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    claim_kind TEXT NOT NULL, -- Type of claim
    outputs JSONB NOT NULL, -- Serialized outputs
    total_amount BYTEA NOT NULL, -- u128 as 16 bytes
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX ON night_distributions(transaction_id);
CREATE INDEX ON night_distributions(created_at);

-- Treasury income tracking
-- Tracks income to treasury (e.g., from block rewards)
CREATE TABLE IF NOT EXISTS treasury_income (
    id BIGSERIAL PRIMARY KEY,
    transaction_id BIGINT NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    amount BYTEA NOT NULL, -- u128 as 16 bytes
    source TEXT NOT NULL, -- Source of income (e.g., 'block_rewards')
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX ON treasury_income(transaction_id);
CREATE INDEX ON treasury_income(created_at);

-- Treasury payments tracking
-- Tracks payments from treasury (both shielded and unshielded)
CREATE TABLE IF NOT EXISTS treasury_payments (
    id BIGSERIAL PRIMARY KEY,
    transaction_id BIGINT NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    payment_type TEXT NOT NULL, -- 'shielded' or 'unshielded'
    token_type TEXT NOT NULL, -- Token type being paid
    outputs JSONB NOT NULL, -- Serialized output instructions
    total_amount BYTEA, -- u128 as 16 bytes (optional, computed from outputs)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX ON treasury_payments(transaction_id);
CREATE INDEX ON treasury_payments(payment_type);
CREATE INDEX ON treasury_payments(created_at);

