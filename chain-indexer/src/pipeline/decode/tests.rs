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
    domain::{
        BlockRef, DustRegistrationEvent,
        node::{self, Node},
    },
    infra::subxt_node::{
        AURA_ENGINE_ID, BABE_ENGINE_ID, Config, SubxtNode,
        rpc::{NodeRpc, ReconnectPolicy, WsTransport},
        runtimes,
    },
    pipeline::{
        decode::{
            AuthoritySetError, BABE_NEXT_EPOCH_DATA, CpuPool, block_authorities, chunks_in_decode,
            decode,
        },
        sourcing::{self, AUTHORITY_SET_ITEMS, MetadataCache, Parent, resolve, storage_key},
    },
};
use futures::{TryStreamExt, stream};
use indexer_common::domain::{BlockHash, BlockNumber, ByteArray, ByteVec, ProtocolVersion};
use parity_scale_codec::{Decode, Encode};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    env, fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    pin::pin,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use subxt::{
    Metadata,
    config::substrate::{Digest, DigestItem, SubstrateHeader},
    utils::H256,
};

/// Records decode fixtures: for each `label=height` in `FIXTURE_HEIGHTS`, the block at that
/// height as the block sourcing pipeline sources it, and the [node::Block] the subxt-based node
/// adapter makes from the same block. Metadata missing from `.node` is saved alongside.
#[tokio::test]
#[ignore = "records fixtures from the node at NODE_URL"]
async fn record_decode_fixtures() {
    let url = env::var("NODE_URL").expect("NODE_URL");
    let name = env::var("FIXTURE_NAME").expect("FIXTURE_NAME");
    let heights = env::var("FIXTURE_HEIGHTS").expect("FIXTURE_HEIGHTS");

    let transport = WsTransport::new(url.as_str(), Default::default())
        .await
        .expect("node can be reached");
    let rpc = NodeRpc::new(
        transport,
        NonZeroUsize::new(64).unwrap(),
        NonZeroUsize::new(4).unwrap(),
        ReconnectPolicy {
            max_delay: Duration::from_secs(1),
            max_attempts: 3,
        },
    );
    let metadata = MetadataCache::default();
    let mut node = SubxtNode::new(Config {
        url: url.clone(),
        reconnect_max_delay: Duration::from_secs(1),
        reconnect_max_attempts: 3,
        subscription_recovery_timeout: Duration::from_secs(30),
        fetch_concurrency: 1,
    })
    .await
    .expect("node adapter connects");
    let genesis_hash = resolve(&rpc, 0..=0)
        .await
        .expect("genesis resolves")
        .pop()
        .flatten()
        .expect("genesis hash");

    fs::create_dir_all(fixtures_dir().join("metadata")).expect("fixtures dir");

    for label_height in heights.split(',') {
        let (label, height) = label_height.split_once('=').expect("label=height");
        let height = height.trim().parse::<BlockNumber>().expect("height");

        let (parent, hash) = if height == 0 {
            (None, genesis_hash)
        } else {
            let mut hashes = resolve(&rpc, height - 1..=height)
                .await
                .expect("hashes resolve")
                .into_iter();
            let parent = hashes.next().flatten().expect("parent hash");
            (Some(parent), hashes.next().flatten().expect("hash"))
        };

        let sourced = sourcing::source(&rpc, &metadata, height, &[hash], parent, true)
            .await
            .expect("block is sourced")
            .pop()
            .expect("one block");
        let made = {
            let after = parent.map(|parent| BlockRef {
                hash: parent,
                height: u64::from(height - 1),
            });
            let blocks = node.finalized_blocks(after);
            let mut blocks = pin!(blocks);
            blocks
                .try_next()
                .await
                .expect("node adapter makes block")
                .expect("one block")
        };
        assert_eq!(made.hash, hash);

        use sourcing::Block::*;
        let sourced_metadata = match &sourced {
            Genesis { metadata, .. } | Block { metadata, .. } => metadata.clone(),
        };
        let metadata_hash = hex(sourced_metadata.hasher().hash());
        if std::panic::catch_unwind(|| metadata_with_hash(&metadata_hash)).is_err() {
            // Not in `.node`: keep the node's metadata with the fixtures.
            let bytes = fetch_metadata_bytes(&rpc, hash).await;
            let file = format!("{}.scale", metadata_hash.as_str().unwrap());
            fs::write(fixtures_dir().join("metadata").join(file), bytes)
                .expect("metadata can be written");
        }

        let recorded_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time after epoch")
            .as_secs();
        let fixture = json!({
            "provenance": {
                "node_url": url,
                "genesis_hash": hex(genesis_hash),
                "height": height,
                "recorded_at": recorded_at,
            },
            "sourced": sourced_json(&sourced),
            "made": render(&made),
        });
        let path = fixtures_dir().join(format!("{name}-{label}.json"));
        fs::write(&path, serde_json::to_string(&fixture).unwrap()).expect("fixture can be written");
        println!("recorded {}", path.display());
    }
}

#[test]
fn test_decode_round_trip() {
    let fixtures = fixtures();
    assert!(!fixtures.is_empty(), "decode fixtures are present");

    for (name, fixture) in fixtures {
        let decoded = node::Block::try_from(sourced_of(&fixture["sourced"]))
            .unwrap_or_else(|error| panic!("{name}: cannot decode: {error:?}"));
        let decoded = render(&decoded);
        let made = &fixture["made"];

        for (field, expected) in made.as_object().expect("made fields") {
            // The node adapter fetched the ledger state root at genesis only; decode carries
            // it for every block.
            if field == "ledger_state_root" && expected.is_null() {
                continue;
            }
            assert_eq!(decoded[field], *expected, "{name}: {field}");
        }
    }
}

#[test]
fn test_unseen_runtime() {
    // A 2.1 spec version without a `.node` entry decodes with the metadata the block carries.
    let block = restamped(fixture_block("devnet-2.1"), 2_001_001);
    let decoded = node::Block::try_from(block).expect("in-range spec version decodes");
    assert_eq!(
        decoded.protocol_version,
        ProtocolVersion::try_from(2_001_001_u32).unwrap()
    );

    let block = restamped(fixture_block("devnet-2.1"), 2_002_000);
    assert!(matches!(
        node::Block::try_from(block),
        Err(super::Error::ProtocolVersion(..))
    ));
}

#[test]
fn test_authorities_by_engine() {
    let aura = header_with(vec![DigestItem::PreRuntime(AURA_ENGINE_ID, 4u64.encode())]);
    assert_eq!(authorities(&aura), Some(vec![[1; 32], [2; 32]]));

    let babe = header_with(vec![babe_pre_digest(1)]);
    assert_eq!(authorities(&babe), Some(vec![[3; 32], [4; 32]]));

    // A block opening a new epoch is verified against the parent's next authorities.
    let babe_epoch = header_with(vec![
        babe_pre_digest(0),
        DigestItem::Consensus(BABE_ENGINE_ID, vec![BABE_NEXT_EPOCH_DATA]),
    ]);
    assert_eq!(authorities(&babe_epoch), Some(vec![[5; 32]]));

    // Across the switch, the first recognized digest decides.
    let mixed = header_with(vec![
        babe_pre_digest(1),
        DigestItem::PreRuntime(AURA_ENGINE_ID, 4u64.encode()),
    ]);
    assert_eq!(authorities(&mixed), Some(vec![[3; 32], [4; 32]]));
    let without_babe_support =
        block_authorities(&mixed, &authority_set(), false, |_| true).expect("authority set");
    assert_eq!(without_babe_support, Some(vec![[1; 32], [2; 32]]));

    assert_eq!(authorities(&header_with(vec![])), None);
}

#[test]
fn test_authority_set_guard() {
    // A runtime without the digest engine's authority set storage fails, naming it.
    let babe = header_with(vec![babe_pre_digest(1)]);
    let error = block_authorities(&babe, &authority_set(), true, |(pallet, _)| {
        pallet != "Babe"
    })
    .expect_err("no BABE storage");
    assert!(matches!(
        error,
        AuthoritySetError::Missing {
            engine: "BABE",
            item: ("Babe", "Authorities")
        }
    ));

    // A present but empty set gives no author.
    let aura = header_with(vec![DigestItem::PreRuntime(AURA_ENGINE_ID, 4u64.encode())]);
    let authorities = block_authorities(&aura, &[], true, |_| true).expect("authority set");
    assert_eq!(authorities, Some(vec![]));
}

#[test]
fn test_cpu_pool_chunks_in_decode() {
    assert_eq!(chunks_in_decode(15, 512), 2);
    assert_eq!(chunks_in_decode(15, 120), 2);
    assert_eq!(chunks_in_decode(32, 16), 3);
    assert_eq!(chunks_in_decode(32, 8), 5);
}

#[tokio::test]
async fn test_cpu_pool_decode_in_height_order() {
    let blocks = fixtures()
        .into_iter()
        .filter(|(name, _)| name.starts_with("devnet-"))
        .map(|(_, fixture)| fixture)
        .collect::<Vec<_>>();
    let expected = blocks
        .iter()
        .map(|fixture| fixture["made"]["hash"].clone())
        .collect::<Vec<_>>();
    let chunks = blocks
        .iter()
        .map(|fixture| Ok(vec![sourced_of(&fixture["sourced"])]))
        .collect::<Vec<_>>();

    let pool = Arc::new(CpuPool::new(NonZeroUsize::new(2).unwrap()).unwrap());
    let decoded = decode(stream::iter(chunks), pool, NonZeroUsize::new(1).unwrap())
        .map_ok(|block| hex(block.hash))
        .try_collect::<Vec<_>>()
        .await
        .expect("blocks decode");

    assert_eq!(decoded, expected);
}

fn authorities(header: &SubstrateHeader<H256>) -> Option<Vec<[u8; 32]>> {
    block_authorities(header, &authority_set(), true, |_| true).expect("authority set")
}

fn authority_set() -> Vec<(ByteVec, ByteVec)> {
    let aura = vec![[1u8; 32], [2; 32]].encode();
    let babe = vec![([3u8; 32], 1u64), ([4; 32], 1)].encode();
    let next_babe = vec![([5u8; 32], 1u64)].encode();
    vec![
        (
            storage_key(AUTHORITY_SET_ITEMS[0]).to_vec().into(),
            aura.into(),
        ),
        (
            storage_key(AUTHORITY_SET_ITEMS[1]).to_vec().into(),
            babe.into(),
        ),
        (
            storage_key(AUTHORITY_SET_ITEMS[2]).to_vec().into(),
            next_babe.into(),
        ),
    ]
}

fn babe_pre_digest(authority_index: u32) -> DigestItem {
    let mut pre_digest = vec![2];
    pre_digest.extend(authority_index.encode());
    DigestItem::PreRuntime(BABE_ENGINE_ID, pre_digest)
}

fn block_hash(value: &Value) -> BlockHash {
    ByteArray(bytes(value).to_vec().try_into().expect("32 bytes"))
}

fn bytes(value: &Value) -> ByteVec {
    const_hex::decode(value.as_str().expect("hex string"))
        .expect("hex")
        .into()
}

/// The SCALE-encoded `RuntimeMetadataPrefixed` the node serves at the given block.
async fn fetch_metadata_bytes(rpc: &NodeRpc<WsTransport>, at: BlockHash) -> Vec<u8> {
    use crate::infra::subxt_node::rpc::Batch;

    let batch = Batch::default().call(at, "Metadata_metadata_at_version", &15u32.encode());
    let result = rpc
        .batch(batch)
        .await
        .expect("metadata call")
        .pop()
        .unwrap();
    let value = result.expect("metadata call succeeds")["value"]
        .as_str()
        .expect("value")
        .to_owned();
    let bytes = const_hex::decode(value).expect("hex");
    Option::<Vec<u8>>::decode(&mut &bytes[..])
        .expect("optional metadata")
        .expect("metadata v15")
}

fn fixture_block(name: &str) -> sourcing::Block {
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(fixture, _)| fixture == name)
        .unwrap_or_else(|| panic!("fixture {name}"));
    sourced_of(&fixture["sourced"])
}

fn fixtures() -> Vec<(String, Value)> {
    let mut fixtures = fs::read_dir(fixtures_dir())
        .expect("fixtures can be read")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "json").then_some(path)
        })
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let fixture = serde_json::from_slice(&fs::read(&path).expect("fixture can be read"))
                .expect("fixture is JSON");
            (name, fixture)
        })
        .collect::<Vec<_>>();
    fixtures.sort_by(|(a, _), (b, _)| a.cmp(b));
    fixtures
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/decode")
}

fn header_with(logs: Vec<DigestItem>) -> SubstrateHeader<H256> {
    SubstrateHeader {
        parent_hash: H256::zero(),
        number: 1,
        state_root: H256::zero(),
        extrinsics_root: H256::zero(),
        digest: Digest { logs },
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> Value {
    const_hex::encode_prefixed(bytes).into()
}

/// The metadata with the given hash: from the `.node` directory, else from the fixtures.
fn metadata_with_hash(hash: &Value) -> Metadata {
    let node_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.node");
    let node_metadata = fs::read_dir(&node_dir)
        .expect(".node can be read")
        .filter_map(|entry| fs::read(entry.ok()?.path().join("metadata.scale")).ok());
    let fixture_metadata = fs::read_dir(fixtures_dir().join("metadata"))
        .into_iter()
        .flatten()
        .filter_map(|entry| fs::read(entry.ok()?.path()).ok());

    node_metadata
        .chain(fixture_metadata)
        .map(|bytes| Metadata::decode(&mut &bytes[..]).expect("metadata can be decoded"))
        .find(|metadata| hex(metadata.hasher().hash()) == *hash)
        .unwrap_or_else(|| panic!("no metadata with hash {hash}"))
}

fn pairs(pairs: &[(ByteVec, ByteVec)]) -> Value {
    pairs
        .iter()
        .map(|(key, value)| json!([hex(key), hex(value)]))
        .collect()
}

fn pairs_of(value: &Value) -> Vec<(ByteVec, ByteVec)> {
    value
        .as_array()
        .expect("pairs")
        .iter()
        .map(|pair| (bytes(&pair[0]), bytes(&pair[1])))
        .collect()
}

/// A [node::Block], every byte field in full, except transactions: their SHA-256 hashes.
fn render(block: &node::Block) -> Value {
    use runtimes::Transaction::*;
    let transactions = block
        .transactions
        .iter()
        .map(|transaction| match transaction {
            Regular(bytes) => json!({ "regular": hex(Sha256::digest(bytes)) }),
            System(bytes) => json!({ "system": hex(Sha256::digest(bytes)) }),
        })
        .collect::<Vec<_>>();

    use DustRegistrationEvent::*;
    let dust_registration_events = block
        .dust_registration_events
        .iter()
        .map(|event| match event {
            Registration {
                cardano_stake_key,
                dust_address,
            } => json!({ "registration": [hex(cardano_stake_key), hex(dust_address)] }),
            Deregistration {
                cardano_stake_key,
                dust_address,
            } => json!({ "deregistration": [hex(cardano_stake_key), hex(dust_address)] }),
            MappingAdded {
                cardano_stake_key,
                dust_address,
                utxo_id,
                utxo_index,
            } => json!({
                "mapping_added": [hex(cardano_stake_key), hex(dust_address), hex(utxo_id), utxo_index]
            }),
            MappingRemoved {
                cardano_stake_key,
                dust_address,
                utxo_id,
                utxo_index,
            } => json!({
                "mapping_removed": [hex(cardano_stake_key), hex(dust_address), hex(utxo_id), utxo_index]
            }),
        })
        .collect::<Vec<_>>();

    json!({
        "hash": hex(block.hash),
        "height": block.height,
        "protocol_version": u32::from(block.protocol_version),
        "parent_hash": hex(block.parent_hash),
        "author": block.author.map(hex),
        "timestamp": block.timestamp,
        "zswap_merkle_tree_root": hex(block.zswap_merkle_tree_root.serialize().expect("root serializes")),
        "ledger_state_root": block.ledger_state_root.as_ref().map(hex),
        "transactions": transactions,
        "dust_registration_events": dust_registration_events,
        "bridge_events": serde_json::to_value(&block.bridge_events).expect("bridge events serialize"),
        "d_parameter": block.d_parameter.as_ref().map(|d_parameter| json!([
            d_parameter.num_permissioned_candidates,
            d_parameter.num_registered_candidates,
        ])),
        "terms_and_conditions": block
            .terms_and_conditions
            .as_ref()
            .map(|terms| json!([hex(terms.hash), terms.url])),
    })
}

/// The block with its header's `MNSV` stamp replaced.
fn restamped(block: sourcing::Block, spec_version: u32) -> sourcing::Block {
    let sourcing::Block::Block {
        hash,
        height,
        header,
        zswap_state_root,
        ledger_state_root,
        system_parameters,
        metadata,
        parent,
        extrinsics,
        events,
    } = block
    else {
        panic!("not genesis");
    };
    let mut decoded = SubstrateHeader::<H256>::decode(&mut &header[..]).unwrap();
    for log in decoded.digest.logs.iter_mut() {
        if let DigestItem::Consensus(engine, data) = log
            && engine == b"MNSV"
        {
            *data = spec_version.encode();
        }
    }

    sourcing::Block::Block {
        hash,
        height,
        header: decoded.encode().into(),
        zswap_state_root,
        ledger_state_root,
        system_parameters,
        metadata,
        parent,
        extrinsics,
        events,
    }
}

/// A [sourcing::Block] as fixture JSON; its metadata by hash.
fn sourced_json(block: &sourcing::Block) -> Value {
    use sourcing::Block::*;
    match block {
        Genesis {
            hash,
            header,
            zswap_state_root,
            ledger_state_root,
            system_parameters: (d_parameter, terms_and_conditions),
            metadata,
            ledger_state: _,
            cnight_mappings,
            extrinsics,
            events,
        } => json!({
            "kind": "genesis",
            "extrinsics": extrinsics.iter().map(hex).collect::<Vec<_>>(),
            "events": hex(events),
            "hash": hex(hash),
            "header": hex(header),
            "zswap_state_root": hex(zswap_state_root),
            "ledger_state_root": hex(ledger_state_root),
            "system_parameters": [hex(d_parameter), hex(terms_and_conditions)],
            "metadata_hash": hex(metadata.hasher().hash()),
            "cnight_mappings": pairs(cnight_mappings),
        }),
        Block {
            hash,
            height,
            header,
            zswap_state_root,
            ledger_state_root,
            system_parameters,
            metadata,
            parent,
            extrinsics,
            events,
        } => json!({
            "kind": "block",
            "hash": hex(hash),
            "height": height,
            "header": hex(header),
            "zswap_state_root": hex(zswap_state_root),
            "ledger_state_root": hex(ledger_state_root),
            "system_parameters": system_parameters
                .as_ref()
                .map(|(d_parameter, terms)| json!([hex(d_parameter), hex(terms)])),
            "metadata_hash": hex(metadata.hasher().hash()),
            "parent": {
                "hash": hex(parent.hash),
                "authority_set": pairs(&parent.authority_set),
            },
            "extrinsics": extrinsics.iter().map(hex).collect::<Vec<_>>(),
            "events": hex(events),
        }),
    }
}

fn sourced_of(sourced: &Value) -> sourcing::Block {
    let metadata = Arc::new(metadata_with_hash(&sourced["metadata_hash"]));
    let system_parameters = |value: &Value| {
        value
            .as_array()
            .map(|results| (bytes(&results[0]), bytes(&results[1])))
    };

    match sourced["kind"].as_str() {
        Some("genesis") => sourcing::Block::Genesis {
            hash: block_hash(&sourced["hash"]),
            header: bytes(&sourced["header"]),
            zswap_state_root: bytes(&sourced["zswap_state_root"]),
            ledger_state_root: bytes(&sourced["ledger_state_root"]),
            system_parameters: system_parameters(&sourced["system_parameters"])
                .expect("genesis system parameters"),
            metadata,
            ledger_state: vec![0xab].into(),
            cnight_mappings: pairs_of(&sourced["cnight_mappings"]),
            extrinsics: sourced["extrinsics"]
                .as_array()
                .expect("extrinsics")
                .iter()
                .map(bytes)
                .collect(),
            events: bytes(&sourced["events"]),
        },
        _ => sourcing::Block::Block {
            hash: block_hash(&sourced["hash"]),
            height: BlockNumber::try_from(sourced["height"].as_u64().expect("height"))
                .expect("height is a block number"),
            header: bytes(&sourced["header"]),
            zswap_state_root: bytes(&sourced["zswap_state_root"]),
            ledger_state_root: bytes(&sourced["ledger_state_root"]),
            system_parameters: system_parameters(&sourced["system_parameters"]),
            metadata,
            parent: Parent {
                hash: block_hash(&sourced["parent"]["hash"]),
                authority_set: pairs_of(&sourced["parent"]["authority_set"]),
            },
            extrinsics: sourced["extrinsics"]
                .as_array()
                .expect("extrinsics")
                .iter()
                .map(bytes)
                .collect(),
            events: bytes(&sourced["events"]),
        },
    }
}
