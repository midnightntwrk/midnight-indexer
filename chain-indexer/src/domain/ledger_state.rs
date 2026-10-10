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

use crate::domain::{
    BlockRef, ContractAction, RegularTransaction, SystemTransaction, Transaction,
    extrinsic::{Applied, Phase, divergence},
    node,
};
use derive_more::derive::{Deref, From};
use fastrace::trace;
use indexer_common::domain::{
    ApplyRegularTransactionOutcome, ApplySystemTransactionOutcome, BlockHash, LedgerVersion,
    NetworkId, ProtocolVersion, SerializedContractAddress, SerializedLedgerStateKey,
    TransactionHash, TransactionResult,
    ledger::{self, LedgerParameters, RootCountRepair},
};
use log::{debug, warn};
use std::{
    collections::{HashMap, HashSet},
    ops::DerefMut,
};
use thiserror::Error;

/// Amount, in milliseconds, by which a block's first regular transaction's well-formed `tblock` is
/// bumped ahead of block time. "First" here and below means the first regular transaction to
/// apply, and any failed ones before it: a failed transaction leaves the ledger state unchanged,
/// so the node's validity cache serves all of them from the parent block's state. The node
/// validates mempool transactions against a `tblock` bumped `slot_duration_secs +
/// skipped_slots_margin` (one slot each, two slots by default) ahead of block time. Midnight
/// slots are 6s, so the default bump is two slots. Block timestamps are milliseconds.
const MEMPOOL_TBLOCK_BUMP_MILLIS: u64 = 2 * 6_000;

/// First node 1.0 runtime `spec_version` whose ledger-8 host functions no longer skew the first
/// regular transaction's well-formed `tblock`. Node 1.0.300 added version 2 of
/// `Ledger8Bridge::apply_transaction`/`validate_guaranteed_execution`, which verify against the
/// block's own time; runtimes before it import version 1, which keeps the skew. Which one ran is
/// decided by the runtime that built the block, so blocks before the `set_code` still skew.
///
/// See <https://github.com/midnightntwrk/midnight-node/issues/1924>.
const FIRST_UNSKEWED_NODE_1_0_SPEC_VERSION: u32 = 1_000_300;

/// Whether the node may have skewed the first regular transaction's well-formed `tblock` by
/// `MEMPOOL_TBLOCK_BUMP_MILLIS` off the parent block time, for a block built by the runtime with the
/// given protocol version; that is the runtime recorded in the block's MNSV digest, not the one in
/// its state, which is newer at a runtime-upgrade enactment block.
///
/// - 0.22, 1.0 before `FIRST_UNSKEWED_NODE_1_0_SPEC_VERSION` and 2.0 serve the first transaction's
///   validity from the strict cache warmed during mempool ingress, i.e. verify it at the bumped
///   `tblock`, if the cache holds it for the parent block's ledger state. The block author may have
///   validated it against an older state, in which case the node verifies it against the block's
///   own time.
/// - 1.0 from `FIRST_UNSKEWED_NODE_1_0_SPEC_VERSION` on (`Ledger8Bridge` version 2) and 2.1 (whose
///   ledger-8 and ledger-9 host functions never skew) verify it against the block's own time.
///
/// The block does not record whether the cache held the transaction. A skewing runtime's first
/// regular transaction is therefore accepted if it is well-formed at the block time or at the
/// bumped `tblock`.
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
    /// reproduced for the first regular transaction (see below). Set, it accepts the transaction
    /// at a `tblock` the node may have used besides the block time, so it is `false` where the
    /// node never used one: the genesis block (height 0), whose transactions never transited the
    /// mempool, and blocks built by a runtime that no longer skews. The caller decides this with
    /// `should_bump_first_regular_tblock`.
    #[trace(properties = { "parent_block_hash": "{parent_block_hash}" })]
    pub fn apply_transactions(
        &mut self,
        transactions: impl IntoIterator<Item = (Phase, node::Transaction, Result<Applied, String>)>,
        block: BlockRef,
        parent_block_hash: BlockHash,
        block_timestamp: u64,
        parent_block_timestamp: u64,
        bump_first_regular_tblock: bool,
    ) -> Result<(Vec<Transaction>, LedgerParameters), Error> {
        // The node validates a pool transaction at the parent block time plus two slots (see its
        // `pallet-midnight` `validate_unsigned`) and caches the result keyed on the ledger state.
        // At inclusion the cache hits only while the state is still the parent's, i.e. before a
        // regular transaction applies (a failed one leaves the state unchanged), so such a
        // transaction is verified at that adjusted `tblock` and later ones at the block time. The
        // base is the parent time, not the block time: adjusting from the block overshoots by the
        // inter-block gap.
        //
        // The cache misses if the author validated the transaction against an older state, and
        // the block does not record which. So verify at the block time and, if malformed there,
        // at the adjusted `tblock`. `apply` always runs at the block time, so the state matches the
        // node.
        let mut no_regular_transaction_applied = true;
        let transactions = transactions
            .into_iter()
            .map(|(phase, transaction, outcome)| {
                use node::Transaction::*;

                match (transaction, outcome) {
                    // Failed at dispatch: the state is unchanged, so the transaction is never
                    // passed to the ledger. It does not count as applied for the
                    // first-regular-`tblock` rule.
                    (Regular(transaction), Err(error)) => {
                        debug!(
                            transaction_hash:% = transaction.hash,
                            phase:?,
                            error:%;
                            "regular transaction failed at dispatch"
                        );

                        self.failed_regular_transaction(transaction)
                            .map(|transaction| Transaction::Regular(transaction.into()))
                    }

                    (Regular(transaction), Ok(applied)) => {
                        let well_formed_timestamp = (no_regular_transaction_applied
                            && bump_first_regular_tblock)
                            .then_some(parent_block_timestamp + MEMPOOL_TBLOCK_BUMP_MILLIS);

                        let (transaction, failure_reason) = self.apply_regular_transaction(
                            transaction,
                            parent_block_hash,
                            block_timestamp,
                            parent_block_timestamp,
                            well_formed_timestamp,
                        )?;
                        no_regular_transaction_applied &=
                            matches!(transaction.transaction_result, TransactionResult::Failure);

                        check_regular_transaction(
                            block,
                            &transaction,
                            failure_reason,
                            &applied,
                            phase,
                        );

                        Ok(Transaction::Regular(transaction.into()))
                    }

                    (System(transaction), outcome) => {
                        let transaction =
                            self.apply_system_transaction(transaction, block_timestamp)?;

                        // Only applied system transactions are ever passed in.
                        match outcome {
                            Ok(applied) => check_hash(block, transaction.hash(), &applied, phase),

                            Err(error) => divergence(
                                block,
                                format_args!(
                                    "system transaction {} in {phase:?}: failed on chain \
                                     ({error}), but it is applied",
                                    transaction.hash()
                                ),
                            ),
                        }

                        Ok(transaction)
                    }
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

    // Converts a regular transaction that failed at dispatch into a domain regular transaction
    // with a `Failure` result. It never reaches the ledger, whose state it leaves unchanged: no
    // fees, no UTXOs, no ledger events, no contract actions, and the state indices and root are the
    // current ones.
    fn failed_regular_transaction(
        &self,
        transaction: node::RegularTransaction,
    ) -> Result<RegularTransaction, Error> {
        let mut transaction = RegularTransaction::from(transaction);

        self.set_state_range(
            &mut transaction,
            self.zswap_first_free(),
            self.dust_commitments_first_free(),
            self.dust_generations_first_free(),
        )?;
        transaction.transaction_result = TransactionResult::Failure;
        transaction.paid_fees = 0;
        transaction.estimated_fees = 0;
        transaction.contract_actions.clear();

        Ok(transaction)
    }

    // Sets where the transaction leaves the ledger state: the current zswap root and, from the
    // given start indices, the current first-free indices as end indices.
    fn set_state_range(
        &self,
        transaction: &mut RegularTransaction,
        zswap_start_index: u64,
        dust_commitment_start_index: u64,
        dust_generation_start_index: u64,
    ) -> Result<(), Error> {
        transaction.zswap_merkle_tree_root = self
            .zswap_merkle_tree_root()
            .serialize()
            .map_err(|error| Error::SerializeMerkleTreeRoot(transaction.hash, error))?;
        transaction.zswap_start_index = zswap_start_index;
        transaction.zswap_end_index = self.zswap_first_free();
        transaction.dust_commitment_start_index = dust_commitment_start_index;
        transaction.dust_commitment_end_index = self.dust_commitments_first_free();
        transaction.dust_generation_start_index = dust_generation_start_index;
        transaction.dust_generation_end_index = self.dust_generations_first_free();

        Ok(())
    }

    // Applies one regular transaction and converts it into a domain regular transaction, with the
    // ledger's reason if it fails.
    //
    // `well_formed_timestamp` is a second `tblock`, in milliseconds, to verify the transaction at
    // if it is malformed at `block_timestamp`. It is the adjusted `tblock` for a regular
    // transaction before any applied one in a block whose runtime skews it, and `None` for every
    // other transaction.
    #[trace(properties = {
        "parent_block_hash": "{parent_block_hash}",
        "block_timestamp": "{block_timestamp}",
        "well_formed_timestamp": "{well_formed_timestamp:?}"
    })]
    fn apply_regular_transaction(
        &mut self,
        transaction: node::RegularTransaction,
        parent_block_hash: BlockHash,
        block_timestamp: u64,
        parent_block_timestamp: u64,
        well_formed_timestamp: Option<u64>,
    ) -> Result<(RegularTransaction, Option<String>), Error> {
        let mut transaction = RegularTransaction::from(transaction);

        // Apply transaction.
        let start_index = self.zswap_first_free();
        let dust_commitment_start_index = self.dust_commitments_first_free();
        let dust_generation_start_index = self.dust_generations_first_free();
        let mut apply = |well_formed_timestamp| {
            self.0.apply_regular_transaction(
                &transaction.raw,
                parent_block_hash,
                block_timestamp,
                parent_block_timestamp,
                well_formed_timestamp,
            )
        };
        // Verify at the block time and, if malformed there, at `well_formed_timestamp`.
        let outcome = match (apply(block_timestamp), well_formed_timestamp) {
            // A malformed transaction leaves the ledger state untouched, so the retry applies to
            // the same state. `well_formed` returns the same `VerifiedTransaction` at either
            // `tblock`; only its checks depend on it. The error reported is the one at block time.
            (Err(error @ ledger::Error::MalformedTransaction(_)), Some(well_formed_timestamp)) => {
                apply(well_formed_timestamp).map_err(|retry_error| {
                    warn!(
                        transaction_hash:% = transaction.hash,
                        parent_block_hash:%,
                        block_timestamp,
                        well_formed_timestamp,
                        retry_error:%;
                        "regular transaction malformed at the retried tblock as well"
                    );
                    error
                })
            }
            (outcome, _) => outcome,
        };
        let ApplyRegularTransactionOutcome {
            transaction_result,
            created_unshielded_utxos,
            spent_unshielded_utxos,
            ledger_events,
            fees,
            failure_reason,
            bridge_claim,
        } = outcome
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
        self.set_state_range(
            &mut transaction,
            start_index,
            dust_commitment_start_index,
            dust_generation_start_index,
        )?;
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

        Ok((transaction, failure_reason))
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

/// The ledger result of an applied regular transaction must be how it was applied on chain; a
/// disagreement is a divergence.
fn check_regular_transaction(
    block: BlockRef,
    transaction: &RegularTransaction,
    failure_reason: Option<String>,
    applied: &Applied,
    phase: Phase,
) {
    use {Applied::*, TransactionResult::*};

    let agrees = matches!(
        (applied, &transaction.transaction_result),
        (Fully { .. }, Success) | (Partially { .. }, PartialSuccess(_))
    );
    if !agrees {
        let reason = failure_reason
            .map(|reason| format!(": {reason}"))
            .unwrap_or_default();
        divergence(
            block,
            format_args!(
                "transaction {} in {phase:?}: {applied:?} on chain, {:?} in the ledger{reason}",
                transaction.hash, transaction.transaction_result
            ),
        );
    }

    check_hash(block, transaction.hash, applied, phase);
}

/// The hash recorded on chain for an applied transaction must be the indexer's.
fn check_hash(block: BlockRef, hash: TransactionHash, applied: &Applied, phase: Phase) {
    let tx_hash = applied.tx_hash();
    if tx_hash != hash {
        divergence(
            block,
            format_args!("transaction {hash} in {phase:?}: hash {tx_hash} on chain"),
        );
    }
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
}

#[cfg(all(test, any(feature = "cloud", feature = "standalone")))]
mod apply_transactions_tblock_tests {
    use super::should_bump_first_regular_tblock;
    use crate::domain::{
        BlockRef, LedgerState, Transaction,
        extrinsic::{Applied, Phase},
        node,
    };
    use indexer_common::{
        domain::{
            BlockHash, LedgerVersion, ProtocolVersion, SerializedTransaction, TransactionHash,
            TransactionResult, ledger,
        },
        error::BoxError,
        testing::{Malformed, NETWORK_ID, dust_registration, init_ledger_db, malformed},
    };
    use std::fs;

    // Block time of every test block, in seconds. Each test sets its parent block time relative to
    // it, and the bumped `tblock` is `parent + 12s`.
    const NOW: u64 = 1_800_000_000;

    // One block applied at `NOW` to a fresh ledger state. Times are in seconds; the adjusted
    // `tblock` is `parent_block_time + 12s`.
    #[cfg(not(feature = "divergence-halt"))]
    struct Case {
        name: &'static str,
        // Intent TTL and dust `ctime` of each regular transaction.
        transactions: &'static [(u64, u64)],
        parent_block_time: u64,
        bump_first_regular_tblock: bool,
        expected: Result<Vec<TransactionResult>, Malformed>,
    }

    // On a runtime that skews, a regular transaction is accepted if well-formed at the block time
    // or at the adjusted `tblock` until one applies; from then on only at the block time.
    //
    // Some cases have the ledger fail a transaction that `apply` takes to be applied on chain,
    // which is a divergence and panics with `divergence-halt`, so this runs in default builds.
    #[tokio::test(flavor = "multi_thread")]
    #[cfg(not(feature = "divergence-halt"))]
    async fn regular_transactions_are_accepted_at_the_adjusted_tblock_until_one_applies()
    -> Result<(), BoxError> {
        use Malformed::{IntentTtlExpired, OutOfDustValidityWindow};
        use TransactionResult::{Failure, Success};

        let _ledger_db = init_ledger_db().await?;

        for ledger_version in [LedgerVersion::V8, LedgerVersion::V9] {
            let protocol_version = skewing_protocol_version(ledger_version);
            let cases = [
                // With the parent block in the previous 6s slot the adjusted `tblock` is
                // `NOW + 6s`. A dust `ctime` of `NOW + 4s` is valid there but not at the block
                // time, and only the first regular transaction is retried there.
                Case {
                    name: "dust ctime ahead, bumped",
                    transactions: &[(NOW + 60, NOW + 4)],
                    parent_block_time: NOW - 6,
                    bump_first_regular_tblock: true,
                    expected: Ok(vec![Success]),
                },
                Case {
                    name: "dust ctime ahead, not bumped",
                    transactions: &[(NOW + 60, NOW + 4)],
                    parent_block_time: NOW - 6,
                    bump_first_regular_tblock: false,
                    expected: Err(OutOfDustValidityWindow),
                },
                Case {
                    name: "dust ctime ahead, second transaction",
                    transactions: &[(NOW + 60, NOW), (NOW + 60, NOW + 4)],
                    parent_block_time: NOW - 6,
                    bump_first_regular_tblock: true,
                    expected: Err(OutOfDustValidityWindow),
                },
                // `NOW + 6s` is past an intent TTL of `NOW + 2s`; the block time is not.
                Case {
                    name: "ttl before adjusted tblock, bumped",
                    transactions: &[(NOW + 2, NOW)],
                    parent_block_time: NOW - 6,
                    bump_first_regular_tblock: true,
                    expected: Ok(vec![Success]),
                },
                // With the two slots before the block skipped the adjusted `tblock` is `NOW - 6s`,
                // before the block time, as it is based on the parent block time. An intent TTL of
                // `NOW - 5s` passes `well_formed` there, and `apply` fails it at the block time.
                Case {
                    name: "ttl before block time, bumped",
                    transactions: &[(NOW - 5, NOW - 18)],
                    parent_block_time: NOW - 18,
                    bump_first_regular_tblock: true,
                    expected: Ok(vec![Failure]),
                },
                Case {
                    name: "ttl before block time, not bumped",
                    transactions: &[(NOW - 5, NOW - 18)],
                    parent_block_time: NOW - 18,
                    bump_first_regular_tblock: false,
                    expected: Err(IntentTtlExpired),
                },
                // A dust `ctime` of `NOW - 5s` is past `NOW - 6s` but valid at the block time.
                Case {
                    name: "dust ctime after adjusted tblock, bumped",
                    transactions: &[(NOW + 40, NOW - 5)],
                    parent_block_time: NOW - 18,
                    bump_first_regular_tblock: true,
                    expected: Ok(vec![Success]),
                },
                Case {
                    name: "dust ctime after adjusted tblock, not bumped",
                    transactions: &[(NOW + 40, NOW - 5)],
                    parent_block_time: NOW - 18,
                    bump_first_regular_tblock: false,
                    expected: Ok(vec![Success]),
                },
                // Malformed at both `tblock`s, with a different error at each: the error at the
                // block time is reported.
                Case {
                    name: "malformed at both, adjusted tblock after the block time",
                    transactions: &[(NOW + 2, NOW + 4)],
                    parent_block_time: NOW - 6,
                    bump_first_regular_tblock: true,
                    expected: Err(OutOfDustValidityWindow),
                },
                Case {
                    name: "malformed at both, adjusted tblock before the block time",
                    transactions: &[(NOW - 5, NOW - 5)],
                    parent_block_time: NOW - 18,
                    bump_first_regular_tblock: true,
                    expected: Err(IntentTtlExpired),
                },
                // A failed regular transaction leaves the state unchanged, so the next one is
                // still verified at the adjusted `tblock` `NOW - 6s`: an intent TTL of `NOW - 3s`
                // passes `well_formed` there, and `apply` fails it at the block time as well.
                Case {
                    name: "ttl before block time, after a failed transaction",
                    transactions: &[(NOW - 5, NOW - 18), (NOW - 3, NOW - 18)],
                    parent_block_time: NOW - 18,
                    bump_first_regular_tblock: true,
                    expected: Ok(vec![Failure, Failure]),
                },
            ];

            for case in cases {
                let mut transactions = vec![];
                for &(ttl, ctime) in case.transactions {
                    transactions.push(dust_registration(ledger_version, ttl, ctime).await?);
                }
                let transactions = transactions
                    .iter()
                    .map(|transaction| (transaction, fully_applied as fn(_) -> _))
                    .collect::<Vec<_>>();

                assert_eq!(
                    apply(
                        NETWORK_ID,
                        protocol_version,
                        &transactions,
                        NOW,
                        case.parent_block_time,
                        case.bump_first_regular_tblock,
                    )?
                    .map(|transactions| {
                        transactions
                            .iter()
                            .map(|transaction| regular(transaction).transaction_result.clone())
                            .collect::<Vec<_>>()
                    }),
                    case.expected,
                    "{ledger_version}: {}",
                    case.name
                );
            }
        }

        Ok(())
    }

    // Preprod block 164460's first (and only) regular transaction, built by a 0.22 runtime: parent
    // 1775081610, block 1775081616, intent TTL 1775081620. The node accepted it, so it verified the
    // transaction at the block time; the adjusted `tblock` 1775081622 is past the TTL. The block
    // author validated the transaction against an older state than the parent block's, so the
    // cached validity result did not apply at inclusion.
    #[tokio::test(flavor = "multi_thread")]
    async fn preprod_164460_transaction_with_its_ttl_between_the_tblocks_is_accepted_at_block_time()
    -> Result<(), BoxError> {
        const BLOCK_HEIGHT: u64 = 164_460;
        const BLOCK_TIME: u64 = 1_775_081_616;
        const PARENT_BLOCK_TIME: u64 = 1_775_081_610;
        const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V0_22(22_000);
        const TRANSACTION_HASH: &str =
            "6a1005eecf695a8f950e8f8e74de0c6336daf55448326bf0db0b3b55c089ad0b";
        const CONTRACT_ADDRESS: &str =
            "18835f54e98cfbf5c789ef76fb79d4cb0e8d84d627ef1e36cdf27cf3cdbaebb7";

        let _ledger_db = init_ledger_db().await?;
        let transaction = fixture("block_164460_tx.raw", TRANSACTION_HASH)?;

        // Against a fresh state instead of preprod's, the transaction passes the time checks and
        // fails the next stateful check: the contract it calls does not exist.
        assert_eq!(
            apply(
                "preprod",
                PROTOCOL_VERSION,
                &[(&transaction, fully_applied)],
                BLOCK_TIME,
                PARENT_BLOCK_TIME,
                should_bump_first_regular_tblock(BLOCK_HEIGHT, PROTOCOL_VERSION),
            )?,
            Err(Malformed::Other(format!(
                "call to non-existant contract ContractAddress({CONTRACT_ADDRESS})"
            )))
        );

        Ok(())
    }

    // Mainnet block 1788980's first (and only) regular transaction, built by a 1.0 runtime (spec
    // 1000000): parent 1784643552, block 1784643558, intent TTL 1784643562. The node accepted it at
    // the block time; the adjusted `tblock` 1784643564 is past the TTL. This is the block a fresh
    // mainnet sync on node 1.0.300 halts at (midnight-node #2216).
    #[tokio::test(flavor = "multi_thread")]
    async fn mainnet_1788980_transaction_with_its_ttl_between_the_tblocks_is_accepted_at_block_time()
    -> Result<(), BoxError> {
        const BLOCK_HEIGHT: u64 = 1_788_980;
        const BLOCK_TIME: u64 = 1_784_643_558;
        const PARENT_BLOCK_TIME: u64 = 1_784_643_552;
        const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V1_0(1_000_000);
        const TRANSACTION_HASH: &str =
            "e769b82781bbfd1e29d602a17916abe6e967ef023eb94d23a4aa8b88a6e35c0a";
        const CONTRACT_ADDRESS: &str =
            "4fd31443997bd04bbf0b94e2ef3d5b0ff05479c4fb80bcac0dc74b2c763282e5";

        let _ledger_db = init_ledger_db().await?;
        let transaction = fixture("block_1788980_tx.raw", TRANSACTION_HASH)?;

        // Against a fresh state instead of mainnet's, the transaction passes the time checks and
        // fails the next stateful check: the contract it calls does not exist.
        assert_eq!(
            apply(
                "mainnet",
                PROTOCOL_VERSION,
                &[(&transaction, fully_applied)],
                BLOCK_TIME,
                PARENT_BLOCK_TIME,
                should_bump_first_regular_tblock(BLOCK_HEIGHT, PROTOCOL_VERSION),
            )?,
            Err(Malformed::Other(format!(
                "call to non-existant contract ContractAddress({CONTRACT_ADDRESS})"
            )))
        );

        Ok(())
    }

    // Preview block 128537's first (and only) regular transaction, built by a 1.0 runtime (spec
    // 1000000), replayed as if it had waited one block in the pool: parent 1784987076 (the
    // original block's time), block 1784987082, intent TTL 1784987084.
    //
    // The adjusted `tblock` 1784987088 is past the TTL; the block time is not, on the block's
    // runtime and on 1.0.300, the first that does not skew.
    #[tokio::test(flavor = "multi_thread")]
    async fn preview_128537_transaction_with_its_ttl_between_the_tblocks_is_accepted_at_block_time()
    -> Result<(), BoxError> {
        const BLOCK_HEIGHT: u64 = 128_537;
        const BLOCK_TIME: u64 = 1_784_987_082;
        const PARENT_BLOCK_TIME: u64 = 1_784_987_076;
        const TRANSACTION_HASH: &str =
            "3864da188d7c1d85062c914f879e1e4c096d443abf139aa9717b5266945959e8";

        let _ledger_db = init_ledger_db().await?;
        let transaction = fixture("block_128537_tx.raw", TRANSACTION_HASH)?;

        // Against a fresh state instead of preview's, the transaction passes the time checks and
        // fails the next stateful check: its dust spend proof does not verify.
        use ProtocolVersion::*;
        for protocol_version in [V1_0(1_000_000), V1_0(1_000_300)] {
            let result = apply(
                "preview",
                protocol_version,
                &[(&transaction, fully_applied)],
                BLOCK_TIME,
                PARENT_BLOCK_TIME,
                should_bump_first_regular_tblock(BLOCK_HEIGHT, protocol_version),
            )?;
            let Err(Malformed::Other(reason)) = result else {
                panic!("{protocol_version:?}: a fresh state has no dust to spend: {result:?}");
            };
            assert!(
                reason.starts_with("dust spend proof failed to verify"),
                "{protocol_version:?}: {reason}"
            );
        }

        Ok(())
    }

    fn regular(transaction: &Transaction) -> &crate::domain::RegularTransaction {
        match transaction {
            Transaction::Regular(transaction) => transaction,
            Transaction::System(_) => panic!("expected a regular transaction"),
        }
    }

    // A transaction that failed at dispatch is never applied: it is recorded as failed with no
    // fees and no effects, and it leaves the ledger state, and the indices recorded for the next
    // one, untouched.
    #[tokio::test(flavor = "multi_thread")]
    async fn failed_transaction_is_recorded_as_failure_and_leaves_the_state_untouched()
    -> Result<(), BoxError> {
        let _ledger_db = init_ledger_db().await?;

        for ledger_version in [LedgerVersion::V8, LedgerVersion::V9] {
            let protocol_version = skewing_protocol_version(ledger_version);
            let transaction = dust_registration(ledger_version, NOW + 60, NOW).await?;

            let transactions = apply(
                NETWORK_ID,
                protocol_version,
                &[(&transaction, failed), (&transaction, fully_applied)],
                NOW,
                NOW - 6,
                true,
            )?
            .unwrap();
            let [failed, applied] = transactions.as_slice() else {
                panic!("{ledger_version}: two transactions expected");
            };
            let (failed, applied) = (regular(failed), regular(applied));

            // Were it applied, this transaction would succeed, as it does second.
            assert_eq!(
                failed.transaction_result,
                TransactionResult::Failure,
                "{ledger_version}"
            );
            assert_eq!(applied.transaction_result, TransactionResult::Success);
            assert_eq!(failed.hash, applied.hash);
            assert_eq!((failed.paid_fees, failed.estimated_fees), (0, 0));
            assert!(failed.created_unshielded_utxos.is_empty());
            assert!(failed.spent_unshielded_utxos.is_empty());
            assert!(failed.ledger_events.is_empty());
            assert!(failed.contract_actions.is_empty());
            assert_eq!(failed.zswap_start_index, failed.zswap_end_index);
            assert_eq!(
                failed.dust_commitment_start_index,
                failed.dust_commitment_end_index
            );
            assert_eq!(
                failed.dust_generation_start_index,
                failed.dust_generation_end_index
            );

            // The state did not move: the next transaction starts where the failed one stood.
            assert_eq!(failed.zswap_end_index, applied.zswap_start_index);
            assert_eq!(
                failed.dust_commitment_end_index,
                applied.dust_commitment_start_index
            );
            assert_eq!(
                failed.dust_generation_end_index,
                applied.dust_generation_start_index
            );
            assert_eq!(
                failed.zswap_merkle_tree_root,
                applied.zswap_merkle_tree_root
            );
        }

        Ok(())
    }

    // A failed transaction does not count as applied for the first-regular-`tblock` rule, so
    // the next regular transaction is still the first: its dust `ctime` of `NOW + 4s` is only
    // valid at the adjusted `tblock` `NOW + 6s`.
    #[tokio::test(flavor = "multi_thread")]
    async fn failed_transaction_does_not_use_up_the_first_regular_tblock_bump()
    -> Result<(), BoxError> {
        let _ledger_db = init_ledger_db().await?;

        for ledger_version in [LedgerVersion::V8, LedgerVersion::V9] {
            let protocol_version = skewing_protocol_version(ledger_version);
            let first = dust_registration(ledger_version, NOW + 60, NOW).await?;
            let second = dust_registration(ledger_version, NOW + 60, NOW + 4).await?;

            let transactions = apply(
                NETWORK_ID,
                protocol_version,
                &[(&first, failed), (&second, fully_applied)],
                NOW,
                NOW - 6,
                true,
            )?
            .unwrap();

            assert_eq!(
                regular(&transactions[1]).transaction_result,
                TransactionResult::Success,
                "{ledger_version}"
            );
        }

        Ok(())
    }

    // A transaction was applied on chain, but the indexer's ledger disagrees, or the hash is not
    // the same: a divergence. By default it is logged and the ledger's result is stored; with
    // `divergence-halt` it panics.
    #[tokio::test(flavor = "multi_thread")]
    #[cfg_attr(
        feature = "divergence-halt",
        should_panic(expected = "Failure in the ledger: ")
    )]
    async fn applied_on_chain_but_failed_in_the_ledger_is_a_divergence() {
        let _ledger_db = init_ledger_db().await.unwrap();
        let ledger_version = LedgerVersion::V9;
        let protocol_version = skewing_protocol_version(ledger_version);

        // Expired at the block time, so the ledger fails it.
        let transaction = dust_registration(ledger_version, NOW - 5, NOW - 18)
            .await
            .unwrap();
        let transactions = apply(
            NETWORK_ID,
            protocol_version,
            &[(&transaction, fully_applied)],
            NOW,
            NOW - 18,
            true,
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            regular(&transactions[0]).transaction_result,
            TransactionResult::Failure
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    #[cfg_attr(
        feature = "divergence-halt",
        should_panic(expected = "Partially { tx_hash")
    )]
    async fn partially_applied_on_chain_but_successful_in_the_ledger_is_a_divergence() {
        let _ledger_db = init_ledger_db().await.unwrap();
        let ledger_version = LedgerVersion::V9;
        let protocol_version = skewing_protocol_version(ledger_version);

        let transaction = dust_registration(ledger_version, NOW + 60, NOW)
            .await
            .unwrap();
        let transactions = apply(
            NETWORK_ID,
            protocol_version,
            &[(&transaction, partially_applied)],
            NOW,
            NOW - 6,
            true,
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            regular(&transactions[0]).transaction_result,
            TransactionResult::Success
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    #[cfg_attr(feature = "divergence-halt", should_panic(expected = "hash abababab"))]
    async fn hash_on_chain_differing_from_the_indexers_is_a_divergence() {
        let _ledger_db = init_ledger_db().await.unwrap();
        let ledger_version = LedgerVersion::V9;
        let protocol_version = skewing_protocol_version(ledger_version);

        let transaction = dust_registration(ledger_version, NOW + 60, NOW)
            .await
            .unwrap();
        let transactions = apply(
            NETWORK_ID,
            protocol_version,
            &[(&transaction, |_| {
                Ok(Applied::Fully {
                    tx_hash: [0xab; 32].into(),
                })
            })],
            NOW,
            NOW - 6,
            true,
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            regular(&transactions[0]).transaction_result,
            TransactionResult::Success
        );
    }

    // The chain's hash and outcome agreeing with the ledger is not a divergence, also with the
    // feature.
    #[tokio::test(flavor = "multi_thread")]
    async fn chain_and_ledger_agreeing_is_not_a_divergence() -> Result<(), BoxError> {
        let _ledger_db = init_ledger_db().await?;
        let ledger_version = LedgerVersion::V9;
        let protocol_version = skewing_protocol_version(ledger_version);

        let raw = dust_registration(ledger_version, NOW + 60, NOW).await?;
        let transactions = apply(
            NETWORK_ID,
            protocol_version,
            &[(&raw, fully_applied)],
            NOW,
            NOW - 6,
            true,
        )?
        .unwrap();

        assert_eq!(
            regular(&transactions[0]).transaction_result,
            TransactionResult::Success
        );

        Ok(())
    }

    // Reads a real ledger-8 transaction from `indexer-common/tests` and checks it is the one with
    // the given hash.
    fn fixture(file_name: &str, hash: &str) -> Result<SerializedTransaction, BoxError> {
        let raw: SerializedTransaction = fs::read(format!(
            "{}/../indexer-common/tests/{file_name}",
            env!("CARGO_MANIFEST_DIR")
        ))?
        .into();
        let transaction = ledger::Transaction::deserialize(&raw, LedgerVersion::V8)?;
        assert_eq!(
            transaction.hash(),
            TransactionHash::from_hex(hash)?,
            "{file_name}"
        );

        Ok(raw)
    }

    // Returns a runtime on the given ledger version that skews the first regular transaction's
    // `tblock`.
    fn skewing_protocol_version(ledger_version: LedgerVersion) -> ProtocolVersion {
        match ledger_version {
            LedgerVersion::V8 => ProtocolVersion::V1_0(1_000_000),
            LedgerVersion::V9 => ProtocolVersion::V2_0(2_000_000),
        }
    }

    fn fully_applied(tx_hash: TransactionHash) -> Result<Applied, String> {
        Ok(Applied::Fully { tx_hash })
    }

    fn partially_applied(tx_hash: TransactionHash) -> Result<Applied, String> {
        Ok(Applied::Partially { tx_hash })
    }

    // Failed at dispatch with `CallFiltered`, as in safe mode.
    fn failed(_: TransactionHash) -> Result<Applied, String> {
        Err("Module(ModuleError { index: 0, error: [5, 0, 0, 0] })".to_string())
    }

    // Applies `transactions` as one block to a fresh ledger state of `network_id`, each with how it
    // was applied on chain, given the indexer's hash for it; the outer error is a test setup
    // failure, the inner one the reason the ledger rejects a transaction. Times are in seconds.
    #[allow(clippy::type_complexity)]
    fn apply(
        network_id: &str,
        protocol_version: ProtocolVersion,
        transactions: &[(
            &SerializedTransaction,
            fn(TransactionHash) -> Result<Applied, String>,
        )],
        block_time: u64,
        parent_block_time: u64,
        bump_first_regular_tblock: bool,
    ) -> Result<Result<Vec<Transaction>, Malformed>, BoxError> {
        let ledger_version = protocol_version.ledger_version();
        let mut ledger_state = LedgerState::new(network_id.try_into()?, ledger_version)?;
        let transactions = transactions
            .iter()
            .enumerate()
            .map(|(i, (raw, outcome))| {
                let transaction = ledger::Transaction::deserialize(raw, ledger_version)?;
                let tx_hash = transaction.hash();
                let transaction = node::Transaction::Regular(node::RegularTransaction {
                    hash: tx_hash,
                    protocol_version,
                    raw: (*raw).clone(),
                    identifiers: transaction.identifiers()?,
                    contract_actions: vec![],
                });
                Ok((
                    Phase::ApplyExtrinsic(i as u32 + 1),
                    transaction,
                    outcome(tx_hash),
                ))
            })
            .collect::<Result<Vec<_>, BoxError>>()?;

        match ledger_state.apply_transactions(
            transactions,
            BlockRef {
                hash: [1; 32].into(),
                height: 1,
            },
            BlockHash::from([0; 32]),
            block_time * 1_000,
            parent_block_time * 1_000,
            bump_first_regular_tblock,
        ) {
            Ok((transactions, _)) => Ok(Ok(transactions)),
            Err(error) => malformed(&error)
                .map(Err)
                .ok_or_else(|| format!("unexpected error: {error:#}").into()),
        }
    }
}
