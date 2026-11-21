-- Nodes reachable from a contract state or contract zswap state root. Those roots are never
-- unpersisted, so their nodes are never garbage: gc's sweep scans only the unpinned rows, and its
-- mark starts from the unpinned roots only. A pinned node's descendants are all pinned.
ALTER TABLE ledger_db_nodes ADD COLUMN pinned BOOLEAN NOT NULL DEFAULT FALSE;
-- Roots whose nodes are pinned. gc marks from the unpinned roots only.
ALTER TABLE ledger_db_roots ADD COLUMN pinned BOOLEAN NOT NULL DEFAULT FALSE;
