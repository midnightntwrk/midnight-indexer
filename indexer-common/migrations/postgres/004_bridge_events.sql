-- Cardano-to-Midnight bridge events (PM-15404).
--
-- The c2m-bridge pallet (midnight-node, runtime pallet index 33) emits five event
-- variants when an observed Cardano transaction is processed:
--
--   UserTransfer            - approved user deposit, NIGHT credited via DistributeNight
--   ReserveTransfer         - top-up of the protocol Reserve pool
--   InvalidTransfer         - malformed metadata, redirected to treasury
--   UnapprovedTransfer      - user deposit not in ApprovedTransactions, redirected to treasury
--   SubminimalFlushTransfer - aggregated subminimum amounts flushed to treasury
--
-- All five share a payload shape with mc_tx_hash (Cardano tx hash, NULL for the aggregate
-- SubminimalFlushTransfer), amount (u64), an optional recipient (only for UserTransfer
-- and UnapprovedTransfer), and midnight_tx_hash (system tx hash on Midnight). The
-- SubminimalFlushTransfer carries an additional `count` field giving the number of
-- aggregated subminimum txs.
--
-- Bridge claims (regular `ClaimRewardsTransaction` with `ClaimKind::CardanoBridge`) are
-- stored separately in `bridge_claims`.

--------------------------------------------------------------------------------
-- types
--------------------------------------------------------------------------------
CREATE TYPE BRIDGE_PALLET_EVENT_VARIANT AS ENUM(
  'UserTransfer',
  'ReserveTransfer',
  'InvalidTransfer',
  'UnapprovedTransfer',
  'SubminimalFlushTransfer'
);

--------------------------------------------------------------------------------
-- bridge_pallet_events
--------------------------------------------------------------------------------
CREATE TABLE bridge_pallet_events (
  id BIGSERIAL PRIMARY KEY,
  block_id BIGINT NOT NULL REFERENCES blocks (id),
  transaction_id BIGINT REFERENCES transactions (id),
  variant BRIDGE_PALLET_EVENT_VARIANT NOT NULL,
  mc_tx_hash BYTEA,
  amount BYTEA NOT NULL,
  recipient BYTEA,
  midnight_tx_hash BYTEA NOT NULL,
  count INTEGER
);

CREATE INDEX ON bridge_pallet_events (block_id);
CREATE INDEX ON bridge_pallet_events (transaction_id);
CREATE INDEX ON bridge_pallet_events (mc_tx_hash) WHERE mc_tx_hash IS NOT NULL;
CREATE INDEX ON bridge_pallet_events (midnight_tx_hash);
CREATE INDEX ON bridge_pallet_events (recipient) WHERE recipient IS NOT NULL;
CREATE INDEX ON bridge_pallet_events (variant, recipient);

--------------------------------------------------------------------------------
-- bridge_claims
--------------------------------------------------------------------------------
CREATE TABLE bridge_claims (
  id BIGSERIAL PRIMARY KEY,
  transaction_id BIGINT NOT NULL REFERENCES transactions (id),
  recipient BYTEA NOT NULL,
  amount BYTEA NOT NULL
);

CREATE INDEX ON bridge_claims (transaction_id);
CREATE INDEX ON bridge_claims (recipient);
