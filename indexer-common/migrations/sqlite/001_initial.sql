--------------------------------------------------------------------------------
-- blocks
--------------------------------------------------------------------------------
CREATE TABLE blocks (
  id INTEGER PRIMARY KEY,
  hash BLOB NOT NULL UNIQUE,
  height INTEGER NOT NULL,
  protocol_version INTEGER NOT NULL,
  parent_hash BLOB NOT NULL,
  author BLOB,
  timestamp INTEGER NOT NULL,
  ledger_parameters BLOB NOT NULL
);
--------------------------------------------------------------------------------
-- transactions
--------------------------------------------------------------------------------
CREATE TABLE transactions (
  id INTEGER PRIMARY KEY,
  block_id INTEGER NOT NULL REFERENCES blocks (id),
  variant TEXT CHECK (variant IN ('Regular', 'System')) NOT NULL,
  hash BLOB NOT NULL,
  protocol_version INTEGER NOT NULL,
  raw BLOB NOT NULL
);
CREATE INDEX transactions_block_id_idx ON transactions (block_id);
CREATE INDEX transactions_hash_idx ON transactions (hash);
--------------------------------------------------------------------------------
-- regular_transactions
--------------------------------------------------------------------------------
CREATE TABLE regular_transactions (
  id INTEGER PRIMARY KEY REFERENCES transactions (id),
  transaction_result TEXT NOT NULL,
  merkle_tree_root BLOB NOT NULL,
  start_index INTEGER NOT NULL,
  end_index INTEGER NOT NULL,
  paid_fees BLOB,
  estimated_fees BLOB
);
CREATE INDEX regular_transactions_transaction_result_idx ON regular_transactions (transaction_result);
CREATE INDEX regular_transactions_start_idx ON regular_transactions (start_index);
CREATE INDEX regular_transactions_end_idx ON regular_transactions (end_index);
--------------------------------------------------------------------------------
-- transaction_identifiers
--------------------------------------------------------------------------------
CREATE TABLE transaction_identifiers (
  id INTEGER PRIMARY KEY,
  transaction_id INTEGER NOT NULL REFERENCES regular_transactions (id),
  identifier BLOB NOT NULL
);
CREATE INDEX transaction_identifiers_transaction_id_idx ON transaction_identifiers (transaction_id);
CREATE INDEX transaction_identifiers_identifier_idx ON transaction_identifiers (identifier);
--------------------------------------------------------------------------------
-- contract_actions
--------------------------------------------------------------------------------
CREATE TABLE contract_actions (
  id INTEGER PRIMARY KEY,
  transaction_id INTEGER NOT NULL REFERENCES transactions (id),
  variant TEXT CHECK (variant IN ('Deploy', 'Call', 'Update')) NOT NULL,
  address BLOB NOT NULL,
  state BLOB NOT NULL,
  chain_state BLOB NOT NULL,
  attributes TEXT NOT NULL
);
CREATE INDEX contract_actions_transaction_id_idx ON contract_actions (transaction_id);
CREATE INDEX contract_actions_address_idx ON contract_actions (address);
CREATE INDEX contract_actions_id_address_idx ON contract_actions (id, address);
--------------------------------------------------------------------------------
-- unshielded_utxos
--------------------------------------------------------------------------------
CREATE TABLE unshielded_utxos (
  id INTEGER PRIMARY KEY,
  creating_transaction_id INTEGER NOT NULL REFERENCES transactions (id),
  spending_transaction_id INTEGER REFERENCES transactions (id),
  owner BLOB NOT NULL,
  token_type BLOB NOT NULL,
  value BLOB NOT NULL,
  output_index INTEGER NOT NULL,
  intent_hash BLOB NOT NULL,
  initial_nonce BLOB NOT NULL,
  registered_for_dust_generation INTEGER NOT NULL,
  UNIQUE (intent_hash, output_index)
);
CREATE INDEX unshielded_creating_idx ON unshielded_utxos (creating_transaction_id);
CREATE INDEX unshielded_spending_idx ON unshielded_utxos (spending_transaction_id);
CREATE INDEX unshielded_owner_idx ON unshielded_utxos (owner);
CREATE INDEX unshielded_token_type_idx ON unshielded_utxos (token_type);
--------------------------------------------------------------------------------
-- ledger_events
--------------------------------------------------------------------------------
CREATE TABLE ledger_events (
  id INTEGER PRIMARY KEY,
  transaction_id INTEGER NOT NULL REFERENCES transactions (id),
  variant TEXT CHECK (
    variant IN (
      'ZswapInput',
      'ZswapOutput',
      'ParamChange',
      'DustInitialUtxo',
      'DustGenerationDtimeUpdate',
      'DustSpendProcessed'
    )
  ) NOT NULL,
  grouping TEXT CHECK (grouping IN ('Zswap', 'Dust')) NOT NULL,
  raw BYTEA NOT NULL,
  attributes TEXT NOT NULL
);
CREATE INDEX ledger_events_transaction_id_idx ON ledger_events (transaction_id);
CREATE INDEX ledger_events_variant_idx ON ledger_events (variant);
CREATE INDEX ledger_events_grouping_idx ON ledger_events (grouping);
CREATE INDEX ledger_events_id_grouping_idx ON ledger_events (id, grouping);
CREATE INDEX ledger_events_transaction_id_grouping_idx ON ledger_events (transaction_id, grouping);
--------------------------------------------------------------------------------
-- contract_balances
--------------------------------------------------------------------------------
CREATE TABLE contract_balances (
  id INTEGER PRIMARY KEY,
  contract_action_id INTEGER NOT NULL REFERENCES contract_actions (id),
  token_type BLOB NOT NULL, -- Serialized TokenType (hex-encoded)
  amount BLOB NOT NULL, -- u128 amount as bytes (for large number support)
  UNIQUE (contract_action_id, token_type)
);
CREATE INDEX contract_balances_action_idx ON contract_balances (contract_action_id);
CREATE INDEX contract_balances_token_type_idx ON contract_balances (token_type);
CREATE INDEX contract_balances_action_token_idx ON contract_balances (contract_action_id, token_type);
--------------------------------------------------------------------------------
-- wallets
--------------------------------------------------------------------------------
CREATE TABLE wallets (
  id BLOB PRIMARY KEY, -- UUID
  session_id BLOB NOT NULL UNIQUE,
  viewing_key BLOB NOT NULL, -- Ciphertext with nonce, no longer unique!
  last_indexed_transaction_id INTEGER NOT NULL DEFAULT 0,
  active BOOLEAN NOT NULL DEFAULT TRUE,
  last_active INTEGER NOT NULL
);
CREATE INDEX wallets_session_id_idx ON wallets (session_id);
CREATE INDEX wallets_last_indexed_transaction_id_idx ON wallets (last_indexed_transaction_id DESC);
--------------------------------------------------------------------------------
-- relevant_transactions
--------------------------------------------------------------------------------
CREATE TABLE relevant_transactions (
  id INTEGER PRIMARY KEY,
  wallet_id BLOB NOT NULL REFERENCES wallets (id),
  transaction_id INTEGER NOT NULL REFERENCES transactions (id),
  UNIQUE (wallet_id, transaction_id)
);
--------------------------------------------------------------------------------
-- CNGD Projection Tables
-- These tables provide projection layer for DUST features
--------------------------------------------------------------------------------

-- DUST generation information tracking
CREATE TABLE dust_generation_info(
    id INTEGER PRIMARY KEY,
    night_utxo_hash BLOB NOT NULL,
    value BLOB NOT NULL,
    owner BLOB NOT NULL,
    nonce BLOB NOT NULL,
    ctime INTEGER NOT NULL,
    merkle_index INTEGER NOT NULL,
    dtime INTEGER
);

CREATE INDEX dust_generation_info_owner_idx ON dust_generation_info(owner);
CREATE INDEX dust_generation_info_utxo_idx ON dust_generation_info(night_utxo_hash);

-- DUST UTXOs tracking
CREATE TABLE dust_utxos(
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
CREATE INDEX dust_utxos_nullifier_prefix_idx ON dust_utxos(substr(hex(nullifier), 1, 8))
WHERE
    nullifier IS NOT NULL;

-- cNIGHT registration tracking
CREATE TABLE cnight_registrations(
    id INTEGER PRIMARY KEY,
    cardano_address BLOB NOT NULL,
    dust_address BLOB NOT NULL,
    is_valid BOOLEAN NOT NULL,
    registered_at INTEGER NOT NULL,
    removed_at INTEGER,
    block_id INTEGER REFERENCES blocks(id),
    UNIQUE (cardano_address, dust_address)
);

CREATE INDEX cnight_registrations_cardano_addr_idx ON cnight_registrations(cardano_address);
CREATE INDEX cnight_registrations_dust_addr_idx ON cnight_registrations(dust_address);
CREATE INDEX cnight_registrations_block_id_idx ON cnight_registrations(block_id);

-- UTXO-to-registration mappings
CREATE TABLE IF NOT EXISTS dust_utxo_mappings(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cardano_address BLOB NOT NULL,
    dust_address BLOB NOT NULL,
    utxo_id BLOB NOT NULL,
    added_at INTEGER NOT NULL,
    removed_at INTEGER,
    block_id INTEGER REFERENCES blocks(id),
    UNIQUE (utxo_id)
);

CREATE INDEX dust_utxo_mappings_cardano_addr_idx ON dust_utxo_mappings(cardano_address);
CREATE INDEX dust_utxo_mappings_dust_addr_idx ON dust_utxo_mappings(dust_address);
CREATE INDEX dust_utxo_mappings_block_id_idx ON dust_utxo_mappings(block_id);

-- Merkle tree storage for DUST commitments
-- TODO: These tables are prepared for merkle tree storage but not populated yet.
-- The node needs to expose merkle tree update events before we can populate them.
-- The ledger has internal MerkleTree<DustGenerationInfo> and MerkleTree<()> for commitments,
-- but doesn't expose tree updates as events yet.
CREATE TABLE dust_commitment_tree(
    id INTEGER PRIMARY KEY,
    block_height INTEGER NOT NULL,
    merkle_index INTEGER NOT NULL UNIQUE,
    root BLOB NOT NULL,
    tree_data TEXT NOT NULL -- JSON stored as TEXT in SQLite
);

CREATE TABLE dust_generation_tree(
    id INTEGER PRIMARY KEY,
    block_height INTEGER NOT NULL,
    merkle_index INTEGER NOT NULL UNIQUE,
    root BLOB NOT NULL,
    tree_data TEXT NOT NULL -- JSON stored as TEXT in SQLite
);

CREATE INDEX dust_commitment_tree_merkle_idx ON dust_commitment_tree(merkle_index);
CREATE INDEX dust_generation_tree_merkle_idx ON dust_generation_tree(merkle_index);

-- DUST events projection (parsed from ledger_events)
CREATE TABLE dust_events(
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

-- System transaction metadata tracking
-- Reserve distribution tracking
CREATE TABLE IF NOT EXISTS reserve_distributions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id INTEGER NOT NULL,
    amount BLOB NOT NULL, -- u128 as 16 bytes
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (transaction_id) REFERENCES transactions(id) ON DELETE CASCADE
);

CREATE INDEX reserve_distributions_transaction_idx ON reserve_distributions(transaction_id);
CREATE INDEX reserve_distributions_created_at_idx ON reserve_distributions(created_at);

-- Parameter updates tracking
CREATE TABLE IF NOT EXISTS parameter_updates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id INTEGER NOT NULL,
    parameters TEXT NOT NULL, -- Serialized LedgerParameters as JSON
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (transaction_id) REFERENCES transactions(id) ON DELETE CASCADE
);

CREATE INDEX parameter_updates_transaction_idx ON parameter_updates(transaction_id);
CREATE INDEX parameter_updates_created_at_idx ON parameter_updates(created_at);

-- NIGHT distribution tracking
CREATE TABLE IF NOT EXISTS night_distributions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id INTEGER NOT NULL,
    claim_kind TEXT NOT NULL, -- Type of claim
    outputs TEXT NOT NULL, -- Serialized outputs as JSON
    total_amount BLOB NOT NULL, -- u128 as 16 bytes
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (transaction_id) REFERENCES transactions(id) ON DELETE CASCADE
);

CREATE INDEX night_distributions_transaction_idx ON night_distributions(transaction_id);
CREATE INDEX night_distributions_created_at_idx ON night_distributions(created_at);

-- Treasury income tracking
CREATE TABLE IF NOT EXISTS treasury_income (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id INTEGER NOT NULL,
    amount BLOB NOT NULL, -- u128 as 16 bytes
    source TEXT NOT NULL, -- Source of income (e.g., 'block_rewards')
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (transaction_id) REFERENCES transactions(id) ON DELETE CASCADE
);

CREATE INDEX treasury_income_transaction_idx ON treasury_income(transaction_id);
CREATE INDEX treasury_income_created_at_idx ON treasury_income(created_at);

-- Treasury payments tracking
CREATE TABLE IF NOT EXISTS treasury_payments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id INTEGER NOT NULL,
    payment_type TEXT NOT NULL, -- 'shielded' or 'unshielded'
    token_type TEXT NOT NULL, -- Token type being paid
    outputs TEXT NOT NULL, -- Serialized output instructions as JSON
    total_amount BLOB, -- u128 as 16 bytes (optional, computed from outputs)
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (transaction_id) REFERENCES transactions(id) ON DELETE CASCADE
);

CREATE INDEX treasury_payments_transaction_idx ON treasury_payments(transaction_id);
CREATE INDEX treasury_payments_payment_type_idx ON treasury_payments(payment_type);
CREATE INDEX treasury_payments_created_at_idx ON treasury_payments(created_at);
