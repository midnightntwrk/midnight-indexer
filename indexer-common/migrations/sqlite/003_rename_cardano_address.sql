--------------------------------------------------------------------------------
-- Migration: Rename cardano_address to cardano_stake_key for clarity
-- Issue: #440 - The column name was confusing as it actually stores a Cardano
--        stake key (reward address), not a regular Cardano address.
--------------------------------------------------------------------------------

-- SQLite does not support RENAME COLUMN directly in older versions,
-- so we need to recreate the table with the new column name.

-- Step 1: Create a new table with the correct column name
CREATE TABLE cnight_registrations_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cardano_stake_key BLOB NOT NULL,
  dust_address BLOB NOT NULL,
  valid INTEGER NOT NULL,
  registered_at INTEGER NOT NULL,
  removed_at INTEGER,
  block_id INTEGER REFERENCES blocks (id),
  utxo_tx_hash BLOB,
  utxo_output_index INTEGER,
  UNIQUE (cardano_stake_key, dust_address)
);

-- Step 2: Copy data from the old table to the new table
INSERT INTO cnight_registrations_new (
  id, cardano_stake_key, dust_address, valid, registered_at, removed_at,
  block_id, utxo_tx_hash, utxo_output_index
)
SELECT
  id, cardano_address, dust_address, valid, registered_at, removed_at,
  block_id, utxo_tx_hash, utxo_output_index
FROM cnight_registrations;

-- Step 3: Drop the old table
DROP TABLE cnight_registrations;

-- Step 4: Rename the new table to the original name
ALTER TABLE cnight_registrations_new RENAME TO cnight_registrations;

-- Step 5: Recreate indexes with the new column name
CREATE INDEX cnight_registrations_cardano_stake_key_idx ON cnight_registrations (cardano_stake_key);
CREATE INDEX cnight_registrations_dust_address_idx ON cnight_registrations (dust_address);
CREATE INDEX cnight_registrations_block_id_idx ON cnight_registrations (block_id);
