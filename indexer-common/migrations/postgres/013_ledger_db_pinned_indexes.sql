-- The unpinned rows, which gc's sweep scans and its mark starts from. Built apart from the columns
-- (012): sqlx runs each migration in one transaction, so the columns' exclusive locks, which block
-- reads, are released before these builds, which over a large ledger DB take a while and block
-- only writes.
CREATE INDEX ledger_db_nodes_unpinned_idx ON ledger_db_nodes (key) WHERE NOT pinned;
CREATE INDEX ledger_db_roots_unpinned_idx ON ledger_db_roots (key) WHERE NOT pinned;
