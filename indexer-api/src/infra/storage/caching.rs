// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! A `CachingStorage` wrapper that embeds `DataLoader` instances for transparent
//! call batching. This is an implementation-only concern; the domain traits are
//! unchanged and callers (resolvers) call standard single-item methods.

use crate::{
    domain::{
        Block, ContractAction, LedgerEvent, RegularTransaction, Transaction, UnshieldedUtxo,
        dust::DustGenerationStatus,
        spo::{
            CommitteeMember, EpochInfo, EpochPerf, FirstValidEpoch, PoolMetadata, PresenceEvent,
            RegisteredStat, RegisteredTotals, Spo, SpoComposite, SpoIdentity, StakeShare,
        },
        system_parameters::{DParameter, TermsAndConditions},
        storage::{
            block::BlockStorage,
            contract_action::ContractActionStorage,
            dust::DustStorage,
            ledger_events::LedgerEventStorage,
            ledger_state::LedgerStateStorage,
            spo::SpoStorage,
            system_parameters::SystemParametersStorage,
            transaction::TransactionStorage,
            unshielded::UnshieldedUtxoStorage,
            wallet::WalletStorage,
        },
    },
    infra::storage::{
        Storage,
        dataloaders::{
            BlockByHashLoader, BlockByHeightLoader, ContractActionsByTransactionIdLoader,
            DustLedgerEventsByTransactionIdLoader, TransactionByIdLoader,
            TransactionsByBlockIdLoader, UnshieldedUtxosCreatedByTransactionIdLoader,
            UnshieldedUtxosSpentByTransactionIdLoader,
            ZswapLedgerEventsByTransactionIdLoader,
        },
    },
};
use async_graphql::dataloader::DataLoader;
use futures::Stream;
use indexer_common::domain::{
    BlockHash, CardanoRewardAddress, LedgerEventGrouping, LedgerVersion, ProtocolVersion,
    SerializedContractAddress, SerializedLedgerStateKey, SerializedTransactionIdentifier,
    SessionId, TransactionHash, UnshieldedAddress, ViewingKey,
};
use sqlx::types::Uuid;
use std::{num::NonZeroU32, sync::Arc};

/// A storage wrapper that transparently batches single-item queries through `DataLoader`,
/// reducing N+1 query patterns without changing the domain `Storage` trait signatures.
#[derive(Clone)]
pub struct CachingStorage {
    inner: Storage,
    block_by_hash: Arc<DataLoader<BlockByHashLoader>>,
    block_by_height: Arc<DataLoader<BlockByHeightLoader>>,
    transaction_by_id: Arc<DataLoader<TransactionByIdLoader>>,
    transactions_by_block_id: Arc<DataLoader<TransactionsByBlockIdLoader>>,
    contract_actions_by_transaction_id: Arc<DataLoader<ContractActionsByTransactionIdLoader>>,
    zswap_ledger_events_by_transaction_id:
        Arc<DataLoader<ZswapLedgerEventsByTransactionIdLoader>>,
    dust_ledger_events_by_transaction_id:
        Arc<DataLoader<DustLedgerEventsByTransactionIdLoader>>,
    unshielded_utxos_created_by_transaction_id:
        Arc<DataLoader<UnshieldedUtxosCreatedByTransactionIdLoader>>,
    unshielded_utxos_spent_by_transaction_id:
        Arc<DataLoader<UnshieldedUtxosSpentByTransactionIdLoader>>,
}

impl CachingStorage {
    pub fn new(inner: Storage) -> Self {
        Self {
            block_by_hash: Arc::new(DataLoader::new(
                BlockByHashLoader(inner.clone()),
                tokio::spawn,
            )),
            block_by_height: Arc::new(DataLoader::new(
                BlockByHeightLoader(inner.clone()),
                tokio::spawn,
            )),
            transaction_by_id: Arc::new(DataLoader::new(
                TransactionByIdLoader(inner.clone()),
                tokio::spawn,
            )),
            transactions_by_block_id: Arc::new(DataLoader::new(
                TransactionsByBlockIdLoader(inner.clone()),
                tokio::spawn,
            )),
            contract_actions_by_transaction_id: Arc::new(DataLoader::new(
                ContractActionsByTransactionIdLoader(inner.clone()),
                tokio::spawn,
            )),
            zswap_ledger_events_by_transaction_id: Arc::new(DataLoader::new(
                ZswapLedgerEventsByTransactionIdLoader(inner.clone()),
                tokio::spawn,
            )),
            dust_ledger_events_by_transaction_id: Arc::new(DataLoader::new(
                DustLedgerEventsByTransactionIdLoader(inner.clone()),
                tokio::spawn,
            )),
            unshielded_utxos_created_by_transaction_id: Arc::new(DataLoader::new(
                UnshieldedUtxosCreatedByTransactionIdLoader(inner.clone()),
                tokio::spawn,
            )),
            unshielded_utxos_spent_by_transaction_id: Arc::new(DataLoader::new(
                UnshieldedUtxosSpentByTransactionIdLoader(inner.clone()),
                tokio::spawn,
            )),
            inner,
        }
    }
}

impl crate::domain::storage::Storage for CachingStorage {}

// ---------------------------------------------------------------------------
// BlockStorage
// ---------------------------------------------------------------------------

impl BlockStorage for CachingStorage {
    async fn get_latest_block(&self) -> Result<Option<Block>, sqlx::Error> {
        self.inner.get_latest_block().await
    }

    async fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>, sqlx::Error> {
        let result: Result<Option<Block>, Arc<sqlx::Error>> = self.block_by_hash.load_one(hash).await;
        result.map_err(|e| sqlx::Error::Protocol(e.to_string()))
    }

    async fn get_block_by_height(&self, height: u32) -> Result<Option<Block>, sqlx::Error> {
        let result: Result<Option<Block>, Arc<sqlx::Error>> = self.block_by_height.load_one(height).await;
        result.map_err(|e| sqlx::Error::Protocol(e.to_string()))
    }

    fn get_blocks(
        &self,
        height: u32,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Block, sqlx::Error>> + Send + use<'_> {
        self.inner.get_blocks(height, batch_size)
    }
}

// ---------------------------------------------------------------------------
// TransactionStorage
// ---------------------------------------------------------------------------

impl TransactionStorage for CachingStorage {
    async fn get_transaction_by_id(&self, id: u64) -> Result<Option<Transaction>, sqlx::Error> {
        let result: Result<Option<Transaction>, Arc<sqlx::Error>> = self.transaction_by_id.load_one(id).await;
        result.map_err(|e| sqlx::Error::Protocol(e.to_string()))
    }

    async fn get_transactions_by_block_id(
        &self,
        id: u64,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        let result: Result<Option<Vec<Transaction>>, Arc<sqlx::Error>> = self.transactions_by_block_id.load_one(id).await;
        result
            .map(|opt| opt.unwrap_or_default())
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))
    }

    async fn get_transactions_by_hash(
        &self,
        hash: TransactionHash,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        self.inner.get_transactions_by_hash(hash).await
    }

    async fn get_transactions_by_identifier(
        &self,
        identifier: &SerializedTransactionIdentifier,
    ) -> Result<Vec<Transaction>, sqlx::Error> {
        self.inner.get_transactions_by_identifier(identifier).await
    }

    fn get_relevant_transactions(
        &self,
        wallet_id: Uuid,
        index: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<RegularTransaction, sqlx::Error>> + Send {
        self.inner
            .get_relevant_transactions(wallet_id, index, batch_size)
    }

    fn get_transactions_by_unshielded_address(
        &self,
        address: UnshieldedAddress,
        from_transaction_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<Transaction, sqlx::Error>> + Send {
        self.inner
            .get_transactions_by_unshielded_address(address, from_transaction_id, batch_size)
    }

    async fn get_highest_transaction_id_for_unshielded_address(
        &self,
        address: UnshieldedAddress,
    ) -> Result<Option<u64>, sqlx::Error> {
        self.inner
            .get_highest_transaction_id_for_unshielded_address(address)
            .await
    }

    async fn get_highest_end_indices(
        &self,
        wallet_id: Uuid,
    ) -> Result<(Option<u64>, Option<u64>, Option<u64>), sqlx::Error> {
        self.inner.get_highest_end_indices(wallet_id).await
    }
}

// ---------------------------------------------------------------------------
// ContractActionStorage
// ---------------------------------------------------------------------------

impl ContractActionStorage for CachingStorage {
    async fn get_contract_deploy_by_address(
        &self,
        address: &SerializedContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        self.inner.get_contract_deploy_by_address(address).await
    }

    async fn get_latest_contract_action_by_address(
        &self,
        address: &SerializedContractAddress,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        self.inner
            .get_latest_contract_action_by_address(address)
            .await
    }

    async fn get_contract_action_by_address_and_block_hash(
        &self,
        address: &SerializedContractAddress,
        hash: BlockHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        self.inner
            .get_contract_action_by_address_and_block_hash(address, hash)
            .await
    }

    async fn get_contract_action_by_address_and_block_height(
        &self,
        address: &SerializedContractAddress,
        height: u32,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        self.inner
            .get_contract_action_by_address_and_block_height(address, height)
            .await
    }

    async fn get_contract_action_by_address_and_transaction_hash(
        &self,
        address: &SerializedContractAddress,
        hash: TransactionHash,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        self.inner
            .get_contract_action_by_address_and_transaction_hash(address, hash)
            .await
    }

    async fn get_contract_action_by_address_and_transaction_identifier(
        &self,
        address: &SerializedContractAddress,
        identifier: &SerializedTransactionIdentifier,
    ) -> Result<Option<ContractAction>, sqlx::Error> {
        self.inner
            .get_contract_action_by_address_and_transaction_identifier(address, identifier)
            .await
    }

    async fn get_contract_actions_by_transaction_id(
        &self,
        id: u64,
    ) -> Result<Vec<ContractAction>, sqlx::Error> {
        let result: Result<Option<Vec<ContractAction>>, Arc<sqlx::Error>> = self.contract_actions_by_transaction_id.load_one(id).await;
        result
            .map(|opt| opt.unwrap_or_default())
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))
    }

    fn get_contract_actions_by_address(
        &self,
        address: &SerializedContractAddress,
        contract_action_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractAction, sqlx::Error>> + Send {
        self.inner
            .get_contract_actions_by_address(address, contract_action_id, batch_size)
    }

    async fn get_unshielded_balances_by_contract_action_id(
        &self,
        contract_action_id: u64,
    ) -> Result<Vec<crate::domain::ContractBalance>, sqlx::Error> {
        self.inner
            .get_unshielded_balances_by_contract_action_id(contract_action_id)
            .await
    }

    async fn get_contract_action_id_by_block_height(
        &self,
        block_height: u32,
    ) -> Result<Option<u64>, sqlx::Error> {
        self.inner
            .get_contract_action_id_by_block_height(block_height)
            .await
    }
}

// ---------------------------------------------------------------------------
// LedgerEventStorage
// ---------------------------------------------------------------------------

impl LedgerEventStorage for CachingStorage {
    fn get_ledger_events(
        &self,
        grouping: LedgerEventGrouping,
        id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<LedgerEvent, sqlx::Error>> + Send {
        self.inner.get_ledger_events(grouping, id, batch_size)
    }

    async fn get_ledger_events_by_transaction_id(
        &self,
        grouping: LedgerEventGrouping,
        transaction_id: u64,
    ) -> Result<Vec<LedgerEvent>, sqlx::Error> {
        match grouping {
            LedgerEventGrouping::Zswap => {
                let result: Result<Option<Vec<LedgerEvent>>, Arc<sqlx::Error>> = 
                    self.zswap_ledger_events_by_transaction_id.load_one(transaction_id).await;
                result
                    .map(|opt| opt.unwrap_or_default())
                    .map_err(|e| sqlx::Error::Protocol(e.to_string()))
            }
            LedgerEventGrouping::Dust => {
                let result: Result<Option<Vec<LedgerEvent>>, Arc<sqlx::Error>> = 
                    self.dust_ledger_events_by_transaction_id.load_one(transaction_id).await;
                result
                    .map(|opt| opt.unwrap_or_default())
                    .map_err(|e| sqlx::Error::Protocol(e.to_string()))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// UnshieldedUtxoStorage
// ---------------------------------------------------------------------------

impl UnshieldedUtxoStorage for CachingStorage {
    async fn get_unshielded_utxos_by_address(
        &self,
        address: UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        self.inner.get_unshielded_utxos_by_address(address).await
    }

    async fn get_unshielded_utxos_created_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let result: Result<Option<Vec<UnshieldedUtxo>>, Arc<sqlx::Error>> = 
            self.unshielded_utxos_created_by_transaction_id.load_one(transaction_id).await;
        result
            .map(|opt| opt.unwrap_or_default())
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))
    }

    async fn get_unshielded_utxos_spent_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let result: Result<Option<Vec<UnshieldedUtxo>>, Arc<sqlx::Error>> = 
            self.unshielded_utxos_spent_by_transaction_id.load_one(transaction_id).await;
        result
            .map(|opt| opt.unwrap_or_default())
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))
    }

    async fn get_unshielded_utxos_by_address_created_by_transaction(
        &self,
        address: UnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        self.inner
            .get_unshielded_utxos_by_address_created_by_transaction(address, transaction_id)
            .await
    }

    async fn get_unshielded_utxos_by_address_spent_by_transaction(
        &self,
        address: UnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        self.inner
            .get_unshielded_utxos_by_address_spent_by_transaction(address, transaction_id)
            .await
    }
}

// ---------------------------------------------------------------------------
// DustStorage
// ---------------------------------------------------------------------------

impl DustStorage for CachingStorage {
    async fn get_dust_generation_status(
        &self,
        cardano_reward_addresses: &[CardanoRewardAddress],
        ledger_version: LedgerVersion,
    ) -> Result<Vec<DustGenerationStatus>, sqlx::Error> {
        self.inner
            .get_dust_generation_status(cardano_reward_addresses, ledger_version)
            .await
    }
}

// ---------------------------------------------------------------------------
// LedgerStateStorage
// ---------------------------------------------------------------------------

impl LedgerStateStorage for CachingStorage {
    async fn get_highest_ledger_state(
        &self,
    ) -> Result<Option<(ProtocolVersion, SerializedLedgerStateKey)>, sqlx::Error> {
        self.inner.get_highest_ledger_state().await
    }
}

// ---------------------------------------------------------------------------
// SpoStorage
// ---------------------------------------------------------------------------

impl SpoStorage for CachingStorage {
    async fn get_spo_identities(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SpoIdentity>, sqlx::Error> {
        self.inner.get_spo_identities(limit, offset).await
    }

    async fn get_spo_identity_by_pool_id(
        &self,
        pool_id: &str,
    ) -> Result<Option<SpoIdentity>, sqlx::Error> {
        self.inner.get_spo_identity_by_pool_id(pool_id).await
    }

    async fn get_spo_count(&self) -> Result<i64, sqlx::Error> {
        self.inner.get_spo_count().await
    }

    async fn get_pool_metadata(&self, pool_id: &str) -> Result<Option<PoolMetadata>, sqlx::Error> {
        self.inner.get_pool_metadata(pool_id).await
    }

    async fn get_pool_metadata_list(
        &self,
        limit: i64,
        offset: i64,
        with_name_only: bool,
    ) -> Result<Vec<PoolMetadata>, sqlx::Error> {
        self.inner
            .get_pool_metadata_list(limit, offset, with_name_only)
            .await
    }

    async fn get_spo_by_pool_id(&self, pool_id: &str) -> Result<Option<Spo>, sqlx::Error> {
        self.inner.get_spo_by_pool_id(pool_id).await
    }

    async fn get_spo_list(
        &self,
        limit: i64,
        offset: i64,
        search: Option<&str>,
    ) -> Result<Vec<Spo>, sqlx::Error> {
        self.inner.get_spo_list(limit, offset, search).await
    }

    async fn get_spo_composite_by_pool_id(
        &self,
        pool_id: &str,
        perf_limit: i64,
    ) -> Result<Option<SpoComposite>, sqlx::Error> {
        self.inner
            .get_spo_composite_by_pool_id(pool_id, perf_limit)
            .await
    }

    async fn get_stake_pool_operator_ids(&self, limit: i64) -> Result<Vec<String>, sqlx::Error> {
        self.inner.get_stake_pool_operator_ids(limit).await
    }

    async fn get_spo_performance_latest(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<EpochPerf>, sqlx::Error> {
        self.inner.get_spo_performance_latest(limit, offset).await
    }

    async fn get_spo_performance_by_spo_sk(
        &self,
        spo_sk: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<EpochPerf>, sqlx::Error> {
        self.inner
            .get_spo_performance_by_spo_sk(spo_sk, limit, offset)
            .await
    }

    async fn get_epoch_performance(
        &self,
        epoch: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<EpochPerf>, sqlx::Error> {
        self.inner.get_epoch_performance(epoch, limit, offset).await
    }

    async fn get_current_epoch_info(&self) -> Result<Option<EpochInfo>, sqlx::Error> {
        self.inner.get_current_epoch_info().await
    }

    async fn get_epoch_utilization(&self, epoch: i64) -> Result<Option<f64>, sqlx::Error> {
        self.inner.get_epoch_utilization(epoch).await
    }

    async fn get_committee(&self, epoch: i64) -> Result<Vec<CommitteeMember>, sqlx::Error> {
        self.inner.get_committee(epoch).await
    }

    async fn get_registered_totals_series(
        &self,
        from_epoch: i64,
        to_epoch: i64,
    ) -> Result<Vec<RegisteredTotals>, sqlx::Error> {
        self.inner
            .get_registered_totals_series(from_epoch, to_epoch)
            .await
    }

    async fn get_registered_spo_series(
        &self,
        from_epoch: i64,
        to_epoch: i64,
    ) -> Result<Vec<RegisteredStat>, sqlx::Error> {
        self.inner
            .get_registered_spo_series(from_epoch, to_epoch)
            .await
    }

    async fn get_registered_presence(
        &self,
        from_epoch: i64,
        to_epoch: i64,
    ) -> Result<Vec<PresenceEvent>, sqlx::Error> {
        self.inner.get_registered_presence(from_epoch, to_epoch).await
    }

    async fn get_registered_first_valid_epochs(
        &self,
        upto_epoch: Option<i64>,
    ) -> Result<Vec<FirstValidEpoch>, sqlx::Error> {
        self.inner.get_registered_first_valid_epochs(upto_epoch).await
    }

    async fn get_stake_distribution(
        &self,
        limit: i64,
        offset: i64,
        search: Option<&str>,
        order_desc: bool,
    ) -> Result<(Vec<StakeShare>, f64), sqlx::Error> {
        self.inner
            .get_stake_distribution(limit, offset, search, order_desc)
            .await
    }
}

// ---------------------------------------------------------------------------
// SystemParametersStorage
// ---------------------------------------------------------------------------

impl SystemParametersStorage for CachingStorage {
    async fn get_terms_and_conditions_at(
        &self,
        block_height: u32,
    ) -> Result<Option<TermsAndConditions>, sqlx::Error> {
        self.inner.get_terms_and_conditions_at(block_height).await
    }

    async fn get_d_parameter_at(
        &self,
        block_height: u32,
    ) -> Result<Option<DParameter>, sqlx::Error> {
        self.inner.get_d_parameter_at(block_height).await
    }

    async fn get_terms_and_conditions_history(
        &self,
    ) -> Result<Vec<TermsAndConditions>, sqlx::Error> {
        self.inner.get_terms_and_conditions_history().await
    }

    async fn get_d_parameter_history(&self) -> Result<Vec<DParameter>, sqlx::Error> {
        self.inner.get_d_parameter_history().await
    }
}

// ---------------------------------------------------------------------------
// WalletStorage
// ---------------------------------------------------------------------------

impl WalletStorage for CachingStorage {
    async fn connect_wallet(&self, viewing_key: &ViewingKey) -> Result<SessionId, sqlx::Error> {
        self.inner.connect_wallet(viewing_key).await
    }

    async fn disconnect_wallet(&self, session_id: SessionId) -> Result<(), sqlx::Error> {
        self.inner.disconnect_wallet(session_id).await
    }

    async fn resolve_session_id(&self, session_id: SessionId) -> Result<Option<Uuid>, sqlx::Error> {
        self.inner.resolve_session_id(session_id).await
    }

    async fn keep_wallet_active(&self, wallet_id: Uuid) -> Result<(), sqlx::Error> {
        self.inner.keep_wallet_active(wallet_id).await
    }
}
