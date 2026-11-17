-- Contract state translated to a new ledger version at a hard fork: at `block_id`, the state of
-- `contract_action_id` was translated to `state_key`.
--
-- Keyed by the action rather than the address: `address` has no referential target, and a
-- translation must never apply to a newer action.

CREATE TABLE contract_action_translations (
  contract_action_id INTEGER NOT NULL REFERENCES contract_actions (id),
  block_id INTEGER NOT NULL REFERENCES blocks (id),
  state_key BLOB NOT NULL,
  PRIMARY KEY (contract_action_id, block_id)
);

CREATE INDEX contract_action_translations_block_id_idx ON contract_action_translations (block_id);
