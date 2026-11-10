-- Contract state translated to a new ledger version at a hard fork: at `block_id`, the state of
-- `contract_action_id` was translated to `state_key`.
--
-- Keyed by the action rather than the address: `address` has no referential target, and a
-- translation must never apply to a newer action.

CREATE TABLE contract_action_translations (
  contract_action_id BIGINT NOT NULL REFERENCES contract_actions (id),
  block_id BIGINT NOT NULL REFERENCES blocks (id),
  state_key BYTEA NOT NULL,
  PRIMARY KEY (contract_action_id, block_id)
);

CREATE INDEX ON contract_action_translations (block_id);
