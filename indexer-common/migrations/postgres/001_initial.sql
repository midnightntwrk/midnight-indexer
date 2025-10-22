--------------------------------------------------------------------------------
-- types
--------------------------------------------------------------------------------
CREATE TYPE CONTRACT_ACTION_VARIANT AS ENUM('Deploy', 'Call', 'Update');
CREATE TYPE LEDGER_EVENT_GROUPING AS ENUM('Zswap', 'Dust');
CREATE TYPE LEDGER_EVENT_VARIANT AS ENUM(
  'ZswapInput',
  'ZswapOutput',
  'ParamChange',
  'DustInitialUtxo',
  'DustGenerationDtimeUpdate',
  'DustSpendProcessed'
);
CREATE TYPE TRANSACTION_VARIANT AS ENUM('Regular', 'System');
--------------------------------------------------------------------------------
-- blocks
--------------------------------------------------------------------------------
CREATE TABLE blocks (
  id BIGSERIAL PRIMARY KEY,
  hash BYTEA NOT NULL UNIQUE,
  height BIGINT NOT NULL UNIQUE,
  protocol_version BIGINT NOT NULL,
  parent_hash BYTEA NOT NULL,
  author BYTEA,
  timestamp BIGINT NOT NULL,
  ledger_parameters BYTEA NOT NULL
);
--------------------------------------------------------------------------------
-- transactions
--------------------------------------------------------------------------------
CREATE TABLE transactions (
  id BIGSERIAL PRIMARY KEY,
  block_id BIGINT NOT NULL REFERENCES blocks (id),
  variant TRANSACTION_VARIANT NOT NULL,
  hash BYTEA NOT NULL,
  protocol_version BIGINT NOT NULL,
  raw BYTEA NOT NULL
);
CREATE INDEX ON transactions (block_id);
CREATE INDEX ON transactions (hash);
--------------------------------------------------------------------------------
-- regular_transactions
--------------------------------------------------------------------------------
CREATE TABLE regular_transactions (
  id BIGINT PRIMARY KEY REFERENCES transactions (id),
  transaction_result JSONB NOT NULL,
  merkle_tree_root BYTEA NOT NULL,
  start_index BIGINT NOT NULL,
  end_index BIGINT NOT NULL,
  paid_fees BYTEA,
  estimated_fees BYTEA,
  identifiers BYTEA[] NOT NULL
);
CREATE INDEX ON regular_transactions (transaction_result);
CREATE INDEX ON regular_transactions USING GIN (transaction_result);
CREATE INDEX ON regular_transactions (start_index);
CREATE INDEX ON regular_transactions (end_index);
--------------------------------------------------------------------------------
-- contract_actions
--------------------------------------------------------------------------------
CREATE TABLE contract_actions (
  id BIGSERIAL PRIMARY KEY,
  transaction_id BIGINT NOT NULL REFERENCES transactions (id),
  variant CONTRACT_ACTION_VARIANT NOT NULL,
  address BYTEA NOT NULL,
  state BYTEA NOT NULL,
  zswap_state BYTEA NOT NULL,
  attributes JSONB NOT NULL
);
CREATE INDEX ON contract_actions (transaction_id);
CREATE INDEX ON contract_actions (address);
CREATE INDEX ON contract_actions (id, address);
--------------------------------------------------------------------------------
-- unshielded_utxos
--------------------------------------------------------------------------------
CREATE TABLE unshielded_utxos (
  id BIGSERIAL PRIMARY KEY,
  creating_transaction_id BIGINT NOT NULL REFERENCES transactions (id),
  spending_transaction_id BIGINT REFERENCES transactions (id),
  owner BYTEA NOT NULL,
  token_type BYTEA NOT NULL,
  value BYTEA NOT NULL,
  intent_hash BYTEA NOT NULL,
  output_index BIGINT NOT NULL,
  ctime BIGINT,
  initial_nonce BYTEA NOT NULL,
  registered_for_dust_generation BOOLEAN NOT NULL,
  UNIQUE (intent_hash, output_index)
);
CREATE INDEX ON unshielded_utxos (creating_transaction_id);
CREATE INDEX ON unshielded_utxos (spending_transaction_id);
CREATE INDEX ON unshielded_utxos (owner);
CREATE INDEX ON unshielded_utxos (creating_transaction_id, owner);
CREATE INDEX ON unshielded_utxos (spending_transaction_id, owner);
CREATE INDEX ON unshielded_utxos (token_type);
--------------------------------------------------------------------------------
-- ledger_events
--------------------------------------------------------------------------------
CREATE TABLE ledger_events (
  id BIGSERIAL PRIMARY KEY,
  transaction_id BIGINT NOT NULL REFERENCES transactions (id),
  variant LEDGER_EVENT_VARIANT NOT NULL,
  grouping LEDGER_EVENT_GROUPING NOT NULL,
  raw BYTEA NOT NULL,
  attributes JSONB NOT NULL
);
CREATE INDEX ON ledger_events (transaction_id);
CREATE INDEX ON ledger_events (variant);
CREATE INDEX ON ledger_events (grouping);
CREATE INDEX ON ledger_events (id, grouping);
CREATE INDEX ON ledger_events (transaction_id, grouping);
--------------------------------------------------------------------------------
-- contract_balances
--------------------------------------------------------------------------------
CREATE TABLE contract_balances (
  id BIGSERIAL PRIMARY KEY,
  contract_action_id BIGINT NOT NULL REFERENCES contract_actions (id),
  token_type BYTEA NOT NULL, -- Serialized TokenType (hex-encoded)
  amount BYTEA NOT NULL, -- u128 amount as bytes (for large number support)
  UNIQUE (contract_action_id, token_type)
);
CREATE INDEX ON contract_balances (contract_action_id);
CREATE INDEX ON contract_balances (token_type);
CREATE INDEX ON contract_balances (contract_action_id, token_type);
--------------------------------------------------------------------------------
-- wallets
--------------------------------------------------------------------------------
CREATE TABLE wallets (
  id UUID PRIMARY KEY,
  session_id BYTEA NOT NULL UNIQUE,
  viewing_key BYTEA NOT NULL, -- Ciphertext with nonce, no longer unique!
  last_indexed_transaction_id BIGINT NOT NULL DEFAULT 0,
  active BOOLEAN NOT NULL DEFAULT TRUE,
  last_active TIMESTAMPTZ NOT NULL
);
CREATE INDEX ON wallets (session_id);
CREATE INDEX ON wallets (last_indexed_transaction_id DESC);
--------------------------------------------------------------------------------
-- relevant_transactions
--------------------------------------------------------------------------------
CREATE TABLE relevant_transactions (
  id BIGSERIAL PRIMARY KEY,
  wallet_id UUID NOT NULL REFERENCES wallets (id),
  transaction_id BIGINT NOT NULL REFERENCES transactions (id),
  UNIQUE (wallet_id, transaction_id)
);
--------------------------------------------------------------------------------
-- DUST Generation Status Tables
-- These tables support the dustGenerationStatus GraphQL query for Protofire dApp
--------------------------------------------------------------------------------
-- DUST generation information tracking
CREATE TABLE dust_generation_info (
  id BIGSERIAL PRIMARY KEY,
  night_utxo_hash BYTEA NOT NULL,
  value BYTEA NOT NULL,
  owner BYTEA NOT NULL,
  nonce BYTEA NOT NULL,
  ctime BIGINT NOT NULL,
  merkle_index BIGINT NOT NULL,
  dtime BIGINT
);
CREATE INDEX ON dust_generation_info (owner);
CREATE INDEX ON dust_generation_info (night_utxo_hash);
-- cNIGHT registration tracking
CREATE TABLE cnight_registrations (
  id BIGSERIAL PRIMARY KEY,
  cardano_address BYTEA NOT NULL,
  dust_address BYTEA NOT NULL,
  valid BOOLEAN NOT NULL,
  registered_at BIGINT NOT NULL,
  removed_at BIGINT,
  block_id BIGINT REFERENCES blocks (id),
  UNIQUE (cardano_address, dust_address)
);
CREATE INDEX ON cnight_registrations (cardano_address);
CREATE INDEX ON cnight_registrations (dust_address);
CREATE INDEX ON cnight_registrations (block_id);
