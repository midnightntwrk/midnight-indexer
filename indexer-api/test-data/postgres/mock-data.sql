-- Mock data for development/testing
-- This is temporary until node/ledger integration is available

BEGIN;

-- Clean existing data (order matters due to foreign keys)
TRUNCATE TABLE dust_events CASCADE;
TRUNCATE TABLE dust_utxos CASCADE;
TRUNCATE TABLE dust_generation_info CASCADE;
TRUNCATE TABLE dust_commitment_tree CASCADE;
TRUNCATE TABLE dust_generation_tree CASCADE;
TRUNCATE TABLE cnight_registrations CASCADE;
TRUNCATE TABLE contract_balances CASCADE;
TRUNCATE TABLE unshielded_utxos CASCADE;
TRUNCATE TABLE relevant_transactions CASCADE;
TRUNCATE TABLE contract_actions CASCADE;
TRUNCATE TABLE transactions CASCADE;
TRUNCATE TABLE blocks CASCADE;
TRUNCATE TABLE wallets CASCADE;

-- Create helper function
CREATE OR REPLACE FUNCTION bytea_to_bigint(b bytea) RETURNS bigint AS $$
DECLARE
    result bigint := 0;
    i int;
BEGIN
    IF b IS NULL OR length(b) = 0 THEN RETURN 0; END IF;
    FOR i IN 0..LEAST(length(b) - 1, 7) LOOP
        result := (result << 8) | get_byte(b, i);
    END LOOP;
    RETURN result;
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- Create function to convert u128 to bytea (16 bytes, big-endian)
CREATE OR REPLACE FUNCTION u128_to_bytea(value numeric) RETURNS bytea AS $$
DECLARE
    hex_str text;
    padding_needed int;
BEGIN
    -- Convert to hex
    hex_str := to_hex(value::bigint);
    
    -- Pad to 32 hex characters (16 bytes)
    padding_needed := 32 - length(hex_str);
    IF padding_needed > 0 THEN
        hex_str := repeat('0', padding_needed) || hex_str;
    END IF;
    
    RETURN decode(hex_str, 'hex');
END;
$$ LANGUAGE plpgsql;

-- Insert 10 blocks with proper 32-byte hashes
-- Note: timestamps are in milliseconds and set to December 2024
INSERT INTO blocks (hash, height, protocol_version, parent_hash, author, timestamp) VALUES
(decode('0000000000000000000000000000000000000000000000000000000000000000', 'hex'), 0, 13000, decode('0000000000000000000000000000000000000000000000000000000000000000', 'hex'), NULL, 1733000000000),
(decode('1111111111111111111111111111111111111111111111111111111111111111', 'hex'), 1, 13000, decode('0000000000000000000000000000000000000000000000000000000000000000', 'hex'), NULL, 1733000002000),
(decode('2222222222222222222222222222222222222222222222222222222222222222', 'hex'), 2, 13000, decode('1111111111111111111111111111111111111111111111111111111111111111', 'hex'), NULL, 1733000004000),
(decode('3333333333333333333333333333333333333333333333333333333333333333', 'hex'), 3, 13000, decode('2222222222222222222222222222222222222222222222222222222222222222', 'hex'), NULL, 1733000006000),
(decode('4444444444444444444444444444444444444444444444444444444444444444', 'hex'), 4, 13000, decode('3333333333333333333333333333333333333333333333333333333333333333', 'hex'), NULL, 1733000008000),
(decode('5555555555555555555555555555555555555555555555555555555555555555', 'hex'), 5, 13000, decode('4444444444444444444444444444444444444444444444444444444444444444', 'hex'), NULL, 1733000010000),
(decode('6666666666666666666666666666666666666666666666666666666666666666', 'hex'), 6, 13000, decode('5555555555555555555555555555555555555555555555555555555555555555', 'hex'), NULL, 1733000012000),
(decode('7777777777777777777777777777777777777777777777777777777777777777', 'hex'), 7, 13000, decode('6666666666666666666666666666666666666666666666666666666666666666', 'hex'), NULL, 1733000014000),
(decode('8888888888888888888888888888888888888888888888888888888888888888', 'hex'), 8, 13000, decode('7777777777777777777777777777777777777777777777777777777777777777', 'hex'), NULL, 1733000016000),
(decode('9999999999999999999999999999999999999999999999999999999999999999', 'hex'), 9, 13000, decode('8888888888888888888888888888888888888888888888888888888888888888', 'hex'), NULL, 1733000018000);

-- Get block IDs
WITH block_ids AS (
    SELECT id, height FROM blocks ORDER BY height
)
-- Insert 11 transactions with proper hashes (use block ids)
INSERT INTO transactions (hash, block_id, protocol_version, transaction_result, identifiers, raw, merkle_tree_root, start_index, end_index, paid_fees, estimated_fees)
SELECT 
    decode(tx.hash_hex, 'hex'),
    bi.id,
    13000,
    '"Success"'::jsonb,
    ARRAY[]::bytea[],
    decode(tx.raw_hex, 'hex'),
    decode(tx.merkle_root_hex, 'hex'),
    tx.start_index,
    tx.end_index,
    u128_to_bytea(tx.paid_fees),
    u128_to_bytea(tx.estimated_fees)
FROM (VALUES
    ('0000000000000000000000000000000000000000000000000000000000000001', 0, '00', '0000000000000000000000000000000000000000000000000000000000000001', 0, 5, 100000, 100000),
    ('1111111111111111111111111111111111111111111111111111111111111111', 1, '01', '1111111111111111111111111111111111111111111111111111111111111111', 6, 10, 150000, 150000),
    ('2222222222222222222222222222222222222222222222222222222222222222', 2, '02', '2222222222222222222222222222222222222222222222222222222222222222', 11, 15, 200000, 200000),
    ('3333333333333333333333333333333333333333333333333333333333333333', 3, '00', '3333333333333333333333333333333333333333333333333333333333333333', 16, 20, 250000, 250000),
    ('4444444444444444444444444444444444444444444444444444444444444444', 4, '00', '4444444444444444444444444444444444444444444444444444444444444444', 21, 25, 300000, 300000),
    ('5555555555555555555555555555555555555555555555555555555555555555', 5, '00', '5555555555555555555555555555555555555555555555555555555555555555', 26, 30, 500000, 500000),
    ('6666666666666666666666666666666666666666666666666666666666666666', 6, '00', '6666666666666666666666666666666666666666666666666666666666666666', 31, 35, 550000, 550000),
    ('7777777777777777777777777777777777777777777777777777777777777777', 7, '00', '7777777777777777777777777777777777777777777777777777777777777777', 36, 40, 600000, 600000),
    ('8888888888888888888888888888888888888888888888888888888888888888', 8, '00', '8888888888888888888888888888888888888888888888888888888888888888', 41, 45, 650000, 650000),
    ('9999999999999999999999999999999999999999999999999999999999999999', 9, '00', '9999999999999999999999999999999999999999999999999999999999999999', 46, 50, 700000, 700000),
    ('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 0, '00', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 51, 55, 1000000000, 1000000000)
) AS tx(hash_hex, block_height, raw_hex, merkle_root_hex, start_index, end_index, paid_fees, estimated_fees)
JOIN block_ids bi ON bi.height = tx.block_height;

COMMIT;

-- Insert UTXO and wallet data
BEGIN;

-- Insert wallets first (using UUID)
INSERT INTO wallets (id, session_id, viewing_key, last_indexed_transaction_id, active, last_active) VALUES
('11111111-1111-1111-1111-111111111111'::uuid, decode('45e101b45163c80c115208691997719c3d3aa23fc2512d32aeb0c2f5af425b8f', 'hex'), decode('6e09aea334dcb2e7cf6ea3734faba08794ef0edc4a56c1c799f7fbe806b62d6a', 'hex'), 10, true, NOW()),
('22222222-2222-2222-2222-222222222222'::uuid, decode('bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'hex'), decode('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'hex'), 10, true, NOW()),
('33333333-3333-3333-3333-333333333333'::uuid, decode('dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', 'hex'), decode('cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', 'hex'), 10, true, NOW());

-- Add unshielded UTXOs (use transaction IDs)
WITH tx_ids AS (
    SELECT id, hash FROM transactions
)
INSERT INTO unshielded_utxos (
    creating_transaction_id, spending_transaction_id, owner, token_type, value, output_index, intent_hash
)
SELECT 
    creating_tx.id,
    NULL, -- not spent
    decode('6e09aea334dcb2e7cf6ea3734faba08794ef0edc4a56c1c799f7fbe806b62d6a', 'hex'),
    decode('0000000000000000000000000000000000000000000000000000000000000000', 'hex'),
    u128_to_bytea(1000000000),
    0,
    decode('6dadbce210a7006eb0f8f11079182a75b6c46c5526b9890ba51348ab07cf9b00', 'hex')
FROM tx_ids creating_tx
WHERE creating_tx.hash = decode('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'hex');

-- Insert cNIGHT registrations to match midnight-explorer test data
-- Note: dust_address must be 32 bytes (64 hex chars) as it's a DustAddress type
INSERT INTO cnight_registrations (cardano_address, dust_address, is_valid, registered_at, removed_at) VALUES
(decode('1234567890abcdef1234567890abcdef', 'hex'), decode('fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321', 'hex'), true, 1733000020000, NULL),
(decode('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'hex'), decode('bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'hex'), true, 1733000022000, NULL),
(decode('cccccccccccccccccccccccccccccccccccccccc', 'hex'), decode('dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', 'hex'), true, 1733000024000, NULL),
(decode('eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', 'hex'), decode('ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff', 'hex'), true, 1733000026000, NULL),
(decode('1111111111111111111111111111111111111111', 'hex'), decode('2222222222222222222222222222222222222222222222222222222222222222', 'hex'), true, 1733000028000, NULL);

-- Insert DUST generation info (updated schema)
WITH tx_ids AS (
    SELECT id, hash FROM transactions
)
INSERT INTO dust_generation_info (night_utxo_hash, value, owner, nonce, ctime, merkle_index, dtime)
VALUES
-- For Test Key 1: 2B NIGHT (2 active generations as per explorer mockData)
(decode('1111111111111111111111111111111111111111111111111111111111111111', 'hex'), u128_to_bytea(2000000000), decode('fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321', 'hex'), decode('00', 'hex'), 1733000030000, 0, 1733000040000),
-- For Test Key 2: 4B NIGHT (1 active generation)
(decode('2222222222222222222222222222222222222222222222222222222222222222', 'hex'), u128_to_bytea(4000000000), decode('bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'hex'), decode('01', 'hex'), 1733000032000, 1, 1733000042000),
-- For Test Key 3: 5B NIGHT (1 active generation)
(decode('3333333333333333333333333333333333333333333333333333333333333333', 'hex'), u128_to_bytea(5000000000), decode('dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', 'hex'), decode('02', 'hex'), 1733000034000, 2, 1733000044000),
-- For Test Key 4: 15B NIGHT (2 active generations: 7B + 8B)
(decode('4444444444444444444444444444444444444444444444444444444444444444', 'hex'), u128_to_bytea(7000000000), decode('ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff', 'hex'), decode('03', 'hex'), 1733000036000, 3, 1733000046000),
(decode('5555555555555555555555555555555555555555555555555555555555555555', 'hex'), u128_to_bytea(8000000000), decode('ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff', 'hex'), decode('04', 'hex'), 1733000038000, 4, 1733000048000),
-- For Test Key 5: 10B NIGHT (1 active generation)
(decode('6666666666666666666666666666666666666666666666666666666666666666', 'hex'), u128_to_bytea(10000000000), decode('2222222222222222222222222222222222222222222222222222222222222222', 'hex'), decode('05', 'hex'), 1733000040000, 5, 1733000050000);

-- Insert DUST UTXOs (3 per generation to match SQLite)
WITH dust_gen AS (
    SELECT id, merkle_index, owner, ctime FROM dust_generation_info
)
INSERT INTO dust_utxos (generation_info_id, commitment, initial_value, owner, nonce, seq, ctime)
SELECT 
    dg.id,
    -- Using deterministic commitments based on id and seq
    CASE seq.num 
        WHEN 0 THEN decode('69286d746e656d6d6f632105f5e10069286d746e656d6d6f632105f5e1000000', 'hex')
        WHEN 1 THEN decode('69286d746e656d6d6f632105f5e11069286d746e656d6d6f632105f5e1100000', 'hex')
        WHEN 2 THEN decode('69286d746e656d6d6f632105f5e12069286d746e656d6d6f632105f5e1200000', 'hex')
    END,
    -- Value is 1/10th of generation value
    CASE (dg.merkle_index + 1)
        WHEN 1 THEN u128_to_bytea(100000000)
        WHEN 2 THEN u128_to_bytea(200000000)
        WHEN 3 THEN u128_to_bytea(300000000)
        WHEN 4 THEN u128_to_bytea(400000000)
        WHEN 5 THEN u128_to_bytea(500000000)
        WHEN 6 THEN u128_to_bytea(600000000)
    END,
    dg.owner,
    -- Using deterministic nonces based on id and seq
    CASE seq.num
        WHEN 0 THEN decode('302063656e6f6e05f5e100302063656e6f6e05f5e100302063656e6f6e050000', 'hex')
        WHEN 1 THEN decode('312063656e6f6e05f5e110312063656e6f6e05f5e110312063656e6f6e050000', 'hex')
        WHEN 2 THEN decode('322063656e6f6e05f5e120322063656e6f6e05f5e120322063656e6f6e050000', 'hex')
    END,
    seq.num,
    dg.ctime
FROM dust_gen dg
CROSS JOIN (SELECT 0 as num UNION ALL SELECT 1 UNION ALL SELECT 2) as seq;

-- Insert merkle trees with 32-byte roots
INSERT INTO dust_commitment_tree (block_height, root, tree_data)
VALUES
(0, decode('3074686769656800746f6f725f746e656d74696d6d6f63307468676965680000', 'hex'), decode('01', 'hex')),
(2, decode('3274686769656800746f6f725f746e656d74696d6d6f63327468676965680000', 'hex'), decode('01', 'hex')),
(4, decode('3474686769656800746f6f725f746e656d74696d6d6f63347468676965680000', 'hex'), decode('01', 'hex')),
(6, decode('3674686769656800746f6f725f746e656d74696d6d6f63367468676965680000', 'hex'), decode('01', 'hex')),
(8, decode('3874686769656800746f6f725f746e656d74696d6d6f63387468676965680000', 'hex'), decode('01', 'hex'));

INSERT INTO dust_generation_tree (block_height, root, tree_data)
VALUES
(0, decode('3074686769656800746f6f725f6e6f69746172656e6267307468676965680000', 'hex'), decode('02', 'hex')),
(2, decode('3274686769656800746f6f725f6e6f69746172656e6267327468676965680000', 'hex'), decode('02', 'hex')),
(4, decode('3474686769656800746f6f725f6e6f69746172656e6467347468676965680000', 'hex'), decode('02', 'hex')),
(6, decode('3674686769656800746f6f725f6e6f69746172656e6667367468676965680000', 'hex'), decode('02', 'hex')),
(8, decode('3874686769656800746f6f725f6e6f69746172656e6867387468676965680000', 'hex'), decode('02', 'hex'));

-- Insert relevant transactions (using wallet and transaction IDs)
WITH wallet_tx AS (
    SELECT w.id as wallet_id, t.id as tx_id
    FROM wallets w
    CROSS JOIN transactions t
    WHERE w.id = '11111111-1111-1111-1111-111111111111'::uuid
    AND t.hash = decode('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'hex')
    
    UNION ALL
    
    SELECT w.id, t.id
    FROM wallets w
    CROSS JOIN transactions t  
    WHERE w.id = '22222222-2222-2222-2222-222222222222'::uuid
    AND t.hash = decode('1111111111111111111111111111111111111111111111111111111111111111', 'hex')
    
    UNION ALL
    
    SELECT w.id, t.id
    FROM wallets w
    CROSS JOIN transactions t
    WHERE w.id = '33333333-3333-3333-3333-333333333333'::uuid
    AND t.hash = decode('2222222222222222222222222222222222222222222222222222222222222222', 'hex')
)
INSERT INTO relevant_transactions (wallet_id, transaction_id)
SELECT wallet_id, tx_id FROM wallet_tx;

-- Show the loaded data for verification
SELECT 'Blocks loaded:' as status, COUNT(*) as count FROM blocks
UNION ALL
SELECT 'Transactions loaded:' as status, COUNT(*) as count FROM transactions
UNION ALL
SELECT 'Wallets loaded:' as status, COUNT(*) as count FROM wallets
UNION ALL
SELECT 'Unshielded UTXOs loaded:' as status, COUNT(*) as count FROM unshielded_utxos
UNION ALL
SELECT 'DUST Registrations loaded:' as status, COUNT(*) as count FROM cnight_registrations
UNION ALL
SELECT 'DUST Generations loaded:' as status, COUNT(*) as count FROM dust_generation_info
UNION ALL
SELECT 'DUST UTXOs loaded:' as status, COUNT(*) as count FROM dust_utxos
UNION ALL
SELECT 'Commitment Trees loaded:' as status, COUNT(*) as count FROM dust_commitment_tree
UNION ALL
SELECT 'Generation Trees loaded:' as status, COUNT(*) as count FROM dust_generation_tree;

COMMIT;