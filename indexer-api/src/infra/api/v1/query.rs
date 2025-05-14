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
        ContextExt, ResultExt,
        v1::{
            Block, BlockOffset, ContractAction, ContractActionOffset, Transaction,
            TransactionOffset,
        },
    },
};
use anyhow::Context as AnyhowContext;
use async_graphql::{Context, Object};
use fastrace::trace;
use metrics::{Counter, counter};
use std::marker::PhantomData;

/// GraphQL queries.
pub struct Query<S> {
    block_calls: Counter,
    transactions_calls: Counter,
    contract_action_calls: Counter,
    _s: PhantomData<S>,
}

impl<S> Default for Query<S> {
    fn default() -> Self {
        let block_calls = counter!("indexer_api_calls_query_block");
        let transactions_calls = counter!("indexer_api_calls_query_transactions");
        let contract_action_calls = counter!("indexer_api_calls_query_contract_action");

        Self {
            block_calls,
            transactions_calls,
            contract_action_calls,
            _s: PhantomData,
        }
    }
}

#[Object]
impl<S> Query<S>
where
    S: Storage,
{
    /// Find a block for the given optional offset; if not present, the latest block is returned.
    #[trace(properties = { "offset": "{offset:?}" })]
    pub async fn block(
        &self,
        cx: &Context<'_>,
        offset: Option<BlockOffset>,
    ) -> async_graphql::Result<Option<Block<S>>> {
        self.block_calls.increment(1);

        let storage = cx.get_storage::<S>();

        let block = match offset {
            Some(BlockOffset::Hash(hash)) => {
                let hash = hash.hex_decode().context("hex-decode hash")?;

                storage
                    .get_block_by_hash(hash)
                    .await
                    .internal("get block by hash")?
            }

            Some(BlockOffset::Height(height)) => storage
                .get_block_by_height(height)
                .await
                .internal("get block by height")?,

            None => storage
                .get_latest_block()
                .await
                .internal("get latest block")?,
        };

        Ok(block.map(Into::into))
    }

    /// Find transactions for the given offset.
    #[trace(properties = { "offset": "{offset:?}" })]
    async fn transactions(
        &self,
        cx: &Context<'_>,
        offset: TransactionOffset,
    ) -> async_graphql::Result<Vec<Transaction<S>>> {
        self.transactions_calls.increment(1);

        let storage = cx.get_storage::<S>();

        match offset {
            TransactionOffset::Hash(hash) => {
                let hash = hash.hex_decode().context("hex-decode hash")?;

                let transactions = storage
                    .get_transactions_by_hash(hash)
                    .await
                    .internal("get transaction by hash")?
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>();

                Ok(transactions)
            }

            TransactionOffset::Identifier(identifier) => {
                let identifier = identifier.hex_decode().context("hex-decode identifier")?;

                let transactions = storage
                    .get_transaction_by_identifier(&identifier)
                    .await
                    .internal("get transaction by identifier")?
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>();

                Ok(transactions)
            }
        }
    }

    /// Find a contract action for the given address and optional offset.
    #[trace(properties = { "address": "{address}", "offset": "{offset:?}" })]
    async fn contract_action(
        &self,
        cx: &Context<'_>,
        address: HexEncoded,
        offset: Option<ContractActionOffset>,
    ) -> async_graphql::Result<Option<ContractAction<S>>> {
        self.contract_action_calls.increment(1);

        let storage = cx.get_storage::<S>();

        let contract_action = match offset {
            Some(ContractActionOffset::BlockOffset(BlockOffset::Hash(hash))) => {
                let address = address.hex_decode().context("hex-decode address")?;
                let hash = hash.hex_decode().context("hex-decode hash")?;

                storage
                    .get_contract_action_by_address_and_block_hash(&address, hash)
                    .await
                    .internal("get contract action by address and block hash")?
            }

            Some(ContractActionOffset::BlockOffset(BlockOffset::Height(height))) => {
                let address = address.hex_decode().context("hex-decode address")?;

                storage
                    .get_contract_action_by_address_and_block_height(&address, height)
                    .await
                    .internal("get contract action by address and block height")?
            }

            Some(ContractActionOffset::TransactionOffset(TransactionOffset::Hash(hash))) => {
                let address = address.hex_decode().context("hex-decode address")?;
                let hash = hash.hex_decode().context("hex-decode hash")?;

                storage
                    .get_contract_action_by_address_and_transaction_hash(&address, hash)
                    .await
                    .internal("get contract action by address and transaction hash")?
            }

            Some(ContractActionOffset::TransactionOffset(TransactionOffset::Identifier(
                identifier,
            ))) => {
                let address = address.hex_decode().context("hex-decode address")?;
                let identifier = identifier.hex_decode().context("hex-decode identifier")?;

                storage
                    .get_contract_action_by_address_and_transaction_identifier(
                        &address,
                        &identifier,
                    )
                    .await
                    .internal("get contract action by address and transaction identifier")?
            }

            None => {
                let address = address.hex_decode().context("hex-decode address")?;

                storage
                    .get_contract_action_by_address(&address)
                    .await
                    .internal("get latest contract action by address")?
            }
        };

        Ok(contract_action.map(Into::into))
    }
}
