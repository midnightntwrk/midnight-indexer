--------------------------------------------------------------------------------
-- types
--------------------------------------------------------------------------------
CREATE TYPE AB_SELECTOR AS ENUM('A', 'B');
--------------------------------------------------------------------------------
-- ledger_state
--------------------------------------------------------------------------------
CREATE TABLE ledger_state (
  id BIGINT PRIMARY KEY,
  block_height BIGINT NOT NULL,
  protocol_version BIGINT NOT NULL,
  ab_selector AB_SELECTOR NOT NULL
);
