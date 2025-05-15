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
    domain::Storage,
    infra::api::{
        ContextExt, ResultExt,
        v2::{Transaction, TransactionOffset},
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
        let block_calls = counter!("indexer_api_calls", "query" => "block");
        let transactions_calls = counter!("indexer_api_calls", "query" => "transactions");
        let contract_action_calls = counter!("indexer_api_calls", "query" => "contract_action");

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
}
