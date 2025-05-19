CREATE TABLE unshielded_utxos(
    id BIGSERIAL PRIMARY KEY,
    creating_transaction_id BIGINT NOT NULL REFERENCES transactions(id),
    output_index BIGINT NOT NULL,
    owner_address BYTEA NOT NULL,
    token_type BYTEA NOT NULL,
    intent_hash BYTEA NOT NULL,
    value BYTEA NOT NULL,
    spending_transaction_id BIGINT REFERENCES transactions(id),
    UNIQUE (creating_transaction_id, output_index)
);

CREATE INDEX unshielded_owner_idx ON unshielded_utxos(owner_address);

CREATE INDEX unshielded_token_type_idx ON unshielded_utxos(token_type);

CREATE INDEX unshielded_spent_idx ON unshielded_utxos(spending_transaction_id);

