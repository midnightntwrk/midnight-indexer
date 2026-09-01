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

//! Offline replay of the mainnet 2026-07-20 runtime-upgrade boundary (#1397 / #1402).
//!
//! Fixtures live in `tests/fixtures/mainnet_runtime_upgrade/` (recorded from
//! `https://rpc.mainnet.midnight.network`, not live RPC at test time).

use super::{
    block_runtime_versions,
    header::{MAINNET_HEADER_1_774_491, MAINNET_HEADER_1_774_492, SubstrateHeaderExt},
    runtimes,
};
use indexer_common::domain::NodeVersion;
use parity_scale_codec::Decode;
use serde::Deserialize;
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
use subxt::{
    Metadata, OfflineClient, SubstrateConfig,
    config::substrate::{SpecVersionForRange, SubstrateHeader},
    error::RuntimeApiError,
    utils::H256,
};

const CONTRACT_ADDRESS: &str = "9ef16e583fbc361ba6016b2751e6f26a5ab2bbf2f7102ea5e28dc8810696eb9c";
const ENACTMENT_HEIGHT: u64 = 1_774_491;
const FIRST_POST_UPGRADE_HEIGHT: u64 = 1_774_492;

#[derive(Debug, Deserialize)]
struct RecordedRuntime {
    #[serde(rename = "specVersion")]
    spec_version: u32,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mainnet_runtime_upgrade")
}

fn recorded_runtimes() -> HashMap<String, RecordedRuntime> {
    serde_json::from_str(
        &fs::read_to_string(fixtures_dir().join("runtime_versions.json"))
            .expect("read runtime_versions.json"),
    )
    .expect("parse runtime_versions.json")
}

fn load_extrinsics(height: u64) -> Vec<Vec<u8>> {
    let dir = fixtures_dir().join(format!("block_{height}_extrinsics"));
    let mut paths = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        })
        .collect()
}

fn decode_header(header: &str) -> SubstrateHeader<H256> {
    let header = const_hex::decode(header).expect("valid header hex");
    SubstrateHeader::decode(&mut header.as_slice()).expect("SCALE decode header")
}

fn metadata_scale(node_version: &str) -> Metadata {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.node")
        .join(node_version)
        .join("metadata.scale");
    let bytes = fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    Metadata::decode_from(&bytes).expect("decode metadata.scale")
}

fn offline_client(
    spec_version: u32,
    transaction_version: u32,
    metadata: Metadata,
) -> OfflineClient<SubstrateConfig> {
    let config = SubstrateConfig::builder()
        .set_spec_version_for_block_ranges([SpecVersionForRange {
            block_range: 0..10_000_000,
            spec_version,
            transaction_version,
        }])
        .set_metadata_for_spec_versions([(spec_version, Arc::new(metadata))])
        .build();
    OfflineClient::new_with_config(config)
}

/// Digest says 22_000, live spec at the hash is 1_000_000: contents and state RPCs
/// must use different node versions. Using the digest for `get_contract_state` is
/// the #1397 crash.
#[test]
fn enactment_block_pairs_digest_content_with_live_spec_state() {
    let header = decode_header(MAINNET_HEADER_1_774_491);
    assert_eq!(header.number, ENACTMENT_HEIGHT);

    let digest = header
        .protocol_version()
        .expect("protocol version of mainnet block 1_774_491 must be supported")
        .expect("mainnet block 1_774_491 must have a MNSV digest");
    assert_eq!(u32::from(digest), 22_000);

    let spec_version = recorded_runtimes()[&ENACTMENT_HEIGHT.to_string()].spec_version;
    assert_eq!(spec_version, 1_000_000);

    let (content, state) =
        block_runtime_versions(digest, spec_version).expect("both protocol versions are supported");
    assert_eq!(content, NodeVersion::V0_22);
    assert_eq!(state, NodeVersion::V1_0);
}

/// First block built by node 1.0 must be a supported protocol version. Indexer
/// versions without node 1.0 fail here with `Unsupported(1_000_000)`.
#[test]
fn first_post_upgrade_block_is_node_1_0() {
    let header = decode_header(MAINNET_HEADER_1_774_492);
    assert_eq!(header.number, FIRST_POST_UPGRADE_HEIGHT);

    let digest = header
        .protocol_version()
        .expect("protocol version of mainnet block 1_774_492 must be supported")
        .expect("mainnet block 1_774_492 must have a MNSV digest");
    assert_eq!(u32::from(digest), 1_000_000);

    let spec_version = recorded_runtimes()[&FIRST_POST_UPGRADE_HEIGHT.to_string()].spec_version;
    assert_eq!(spec_version, 1_000_000);

    let (content, state) =
        block_runtime_versions(digest, spec_version).expect("node 1.0 is supported");
    assert_eq!(content, NodeVersion::V1_0);
    assert_eq!(state, NodeVersion::V1_0);
}

/// The #1397 failure: v0.22 `get_contract_state` codegen is rejected by node 1.0
/// metadata. #1346 calls this API with the *state* module (v1.0) instead.
#[test]
fn get_contract_state_from_digest_runtime_is_incompatible_with_enactment_state() {
    let dummy = vec![0u8; 32];

    let v0_22_payload = runtimes::runtime_0_22_0::runtime_apis()
        .midnight_runtime_api()
        .get_contract_state(dummy.clone());
    let v1_0_payload = runtimes::runtime_1_0_0::runtime_apis()
        .midnight_runtime_api()
        .get_contract_state(dummy);

    let state_client = offline_client(1_000_000, 3, metadata_scale("1.0.0"));
    let at = state_client
        .at_block(ENACTMENT_HEIGHT)
        .expect("offline client at enactment height");

    let digest_call = at.runtime_apis().validate(&v0_22_payload);
    assert!(
        matches!(digest_call, Err(RuntimeApiError::IncompatibleCodegen)),
        "v0.22 get_contract_state against 1.0 metadata must fail (got {digest_call:?})"
    );

    at.runtime_apis()
        .validate(&v1_0_payload)
        .expect("v1.0 get_contract_state must validate against 1.0 metadata");
}

/// Enactment block 1_774_491 contains the contract call that forced a
/// `get_contract_state` read. Decoded from recorded extrinsics with the *content*
/// runtime (0.22), not the live spec.
#[tokio::test]
async fn enactment_block_contains_the_known_contract_call() {
    let content_client = offline_client(22_000, 2, metadata_scale("0.22.0"));
    let at = content_client
        .at_block(ENACTMENT_HEIGHT)
        .expect("offline client at enactment height");
    let extrinsics = at
        .extrinsics()
        .from_bytes(load_extrinsics(ENACTMENT_HEIGHT))
        .await;

    let expected = const_hex::decode(CONTRACT_ADDRESS).expect("valid contract address");
    let mut found = false;

    for extrinsic in extrinsics.iter() {
        let extrinsic = extrinsic.expect("decode recorded extrinsic");
        let Ok(call) = extrinsic.decode_call_data_as::<runtimes::runtime_0_22_0::Call>() else {
            continue;
        };

        let midnight_tx = match call {
            runtimes::runtime_0_22_0::Call::Midnight(
                runtimes::runtime_0_22_0::runtime_types::pallet_midnight::pallet::Call::send_mn_transaction {
                    midnight_tx,
                },
            ) => midnight_tx,
            _ => continue,
        };

        // Ledger deserialize needs a storage backend; the contract address is
        // SCALE-encoded inside the midnight tx bytes.
        if midnight_tx
            .windows(expected.len())
            .any(|window| window == expected)
        {
            found = true;
            break;
        }
    }

    assert!(
        found,
        "mainnet block 1_774_491 must contain the known contract call"
    );
}

/// First post-upgrade block decodes against node 1.0 metadata.
#[tokio::test]
async fn first_post_upgrade_block_decodes_with_node_1_0() {
    let client = offline_client(1_000_000, 3, metadata_scale("1.0.0"));
    let at = client
        .at_block(FIRST_POST_UPGRADE_HEIGHT)
        .expect("offline client at first post-upgrade height");
    let extrinsics = at
        .extrinsics()
        .from_bytes(load_extrinsics(FIRST_POST_UPGRADE_HEIGHT))
        .await;

    let decoded = extrinsics
        .iter()
        .map(|extrinsic| {
            extrinsic
                .expect("decode recorded extrinsic")
                .decode_call_data_as::<runtimes::runtime_1_0_0::Call>()
                .expect("1.0 metadata must decode the first post-upgrade block")
        })
        .collect::<Vec<_>>();
    assert_eq!(decoded.len(), 4);
}
