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

//! Internal `DataLoader` implementations for the infra storage layer.
//!
//! These loaders batch single-item lookups into bulk SQL queries transparently.
//! They are NOT part of the domain — the domain traits remain unchanged.

use crate::{
    domain::{Block, ContractAction, LedgerEvent, Transaction, UnshieldedUtxo},
    infra::storage::Storage,
};
use async_graphql::dataloader::Loader;
use indexer_common::domain::{BlockHash, LedgerEventGrouping, TransactionHash};
use std::{collections::HashMap, sync::Arc};
use futures::Future;

// ---------------------------------------------------------------------------
// Block loaders
// ---------------------------------------------------------------------------

pub struct BlockByHashLoader(pub Storage);

impl Loader<BlockHash> for BlockByHashLoader {
    type Value = Block;
    type Error = Arc<sqlx::Error>;

    fn load(&self, keys: &[BlockHash]) -> impl Future<Output = Result<HashMap<BlockHash, Block>, Arc<sqlx::Error>>> + Send {
        let storage = self.0.clone();
        let keys = keys.to_vec();
        async move {
            let blocks = storage.get_blocks_by_hashes(&keys).await.map_err(Arc::new)?;
            Ok(blocks.into_iter().map(|b| (b.hash.clone(), b)).collect())
        }
    }
}

pub struct BlockByHeightLoader(pub Storage);

impl Loader<u32> for BlockByHeightLoader {
    type Value = Block;
    type Error = Arc<sqlx::Error>;

    fn load(&self, keys: &[u32]) -> impl Future<Output = Result<HashMap<u32, Block>, Arc<sqlx::Error>>> + Send {
        let storage = self.0.clone();
        let keys = keys.to_vec();
        async move {
            let blocks = storage.get_blocks_by_heights(&keys).await.map_err(Arc::new)?;
            Ok(blocks.into_iter().map(|b| (b.height, b)).collect())
        }
    }
}

// ---------------------------------------------------------------------------
// Transaction loaders
// ---------------------------------------------------------------------------

pub struct TransactionByHashLoader(pub Storage);

impl Loader<TransactionHash> for TransactionByHashLoader {
    type Value = Transaction;
    type Error = Arc<sqlx::Error>;

    fn load(
        &self,
        keys: &[TransactionHash],
    ) -> impl Future<Output = Result<HashMap<TransactionHash, Transaction>, Arc<sqlx::Error>>> + Send {
        let storage = self.0.clone();
        let keys = keys.to_vec();
        async move {
            let txs: Vec<Transaction> = storage.get_transactions_by_hashes(&keys).await.map_err(Arc::new)?;
            Ok(txs.into_iter().map(|t| (t.hash().clone(), t)).collect())
        }
    }
}

pub struct TransactionByIdLoader(pub Storage);

impl Loader<u64> for TransactionByIdLoader {
    type Value = Transaction;
    type Error = Arc<sqlx::Error>;

    fn load(&self, keys: &[u64]) -> impl Future<Output = Result<HashMap<u64, Transaction>, Arc<sqlx::Error>>> + Send {
        let storage = self.0.clone();
        let keys = keys.to_vec();
        async move {
            let txs: Vec<Transaction> = storage.get_transactions_by_ids(&keys).await.map_err(Arc::new)?;
            Ok(txs.into_iter().map(|t| (t.id(), t)).collect())
        }
    }
}

pub struct TransactionsByBlockIdLoader(pub Storage);

impl Loader<u64> for TransactionsByBlockIdLoader {
    type Value = Vec<Transaction>;
    type Error = Arc<sqlx::Error>;

    fn load(&self, keys: &[u64]) -> impl Future<Output = Result<HashMap<u64, Vec<Transaction>>, Arc<sqlx::Error>>> + Send {
        let storage = self.0.clone();
        let keys = keys.to_vec();
        async move {
            let txs = storage.get_transactions_by_block_ids(&keys).await.map_err(Arc::new)?;
            let mut map: HashMap<u64, Vec<Transaction>> = HashMap::new();
            for tx in txs {
                map.entry(tx.block_id()).or_default().push(tx);
            }
            Ok(map)
        }
    }
}

// ---------------------------------------------------------------------------
// Contract Action loaders
// ---------------------------------------------------------------------------

pub struct ContractActionsByTransactionIdLoader(pub Storage);

impl Loader<u64> for ContractActionsByTransactionIdLoader {
    type Value = Vec<ContractAction>;
    type Error = Arc<sqlx::Error>;

    fn load(&self, keys: &[u64]) -> impl Future<Output = Result<HashMap<u64, Vec<ContractAction>>, Arc<sqlx::Error>>> + Send {
        let storage = self.0.clone();
        let keys = keys.to_vec();
        async move {
            let actions = storage
                .get_contract_actions_by_transaction_ids(&keys)
                .await
                .map_err(Arc::new)?;
            
            let mut map: HashMap<u64, Vec<ContractAction>> = HashMap::new();
            for action in actions {
                map.entry(action.transaction_id).or_default().push(action);
            }
            Ok(map)
        }
    }
}

// ---------------------------------------------------------------------------
// Ledger Event loaders
// ---------------------------------------------------------------------------

pub struct ZswapLedgerEventsByTransactionIdLoader(pub Storage);

impl Loader<u64> for ZswapLedgerEventsByTransactionIdLoader {
    type Value = Vec<LedgerEvent>;
    type Error = Arc<sqlx::Error>;

    fn load(&self, keys: &[u64]) -> impl Future<Output = Result<HashMap<u64, Vec<LedgerEvent>>, Arc<sqlx::Error>>> + Send {
        let storage = self.0.clone();
        let keys = keys.to_vec();
        async move {
            let events = storage
                .get_ledger_events_by_transaction_ids(LedgerEventGrouping::Zswap, &keys)
                .await
                .map_err(Arc::new)?;
            
            let mut map: HashMap<u64, Vec<LedgerEvent>> = HashMap::new();
            for event in events {
                map.entry(event.transaction_id).or_default().push(event);
            }
            Ok(map)
        }
    }
}

pub struct DustLedgerEventsByTransactionIdLoader(pub Storage);

impl Loader<u64> for DustLedgerEventsByTransactionIdLoader {
    type Value = Vec<LedgerEvent>;
    type Error = Arc<sqlx::Error>;

    fn load(&self, keys: &[u64]) -> impl Future<Output = Result<HashMap<u64, Vec<LedgerEvent>>, Arc<sqlx::Error>>> + Send {
        let storage = self.0.clone();
        let keys = keys.to_vec();
        async move {
            let events = storage
                .get_ledger_events_by_transaction_ids(LedgerEventGrouping::Dust, &keys)
                .await
                .map_err(Arc::new)?;
            
            let mut map: HashMap<u64, Vec<LedgerEvent>> = HashMap::new();
            for event in events {
                map.entry(event.transaction_id).or_default().push(event);
            }
            Ok(map)
        }
    }
}

// ---------------------------------------------------------------------------
// Unshielded UTXO loaders
// ---------------------------------------------------------------------------

pub struct UnshieldedUtxosCreatedByTransactionIdLoader(pub Storage);

impl Loader<u64> for UnshieldedUtxosCreatedByTransactionIdLoader {
    type Value = Vec<UnshieldedUtxo>;
    type Error = Arc<sqlx::Error>;

    fn load(&self, keys: &[u64]) -> impl Future<Output = Result<HashMap<u64, Vec<UnshieldedUtxo>>, Arc<sqlx::Error>>> + Send {
        let storage = self.0.clone();
        let keys = keys.to_vec();
        async move {
            let utxos = storage
                .get_unshielded_utxos_created_by_transaction_ids(&keys)
                .await
                .map_err(Arc::new)?;
            
            let mut map: HashMap<u64, Vec<UnshieldedUtxo>> = HashMap::new();
            for utxo in utxos {
                map.entry(utxo.creating_transaction_id).or_default().push(utxo);
            }
            Ok(map)
        }
    }
}

pub struct UnshieldedUtxosSpentByTransactionIdLoader(pub Storage);

impl Loader<u64> for UnshieldedUtxosSpentByTransactionIdLoader {
    type Value = Vec<UnshieldedUtxo>;
    type Error = Arc<sqlx::Error>;

    fn load(&self, keys: &[u64]) -> impl Future<Output = Result<HashMap<u64, Vec<UnshieldedUtxo>>, Arc<sqlx::Error>>> + Send {
        let storage = self.0.clone();
        let keys = keys.to_vec();
        async move {
            let utxos = storage
                .get_unshielded_utxos_spent_by_transaction_ids(&keys)
                .await
                .map_err(Arc::new)?;
            
            let mut map: HashMap<u64, Vec<UnshieldedUtxo>> = HashMap::new();
            for utxo in utxos {
                if let Some(spent_id) = utxo.spending_transaction_id {
                    map.entry(spent_id).or_default().push(utxo);
                }
            }
            Ok(map)
        }
    }
}
