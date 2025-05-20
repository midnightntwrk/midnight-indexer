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

pub mod chain_indexer_data;
pub mod e2e;
pub mod graphql_ws_client;

use chacha20poly1305::aead::rand_core::OsRng;
use futures::executor::block_on;
use indexer_common::{
    domain::{NetworkId, ProtocolVersion, RawTransaction},
    error::BoxError,
    serialize::SerializableExt,
};
use midnight_ledger::{
    base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode},
    prove::{ExternalResolver, Resolver},
    storage::DefaultDB,
    structure::Transaction as LedgerTransaction,
    transient_crypto::proofs::ProofPreimage,
    zswap::{Offer, ZSWAP_EXPECTED_FILES, prove::ZswapResolver},
};

pub const PROTOCOL_VERSION_0_1: ProtocolVersion = ProtocolVersion(1_000);

pub fn create_raw_transaction(network_id: NetworkId) -> Result<RawTransaction, BoxError> {
    let empty_offer = Offer::<ProofPreimage> {
        inputs: vec![],
        outputs: vec![],
        transient: vec![],
        deltas: vec![],
    };
    let pre_transaction = LedgerTransaction::<_, DefaultDB>::new(empty_offer, None, None);

    let zswap_resolver = ZswapResolver(MidnightDataProvider::new(
        FetchMode::OnDemand,
        OutputMode::Log,
        ZSWAP_EXPECTED_FILES.to_owned(),
    ));
    let external_resolver: ExternalResolver = Box::new(|_| Box::pin(std::future::ready(Ok(None))));
    let resolver = Resolver::new(zswap_resolver, external_resolver);

    let transaction = block_on(pre_transaction.prove(OsRng, &resolver, &resolver))?;
    let raw_transaction = transaction.serialize(network_id)?.into();

    Ok(raw_transaction)
}
