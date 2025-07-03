// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
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
    ByteArray, ByteVec, NetworkId, PROTOCOL_VERSION_000_013_000, ProtocolVersion,
    RawContractAddress, RawLedgerState, RawTransaction, RawZswapState, RawZswapStateRoot,
    TransactionResult, TransactionResultWithEvents, UnshieldedUtxo,
    ledger::{Error, LedgerTransactionV5, NetworkIdExt, SerializableV5Ext},
};
use fastrace::trace;
use midnight_base_crypto::{hash::HashOutput as HashOutputV5, time::Timestamp as TimestampV5};
use midnight_coin_structure::contract::ContractAddress as ContractAddressV5;
use midnight_ledger::{
    semantics::{
        TransactionContext as TransactionContextV5, TransactionResult as TransactionResultV5,
    },
    structure::LedgerState as LedgerStateV5,
};
use midnight_onchain_runtime::context::BlockContext as BlockContextV5;
use midnight_serialize::deserialize as deserialize_v5;
use midnight_storage::DefaultDB as DefaultDBV5;
use midnight_transient_crypto::merkle_tree::{
    MerkleTreeCollapsedUpdate as MerkleTreeCollapsedUpdateV5,
    MerkleTreeDigest as MerkleTreeDigestV5,
};
use midnight_zswap::ledger::State as ZswapStateV5;

/// Facade for `LedgerState` from `midnight_ledger` across supported (protocol) versions.
#[derive(Debug, Clone)]
pub enum LedgerState {
    V5(LedgerStateV5<DefaultDBV5>),
}

impl LedgerState {
    /// Deserialize the given raw ledger state using the given protocol version and network ID.
    #[trace(properties = {
        "network_id": "{network_id}",
        "protocol_version": "{protocol_version}"
    })]
    pub fn deserialize(
        ledger_state: impl AsRef<[u8]>,
        network_id: NetworkId,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
            let ledger_state =
                deserialize_v5(&mut ledger_state.as_ref(), network_id.into_ledger_v5())
                    .map_err(|error| Error::Io("cannot deserialize LedgerStateV5", error))?;
            Ok(Self::V5(ledger_state))
        } else {
            Err(Error::InvalidProtocolVersion(protocol_version))
        }
    }

    /// Serialize this ledger state using the given network ID.
    #[trace(properties = { "network_id": "{network_id}" })]
    pub fn serialize(&self, network_id: NetworkId) -> Result<RawLedgerState, Error> {
        match self {
            LedgerState::V5(ledger_state) => {
                let bytes = ledger_state
                    .serialize(network_id)
                    .map_err(|error| Error::Io("cannot serialize LedgerStateV5", error))?;
                Ok(bytes.into())
            }
        }
    }

    /// Apply the given raw transactions to this ledger state.
    #[trace(properties = { "network_id": "{network_id}" })]
    pub fn apply_transaction(
        &mut self,
        transaction: &RawTransaction,
        block_parent_hash: ByteArray<32>,
        block_timestamp: u64,
        network_id: NetworkId,
    ) -> Result<TransactionResult, Error> {
        match self {
            LedgerState::V5(ledger_state) => {
                let ledger_transaction = deserialize_v5::<LedgerTransactionV5, _>(
                    &mut transaction.as_ref(),
                    network_id.into_ledger_v5(),
                )
                .map_err(|error| Error::Io("cannot deserialize LedgerTransactionV5", error))?;

                // Apply transaction to ledger state.
                let cx = TransactionContextV5 {
                    ref_state: ledger_state.clone(),
                    block_context: BlockContextV5 {
                        tblock: timestamp_v5(block_timestamp),
                        tblock_err: 30,
                        parent_block_hash: HashOutputV5(block_parent_hash.0),
                    },
                    whitelist: None,
                };
                let (ledger_state, transaction_result) =
                    ledger_state.apply(&ledger_transaction, &cx);
                *self = LedgerState::V5(ledger_state);

                let transaction_result = match transaction_result {
                    TransactionResultV5::Success(_events) => TransactionResult::Success,

                    TransactionResultV5::PartialSuccess(segments, _events) => {
                        let segments = segments
                            .into_iter()
                            .map(|(id, result)| (id, result.is_ok()))
                            .collect::<Vec<_>>();
                        TransactionResult::PartialSuccess(segments)
                    }

                    TransactionResultV5::Failure(_) => TransactionResult::Failure,
                };

                Ok(transaction_result)
            }
        }
    }

    /// Apply the given raw transaction and capture any DUST events emitted.
    #[trace(properties = { "network_id": "{network_id}" })]
    pub fn apply_transaction_with_events(
        &mut self,
        transaction: &RawTransaction,
        block_parent_hash: ByteArray<32>,
        block_timestamp: u64,
        network_id: NetworkId,
    ) -> Result<TransactionResultWithEvents, Error> {
        // For now, just apply the transaction normally and return empty events.
        // The actual event conversion will be implemented when the ledger
        // abstraction is extended to handle events properly.
        //
        // Currently blocked by:
        // - The midnight-ledger library needs to expose DUST events through its API
        // - TransactionResultV5 from the ledger only provides limited event information
        // - We need structured DUST event data from the ledger before we can extract and convert
        //   them to our domain model
        let result =
            self.apply_transaction(transaction, block_parent_hash, block_timestamp, network_id)?;

        Ok(TransactionResultWithEvents {
            result,
            dust_events: Vec::new(), /* TODO: Implement event extraction once ledger support is
                                      * available. */
        })
    }

    /// Get the first free index of the zswap state.
    pub fn zswap_first_free(&self) -> u64 {
        match self {
            LedgerState::V5(ledger_state) => ledger_state.zswap.first_free,
        }
    }

    /// Get the merkle tree root of the zswap state.
    pub fn zswap_merkle_tree_root(&self) -> ZswapStateRoot {
        match self {
            LedgerState::V5(ledger_state) => {
                let root = ledger_state.zswap.coin_coms.root();
                ZswapStateRoot::V5(root)
            }
        }
    }

    /// Extract the zswap state for the given contract address.
    pub fn extract_contract_zswap_state(
        &self,
        address: &RawContractAddress,
        network_id: NetworkId,
    ) -> Result<RawZswapState, Error> {
        match self {
            LedgerState::V5(ledger_state) => {
                let address = deserialize_v5::<ContractAddressV5, _>(
                    &mut address.as_ref(),
                    network_id.into_ledger_v5(),
                )
                .map_err(|error| Error::Io("cannot deserialize ContractAddressV5", error))?;

                let mut contract_zswap_state = ZswapStateV5::new();
                contract_zswap_state.coin_coms = ledger_state.zswap.filter(&[address]);
                let contract_zswap_state = contract_zswap_state
                    .serialize(network_id)
                    .map_err(|error| Error::Io("cannot serialize ZswapStateV5", error))?;

                Ok(contract_zswap_state.into())
            }
        }
    }

    /// Extract the UTXOs.
    pub fn extract_utxos(&self) -> Vec<UnshieldedUtxo> {
        match self {
            LedgerState::V5(ledger_state) => ledger_state
                .utxo
                .utxos
                .iter()
                .map(|entry| {
                    // With dust feature, utxos is HashMap<Utxo, UtxoMeta, D>
                    // and iter returns Sp<(Sp<Utxo>, Sp<UtxoMeta>)>
                    let utxo_tuple = &*entry;
                    let utxo = &*utxo_tuple.0;
                    UnshieldedUtxo {
                        value: utxo.value,
                        owner_address: utxo.owner.0.0.into(),
                        token_type: utxo.type_.0.0.into(),
                        intent_hash: utxo.intent_hash.0.0.into(),
                        output_index: utxo.output_no,
                    }
                })
                .collect(),
        }
    }

    /// Extract the serialized merkle-tree collapsed update for the given indices.
    pub fn collapsed_update(
        &self,
        start_index: u64,
        end_index: u64,
        network_id: NetworkId,
    ) -> Result<ByteVec, Error> {
        match self {
            LedgerState::V5(ledger_state) => {
                let update = MerkleTreeCollapsedUpdateV5::new(
                    &ledger_state.zswap.coin_coms,
                    start_index,
                    end_index,
                )?
                .serialize(network_id)
                .map_err(|error| {
                    Error::Io("cannot serialize MerkleTreeCollapsedUpdateV5", error)
                })?;

                Ok(update.into())
            }
        }
    }

    /// To be called after applying transactions.
    pub fn post_apply_transactions(&mut self, block_timestamp: u64) {
        match self {
            LedgerState::V5(ledger_state) => {
                let timestamp = timestamp_v5(block_timestamp);
                let ledger_state = ledger_state.post_block_update(timestamp);
                *self = LedgerState::V5(ledger_state);
            }
        }
    }
}

impl Default for LedgerState {
    fn default() -> Self {
        LedgerState::V5(Default::default())
    }
}

/// Facade for zswap state root across supported (protocol) versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZswapStateRoot {
    V5(MerkleTreeDigestV5),
}

impl ZswapStateRoot {
    /// Deserialize the given raw zswap state root using the given protocol version and network ID.
    #[trace(properties = {
        "network_id": "{network_id}",
        "protocol_version": "{protocol_version}"
    })]
    pub fn deserialize(
        raw: impl AsRef<[u8]>,
        protocol_version: ProtocolVersion,
        network_id: NetworkId,
    ) -> Result<Self, Error> {
        if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
            let digest = deserialize_v5(&mut raw.as_ref(), network_id.into_ledger_v5())
                .map_err(|error| Error::Io("cannot deserialize MerkleTreeDigestV5", error))?;
            Ok(ZswapStateRoot::V5(digest))
        } else {
            Err(Error::InvalidProtocolVersion(protocol_version))
        }
    }

    /// Serialize this zswap state root using the given network ID.
    #[trace(properties = { "network_id": "{network_id}" })]
    pub fn serialize(&self, network_id: NetworkId) -> Result<RawZswapStateRoot, Error> {
        match self {
            ZswapStateRoot::V5(digest) => {
                let bytes = digest
                    .serialize(network_id)
                    .map_err(|error| Error::Io("cannot serialize zswap merkle tree root", error))?;
                Ok(bytes.into())
            }
        }
    }
}

fn timestamp_v5(block_timestamp: u64) -> TimestampV5 {
    TimestampV5::from_secs(block_timestamp / 1000)
}
