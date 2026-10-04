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

//! [Batch], a set of calls built with one method per RPC method.

use crate::infra::subxt_node::rpc::{Call, hex, method};
use indexer_common::domain::BlockHash;
use serde_json::Value;

/// A set of calls to send together, built with one consuming method per RPC method.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Batch(pub(super) Vec<Call>);

impl Batch {
    /// `archive_v1_hashByHeight`: the hashes of the blocks at the given height.
    pub fn hash_by_height(self, height: u64) -> Self {
        self.push(method::ARCHIVE_HASH_BY_HEIGHT, vec![height.into()])
    }

    /// `archive_v1_header`: the SCALE-encoded header of the given block.
    pub fn header(self, hash: BlockHash) -> Self {
        self.push(method::ARCHIVE_HEADER, vec![hex(hash.0)])
    }

    /// `archive_v1_body`: the SCALE-encoded extrinsics of the given block.
    pub fn body(self, hash: BlockHash) -> Self {
        self.push(method::ARCHIVE_BODY, vec![hex(hash.0)])
    }

    /// `archive_v1_call`: the SCALE-encoded result of a runtime API function at the given block.
    pub fn call(self, hash: BlockHash, function: &str, parameters: &[u8]) -> Self {
        self.push(
            method::ARCHIVE_CALL,
            vec![hex(hash.0), function.into(), hex(parameters)],
        )
    }

    /// `archive_v1_genesisHash`: the hash of the genesis block.
    pub fn genesis_hash(self) -> Self {
        self.push(method::ARCHIVE_GENESIS_HASH, vec![])
    }

    /// `chainHead_v1_unpin`: release the given blocks pinned by a `chainHead_v1_follow`
    /// subscription.
    pub fn unpin(self, subscription: Value, hashes: &[BlockHash]) -> Self {
        let hashes = hashes.iter().map(|hash| hex(hash.0)).collect::<Vec<_>>();
        self.push(method::CHAIN_HEAD_UNPIN, vec![subscription, hashes.into()])
    }

    /// `chainSpec_v1_properties`: the chain spec's properties.
    pub fn chain_spec_properties(self) -> Self {
        self.push(method::CHAIN_SPEC_PROPERTIES, vec![])
    }

    /// `rpc_methods`: the methods the node serves.
    pub fn rpc_methods(self) -> Self {
        self.push(method::RPC_METHODS, vec![])
    }

    /// The number of calls.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no calls.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn push(mut self, method: &'static str, params: Vec<Value>) -> Self {
        self.0.push(Call { method, params });
        self
    }
}
