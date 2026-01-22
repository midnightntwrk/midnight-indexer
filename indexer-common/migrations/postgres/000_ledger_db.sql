--------------------------------------------------------------------------------
-- nodes
--------------------------------------------------------------------------------
CREATE TABLE ledger_db_nodes (
  key BYTEA PRIMARY KEY,
  object BYTEA NOT NULL,
  ref_count BIGINT NOT NULL
);
CREATE INDEX ON ledger_db_nodes (ref_count);
--------------------------------------------------------------------------------
-- roots
--------------------------------------------------------------------------------
CREATE TABLE ledger_db_roots (key BYTEA PRIMARY KEY, count BIGINT NOT NULL);
CREATE INDEX ON ledger_db_nodes (count);
