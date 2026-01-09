--------------------------------------------------------------------------------
-- ledger_state
--------------------------------------------------------------------------------
CREATE TABLE ledger_state (
  id INTEGER PRIMARY KEY,
  block_height INTEGER NOT NULL,
  protocol_version INTEGER NOT NULL,
  ab_selector TEXT CHECK (ab_selector IN ('A', 'B')) NOT NULL
);
