CREATE TABLE blocks(
    id INTEGER PRIMARY KEY,
    hash BLOB NOT NULL UNIQUE,
    height INTEGER NOT NULL,
    protocol_version INTEGER NOT NULL,
    parent_hash BLOB NOT NULL,
    author BLOB,
    timestamp INTEGER NOT NULL
);

CREATE TABLE transactions(
    id INTEGER PRIMARY KEY,
    block_id INTEGER NOT NULL,
    hash BLOB NOT NULL,
    protocol_version INTEGER NOT NULL,
    transaction_result TEXT NOT NULL,
    raw BLOB NOT NULL,
    merkle_tree_root BLOB NOT NULL,
    start_index INTEGER NOT NULL,
    end_index INTEGER NOT NULL,
    paid_fees BLOB,
    estimated_fees BLOB,
    FOREIGN KEY (block_id) REFERENCES blocks(id)
);

CREATE INDEX transactions_block_id ON transactions(block_id);

CREATE INDEX transactions_hash ON transactions(hash);

CREATE INDEX transactions_transaction_result ON transactions(transaction_result);

CREATE INDEX transactions_start_index ON transactions(start_index);

CREATE INDEX transactions_end_index ON transactions(end_index);

CREATE TABLE transaction_identifiers(
    id INTEGER PRIMARY KEY,
    transaction_id INTEGER NOT NULL,
    identifier BLOB NOT NULL,
    FOREIGN KEY (transaction_id) REFERENCES transactions(id)
);

CREATE INDEX transaction_identifiers_transaction_id ON transaction_identifiers(transaction_id);

CREATE INDEX transaction_identifiers_identifier ON transaction_identifiers(identifier);

CREATE TABLE contract_actions(
    id INTEGER PRIMARY KEY,
    transaction_id INTEGER NOT NULL,
    address BLOB NOT NULL,
    state BLOB NOT NULL,
    zswap_state BLOB NOT NULL,
    variant TEXT CHECK (variant IN ('Deploy', 'Call', 'Update')) NOT NULL,
    attributes TEXT NOT NULL,
    FOREIGN KEY (transaction_id) REFERENCES transactions(id)
);

CREATE INDEX contract_actions_transaction_id ON contract_actions(transaction_id);

CREATE INDEX contract_actions_address ON contract_actions(address);

CREATE INDEX contract_actions_id_address ON contract_actions(id, address);

CREATE TABLE wallets(
    id BLOB PRIMARY KEY, -- UUID
    session_id BLOB NOT NULL UNIQUE,
    viewing_key BLOB NOT NULL, -- Ciphertext with nonce, no longer unique!
    last_indexed_transaction_id INTEGER NOT NULL DEFAULT 0,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    last_active INTEGER NOT NULL
);

CREATE INDEX wallets_session_id ON wallets(session_id);

CREATE INDEX wallets_last_indexed_transaction_id ON wallets(last_indexed_transaction_id DESC);

CREATE TABLE relevant_transactions(
    id INTEGER PRIMARY KEY,
    wallet_id BLOB NOT NULL,
    transaction_id INTEGER NOT NULL,
    FOREIGN KEY (wallet_id) REFERENCES wallets(id),
    FOREIGN KEY (transaction_id) REFERENCES transactions(id),
    UNIQUE (wallet_id, transaction_id)
);

CREATE TABLE unshielded_utxos(
    id INTEGER PRIMARY KEY,
    creating_transaction_id INTEGER NOT NULL,
    spending_transaction_id INTEGER,
    owner BLOB NOT NULL,
    token_type BLOB NOT NULL,
    value BLOB NOT NULL,
    output_index INTEGER NOT NULL,
    intent_hash BLOB NOT NULL,
    FOREIGN KEY (creating_transaction_id) REFERENCES transactions(id),
    FOREIGN KEY (spending_transaction_id) REFERENCES transactions(id),
    UNIQUE (intent_hash, output_index)
);

CREATE INDEX unshielded_creating_idx ON unshielded_utxos(creating_transaction_id);

CREATE INDEX unshielded_spending_idx ON unshielded_utxos(spending_transaction_id);

CREATE INDEX unshielded_owner_idx ON unshielded_utxos(owner);

CREATE INDEX unshielded_token_type_idx ON unshielded_utxos(token_type);

CREATE TABLE zswap_state(
    id BLOB PRIMARY KEY, -- UUID
    value BLOB NOT NULL,
    last_index INTEGER
);

CREATE TABLE contract_balances(
    id INTEGER PRIMARY KEY,
    contract_action_id INTEGER NOT NULL REFERENCES contract_actions(id),
    token_type BLOB NOT NULL, -- Serialized TokenType (hex-encoded)
    amount BLOB NOT NULL, -- u128 amount as bytes (for large number support)
    UNIQUE (contract_action_id, token_type)
);

CREATE INDEX contract_balances_action_idx ON contract_balances(contract_action_id);

CREATE INDEX contract_balances_token_type_idx ON contract_balances(token_type);

CREATE INDEX contract_balances_action_token_idx ON contract_balances(contract_action_id, token_type);

CREATE TABLE dust_generation_info (
    id INTEGER PRIMARY KEY,
    night_utxo_hash BLOB NOT NULL,
    value BLOB NOT NULL,
    owner BLOB NOT NULL,
    nonce BLOB NOT NULL,
    ctime INTEGER NOT NULL,
    index INTEGER NOT NULL,
    dtime INTEGER
);

CREATE INDEX dust_generation_info_owner_idx ON dust_generation_info(owner);
CREATE INDEX dust_generation_info_utxo_idx ON dust_generation_info(night_utxo_hash);

CREATE TABLE dust_utxos (
    id INTEGER PRIMARY KEY,
    generation_info_id INTEGER NOT NULL,
    spent_at_transaction_id INTEGER,
    commitment BLOB NOT NULL,
    initial_value BLOB NOT NULL,
    owner BLOB NOT NULL,
    nonce BLOB NOT NULL,
    seq INTEGER NOT NULL,
    ctime INTEGER NOT NULL,
    nullifier BLOB,
    FOREIGN KEY (generation_info_id) REFERENCES dust_generation_info(id),
    FOREIGN KEY (spent_at_transaction_id) REFERENCES transactions(id)
);

CREATE INDEX dust_utxos_owner_idx ON dust_utxos(owner);
CREATE INDEX dust_utxos_generation_idx ON dust_utxos(generation_info_id);
CREATE INDEX dust_utxos_spent_idx ON dust_utxos(spent_at_transaction_id);
CREATE INDEX dust_utxos_nullifier_prefix_idx ON dust_utxos(substr(hex(nullifier), 1, 8)) WHERE nullifier IS NOT NULL;

CREATE TABLE cnight_registrations (
    id INTEGER PRIMARY KEY,
    cardano_address BLOB NOT NULL,
    dust_address BLOB NOT NULL,
    is_valid BOOLEAN NOT NULL,
    registered_at INTEGER NOT NULL,
    removed_at INTEGER,
    UNIQUE(cardano_address, dust_address)
);

CREATE INDEX cnight_registrations_cardano_addr_idx ON cnight_registrations(cardano_address);
CREATE INDEX cnight_registrations_dust_addr_idx ON cnight_registrations(dust_address);

-- TODO: These tables are for future merkle tree storage once ledger integration is complete.
CREATE TABLE dust_commitment_tree (
    id INTEGER PRIMARY KEY,
    block_height INTEGER NOT NULL,
    root BLOB NOT NULL,
    tree_data BLOB NOT NULL
);

CREATE TABLE dust_generation_tree (
    id INTEGER PRIMARY KEY,
    block_height INTEGER NOT NULL,
    root BLOB NOT NULL,
    tree_data BLOB NOT NULL
);

CREATE TABLE dust_events (
    id INTEGER PRIMARY KEY,
    transaction_id INTEGER NOT NULL,
    transaction_hash BLOB NOT NULL,
    logical_segment INTEGER NOT NULL,
    physical_segment INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    event_data TEXT NOT NULL,
    FOREIGN KEY (transaction_id) REFERENCES transactions(id)
);

CREATE INDEX dust_events_transaction_idx ON dust_events(transaction_id);
CREATE INDEX dust_events_type_idx ON dust_events(event_type);

