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
    NetworkId, SerializedContractAddress, SerializedLedgerStateKey, TransactionHash,
    TransactionResult,
    ledger::{self, LedgerParameters},
};
use std::ops::DerefMut;
use thiserror::Error;

/// The node bumps a mempool transaction's well-formed `tblock` two slots (2 × 6s) ahead of the
/// parent block time; reproduce that same offset here. See `apply_transactions`.
const MEMPOOL_TBLOCK_BUMP_MILLIS: u64 = 2 * 6_000;

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

    /// Apply the given node transactions to this ledger state and return domain transactions.
    ///
    /// `bump_first_regular_tblock` selects whether the node's mempool-cached validity result is
    /// reproduced for the first regular transaction (see below). It must be `false` for the genesis
    /// block (height 0): the transactions embedded in genesis never transited the mempool, so the
    /// node never cached a bumped result for them and validated them against the real block time.
    /// Bumping them would push the well-formed `tblock` past a bootstrap transaction's intent TTL
    /// and wrongly reject it.
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
        // actually applied; Deploy and Update only execute in their physical segment.
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

        // Update contract actions.
        for contract_action in transaction.contract_actions.iter_mut() {
            let zswap_state = self
                .extract_contract_zswap_state(&contract_action.address)
                .map_err(|error| Error::ExtractContractZswapState(transaction.hash, error))?;
            contract_action.zswap_state = zswap_state;

            // Actions from failed segments are gone by now, so every remaining one must have a
            // state on the node. If not, our view of the ledger disagrees with the node's, which
            // is not something we can silently index around.
            if contract_action.state.is_empty() {
                return Err(Error::MissingContractState(
                    transaction.hash,
                    contract_action.address.clone(),
                ));
            }

            let contract_state = ledger::ContractState::deserialize(
                &contract_action.state,
                transaction.protocol_version.ledger_version(),
            )
            .map_err(|error| {
                Error::DeserializeContractState(
                    transaction.hash,
                    contract_action.address.clone(),
                    error,
                )
            })?;
            let balances = contract_state.balances().map_err(|error| {
                Error::GetContractBalances(transaction.hash, contract_action.address.clone(), error)
            })?;
            contract_action.extracted_balances = balances;
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

    #[error("cannot extract contract zswap state for transaction {0}")]
    ExtractContractZswapState(
        TransactionHash,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error("cannot deserialize contract state for transaction {0} and contract address {1}")]
    DeserializeContractState(
        TransactionHash,
        SerializedContractAddress,
        #[source] indexer_common::domain::ledger::Error,
    ),

    #[error("no contract state for transaction {0} and contract address {1}")]
    MissingContractState(TransactionHash, SerializedContractAddress),

    #[error("transaction {0} has a contract action in segment {1}, but no result for that segment")]
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
            state: Default::default(),
            segment,
            has_guaranteed_transcript,
            zswap_state: Default::default(),
            extracted_balances: Default::default(),
            attributes,
        }
    }

    fn segments(actions: &[ContractAction]) -> Vec<u16> {
        actions.iter().map(|action| action.segment).collect()
    }
}
