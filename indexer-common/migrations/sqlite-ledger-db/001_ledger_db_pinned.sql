-- Nodes reachable from a contract state or contract zswap state root. Those roots are never
-- unpersisted, so their nodes are never garbage: gc's sweep scans only the unpinned rows, through
-- the partial index, and its mark starts from the unpinned roots only. A pinned node's descendants
-- are all pinned.
ALTER TABLE ledger_db_nodes ADD COLUMN pinned BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX ledger_db_nodes_unpinned_idx ON ledger_db_nodes (key) WHERE NOT pinned;
-- Roots whose nodes are pinned. gc marks from the unpinned roots only, read through the partial
-- index.
ALTER TABLE ledger_db_roots ADD COLUMN pinned BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX ledger_db_roots_unpinned_idx ON ledger_db_roots (key) WHERE NOT pinned;
