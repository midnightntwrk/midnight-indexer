CREATE TABLE unshielded_utxos(
    id INTEGER PRIMARY KEY,
    creating_transaction_id INTEGER NOT NULL,
    output_index INTEGER NOT NULL,
    owner_address BLOB NOT NULL,
    token_type BLOB NOT NULL,
    intent_hash BLOB NOT NULL,
    value BLOB NOT NULL,
    spending_transaction_id INTEGER,
    FOREIGN KEY (creating_transaction_id) REFERENCES transactions(id),
    FOREIGN KEY (spending_transaction_id) REFERENCES transactions(id),
    UNIQUE (creating_transaction_id, output_index)
);

CREATE INDEX unshielded_owner_idx ON unshielded_utxos(owner_address);

CREATE INDEX unshielded_token_type_idx ON unshielded_utxos(token_type);

CREATE INDEX unshielded_spent_idx ON unshielded_utxos(spending_transaction_id);

