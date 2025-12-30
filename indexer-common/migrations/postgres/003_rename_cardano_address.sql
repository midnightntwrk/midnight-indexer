--------------------------------------------------------------------------------
-- Migration: Rename cardano_address to cardano_stake_key for clarity
-- Issue: #440 - The column name was confusing as it actually stores a Cardano
--        stake key (reward address), not a regular Cardano address.
--------------------------------------------------------------------------------

-- Rename the column in cnight_registrations table
ALTER TABLE cnight_registrations
    RENAME COLUMN cardano_address TO cardano_stake_key;

-- Drop and recreate the index with the new column name
DROP INDEX IF EXISTS cnight_registrations_cardano_address_idx;
CREATE INDEX ON cnight_registrations (cardano_stake_key);

-- Update the unique constraint
-- Note: PostgreSQL automatically updates the constraint when the column is renamed,
-- but we recreate it for clarity and to ensure the constraint name reflects the change.
ALTER TABLE cnight_registrations
    DROP CONSTRAINT IF EXISTS cnight_registrations_cardano_address_dust_address_key;
ALTER TABLE cnight_registrations
    ADD CONSTRAINT cnight_registrations_cardano_stake_key_dust_address_key
    UNIQUE (cardano_stake_key, dust_address);
