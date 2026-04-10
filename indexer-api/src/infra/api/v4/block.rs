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

use crate::{
    domain::{self, storage::Storage},
    infra::api::{
        ApiResult, ContextExt, ResultExt,
        v4::{
            HexEncodable, HexEncoded,
            system_parameters::{DParameter, SystemParameters, TermsAndConditions},
            transaction::Transaction,
        },
    },
};
use async_graphql::{ComplexObject, Context, OneofObject, SimpleObject};
use derive_more::Debug;
use indexer_common::domain::BlockHash;
use std::marker::PhantomData;

/// A block with its relevant data.
#[derive(Debug, SimpleObject)]
#[graphql(complex)]
pub struct Block<S>
where
    S: Storage,
{
    /// The block hash.
    hash: HexEncoded,

    /// The block height.
    height: u32,

    /// The protocol version.
    protocol_version: u32,

    /// The UNIX timestamp.
    timestamp: u64,

    /// The hex-encoded block author.
    author: Option<HexEncoded>,

    /// The hex-encoded serialized zswap state Merkle tree root.
    #[debug(skip)]
    zswap_merkle_tree_root: HexEncoded,

    /// The hex-encoded ledger parameters for this block.
    ledger_parameters: HexEncoded,

    #[graphql(skip)]
    id: u64,

    #[graphql(skip)]
    parent_hash: BlockHash,

    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> Block<S>
where
    S: Storage,
{
    /// The parent of this block.
    async fn parent(&self, cx: &Context<'_>) -> ApiResult<Option<Block<S>>> {
        let block = cx
            .get_block_by_hash_loader::<S>()
            .load_one(self.parent_hash)
            .await
            .map_err_into_server_error(|| format!("get block by hash {}", self.parent_hash))?;

        Ok(block.map(Into::into))
    }

    /// The transactions within this block.
    async fn transactions(&self, cx: &Context<'_>) -> ApiResult<Vec<Transaction<S>>> {
        let transactions = cx
            .get_transactions_by_block_id_loader::<S>()
            .load_one(self.id)
            .await
            .map_err_into_server_error(|| format!("get transactions by block id {}", self.id))?
            .unwrap_or_default();

        Ok(transactions.into_iter().map(Into::into).collect())
    }

    /// The system parameters (governance) at this block height.
    async fn system_parameters(&self, cx: &Context<'_>) -> ApiResult<SystemParameters> {
        let storage = cx.get_storage::<S>();

        let d_param = storage
            .get_d_parameter_at(self.height)
            .await
            .map_err_into_server_error(|| {
                format!("get D-parameter at block height {}", self.height)
            })?
            .map(DParameter::from)
            .unwrap_or(DParameter {
                num_permissioned_candidates: 0,
                num_registered_candidates: 0,
            });

        let terms_and_conditions = storage
            .get_terms_and_conditions_at(self.height)
            .await
            .map_err_into_server_error(|| format!("get T&C at block height {}", self.height))?
            .map(TermsAndConditions::from);

        Ok(SystemParameters {
            d_parameter: d_param,
            terms_and_conditions,
        })
    }

    /// The hex-encoded dust commitment Merkle tree root at the latest indexed state.
    async fn dust_commitment_merkle_tree_root(
        &self,
        cx: &Context<'_>,
    ) -> ApiResult<Option<HexEncoded>> {
        let ledger_state_cache = cx.get_ledger_state_cache();
        let storage = cx.get_storage::<S>();

        match ledger_state_cache.dust_merkle_tree_roots(storage).await {
            Ok(roots) => Ok(Some(roots.commitment_root.hex_encode())),
            Err(_) => Ok(None),
        }
    }

    /// The hex-encoded dust generation Merkle tree root at the latest indexed state.
    async fn dust_generation_merkle_tree_root(
        &self,
        cx: &Context<'_>,
    ) -> ApiResult<Option<HexEncoded>> {
        let ledger_state_cache = cx.get_ledger_state_cache();
        let storage = cx.get_storage::<S>();

        match ledger_state_cache.dust_merkle_tree_roots(storage).await {
            Ok(roots) => Ok(Some(roots.generation_root.hex_encode())),
            Err(_) => Ok(None),
        }
    }
}

impl<S> From<domain::Block> for Block<S>
where
    S: Storage,
{
    fn from(value: domain::Block) -> Self {
        let domain::Block {
            id,
            hash,
            height,
            protocol_version,
            author,
            timestamp,
            parent_hash,
            zswap_merkle_tree_root,
            ledger_parameters,
        } = value;

        Block {
            hash: hash.hex_encode(),
            height,
            protocol_version: protocol_version.into(),
            author: author.map(|author| author.hex_encode()),
            zswap_merkle_tree_root: zswap_merkle_tree_root.hex_encode(),
            ledger_parameters: ledger_parameters.hex_encode(),
            timestamp,
            id,
            parent_hash,
            _s: PhantomData,
        }
    }
}

/// Either a block hash or a block height.
#[derive(Debug, OneofObject)]
pub enum BlockOffset {
    /// A hex-encoded block hash.
    Hash(HexEncoded),

    /// A block height.
    Height(u32),
}
