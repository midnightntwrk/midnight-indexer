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
        v2::{Block, BlockOffsetInput, ContractAction, ContractOffset, Transaction, TransactionOffset},
        ContextExt,
    },
};
use anyhow::Context as AnyhowContext;
use async_graphql::{Context, Object};
use indexer_common::error::StdErrorExt;
use std::marker::PhantomData;
use tracing::{error, instrument};

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
    #[instrument(skip(self, cx))]
    pub async fn block(
        &self,
        cx: &Context<'_>,
        offset: Option<BlockOffsetInput>,
    ) -> async_graphql::Result<Option<Block<S>>> {
        let storage = cx.get_storage::<S>()?;

        let block = match offset {
            Some(BlockOffsetInput::Hash(hash)) => {
                let hash = hash.hex_decode().context("decode hash")?;
                storage
                    .get_block_by_hash(&hash)
                    .await
                    .inspect_err(|error| {
                        error!(error = error.as_chain(), "cannot get block by hash")
                    })?
            }

            Some(BlockOffsetInput::Height(height)) => storage
                .get_block_by_height(height)
                .await
                .inspect_err(|error| {
                    error!(error = error.as_chain(), "cannot get block by height")
                })?,

            None => storage
                .get_latest_block()
                .await
                .inspect_err(|error| error!(error = error.as_chain(), "cannot get latest block"))?,
        };

        Ok(block.map(Into::into))
    }

    /// Tries to find a [Transaction] for the given [TransactionOffset].
    #[instrument(skip(self, cx))]
    async fn transaction(
        &self,
        cx: &Context<'_>,
        offset: TransactionOffset,
    ) -> async_graphql::Result<Option<Transaction<S>>> {
        let storage = cx.get_storage::<S>()?;

        let transaction = match offset {
            TransactionOffset::Hash(hash) => {
                let hash = hash.hex_decode().context("decode hash")?;
                storage
                    .get_transactions_by_hash(&hash)
                    .await
                    .inspect_err(|error| {
                        error!(error = error.as_chain(), "cannot get transaction by hash")
                    })?
            }

            TransactionOffset::Identifier(identifier) => {
                let identifier = identifier.hex_decode().context("decode identifier")?;
                storage
                    .get_transaction_by_identifier(&identifier)
                    .await
                    .inspect_err(|error| {
                        error!(
                            error = error.as_chain(),
                            "cannot get transaction by identifier"
                        )
                    })?
            }
        };

        Ok(transaction.map(Into::into))
    }

    /// Tries to find a [Contract] for the given address and optional [ContractOffset].
    #[instrument(skip(self, cx))]
    async fn contract(
        &self,
        cx: &Context<'_>,
        address: HexEncoded,
        offset: Option<ContractOffset>,
    ) -> async_graphql::Result<Option<ContractAction>> {
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
                            error = error.as_chain(),
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
                            error = error.as_chain(),
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
                            error = error.as_chain(),
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
                            error = error.as_chain(),
                            "get contract by address and transaction identifier"
                        )
                    })?
            }

            None => {
                let address = address.hex_decode().context("decode address")?;
                storage
                    .get_latest_contract_action_by_address(&address)
                    .await
                    .inspect_err(|error| {
                        error!(error = error.as_chain(), "get latest contract by address")
                    })?
            }
        };

        Ok(contract.map(Into::into))
    }
}
