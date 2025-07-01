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

mod contract_state;
mod ledger_state;
mod secret_key;
mod transaction;

pub use contract_state::*;
pub use ledger_state::*;
pub use secret_key::*;
pub use transaction::*;

use crate::{
    domain::{NetworkId, ProtocolVersion},
    error::BoxError,
};
use fastrace::trace;
use midnight_base_crypto::signatures::Signature as SignatureV5;
use midnight_ledger::structure::{ProofMarker as ProofMarkerV5, Transaction as TransactionV5};
use midnight_serialize::{
    NetworkId as NetworkIdV5, Serializable as SerializableV5, serialize as serialize_v5,
};
use midnight_storage::DefaultDB as DefaultDBV5;
use midnight_transient_crypto::{
    commitment::PedersenRandomness as PedersenRandomnessV5,
    merkle_tree::InvalidUpdate as InvalidUpdateV5,
};
use std::io;
use thiserror::Error;

type LedgerTransactionV5 =
    TransactionV5<SignatureV5, ProofMarkerV5, PedersenRandomnessV5, DefaultDBV5>;

/// Ledger related errors.
#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Io(&'static str, #[source] io::Error),

    #[error("invalid protocol version {0}")]
    InvalidProtocolVersion(ProtocolVersion),

    #[error("cannot get contract state from node")]
    GetContractState(#[source] BoxError),

    #[error("serialized TokenType should have 32 bytes, but had {0}")]
    TokenTypeLen(usize),

    #[error("invalid merkle-tree collapsed update")]
    InvalidUpdate(#[from] InvalidUpdateV5),
}

/// Extension methods for `Serializable` implementations.
pub trait SerializableV5Ext
where
    Self: SerializableV5,
{
    /// Serialize this `Serializable` implementation.
    #[trace(properties = { "network_id": "{network_id}" })]
    fn serialize(&self, network_id: NetworkId) -> Result<Vec<u8>, io::Error> {
        let mut bytes = Vec::with_capacity(Self::serialized_size(self) + 1);
        serialize_v5(self, &mut bytes, network_id.into_ledger_v5())?;
        Ok(bytes)
    }
}

impl<T> SerializableV5Ext for T where T: SerializableV5 {}

/// Extension methods for network ID.
trait NetworkIdExt {
    /// Convert this network ID into a ledger v5 one.
    fn into_ledger_v5(self) -> NetworkIdV5;
}

impl NetworkIdExt for NetworkId {
    fn into_ledger_v5(self) -> NetworkIdV5 {
        match self {
            NetworkId::Undeployed => NetworkIdV5::Undeployed,
            NetworkId::DevNet => NetworkIdV5::DevNet,
            NetworkId::TestNet => NetworkIdV5::TestNet,
            NetworkId::MainNet => NetworkIdV5::MainNet,
        }
    }
}
