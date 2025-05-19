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

use crate::{
    domain::{HexEncoded, Storage},
    infra::api::{
        v1::{
            self, addr_to_common, Block, BlockOffsetInput, ContractCallOrDeploy, ContractOffset,
            Transaction, TransactionOffset, UnshieldedAddress, UnshieldedOffset,
        },
        ContextExt,
    },
};
use anyhow::Context as AnyhowContext;
use async_graphql::{Context, Object};
use fastrace::trace;
use indexer_common::error::StdErrorExt;
use log::error;
use std::marker::PhantomData;

/// GraphQL queries.
pub struct Query<S> {
    _s: PhantomData<S>,
}

impl<S> Default for Query<S> {
    fn default() -> Self {
        Self { _s: PhantomData }
    }
}

#[Object]
impl<S> Query<S>
where
    S: Storage,
{
    /// Tries to find a block for the given optional block offset; if not present, the latest block
    /// is queried.
    #[trace]
    pub async fn block(
        &self,
        cx: &Context<'_>,
        offset: Option<BlockOffsetInput>,
    ) -> async_graphql::Result<Option<Block<S>>> {
        let storage = cx.get_storage::<S>()?;

        let block = match offset {
            Some(BlockOffsetInput::Hash(hash)) => {
                let hash = hash.hex_decode().context("decode hash")?;
                storage.get_block_by_hash(&hash).await.inspect_err(
                    |error| error!(error:? = error.as_chain(); "cannot get block by hash"),
                )?
            }

            Some(BlockOffsetInput::Height(height)) => {
                storage.get_block_by_height(height).await.inspect_err(
                    |error| error!(error:? = error.as_chain(); "cannot get block by height"),
                )?
            }

            None => storage.get_latest_block().await.inspect_err(
                |error| error!(error:? = error.as_chain(); "cannot get latest block"),
            )?,
        };

        Ok(block.map(Into::into))
    }

    /// Tries to find a [Transaction] for the given [TransactionOffset].
    #[trace]
    #[graphql(deprecation = "use v2/transaction")]
    async fn transactions(
        &self,
        cx: &Context<'_>,
        hash: Option<HexEncoded>,
        identifier: Option<HexEncoded>,
        address: Option<UnshieldedAddress>,
    ) -> async_graphql::Result<Vec<Transaction<S>>> {
        if let Some(addr) = address {
            let storage = cx.get_storage::<S>()?;
            let network_id = cx.get_network_id()?;

            let common_address = addr_to_common(&addr, network_id)?;
            let txs = storage
                .get_transactions_involving_unshielded(&common_address)
                .await
                .inspect_err(
                    |error| error!(error:? = error.as_chain(); "cannot get txs by address"),
                )?;
            return Ok(txs.into_iter().map(Transaction::<S>::from).collect());
        }
        match (hash, identifier) {
            (Some(hash), None) => {
                let storage = cx.get_storage::<S>()?;
                let hash = hash.hex_decode().context("decode hash")?;
                let transactions = storage
                    .get_transactions_by_hash(&hash)
                    .await
                    .inspect_err(
                        |error| error!(error:? = error.as_chain(); "cannot get transaction by hash"),
                    )?
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>();

                Ok(transactions)
            }

            (None, Some(identifier)) => {
                let storage = cx.get_storage::<S>()?;
                let identifier = identifier.hex_decode().context("decode identifier")?;
                let transactions = storage
                    .get_transaction_by_identifier(&identifier)
                    .await
                    .inspect_err(|error| {
                        error!(
                            error:? = error.as_chain();
                            "cannot get transaction by identifier"
                        )
                    })?
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>();

                Ok(transactions)
            }

            _ => Err(async_graphql::Error::new(
                "either hash or identifier must be given and not both",
            )),
        }
    }

    /// Tries to find a [Contract] for the given address and optional [ContractOffset].
    #[trace]
    async fn contract(
        &self,
        cx: &Context<'_>,
        address: HexEncoded,
        offset: Option<ContractOffset>,
    ) -> async_graphql::Result<Option<ContractCallOrDeploy>> {
        let storage = cx.get_storage::<S>()?;

        let contract = match offset {
            Some(ContractOffset::BlockOffsetInput(BlockOffsetInput::Hash(hash))) => {
                let address = address.hex_decode().context("decode address")?;
                let hash = hash.hex_decode().context("decode hash")?;
                storage
                    .get_contract_action_by_address_and_block_hash(&address, &hash)
                    .await
                    .inspect_err(|error| {
                        error!(
                            error:? = error.as_chain();
                            "get contract by address and block hash"
                        )
                    })?
            }

            Some(ContractOffset::BlockOffsetInput(BlockOffsetInput::Height(height))) => {
                let address = address.hex_decode().context("decode address")?;
                storage
                    .get_contract_action_by_address_and_block_height(&address, height)
                    .await
                    .inspect_err(|error| {
                        error!(
                            error:? = error.as_chain();
                            "get contract by address and block height"
                        )
                    })?
            }

            Some(ContractOffset::TransactionOffset(TransactionOffset::Hash(hash))) => {
                let address = address.hex_decode().context("decode address")?;
                let hash = hash.hex_decode().context("decode hash")?;
                storage
                    .get_contract_action_by_address_and_transaction_hash(&address, &hash)
                    .await
                    .inspect_err(|error| {
                        error!(
                            error:? = error.as_chain();
                            "get contract by address and transaction hash"
                        )
                    })?
            }

            Some(ContractOffset::TransactionOffset(TransactionOffset::Identifier(identifier))) => {
                let address = address.hex_decode().context("decode address")?;
                let identifier = identifier.hex_decode().context("decode identifier")?;
                storage
                    .get_contract_action_by_address_and_transaction_identifier(
                        &address,
                        &identifier,
                    )
                    .await
                    .inspect_err(|error| {
                        error!(
                            error:? = error.as_chain();
                            "get contract by address and transaction identifier"
                        )
                    })?
            }

            None => {
                let address = address.hex_decode().context("decode address")?;
                storage
                    .get_latest_contract_action_by_address(&address)
                    .await
                    .inspect_err(
                        |error| error!(error:? = error.as_chain(); "get latest contract by address"),
                    )?
            }
        };

        Ok(contract.map(Into::into))
    }

    /// Retrieve all unshielded UTXOs (both spent and unspent) associated with a given address.
    #[trace]
    async fn unshielded_utxos(
        &self,
        cx: &Context<'_>,
        address: UnshieldedAddress,
        offset: Option<UnshieldedOffset>,
    ) -> async_graphql::Result<Vec<v1::UnshieldedUtxo<S>>> {
        let storage = cx.get_storage::<S>()?;
        let network_id = cx.get_network_id()?;

        let common_address = addr_to_common(&address, network_id)?;
        let utxos = match offset {
            Some(UnshieldedOffset::BlockOffsetInput(BlockOffsetInput::Height(start))) => {
                storage
                    .get_unshielded_utxos_by_address_from_height(&common_address, start)
                    .await?
            }

            Some(UnshieldedOffset::BlockOffsetInput(BlockOffsetInput::Hash(hash))) => {
                let block_hash = hash.hex_decode().context("decode block hash")?;
                storage
                    .get_unshielded_utxos_by_address_from_block_hash(&common_address, &block_hash)
                    .await?
            }

            Some(UnshieldedOffset::TransactionOffset(TransactionOffset::Hash(hash))) => {
                let tx_hash = hash.hex_decode().context("decode tx hash")?;
                storage
                    .get_unshielded_utxos_by_address_from_tx_hash(&common_address, &tx_hash)
                    .await?
            }

            Some(UnshieldedOffset::TransactionOffset(TransactionOffset::Identifier(id))) => {
                let identifier = id.hex_decode().context("decode tx identifier")?;
                storage
                    .get_unshielded_utxos_by_address_from_tx_identifier(
                        &common_address,
                        &identifier,
                    )
                    .await?
            }

            // no offset -> full list
            None => {
                storage
                    .get_unshielded_utxos_by_address(&common_address)
                    .await?
            }
        };

        Ok(utxos
            .into_iter()
            .map(v1::UnshieldedUtxo::<S>::from)
            .collect())
    }
}
