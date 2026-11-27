-- Each node is stored as parts: `object` holds its payload, `child_keys` the hashes of its
-- by-reference children as concatenated 32-byte hashes, and `child_values` the rest of its
-- children section, so gc's mark reads only `child_keys` and nothing is stored twice. Both are
-- NULL for rows written before they existed, whose `object` holds the whole node.
ALTER TABLE ledger_db_nodes ADD COLUMN child_keys BLOB;
ALTER TABLE ledger_db_nodes ADD COLUMN child_values BLOB;
