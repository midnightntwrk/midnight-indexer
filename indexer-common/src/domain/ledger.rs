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
    domain::{
        ByteArrayLenError, ByteVec, PROTOCOL_VERSION_000_020_000, ProtocolVersion,
        SerializedContractAddress, SerializedLedgerStateKey, dust::DustParameters,
    },
    error::BoxError,
    infra::redb_db::RedbDb,
};
use fastrace::trace;
use midnight_base_crypto_v7_0_0::signatures::Signature as SignatureV7_0_0;
use midnight_ledger_v7_0_0::{
    dust::INITIAL_DUST_PARAMETERS as INITIAL_DUST_PARAMETERS_V7_0_0,
    structure::ProofMarker as ProofMarkerV7_0_0,
};
use midnight_serialize_v7_0_0::{
    Serializable as SerializableV7_0_0, Tagged as TaggedV7_0_0,
    tagged_serialize as tagged_serialize_v7_0_0,
};
use midnight_transient_crypto_v7_0_0::commitment::PureGeneratorPedersen as PureGeneratorPedersenV7_0_0;
use std::{io, string::FromUtf8Error};
use thiserror::Error;

type TransactionV7_0_0 = midnight_ledger_v7_0_0::structure::Transaction<
    SignatureV7_0_0,
    ProofMarkerV7_0_0,
    PureGeneratorPedersenV7_0_0,
    RedbDb,
>;
type IntentV7_0_0 = midnight_ledger_v7_0_0::structure::Intent<
    SignatureV7_0_0,
    ProofMarkerV7_0_0,
    PureGeneratorPedersenV7_0_0,
    RedbDb,
>;

/// Ledger related errors.
#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot load ledger state for key {}", const_hex::encode(.0))]
    LoadLedgerState(SerializedLedgerStateKey, #[source] io::Error),

    #[error("cannot serialize {0}")]
    Serialize(&'static str, #[source] io::Error),

    #[error("cannot deserialize {0}")]
    Deserialize(&'static str, #[source] io::Error),

    #[error("cannot convert {0} to UTF-8 string")]
    FromUtf8(&'static str, #[source] FromUtf8Error),

    #[error("invalid protocol version {0}")]
    InvalidProtocolVersion(ProtocolVersion),

    #[error("cannot get contract state from node for address {0}")]
    GetContractState(SerializedContractAddress, #[source] BoxError),

    #[error(transparent)]
    ByteArrayLen(ByteArrayLenError),

    #[error("invalid merkle-tree collapsed update")]
    InvalidUpdate(#[source] BoxError),

    #[error("malformed transaction")]
    MalformedTransaction(#[source] BoxError),

    #[error("invalid system transaction")]
    SystemTransaction(#[source] BoxError),

    #[error("block limit exceeded during post_block_update")]
    BlockLimitExceeded(#[source] BoxError),

    #[error("cannot calculate transaction cost")]
    TransactionCost(#[source] BoxError),
}

/// Extension methods for `Serializable` implementations.
pub trait SerializableV7_0_0Ext
where
    Self: SerializableV7_0_0,
{
    /// Serialize this `Serializable` implementation.
    #[trace]
    fn serialize_v7_0_0(&self) -> Result<ByteVec, io::Error> {
        let mut bytes = Vec::with_capacity(self.serialized_size());
        SerializableV7_0_0::serialize(self, &mut bytes)?;
        Ok(bytes.into())
    }
}

impl<T> SerializableV7_0_0Ext for T where T: SerializableV7_0_0 {}

/// Extension methods for `Serializable + Tagged` implementations.
pub trait TaggedSerializableV7_0_0Ext
where
    Self: SerializableV7_0_0 + TaggedV7_0_0 + Sized,
{
    /// Serialize this `Serializable + Tagged` implementation.
    #[trace]
    fn tagged_serialize_v7_0_0(&self) -> Result<ByteVec, io::Error> {
        let mut bytes = Vec::with_capacity(self.serialized_size() + 32);
        tagged_serialize_v7_0_0(self, &mut bytes)?;
        Ok(bytes.into())
    }
}

impl<T> TaggedSerializableV7_0_0Ext for T where T: SerializableV7_0_0 + TaggedV7_0_0 {}

/// Get DUST parameters for the given protocol version.
/// Returns the initial DUST parameters from the ledger specification.
/// These parameters define the economic properties of DUST generation:
/// - `night_dust_ratio`: Maximum DUST capacity per NIGHT (5 DUST per NIGHT).
/// - `generation_decay_rate`: Rate of DUST generation (~1 week to reach max).
/// - `dust_grace_period`: Maximum time window for DUST spends (3 hours).
pub fn dust_parameters(protocol_version: ProtocolVersion) -> Result<DustParameters, Error> {
    if protocol_version.is_compatible(PROTOCOL_VERSION_000_020_000) {
        Ok(DustParameters {
            night_dust_ratio: INITIAL_DUST_PARAMETERS_V7_0_0.night_dust_ratio,
            generation_decay_rate: INITIAL_DUST_PARAMETERS_V7_0_0.generation_decay_rate,
            dust_grace_period: INITIAL_DUST_PARAMETERS_V7_0_0
                .dust_grace_period
                .as_seconds() as u64,
        })
    } else {
        Err(Error::InvalidProtocolVersion(protocol_version))
    }
}
