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

CREATE TABLE relevant_transactions(
    id INTEGER PRIMARY KEY,
    wallet_id BLOB NOT NULL,
    transaction_id INTEGER NOT NULL,
    FOREIGN KEY (wallet_id) REFERENCES wallets(id),
    FOREIGN KEY (transaction_id) REFERENCES transactions(id),
    UNIQUE (wallet_id, transaction_id)
);

CREATE TABLE zswap_state(
    id BLOB PRIMARY KEY, -- UUID
    value BLOB NOT NULL,
    last_index INTEGER
);

CREATE TABLE unshielded_utxos(
    id INTEGER PRIMARY KEY,
    creating_transaction_id INTEGER NOT NULL,
    output_index INTEGER NOT NULL,
    owner_address BLOB NOT NULL,
    token_type BLOB NOT NULL,
    intent_hash BLOB NOT NULL,
    value BLOB NOT NULL,
    spending_transaction_id INTEGER,
    FOREIGN KEY (creating_transaction_id) REFERENCES transactions(id),
    FOREIGN KEY (spending_transaction_id) REFERENCES transactions(id),
    UNIQUE (creating_transaction_id, output_index)
);

CREATE INDEX unshielded_owner_idx ON unshielded_utxos(owner_address);

CREATE INDEX unshielded_token_type_idx ON unshielded_utxos(token_type);

CREATE INDEX unshielded_spent_idx ON unshielded_utxos(spending_transaction_id);
