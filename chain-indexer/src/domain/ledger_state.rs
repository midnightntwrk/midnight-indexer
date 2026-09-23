// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::domain::{ContractAction, RegularTransaction, SystemTransaction, Transaction, node};
use derive_more::derive::{Deref, From};
use fastrace::trace;
use indexer_common::domain::{
    ApplyRegularTransactionOutcome, ApplySystemTransactionOutcome, BlockHash, LedgerVersion,
    NetworkId, ProtocolVersion, SerializedContractAddress, SerializedLedgerStateKey,
    TransactionHash, TransactionResult,
    ledger::{LedgerParameters, RootCountRepair},
};
use std::{
    collections::{HashMap, HashSet},
    ops::DerefMut,
};
use thiserror::Error;

/// Amount, in milliseconds, by which the first regular transaction's dust-validity `tblock` is
/// bumped ahead of block time. The node validates mempool transactions against a `tblock` bumped
/// `slot_duration_secs + skipped_slots_margin` (one slot each, two slots by default) ahead of block
/// time. Midnight slots are 6s, so the default bump is two slots. Block timestamps are milliseconds.
const MEMPOOL_TBLOCK_BUMP_MILLIS: u64 = 2 * 6_000;

/// First node 1.0 runtime `spec_version` whose ledger-8 host functions no longer skew the first
/// regular transaction's well-formed `tblock`. Node 1.0.300 added version 2 of
/// `Ledger8Bridge::apply_transaction`/`validate_guaranteed_execution`, which verify against the
/// block's own time; runtimes before it import version 1, which keeps the skew. Which one ran is
/// decided by the runtime that built the block, so blocks before the `set_code` still skew.
///
/// See <https://github.com/midnightntwrk/midnight-node/issues/1924>.
const FIRST_UNSKEWED_NODE_1_0_SPEC_VERSION: u32 = 1_000_300;

/// Whether the node skewed the first regular transaction's well-formed `tblock` by
/// `MEMPOOL_TBLOCK_BUMP_MILLIS` off the parent block time, for a block built by the runtime with the
/// given protocol version; that is the runtime recorded in the block's MNSV digest, not the one in
/// its state, which is newer at a runtime-upgrade enactment block.
///
/// - 0.22, 1.0 before `FIRST_UNSKEWED_NODE_1_0_SPEC_VERSION` and 2.0 serve the first transaction's
///   validity from the strict cache warmed during mempool ingress, i.e. verify it at the bumped
///   `tblock`.
/// - 1.0 from `FIRST_UNSKEWED_NODE_1_0_SPEC_VERSION` on (`Ledger8Bridge` version 2) and 2.1 (whose
///   ledger-8 and ledger-9 host functions never skew) verify it against the block's own time.
///
/// Bumping where the node does not makes the indexer stricter on the intent TTL than the node by
/// up to one block interval, so a short-TTL transaction the node accepted fails `well_formed`
/// here and halts indexing.
fn node_skews_first_regular_tblock(protocol_version: ProtocolVersion) -> bool {
    match protocol_version {
        ProtocolVersion::V0_22(_) | ProtocolVersion::V2_0(_) => true,
        ProtocolVersion::V1_0(spec_version) => spec_version < FIRST_UNSKEWED_NODE_1_0_SPEC_VERSION,
        ProtocolVersion::V2_1(_) => false,
    }
}

/// Whether indexing a block at `height` must reproduce the node's skewed first regular
/// transaction `tblock`. Genesis transactions never passed through the mempool, so they never use
/// the cached validity result that caused the skew.
pub(crate) fn should_bump_first_regular_tblock(
    height: u64,
    protocol_version: ProtocolVersion,
) -> bool {
    height > 0 && node_skews_first_regular_tblock(protocol_version)
}

/// New type for ledger state from indexer_common.
#[derive(Debug, Clone, From, Deref)]
pub struct LedgerState(pub indexer_common::domain::ledger::LedgerState);

impl DerefMut for LedgerState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl LedgerState {
    pub fn new(network_id: NetworkId, ledger_version: LedgerVersion) -> Result<Self, Error> {
        indexer_common::domain::ledger::LedgerState::new(network_id, ledger_version)
            .map_err(Error::Create)
            .map(Into::into)
    }

    pub fn from_genesis(
        raw: impl AsRef<[u8]>,
        ledger_version: LedgerVersion,
    ) -> Result<Self, Error> {
        indexer_common::domain::ledger::LedgerState::from_genesis(raw, ledger_version)
            .map_err(Error::Create)
            .map(Into::into)
    }

    pub fn load(
        key: &SerializedLedgerStateKey,
        ledger_version: LedgerVersion,
    ) -> Result<Self, Error> {
        indexer_common::domain::ledger::LedgerState::load(key, ledger_version)
            .map_err(Error::Load)
            .map(Into::into)
    }

    pub fn translate(self, ledger_version: LedgerVersion) -> Result<Self, Error> {
        self.0
            .translate(ledger_version)
            .map_err(Error::Translate)
            .map(Into::into)
    }

    /// Unpersist a previously-persisted ledger state by its serialized key.
    /// Balances a prior `persist()` call so storage-core's gc-v1 can reclaim
    /// the now-unreachable arena nodes on a subsequent `gc()` pass.
    pub fn unpersist(
        key: &SerializedLedgerStateKey,
        ledger_version: LedgerVersion,
    ) -> Result<(), Error> {
        indexer_common::domain::ledger::LedgerState::unpersist(key, ledger_version)
            .map_err(Error::Unpersist)
    }

    /// The raw arena hash bytes of a serialized ledger state key, e.g. to check membership in
    /// [Self::persisted_root_hashes].
    pub fn root_hash_bytes(
        key: &SerializedLedgerStateKey,
        ledger_version: LedgerVersion,
    ) -> Result<Vec<u8>, indexer_common::domain::ledger::Error> {
        indexer_common::domain::ledger::LedgerState::root_hash_bytes(key, ledger_version)
    }

    /// The raw arena hash bytes of all currently persisted gc roots, fetched from the ledger DB.
    pub fn persisted_root_hashes() -> HashSet<Vec<u8>> {
        indexer_common::domain::ledger::LedgerState::persisted_root_hashes()
    }

    /// See [`indexer_common::domain::ledger::LedgerState::repair_root_counts`].
    pub fn repair_root_counts<'a>(
        window: impl IntoIterator<Item = (&'a SerializedLedgerStateKey, LedgerVersion)>,
    ) -> Result<RootCountRepair, indexer_common::domain::ledger::Error> {
        indexer_common::domain::ledger::LedgerState::repair_root_counts(window)
    }

    /// Run a time-bounded mark-and-sweep gc on the ledger DB and return the
    /// number of arena nodes culled.
    pub fn gc(bound: std::time::Duration) -> usize {
        indexer_common::domain::ledger::LedgerState::gc(bound)
    }

    /// Apply the given node transactions to this ledger state and return domain transactions.
    ///
    /// `bump_first_regular_tblock` selects whether the node's mempool-cached validity result is
    /// reproduced for the first regular transaction (see below). It must be `false` for the genesis
    /// block (height 0): the transactions embedded in genesis never transited the mempool, so the
    /// node never cached a bumped result for them and validated them against the real block time.
    /// Bumping them would push the well-formed `tblock` past a bootstrap transaction's intent TTL
    /// and wrongly reject it. It must also be `false` for blocks built by a runtime that no longer
    /// skews, see [node_skews_first_regular_tblock].
    #[trace(properties = { "parent_block_hash": "{parent_block_hash}" })]
    pub fn apply_transactions(
        &mut self,
        transactions: impl IntoIterator<Item = node::Transaction>,
        parent_block_hash: BlockHash,
        block_timestamp: u64,
        parent_block_timestamp: u64,
        bump_first_regular_tblock: bool,
    ) -> Result<(Vec<Transaction>, LedgerParameters), Error> {
        // The node validates a mempool transaction against a `tblock` bumped two slots ahead of the
        // *parent* (last produced) block's time, then caches the well-formed result keyed on
        // (tx_hash, ledger_state_key). At block inclusion only the first regular transaction still
        // matches that key, so the node reuses the cached (bumped) validity result and skips
        // re-checking it against the real block time; later transactions get a fresh check against
        // block time. The bump base is the parent block time (`get_block_context().tblock` during
        // pool validation still holds the last produced block's timestamp; see the node's
        // `pallet-midnight` `validate_unsigned`), NOT the current block time — bumping from the
        // current block overshoots by the inter-block gap and can push `tblock` past a
        // transaction's intent TTL, wrongly rejecting a tx the node accepted.
        //
        // Reproduce that by bumping only the first regular transaction's well-formed `tblock` off
        // the parent block time. `apply` always runs against the real block time, so the resulting
        // state matches the node.
        let mut first_regular_transaction = true;
        let transactions = transactions
            .into_iter()
            .map(|transaction| match transaction {
                node::Transaction::Regular(transaction) => {
                    let well_formed_timestamp =
                        if first_regular_transaction && bump_first_regular_tblock {
                            parent_block_timestamp + MEMPOOL_TBLOCK_BUMP_MILLIS
                        } else {
                            block_timestamp
                        };
                    first_regular_transaction = false;

                    self.apply_regular_transaction(
                        transaction,
                        parent_block_hash,
                        block_timestamp,
                        parent_block_timestamp,
                        well_formed_timestamp,
                    )
                }

                node::Transaction::System(transaction) => {
                    self.apply_system_transaction(transaction, block_timestamp)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        let ledger_parameters = self
            .finalize_apply_transactions(block_timestamp)
            .map_err(Error::PostApplyTransactions)?;

        Ok((transactions, ledger_parameters))
    }

    /// The highest used zswap state index or none.
    pub fn highest_zswap_state_index(&self) -> Option<u64> {
        (self.zswap_first_free() != 0).then(|| self.zswap_first_free() - 1)
    }

    /// Capture the ledger-arena key and the token balances of the contract state of every contract
    /// action in the given block's transactions, and assign them to those actions.
    ///
    /// This must run **once per block, after every transaction has been applied**, and not inside
    /// `apply_regular_transaction`. The blob this replaces came from a node runtime API called *at
    /// the block*, i.e. the contract's end-of-block state, so every action on one address within a
    /// block reported identical bytes; capturing per transaction would change both the stored state
    /// and the denormalized `contract_balances` for multi-action blocks. It must also run after the
    /// genesis branch in `get_and_index_block`, which *replaces* the ledger state after applying:
    /// a key captured before that would point into a state that is not the one persisted.
    ///
    /// Keys are looked up once per distinct address, since content addressing makes K actions on
    /// one address in one block share one key and one root increment. An address whose contract is
    /// absent from the ledger state — a failed action — is left with no key, which reads back as
    /// the empty state it reads back as today.
    ///
    /// Returns the number of distinct addresses for which no contract state could be captured, for
    /// the caller to report: a rising count means states are silently not being captured.
    #[trace]
    pub fn capture_contract_state_keys(
        &self,
        transactions: &mut [Transaction],
    ) -> Result<usize, Error> {
        let mut captured = HashMap::new();

        for transaction in transactions.iter_mut() {
            let Transaction::Regular(transaction) = transaction else {
                continue;
            };

            for contract_action in transaction.contract_actions.iter_mut() {
                let (key, balances) = match captured.get(&contract_action.address) {
                    Some(captured) => captured,

                    None => {
                        let contract_state = self
                            .0
                            .contract_state(&contract_action.address)
                            .map_err(|error| {
                                Error::GetContractStateKey(
                                    transaction.hash,
                                    contract_action.address.clone(),
                                    error,
                                )
                            })?;

                        // The balances come off the state the accessor hands back, so this replaces
                        // the ~860 KB deserialize per action the indexing path used to do with one
                        // read per address off a pointer that is already in hand.
                        let captured_state = match contract_state {
                            Some((key, contract_state)) => {
                                let balances = contract_state.balances().map_err(|error| {
                                    Error::GetContractBalances(
                                        transaction.hash,
                                        contract_action.address.clone(),
                                        error,
                                    )
                                })?;

                                (Some(key), balances)
                            }

                            None => (None, vec![]),
                        };

                        captured
                            .entry(contract_action.address.clone())
                            .insert_entry(captured_state)
                            .into_mut()
                    }
                };

                contract_action.state_key = key.clone();
                contract_action.extracted_balances = balances.clone();
            }
        }

        Ok(captured.values().filter(|(key, _)| key.is_none()).count())
    }

    #[trace(properties = {
        "parent_block_hash": "{parent_block_hash}",
        "block_timestamp": "{block_timestamp}",
        "well_formed_timestamp": "{well_formed_timestamp}"
    })]
    fn apply_regular_transaction(
        &mut self,
        transaction: node::RegularTransaction,
        parent_block_hash: BlockHash,
        block_timestamp: u64,
        parent_block_timestamp: u64,
        well_formed_timestamp: u64,
    ) -> Result<Transaction, Error> {
        let mut transaction = RegularTransaction::from(transaction);

        // Apply transaction.
        let start_index = self.zswap_first_free();
        let dust_commitment_start_index = self.dust_commitments_first_free();
        let dust_generation_start_index = self.dust_generations_first_free();
        let ApplyRegularTransactionOutcome {
            transaction_result,
            created_unshielded_utxos,
            spent_unshielded_utxos,
            ledger_events,
            fees,
            bridge_claim,
        } = self
            .0
            .apply_regular_transaction(
                &transaction.raw,
                parent_block_hash,
                block_timestamp,
                parent_block_timestamp,
                well_formed_timestamp,
            )
            .map_err(|error| Error::ApplyRegularTransaction(Some(transaction.hash), error))?;

        // Contract actions are owned by a physical intent segment, but Calls may also execute a
        // guaranteed transcript in logical segment 0. Retain an action if either execution phase
        // actually applied; Deploy and Update only execute in their physical segment. This runs
        // before any state is captured below, so a rolled-back action neither gets a key nor
        // reaches the API. A segment with no result is an error rather than "not applied": the
        // ledger reports every intent segment, so a missing one means the result shape changed.
        retain_applied_contract_actions(&mut transaction.contract_actions, &transaction_result)
            .map_err(|segment| {
                Error::MissingContractActionSegmentResult(transaction.hash, segment)
            })?;

        // Update transaction.
        transaction.transaction_result = transaction_result;
        transaction.zswap_merkle_tree_root = self
            .zswap_merkle_tree_root()
            .serialize()
            .map_err(|error| Error::SerializeMerkleTreeRoot(transaction.hash, error))?;
        transaction.zswap_start_index = start_index;
        transaction.zswap_end_index = self.zswap_first_free();
        transaction.dust_commitment_start_index = dust_commitment_start_index;
        transaction.dust_commitment_end_index = self.dust_commitments_first_free();
        transaction.dust_generation_start_index = dust_generation_start_index;
        transaction.dust_generation_end_index = self.dust_generations_first_free();
        transaction.created_unshielded_utxos = created_unshielded_utxos;
        transaction.spent_unshielded_utxos = spent_unshielded_utxos;
        transaction.ledger_events = ledger_events;
        transaction.paid_fees = fees;
        transaction.estimated_fees = fees;
        transaction.bridge_claim = bridge_claim;

        // Update contract actions. The zswap state is captured here rather than in the block-level
        // pass because it is per transaction: it is filtered out of the global commitment tree,
        // which grows with every transaction, so this reproduces exactly what is stored today.
        // The contract state key and the balances derived from it are captured once per block, see
        // `capture_contract_state_keys`.
        for contract_action in transaction.contract_actions.iter_mut() {
            let zswap_state_key = self
                .contract_zswap_state_key(&contract_action.address)
                .map_err(|error| Error::GetContractZswapStateKey(transaction.hash, error))?;
            contract_action.zswap_state_key = Some(zswap_state_key);
        }

        Ok(Transaction::Regular(transaction.into()))
    }

    #[trace(properties = {
        "block_timestamp": "{block_timestamp}"
    })]
    fn apply_system_transaction(
        &mut self,
        transaction: node::SystemTransaction,
        block_timestamp: u64,
    ) -> Result<Transaction, Error> {
        let mut transaction = SystemTransaction::from(transaction);

        // Apply transaction.
        let ApplySystemTransactionOutcome {
            created_unshielded_utxos,
            ledger_events,
        } = self
            .0
            .apply_system_transaction(&transaction.raw, block_timestamp)
            .map_err(|error| Error::ApplySystemTransaction(Some(transaction.hash), error))?;

        // Update transaction.
        transaction.created_unshielded_utxos = created_unshielded_utxos;
        transaction.ledger_events = ledger_events;

        Ok(Transaction::System(transaction))
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Create(indexer_common::domain::ledger::Error),

    #[error(transparent)]
    Load(indexer_common::domain::ledger::Error),

    #[error(transparent)]
    Translate(indexer_common::domain::ledger::Error),

    #[error(transparent)]
    Unpersist(indexer_common::domain::ledger::Error),

    #[error("cannot apply regular transaction {hash}", hash = stringify_hash(.0))]
    ApplyRegularTransaction(
        Option<TransactionHash>,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error("cannot apply system transaction {hash}", hash = stringify_hash(.0))]
    ApplySystemTransaction(
        Option<TransactionHash>,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error("cannot finalize transaction application")]
    PostApplyTransactions(#[source] indexer_common::domain::ledger::Error),

    #[error("cannot serialize Merkle tree root for transaction {0}")]
    SerializeMerkleTreeRoot(
        TransactionHash,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error("cannot capture contract zswap state key for transaction {0}")]
    GetContractZswapStateKey(
        TransactionHash,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error("cannot capture contract state key for transaction {0} and contract address {1}")]
    GetContractStateKey(
        TransactionHash,
        SerializedContractAddress,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error(
        "transaction {0} is missing a result for logical segment {1} required by a contract action"
    )]
    MissingContractActionSegmentResult(TransactionHash, u16),

    #[error("cannot get contract balances for transaction {0} and contract address {1}")]
    GetContractBalances(
        TransactionHash,
        SerializedContractAddress,
        #[source] indexer_common::domain::ledger::Error,
    ),
}

fn stringify_hash(hash: &Option<TransactionHash>) -> String {
    hash.map(|hash| hash.to_string())
        .unwrap_or_else(|| "<hash unavailable>".to_string())
}

/// Keep contract actions with at least one execution phase which applied to the ledger state.
fn retain_applied_contract_actions(
    contract_actions: &mut Vec<ContractAction>,
    transaction_result: &TransactionResult,
) -> Result<(), u16> {
    // Validate before mutating so an inconsistent result never leaves a partially filtered list.
    for action in contract_actions.iter() {
        if transaction_result
            .segment_succeeded(action.segment)
            .is_none()
        {
            return Err(action.segment);
        }
        if action.has_guaranteed_transcript && transaction_result.segment_succeeded(0).is_none() {
            return Err(0);
        }
    }

    contract_actions.retain(|action| {
        transaction_result
            .segment_succeeded(action.segment)
            .expect("segment result validated above")
            || (action.has_guaranteed_transcript
                && transaction_result
                    .segment_succeeded(0)
                    .expect("guaranteed-segment result validated above"))
    });

    Ok(())
}

#[cfg(test)]
mod contract_action_tests {
    use super::retain_applied_contract_actions;
    use crate::domain::ContractAction;
    use indexer_common::domain::{ContractAttributes, TransactionResult};

    #[test]
    fn retains_only_contract_actions_with_an_applied_execution_phase() {
        let actions = || {
            vec![
                action(7, ContractAttributes::Deploy, false),
                action(8, call(), false),
                action(9, call(), true),
                action(u16::MAX, ContractAttributes::Update, false),
            ]
        };

        let mut all_succeeded = actions();
        retain_applied_contract_actions(&mut all_succeeded, &TransactionResult::Success).unwrap();
        assert_eq!(segments(&all_succeeded), vec![7, 8, 9, u16::MAX]);

        let mut all_failed = actions();
        retain_applied_contract_actions(&mut all_failed, &TransactionResult::Failure).unwrap();
        assert!(all_failed.is_empty());

        let mut partial = actions();
        retain_applied_contract_actions(
            &mut partial,
            &TransactionResult::PartialSuccess(vec![
                (0, true),
                (7, false),
                (8, false),
                (9, false),
                (u16::MAX, true),
            ]),
        )
        .unwrap();

        // Regression assertion: 4.3.301 incorrectly removed segment 9 solely because its physical
        // segment failed, despite its guaranteed transcript having executed in segment 0.
        assert_eq!(segments(&partial), vec![9, u16::MAX]);

        let mut inconsistent = vec![action(9, call(), true)];
        let original = inconsistent.clone();
        assert_eq!(
            retain_applied_contract_actions(
                &mut inconsistent,
                &TransactionResult::PartialSuccess(vec![(0, true)]),
            ),
            Err(9)
        );
        assert_eq!(inconsistent, original);
    }

    fn call() -> ContractAttributes {
        ContractAttributes::Call {
            entry_point: "entry-point".to_owned(),
        }
    }

    fn action(
        segment: u16,
        attributes: ContractAttributes,
        has_guaranteed_transcript: bool,
    ) -> ContractAction {
        ContractAction {
            address: Default::default(),
            segment,
            has_guaranteed_transcript,
            raw_entry_point: match &attributes {
                ContractAttributes::Call { entry_point } => {
                    Some(entry_point.as_bytes().to_vec().into())
                }
                _ => None,
            },
            state_key: Default::default(),
            zswap_state_key: Default::default(),
            extracted_balances: Default::default(),
            attributes,
        }
    }

    fn segments(actions: &[ContractAction]) -> Vec<u16> {
        actions.iter().map(|action| action.segment).collect()
    }
}

#[cfg(test)]
mod tblock_skew_tests {
    use super::{node_skews_first_regular_tblock, should_bump_first_regular_tblock};
    use indexer_common::domain::ProtocolVersion;

    #[test]
    fn skews_only_for_runtimes_importing_the_skewing_host_functions() {
        let skews = |spec_version: u32| {
            node_skews_first_regular_tblock(
                ProtocolVersion::try_from(spec_version).expect("supported protocol version"),
            )
        };

        assert!(skews(22_000));
        assert!(skews(1_000_000));
        assert!(skews(1_000_002));
        assert!(skews(1_000_299));
        assert!(!skews(1_000_300));
        assert!(!skews(1_000_999));
        assert!(skews(2_000_000));
        assert!(!skews(2_001_000));
    }

    #[test]
    fn bumps_only_non_genesis_blocks_built_by_skewing_runtimes() {
        assert!(!should_bump_first_regular_tblock(
            0,
            ProtocolVersion::V0_22(22_000),
        ));
        assert!(should_bump_first_regular_tblock(
            1,
            ProtocolVersion::V1_0(1_000_299),
        ));
        assert!(!should_bump_first_regular_tblock(
            1,
            ProtocolVersion::V1_0(1_000_300),
        ));
        assert!(should_bump_first_regular_tblock(
            1,
            ProtocolVersion::V2_0(2_000_000),
        ));
        assert!(!should_bump_first_regular_tblock(
            1,
            ProtocolVersion::V2_1(2_001_000),
        ));
    }

    /// Preview block 128537's first (and only) regular transaction, replayed as if it had waited
    /// one block in the pool and been included on a node 1.0.300 runtime: parent 1784987076 (the
    /// original block's time), block 1784987082, intent TTL 1784987084.
    ///
    /// Node 1.0.300 verifies it at the block's own time (1784987082 <= TTL) and accepts it. The
    /// unconditional bump verifies it at parent + 12s (1784987088 > TTL) and halts indexing on a
    /// block the node accepted. This is the regression for gating the bump on the runtime.
    #[cfg(feature = "standalone")]
    #[tokio::test]
    async fn first_tx_on_node_1_0_300_runtime_is_verified_at_block_time() {
        use crate::domain::{LedgerState, node};
        use indexer_common::{
            domain::{BlockHash, ByteVec, LedgerVersion, NetworkId, ledger},
            infra::ledger_db,
        };

        const PARENT_BLOCK_TIMESTAMP: u64 = 1_784_987_076_000;
        const BLOCK_TIMESTAMP: u64 = 1_784_987_082_000;
        const INTENT_TTL_EXPIRED: &str = "Intent TTL has expired";

        let temp_dir = tempfile::tempdir().expect("create tempdir");
        ledger_db::init(ledger_db::Config {
            cache_max_nodes: 1_024,
            cnn_url: temp_dir
                .path()
                .join("ledger-db.sqlite")
                .display()
                .to_string(),
        })
        .await
        .expect("init ledger DB");

        let raw: ByteVec = std::fs::read(format!(
            "{}/../indexer-common/tests/block_128537_tx.raw",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("read block_128537_tx.raw")
        .into();
        let transaction = ledger::Transaction::deserialize(&raw, LedgerVersion::V8)
            .expect("deserialize fixture transaction");
        let hash = transaction.hash();
        let identifiers = transaction.identifiers().expect("identifiers");

        let apply = |protocol_version: ProtocolVersion| {
            let network_id: NetworkId = "preview".try_into().expect("network id");
            let mut ledger_state =
                LedgerState::new(network_id, LedgerVersion::V8).expect("create ledger state");
            let transaction = node::Transaction::Regular(node::RegularTransaction {
                hash,
                protocol_version,
                raw: raw.clone(),
                identifiers: identifiers.clone(),
                contract_actions: vec![],
            });

            ledger_state
                .apply_transactions(
                    [transaction],
                    BlockHash::from([0; 32]),
                    BLOCK_TIMESTAMP,
                    PARENT_BLOCK_TIMESTAMP,
                    node_skews_first_regular_tblock(protocol_version),
                )
                .map(|_| ())
                .map_err(|error| format!("{:#}", anyhow::Error::from(error)))
        };

        // A skewing runtime: the bump rejects the transaction on the intent TTL.
        let error = apply(ProtocolVersion::V1_0(1_000_000)).expect_err("bump must reject");
        assert!(
            error.contains(INTENT_TTL_EXPIRED) && error.contains("Timestamp(1784987088)"),
            "unexpected error: {error}"
        );

        // A node 1.0.300 runtime: the intent TTL check passes. The transaction still fails later,
        // against a fresh state instead of preview's, which is irrelevant to the `tblock`.
        match apply(ProtocolVersion::V1_0(1_000_300)) {
            Ok(()) => {}
            Err(error) => assert!(
                !error.contains(INTENT_TTL_EXPIRED),
                "node 1.0.300 runtime must not reject on the intent TTL: {error}"
            ),
        }
    }
}
