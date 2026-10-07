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

mod v0_22_0;
mod v1_0_300;
mod v2_0_0;
mod v2_1_0;

// To see how this is generated, look in build.rs
include!(concat!(env!("OUT_DIR"), "/generated_runtime.rs"));

use crate::{
    domain::{DParameter, DustRegistrationEvent, TermsAndConditions},
    infra::subxt_node::{ContentSource, OnlineClientAtBlock, SubxtNodeError},
};
use indexer_common::domain::{ByteVec, NodeVersion};
use subxt::{SubstrateConfig, client::OfflineClientAtBlockT};

/// A client at a block, online or offline.
pub type AtBlock<C> = subxt::client::ClientAtBlock<SubstrateConfig, C>;

/// Runtime specific block details.
pub struct BlockDetails {
    pub timestamp: Option<u64>,
    pub transactions: Vec<Transaction>,
    pub dust_registration_events: Vec<DustRegistrationEvent>,
    /// c2m-bridge events. Only populated for node 2.0+, where the
    /// `c2m-bridge` pallet exists in the runtime metadata. Empty for earlier
    /// node versions (the pallet did not yet exist there).
    pub bridge_events: Vec<indexer_common::domain::bridge::BridgeEvent>,
}

/// Runtime specific (serialized) transaction.
pub enum Transaction {
    Regular(ByteVec),
    System(ByteVec),
}

/// Make block details depending on the given protocol version: fetch the block's raw extrinsics
/// and events, and decode them.
pub async fn make_block_details(
    authorities: &mut Option<Vec<[u8; 32]>>,
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
    content: Option<ContentSource>,
) -> Result<BlockDetails, SubxtNodeError> {
    // Enactment block: decode this block's raw extrinsic bytes against the parent (old-runtime)
    // client. Raw event bytes are metadata-independent, so they are fetched from this block and
    // re-decoded against the same client.
    let (content_client, extrinsics) = match content {
        Some(ContentSource {
            client,
            extrinsic_bodies,
        }) => (Some(client), extrinsic_bodies),
        None => {
            let extrinsics = block
                .extrinsics()
                .fetch()
                .await
                .map_err(|error| SubxtNodeError::FetchExtrinsics(error.into()))?
                .iter()
                .map(|extrinsic| {
                    extrinsic
                        .map(|extrinsic| extrinsic.bytes().to_vec())
                        .map_err(|error| SubxtNodeError::GetNextExtrinsic(error.into()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            (None, extrinsics)
        }
    };
    let client = content_client.as_ref().unwrap_or(block);

    let events = block
        .events()
        .fetch()
        .await
        .map_err(|error| SubxtNodeError::FetchEvents(error.into()))?
        .bytes()
        .to_vec();

    decode_block_details(authorities, node_version, client, extrinsics, events).await
}

/// Decode block details from raw extrinsics and events depending on the given protocol version.
pub async fn decode_block_details(
    authorities: &mut Option<Vec<[u8; 32]>>,
    node_version: NodeVersion,
    client: &AtBlock<impl OfflineClientAtBlockT<SubstrateConfig>>,
    extrinsics: Vec<Vec<u8>>,
    events: Vec<u8>,
) -> Result<BlockDetails, SubxtNodeError> {
    // TODO Replace this often repeated pattern with a macro?
    match node_version {
        NodeVersion::V0_22 => {
            v0_22_0::make_block_details(authorities, client, extrinsics, events).await
        }
        NodeVersion::V1_0 => {
            v1_0_300::make_block_details(authorities, client, extrinsics, events).await
        }
        NodeVersion::V2_0 => {
            v2_0_0::make_block_details(authorities, client, extrinsics, events).await
        }
        NodeVersion::V2_1 => {
            v2_1_0::make_block_details(authorities, client, extrinsics, events).await
        }
    }
}

/// Fetch authorities depending on the given protocol version.
pub async fn fetch_authorities(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<Vec<[u8; 32]>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::fetch_authorities(block).await,
        NodeVersion::V1_0 => v1_0_300::fetch_authorities(block).await,
        NodeVersion::V2_0 => v2_0_0::fetch_authorities(block).await,
        NodeVersion::V2_1 => v2_1_0::fetch_authorities(block).await,
    }
}

/// Decode slot depending on the given protocol version.
pub fn decode_slot(slot: &[u8], node_version: NodeVersion) -> Result<u64, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::decode_slot(slot),
        NodeVersion::V1_0 => v1_0_300::decode_slot(slot),
        NodeVersion::V2_0 => v2_0_0::decode_slot(slot),
        NodeVersion::V2_1 => v2_1_0::decode_slot(slot),
    }
}

pub async fn get_zswap_merkle_tree_root(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<Vec<u8>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::get_zswap_merkle_tree_root(block).await,
        NodeVersion::V1_0 => v1_0_300::get_zswap_merkle_tree_root(block).await,
        NodeVersion::V2_0 => v2_0_0::get_zswap_merkle_tree_root(block).await,
        NodeVersion::V2_1 => v2_1_0::get_zswap_merkle_tree_root(block).await,
    }
}

/// Get the pure ledger state root (without StorableLedgerState wrapping) at the given block.
pub async fn get_ledger_state_root(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<Option<Vec<u8>>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::get_ledger_state_root(block).await,
        NodeVersion::V1_0 => v1_0_300::get_ledger_state_root(block).await,
        NodeVersion::V2_0 => v2_0_0::get_ledger_state_root(block).await,
        NodeVersion::V2_1 => v2_1_0::get_ledger_state_root(block).await,
    }
}

/// Get D-Parameter depending on the given protocol version.
pub async fn get_d_parameter(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<DParameter, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::get_d_parameter(block).await,
        NodeVersion::V1_0 => v1_0_300::get_d_parameter(block).await,
        NodeVersion::V2_0 => v2_0_0::get_d_parameter(block).await,
        NodeVersion::V2_1 => v2_1_0::get_d_parameter(block).await,
    }
}

/// Fetch genesis cNight registrations from pallet storage.
/// At genesis, Substrate does not emit events (Parity PR #5463), so we query
/// the cNightObservation.Mappings storage directly at block 0.
pub async fn fetch_genesis_cnight_registrations(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<Vec<DustRegistrationEvent>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::fetch_genesis_cnight_registrations(block).await,
        NodeVersion::V1_0 => v1_0_300::fetch_genesis_cnight_registrations(block).await,
        NodeVersion::V2_0 => v2_0_0::fetch_genesis_cnight_registrations(block).await,
        NodeVersion::V2_1 => v2_1_0::fetch_genesis_cnight_registrations(block).await,
    }
}

/// Get Terms and Conditions depending on the given protocol version.
pub async fn get_terms_and_conditions(
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
) -> Result<Option<TermsAndConditions>, SubxtNodeError> {
    match node_version {
        NodeVersion::V0_22 => v0_22_0::get_terms_and_conditions(block).await,
        NodeVersion::V1_0 => v1_0_300::get_terms_and_conditions(block).await,
        NodeVersion::V2_0 => v2_0_0::get_terms_and_conditions(block).await,
        NodeVersion::V2_1 => v2_1_0::get_terms_and_conditions(block).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::{fs, path::Path};
    use subxt::{Metadata, client::OfflineClient, config::substrate::SpecVersionForRange};

    /// A recorded block: raw extrinsics and `System.Events` bytes, as served by the node's RPC.
    #[derive(Deserialize)]
    struct Fixture {
        height: u64,
        spec_version: u32,
        extrinsics: Vec<String>,
        events: Option<String>,
    }

    fn fixture(name: &str) -> Fixture {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/blocks")
            .join(format!("{name}.json"));
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    fn unhex(hex: &str) -> Vec<u8> {
        const_hex::decode(hex).unwrap()
    }

    /// Decode a recorded block offline with the committed metadata of the given node version.
    async fn decode(
        name: &str,
        node_version: NodeVersion,
        metadata_dir: &str,
    ) -> Result<BlockDetails, SubxtNodeError> {
        let fixture = fixture(name);

        let metadata = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.node")
            .join(metadata_dir)
            .join("metadata.scale");
        let metadata = Metadata::decode_from(&fs::read(metadata).unwrap()).unwrap();
        let config = SubstrateConfig::builder()
            .set_metadata_for_spec_versions([(fixture.spec_version, metadata.into())])
            .set_spec_version_for_block_ranges([SpecVersionForRange {
                block_range: 0..u64::MAX,
                spec_version: fixture.spec_version,
                transaction_version: 1,
            }])
            .build();
        let client = OfflineClient::new_with_config(config)
            .at_block(fixture.height)
            .unwrap();

        let extrinsics = fixture.extrinsics.iter().map(|e| unhex(e)).collect();
        let events = fixture.events.as_deref().map(unhex).unwrap_or_default();

        decode_block_details(&mut None, node_version, &client, extrinsics, events).await
    }

    /// A compact, ordered description of the block details so that golden values stay readable:
    /// transactions as `R`/`S` with length and first and last four bytes, and for registrations and
    /// bridge events their count with the first and last one.
    fn summary(details: &BlockDetails) -> Vec<String> {
        fn describe<T: std::fmt::Debug>(name: &str, items: &[T]) -> Vec<String> {
            [format!("{name}: {}", items.len())]
                .into_iter()
                .chain(items.first().map(|item| format!("{name} first: {item:?}")))
                .chain(items.last().map(|item| format!("{name} last: {item:?}")))
                .collect()
        }

        let transactions = details.transactions.iter().map(|transaction| {
            let (kind, bytes) = match transaction {
                Transaction::Regular(bytes) => ("R", bytes),
                Transaction::System(bytes) => ("S", bytes),
            };
            let bytes = bytes.as_ref();
            format!(
                "{kind} {} {}..{}",
                bytes.len(),
                const_hex::encode(&bytes[..4]),
                const_hex::encode(&bytes[bytes.len() - 4..])
            )
        });

        [format!("timestamp {:?}", details.timestamp)]
            .into_iter()
            .chain(transactions)
            .chain(describe("registrations", &details.dust_registration_events))
            .chain(describe("bridge events", &details.bridge_events))
            .collect()
    }

    #[tokio::test]
    async fn genesis_block_of_devnet_runtime_1_0_300() {
        let details = decode("devnet-0", NodeVersion::V1_0, "1.0.300")
            .await
            .unwrap();
        assert_eq!(
            summary(&details),
            [
                "timestamp Some(1754395200000)",
                "S 41 6d69646e..a47e8d03",
                "S 1455 6d69646e..297c3fea",
                "S 726 6d69646e..d107a10f",
                "R 145163 6d69646e..9baaf905",
                "R 215 6d69646e..e0396700",
                "R 215 6d69646e..50d91700",
                "R 215 6d69646e..db874900",
                "R 215 6d69646e..95694600",
                "R 215 6d69646e..8a610500",
                "R 215 6d69646e..8af57000",
                "R 215 6d69646e..c270f700",
                "R 215 6d69646e..defe7500",
                "R 215 6d69646e..ade03500",
                "R 215 6d69646e..5ebafa00",
                "R 215 6d69646e..41054200",
                "R 215 6d69646e..a8f49300",
                "R 215 6d69646e..d027e900",
                "R 215 6d69646e..a99c1600",
                "R 215 6d69646e..76b31300",
                "R 215 6d69646e..a4e9d900",
                "R 215 6d69646e..39747400",
                "R 215 6d69646e..7a699500",
                "R 215 6d69646e..455a0800",
                "R 215 6d69646e..add33800",
                "S 726 6d69646e..d107a10f",
                "S 6900 6d69646e..1649624b",
                "registrations: 0",
                "bridge events: 0",
            ]
        );
    }

    #[tokio::test]
    async fn registrations_block_of_devnet_runtime_1_0_300() {
        let details = decode("devnet-2", NodeVersion::V1_0, "1.0.300")
            .await
            .unwrap();
        assert_eq!(
            summary(&details),
            [
                "timestamp Some(1789577448000)",
                "S 5429 6d69646e..61916f56",
                "registrations: 237",
                "registrations first: Registration { cardano_stake_key: e0c305c7…, dust_address: 73d75758… }",
                "registrations last: MappingAdded { cardano_stake_key: e030ab1e…, dust_address: 73116136…, utxo_id: d26ca37b…, utxo_index: 0 }",
                "bridge events: 0",
            ]
        );
    }

    #[tokio::test]
    async fn midnight_transaction_block_of_devnet_runtime_1_0_300() {
        let details = decode("devnet-8464", NodeVersion::V1_0, "1.0.300")
            .await
            .unwrap();
        assert_eq!(
            summary(&details),
            [
                "timestamp Some(1789634874000)",
                "R 3910 6d69646e..70e36038",
                "registrations: 0",
                "bridge events: 0",
            ]
        );
    }

    #[tokio::test]
    async fn inherents_only_block_of_devnet_runtime_2_1_0() {
        let details = decode("devnet-270248", NodeVersion::V2_1, "2.1.0-rc.4")
            .await
            .unwrap();
        assert_eq!(
            summary(&details),
            [
                "timestamp Some(1791207000000)",
                "registrations: 0",
                "bridge events: 0",
            ]
        );
    }
}
