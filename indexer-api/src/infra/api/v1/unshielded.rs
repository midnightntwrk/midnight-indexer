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

//! GraphQL types for contract unshielded token balances.
use crate::{
    domain::{self, storage::Storage},
    infra::api::{
        ApiResult, AsBytesExt, ContextExt, HexEncoded, OptionExt, ResultExt,
        v1::{
            block::BlockOffset,
            transaction::{Transaction, TransactionOffset},
        },
    },
};
use async_graphql::{ComplexObject, Context, OneofObject, SimpleObject, scalar};
use bech32::{Bech32m, Hrp};
use derive_more::Debug;
use indexer_common::domain::{
    ByteArrayLenError, NetworkId, RawUnshieldedAddress, UnknownNetworkIdError,
};
use log::error;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use thiserror::Error;

const HRP_UNSHIELDED_BASE: &str = "mn_addr";

/// Represents an unshielded UTXO.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct UnshieldedUtxo<S: Storage> {
    /// Owner address (Bech32m, `mn_addr…`)
    owner: UnshieldedAddress,

    /// Token type (hex-encoded)
    token_type: HexEncoded,

    /// UTXO value (quantity) as a string to support u128
    value: String,

    /// Index of this output within its creating transaction
    output_index: u32,

    /// The hash of the intent that created this output (hex-encoded)
    intent_hash: HexEncoded,

    #[graphql(skip)]
    creating_transaction_id: u64,

    #[graphql(skip)]
    spending_transaction_id: Option<u64>,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S: Storage> UnshieldedUtxo<S> {
    /// Transaction that created this UTXO.
    async fn created_at_transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        let id = self.creating_transaction_id;

        let transaction = cx
            .get_storage::<S>()
            .get_transaction_by_id(id)
            .await
            .map_err_into_server_error(|| format!("get transaction by ID {id})"))?
            .ok_or_server_error(|| format!("transaction with ID {id} not found"))?;

        Ok(transaction.into())
    }

    /// Transaction that spent this UTXO.
    async fn spent_at_transaction(&self, cx: &Context<'_>) -> ApiResult<Option<Transaction<S>>> {
        let Some(id) = self.spending_transaction_id else {
            return Ok(None);
        };

        let transaction = cx
            .get_storage::<S>()
            .get_transaction_by_id(id)
            .await
            .map_err_into_server_error(|| format!("get transaction by ID {id}"))?
            .ok_or_server_error(|| format!("transaction with ID {id} not found"))?;

        Ok(Some(transaction.into()))
    }
}

impl<S: Storage> From<(domain::UnshieldedUtxo, NetworkId)> for UnshieldedUtxo<S> {
    fn from((utxo, network_id): (domain::UnshieldedUtxo, NetworkId)) -> Self {
        Self {
            owner: UnshieldedAddress::bech32m_encode(utxo.owner, network_id),
            token_type: utxo.token_type.hex_encode(),
            value: utxo.value.to_string(),
            output_index: utxo.output_index,
            intent_hash: utxo.intent_hash.hex_encode(),
            creating_transaction_id: utxo.creating_transaction_id,
            spending_transaction_id: utxo.spending_transaction_id,
            _s: PhantomData,
        }
    }
}

/// Either a block offset or a transaction offset.
#[derive(Debug, OneofObject)]
pub enum UnshieldedOffset {
    /// Either a block hash or a block height.
    BlockOffset(BlockOffset),

    /// Either a transaction hash or a transaction identifier.
    TransactionOffset(TransactionOffset),
}

/// Bech32m-encoded unshielded address.
///
/// Format:
/// - MainNet: `mn_addr` + bech32m data
/// - DevNet: `mn_addr_dev` + bech32m data
/// - TestNet: `mn_addr_test` + bech32m data
/// - Undeployed: `mn_addr_undeployed` + bech32m data
///
/// The inner string is validated to ensure proper bech32m-encoding and correct HRP prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct UnshieldedAddress(pub String);

scalar!(UnshieldedAddress);

impl UnshieldedAddress {
    /// Converts this API address into a domain address, validating the bech32m format and
    /// network ID.
    ///
    /// Format expectations:
    /// - For mainnet: "mn_addr" + bech32m data
    /// - For other networks: "mn_addr_" + network-id + bech32m data where network-id is one of:
    ///   "dev", "test", "undeployed"
    pub fn try_into_domain(
        &self,
        network_id: NetworkId,
    ) -> Result<RawUnshieldedAddress, UnshieldedAddressFormatError> {
        let (hrp, bytes) = bech32::decode(&self.0).map_err(UnshieldedAddressFormatError::Decode)?;
        let hrp = hrp.to_lowercase();

        let Some(n) = hrp.strip_prefix(HRP_UNSHIELDED_BASE) else {
            return Err(UnshieldedAddressFormatError::InvalidHrp(hrp));
        };
        let n = n.strip_prefix("_").unwrap_or(n).try_into()?;
        if n != network_id {
            return Err(UnshieldedAddressFormatError::UnexpectedNetworkId(
                n, network_id,
            ));
        }

        let address = bytes.try_into()?;

        Ok(address)
    }

    /// Encode raw bytes into a Bech32m-encoded address.
    pub fn bech32m_encode(bytes: impl AsRef<[u8]>, network_id: NetworkId) -> Self {
        let hrp = match network_id {
            NetworkId::MainNet => HRP_UNSHIELDED_BASE.to_string(),
            NetworkId::DevNet => format!("{HRP_UNSHIELDED_BASE}_dev"),
            NetworkId::TestNet => format!("{HRP_UNSHIELDED_BASE}_test"),
            NetworkId::Undeployed => format!("{HRP_UNSHIELDED_BASE}_undeployed"),
        };
        let hrp = Hrp::parse(&hrp).expect("unshielded address HRP can be parsed");

        let encoded = bech32::encode::<Bech32m>(hrp, bytes.as_ref())
            .expect("bytes for unshielded address can be Bech32m-encoded");
        Self(encoded)
    }
}

#[derive(Debug, Error)]
pub enum UnshieldedAddressFormatError {
    #[error("cannot bech32m-decode unshielded address")]
    Decode(#[from] bech32::DecodeError),

    #[error("invalid bech32m HRP {0}, expected 'mn_addr' prefix")]
    InvalidHrp(String),

    #[error(transparent)]
    UnknownNetworkId(#[from] UnknownNetworkIdError),

    #[error("network ID mismatch: got {0}, expected {1}")]
    UnexpectedNetworkId(NetworkId, NetworkId),

    #[error("cannot convert into unshielded address")]
    ByteArrayLen(#[from] ByteArrayLenError),
}

/// Represents a token balance held by a contract.
/// This type is exposed through the GraphQL API to allow clients to query
/// unshielded token balances for any contract action (Deploy, Call, Update).
#[derive(Debug, Clone, PartialEq, Eq, SimpleObject)]
pub struct ContractBalance {
    /// Hex-encoded token type identifier.
    pub token_type: HexEncoded,

    /// Balance amount as string to support larger integer values (up to 16 bytes).
    pub amount: String,
}

impl From<domain::ContractBalance> for ContractBalance {
    fn from(balance: domain::ContractBalance) -> Self {
        let domain::ContractBalance { token_type, amount } = balance;
        Self {
            token_type: token_type.hex_encode(),
            amount: amount.to_string(),
        }
    }
}
