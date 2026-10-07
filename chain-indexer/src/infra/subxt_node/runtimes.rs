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
    domain::{
        BlockRef, DParameter, DustRegistrationEvent, TermsAndConditions,
        extrinsic::{Applied, EventIndex, ExtrinsicIndex, Phase, divergence},
    },
    infra::subxt_node::{ContentSource, OnlineClientAtBlock, SubxtNodeError},
};
use indexer_common::domain::{ByteVec, NodeVersion, TransactionHash, bridge::BridgeEvent};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use subxt::{SubstrateConfig, client::OfflineClientAtBlockT};

/// A client at a block, online or offline.
pub type AtBlock<C> = subxt::client::ClientAtBlock<SubstrateConfig, C>;

/// Runtime specific block details.
pub struct BlockDetails {
    pub timestamp: Option<u64>,
    /// Transactions in execution order, each with its phase and how it was applied or why it
    /// failed.
    pub transactions: Vec<(Phase, Transaction, Result<Applied, String>)>,
    /// DUST registration events in execution order, each with its phase and event index.
    pub dust_registration_events: Vec<(Phase, EventIndex, DustRegistrationEvent)>,
    /// c2m-bridge events. Only populated for node 2.0+, where the
    /// `c2m-bridge` pallet exists in the runtime metadata. Empty for earlier
    /// node versions (the pallet did not yet exist there).
    pub bridge_events: Vec<(Phase, EventIndex, BridgeEvent)>,
}

/// Runtime specific (serialized) transaction.
#[derive(Debug, PartialEq, Eq)]
pub enum Transaction {
    Regular(ByteVec),
    System(ByteVec),
}

impl Transaction {
    /// The hash the ledger computes: SHA-256 over the transaction's tagged serialization, which is
    /// the bytes on chain.
    fn hash(&self) -> TransactionHash {
        let (Self::Regular(bytes) | Self::System(bytes)) = self;
        <[u8; 32]>::from(Sha256::digest(bytes)).into()
    }
}

/// Make block details depending on the given protocol version: fetch the block's raw extrinsics
/// and events, and decode them.
pub async fn make_block_details(
    authorities: &mut Option<Vec<[u8; 32]>>,
    node_version: NodeVersion,
    block: &OnlineClientAtBlock,
    content: Option<ContentSource>,
) -> Result<BlockDetails, SubxtNodeError> {
    let block_ref = BlockRef {
        hash: block.block_hash().0.into(),
        height: block.block_number(),
    };

    // Enactment block: decode this block's raw extrinsic bytes against the parent (old-runtime)
    // client. Raw event bytes are metadata-independent, so they are fetched from this block and
    // re-decoded against the same client.
    let (client, extrinsics) = match content {
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
    let client = client.as_ref().unwrap_or(block);

    let events = block
        .events()
        .fetch()
        .await
        .map_err(|error| SubxtNodeError::FetchEvents(error.into()))?
        .bytes()
        .to_vec();

    decode_block_details(
        authorities,
        node_version,
        client,
        block_ref,
        extrinsics,
        events,
    )
    .await
}

/// Decode block details from raw extrinsics and events depending on the given protocol version.
pub async fn decode_block_details(
    authorities: &mut Option<Vec<[u8; 32]>>,
    node_version: NodeVersion,
    client: &AtBlock<impl OfflineClientAtBlockT<SubstrateConfig>>,
    block: BlockRef,
    extrinsics: Vec<Vec<u8>>,
    events: Vec<u8>,
) -> Result<BlockDetails, SubxtNodeError> {
    // TODO Replace this often repeated pattern with a macro?
    match node_version {
        NodeVersion::V0_22 => {
            v0_22_0::make_block_details(authorities, client, block, extrinsics, events).await
        }
        NodeVersion::V1_0 => {
            v1_0_300::make_block_details(authorities, client, block, extrinsics, events).await
        }
        NodeVersion::V2_0 => {
            v2_0_0::make_block_details(authorities, client, block, extrinsics, events).await
        }
        NodeVersion::V2_1 => {
            v2_1_0::make_block_details(authorities, client, block, extrinsics, events).await
        }
    }
}

/// A runtime's decoded call.
trait CallExt {
    /// The transaction the call carries. Exhaustive, so that a new pallet or call does not compile
    /// until it is handled.
    fn transaction(self) -> Option<Transaction>;
    /// The time set by `Timestamp::set`.
    fn timestamp(&self) -> Option<u64>;
}

/// A runtime's decoded event.
trait EventExt {
    /// `TxApplied`, `TxPartialSuccess` or `ExtrinsicFailed`: the outcome of the extrinsic whose
    /// phase the event is recorded under.
    fn outcome(&self) -> Option<Result<Applied, String>>;
    /// `ExtrinsicSuccess`, which ends every successful dispatch.
    fn is_success(&self) -> bool;
}

/// An applied system transaction.
trait SystemTransaction {
    /// The applied system transaction's bytes and hash, if any.
    fn transaction(&self) -> Option<(ByteVec, TransactionHash)>;
}

impl From<subxt::events::Phase> for Phase {
    fn from(phase: subxt::events::Phase) -> Self {
        use subxt::events::Phase::*;

        match phase {
            Initialization => Self::Initialization,
            ApplyExtrinsic(index) => Self::ApplyExtrinsic(index),
            Finalization => Self::Finalization,
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

/// The block's transactions in execution order, each with its phase and how it was applied or why
/// it failed.
fn transactions(
    block: BlockRef,
    calls: Vec<impl CallExt>,
    events: &[(Phase, EventIndex, impl EventExt + SystemTransaction)],
) -> Vec<(Phase, Transaction, Result<Applied, String>)> {
    let mut extrinsics = calls
        .into_iter()
        .enumerate()
        .filter_map(|(index, call)| {
            call.transaction()
                .map(|transaction| (index as ExtrinsicIndex, transaction))
        })
        .collect::<BTreeMap<_, _>>();

    if block.height == 0 {
        return genesis_transactions(extrinsics);
    }

    let transactions = events
        .iter()
        .filter_map(|(phase, _, event)| {
            if let Some((bytes, tx_hash)) = event.transaction() {
                Some((
                    *phase,
                    Transaction::System(bytes),
                    Ok(Applied::Fully { tx_hash }),
                ))
            } else if let Some(outcome) = event.outcome() {
                regular_transaction(block, &mut extrinsics, *phase, outcome)
            } else if event.is_success() {
                successful_transaction(block, &mut extrinsics, *phase)
            } else {
                None
            }
        })
        .collect();
    unaccounted(block, extrinsics);

    transactions
}

/// The transactions of the genesis block, which is not executed and has no events: every one in
/// the body, in body order, taken as applied, with the hash computed from its bytes.
fn genesis_transactions(
    extrinsics: BTreeMap<ExtrinsicIndex, Transaction>,
) -> Vec<(Phase, Transaction, Result<Applied, String>)> {
    extrinsics
        .into_iter()
        .map(|(index, transaction)| {
            let tx_hash = transaction.hash();

            (
                Phase::ApplyExtrinsic(index),
                transaction,
                Ok(Applied::Fully { tx_hash }),
            )
        })
        .collect()
}

/// Take the regular transaction of extrinsic `i` for an outcome under `ApplyExtrinsic(i)`, so that
/// a second outcome finds none. An applied outcome with nothing to take is a divergence; a failure
/// with nothing to take is that of another extrinsic and is ignored.
fn regular_transaction(
    block: BlockRef,
    extrinsics: &mut BTreeMap<ExtrinsicIndex, Transaction>,
    phase: Phase,
    outcome: Result<Applied, String>,
) -> Option<(Phase, Transaction, Result<Applied, String>)> {
    use Phase::*;

    let transaction = match phase {
        ApplyExtrinsic(index) => match extrinsics.get(&index) {
            Some(Transaction::Regular(_)) => extrinsics.remove(&index),
            Some(Transaction::System(_)) | None => None,
        },
        Initialization | Finalization => None,
    };

    match (transaction, outcome) {
        (Some(transaction), outcome) => Some((phase, transaction, outcome)),
        (None, Ok(applied)) => {
            divergence(
                block,
                format_args!("{applied:?} under {phase:?}, which has no Midnight transaction"),
            );
            None
        }
        (None, Err(_)) => None,
    }
}

/// Take the regular transaction of extrinsic `i` still left when `ExtrinsicSuccess` under
/// `ApplyExtrinsic(i)` ends its dispatch: it succeeded without `TxApplied` or `TxPartialSuccess`,
/// which is a divergence. It is still taken as applied, with the hash computed from its bytes, so
/// that it is indexed as it was before outcomes were followed.
fn successful_transaction(
    block: BlockRef,
    extrinsics: &mut BTreeMap<ExtrinsicIndex, Transaction>,
    phase: Phase,
) -> Option<(Phase, Transaction, Result<Applied, String>)> {
    let Phase::ApplyExtrinsic(index) = phase else {
        return None;
    };

    match extrinsics.get(&index) {
        Some(Transaction::Regular(_)) => {
            divergence(
                block,
                format_args!(
                    "Midnight extrinsic {index} succeeded without TxApplied or TxPartialSuccess"
                ),
            );
            extrinsics.remove(&index).map(|transaction| {
                let tx_hash = transaction.hash();
                (phase, transaction, Ok(Applied::Fully { tx_hash }))
            })
        }
        Some(Transaction::System(_)) | None => None,
    }
}

/// Report every regular transaction that no outcome accounted for. A top-level system transaction
/// left over is not reported: it is `Root`-only, so in an executed block it can only have failed,
/// like any other failed call, and one that applied would be taken from its
/// `SystemTransactionApplied` event.
fn unaccounted(block: BlockRef, extrinsics: BTreeMap<ExtrinsicIndex, Transaction>) {
    extrinsics
        .iter()
        .filter(|(_, transaction)| matches!(transaction, Transaction::Regular(_)))
        .for_each(|(index, _)| {
            divergence(
                block,
                format_args!("Midnight extrinsic {index} has no outcome"),
            )
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexer_common::{
        domain::{LedgerVersion, ProtocolVersion, ledger},
        error::BoxError,
        testing::init_ledger_db,
    };
    use parity_scale_codec::{Compact, Encode};
    use serde::Deserialize;
    use std::{fs, path::Path};
    use subxt::{Metadata, client::OfflineClient, config::substrate::SpecVersionForRange};

    /// A recorded block: raw extrinsics and `System.Events` bytes, as served by the node's RPC.
    #[derive(Deserialize)]
    struct Fixture {
        height: u64,
        hash: String,
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

    /// Decode a block offline with the committed metadata in the given directory.
    async fn decode_raw(
        block: BlockRef,
        spec_version: u32,
        metadata_dir: &str,
        extrinsics: Vec<Vec<u8>>,
        events: Vec<u8>,
    ) -> Result<BlockDetails, SubxtNodeError> {
        let config = SubstrateConfig::builder()
            .set_metadata_for_spec_versions([(spec_version, metadata(metadata_dir).into())])
            .set_spec_version_for_block_ranges([SpecVersionForRange {
                block_range: 0..u64::MAX,
                spec_version,
                transaction_version: 1,
            }])
            .build();
        let client = OfflineClient::new_with_config(config)
            .at_block(block.height)
            .unwrap();
        let node_version = ProtocolVersion::try_from(spec_version)
            .unwrap()
            .node_version();

        decode_block_details(&mut None, node_version, &client, block, extrinsics, events).await
    }

    fn metadata(metadata_dir: &str) -> Metadata {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.node")
            .join(metadata_dir)
            .join("metadata.scale");
        Metadata::decode_from(&fs::read(path).unwrap()).unwrap()
    }

    /// Decode a recorded block offline with the committed metadata in the given directory.
    async fn decode(name: &str, metadata_dir: &str) -> Result<BlockDetails, SubxtNodeError> {
        let fixture = fixture(name);
        let block = BlockRef {
            hash: const_hex::decode_to_array(&fixture.hash).unwrap().into(),
            height: fixture.height,
        };
        let extrinsics = fixture.extrinsics.iter().map(|e| unhex(e)).collect();
        let events = fixture.events.as_deref().map(unhex).unwrap_or_default();

        decode_raw(
            block,
            fixture.spec_version,
            metadata_dir,
            extrinsics,
            events,
        )
        .await
    }

    /// A compact, ordered description of the block details so that golden values stay readable:
    /// transactions as `R`/`S` with length, first and last four bytes, phase and how they were
    /// applied with the first four bytes of the hash, and for registrations and bridge events
    /// their count with the first and last one.
    fn summary(details: &BlockDetails) -> Vec<String> {
        fn describe<T: std::fmt::Debug>(name: &str, items: &[T]) -> Vec<String> {
            [format!("{name}: {}", items.len())]
                .into_iter()
                .chain(items.first().map(|item| format!("{name} first: {item:?}")))
                .chain(items.last().map(|item| format!("{name} last: {item:?}")))
                .collect()
        }

        let transactions = details
            .transactions
            .iter()
            .map(|(phase, transaction, outcome)| {
                use Transaction::*;

                let (kind, bytes) = match transaction {
                    Regular(bytes) => ("R", bytes.as_ref()),
                    System(bytes) => ("S", bytes.as_ref()),
                };
                let outcome = match outcome {
                    Ok(Applied::Fully { tx_hash }) => {
                        format!("fully {}", const_hex::encode(&tx_hash.as_ref()[..4]))
                    }
                    Ok(Applied::Partially { tx_hash }) => {
                        format!("partially {}", const_hex::encode(&tx_hash.as_ref()[..4]))
                    }
                    Err(error) => format!("failed {error}"),
                };
                format!(
                    "{kind} {} {}..{} {phase:?} {outcome}",
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

    // The genesis transactions are hashed from their bytes; these hashes are the ones devnet's
    // indexer serves for its block 0.
    #[tokio::test]
    async fn genesis_block_of_devnet_runtime_1_0_300() {
        let details = decode("devnet-0", "1.0.300").await.unwrap();
        assert_eq!(
            summary(&details),
            [
                "timestamp Some(1754395200000)",
                "S 41 6d69646e..a47e8d03 ApplyExtrinsic(0) fully c17745ff",
                "S 1455 6d69646e..297c3fea ApplyExtrinsic(1) fully 8e5bb10c",
                "S 726 6d69646e..d107a10f ApplyExtrinsic(2) fully d38909e6",
                "R 145163 6d69646e..9baaf905 ApplyExtrinsic(3) fully e438e230",
                "R 215 6d69646e..e0396700 ApplyExtrinsic(4) fully 8421e23d",
                "R 215 6d69646e..50d91700 ApplyExtrinsic(5) fully aae5a3df",
                "R 215 6d69646e..db874900 ApplyExtrinsic(6) fully b1f50b3e",
                "R 215 6d69646e..95694600 ApplyExtrinsic(7) fully 5ac3fa21",
                "R 215 6d69646e..8a610500 ApplyExtrinsic(8) fully 85c89bd6",
                "R 215 6d69646e..8af57000 ApplyExtrinsic(9) fully fa9072e2",
                "R 215 6d69646e..c270f700 ApplyExtrinsic(10) fully 22271aff",
                "R 215 6d69646e..defe7500 ApplyExtrinsic(11) fully 249f7b67",
                "R 215 6d69646e..ade03500 ApplyExtrinsic(12) fully ab7ba6d3",
                "R 215 6d69646e..5ebafa00 ApplyExtrinsic(13) fully f19d4ddd",
                "R 215 6d69646e..41054200 ApplyExtrinsic(14) fully 2b47bba6",
                "R 215 6d69646e..a8f49300 ApplyExtrinsic(15) fully a87a7718",
                "R 215 6d69646e..d027e900 ApplyExtrinsic(16) fully 67cf3af3",
                "R 215 6d69646e..a99c1600 ApplyExtrinsic(17) fully db1689a9",
                "R 215 6d69646e..76b31300 ApplyExtrinsic(18) fully 64fea949",
                "R 215 6d69646e..a4e9d900 ApplyExtrinsic(19) fully c7c46fdb",
                "R 215 6d69646e..39747400 ApplyExtrinsic(20) fully bec424df",
                "R 215 6d69646e..7a699500 ApplyExtrinsic(21) fully fc4e4392",
                "R 215 6d69646e..455a0800 ApplyExtrinsic(22) fully ddfa3012",
                "R 215 6d69646e..add33800 ApplyExtrinsic(23) fully 664e1a19",
                "S 726 6d69646e..d107a10f ApplyExtrinsic(24) fully 7c5582f0",
                "S 6900 6d69646e..1649624b ApplyExtrinsic(25) fully 692b1e4d",
                "registrations: 0",
                "bridge events: 0",
            ]
        );
    }

    #[tokio::test]
    async fn registrations_block_of_devnet_runtime_1_0_300() {
        let details = decode("devnet-2", "1.0.300").await.unwrap();
        assert_eq!(
            summary(&details),
            [
                "timestamp Some(1789577448000)",
                "S 5429 6d69646e..61916f56 ApplyExtrinsic(1) fully 92130144",
                "registrations: 237",
                "registrations first: (ApplyExtrinsic(1), 1, Registration { cardano_stake_key: e0c305c7…, dust_address: 73d75758… })",
                "registrations last: (ApplyExtrinsic(1), 237, MappingAdded { cardano_stake_key: e030ab1e…, dust_address: 73116136…, utxo_id: d26ca37b…, utxo_index: 0 })",
                "bridge events: 0",
            ]
        );
    }

    #[tokio::test]
    async fn midnight_transaction_block_of_devnet_runtime_1_0_300() {
        let details = decode("devnet-8464", "1.0.300").await.unwrap();
        assert_eq!(
            summary(&details),
            [
                "timestamp Some(1789634874000)",
                "R 3910 6d69646e..70e36038 ApplyExtrinsic(3) fully e0837efd",
                "registrations: 0",
                "bridge events: 0",
            ]
        );
    }

    #[tokio::test]
    async fn inherents_only_block_of_devnet_runtime_2_1_0() {
        let details = decode("devnet-270248", "2.1.0-rc.4").await.unwrap();
        assert_eq!(
            summary(&details),
            [
                "timestamp Some(1791207000000)",
                "registrations: 0",
                "bridge events: 0",
            ]
        );
    }

    /// The hash recorded on chain for a transaction, and the one computed from the bytes at
    /// genesis, is the one the indexer's ledger computes, so the two can be compared.
    #[tokio::test(flavor = "multi_thread")]
    async fn recorded_hashes_equal_the_ledgers_hashes() -> Result<(), BoxError> {
        let _ledger_db = init_ledger_db().await?;

        let mut checked = 0;
        for name in ["devnet-0", "devnet-2", "devnet-8464"] {
            let details = decode(name, "1.0.300").await?;
            for (_, transaction, outcome) in details.transactions {
                use Transaction::*;

                let hash = match transaction {
                    Regular(bytes) => {
                        ledger::Transaction::deserialize(&bytes, LedgerVersion::V8)?.hash()
                    }
                    System(bytes) => {
                        ledger::SystemTransaction::deserialize(&bytes, LedgerVersion::V8)?.hash()
                    }
                };
                assert_eq!(outcome?.tx_hash(), hash, "{name}");
                checked += 1;
            }
        }
        assert_eq!(checked, 28);

        Ok(())
    }

    fn block() -> BlockRef {
        BlockRef {
            hash: [0xb1; 32].into(),
            height: 5,
        }
    }

    fn bytes(n: u8) -> ByteVec {
        vec![n; 8].into()
    }

    fn fully(n: u8) -> Applied {
        Applied::Fully {
            tx_hash: [n; 32].into(),
        }
    }

    fn regular_item(
        index: u32,
        n: u8,
        outcome: Result<Applied, String>,
    ) -> (Phase, Transaction, Result<Applied, String>) {
        (
            Phase::ApplyExtrinsic(index),
            Transaction::Regular(bytes(n)),
            outcome,
        )
    }

    fn system_item(phase: Phase, n: u8) -> (Phase, Transaction, Result<Applied, String>) {
        (phase, Transaction::System(bytes(n)), Ok(fully(n)))
    }

    #[test]
    fn an_outcome_takes_the_regular_transaction_once() {
        let mut extrinsics = BTreeMap::from([(1, Transaction::Regular(bytes(1)))]);

        assert_eq!(
            regular_transaction(
                block(),
                &mut extrinsics,
                Phase::ApplyExtrinsic(1),
                Ok(fully(1))
            ),
            Some(regular_item(1, 1, Ok(fully(1))))
        );
        assert!(extrinsics.is_empty());
    }

    #[test]
    fn a_failure_of_another_extrinsic_is_ignored() {
        let mut extrinsics = BTreeMap::from([(1, Transaction::System(bytes(1)))]);

        for phase in [
            Phase::ApplyExtrinsic(0),
            Phase::ApplyExtrinsic(1),
            Phase::ApplyExtrinsic(5),
            Phase::Finalization,
        ] {
            assert_eq!(
                regular_transaction(block(), &mut extrinsics, phase, Err("failed".into())),
                None
            );
        }
        assert_eq!(
            extrinsics,
            BTreeMap::from([(1, Transaction::System(bytes(1)))])
        );
    }

    #[test]
    #[cfg_attr(
        feature = "divergence-halt",
        should_panic(
            expected = "at height 5: Fully { tx_hash: 01010101… } under ApplyExtrinsic(0), which \
                        has no Midnight transaction"
        )
    )]
    fn an_applied_outcome_with_no_midnight_transaction_is_a_divergence() {
        let mut extrinsics = BTreeMap::new();

        for phase in [
            Phase::ApplyExtrinsic(0),
            Phase::ApplyExtrinsic(5),
            Phase::Initialization,
        ] {
            assert_eq!(
                regular_transaction(block(), &mut extrinsics, phase, Ok(fully(1))),
                None
            );
        }
    }

    #[test]
    #[cfg_attr(
        feature = "divergence-halt",
        should_panic(expected = "at height 5: Midnight extrinsic 1 has no outcome")
    )]
    fn a_midnight_extrinsic_without_an_outcome_is_a_divergence() {
        unaccounted(
            block(),
            BTreeMap::from([(1, Transaction::Regular(bytes(1)))]),
        );
    }

    #[test]
    fn genesis_takes_every_transaction_in_body_order_hashed_from_its_bytes() {
        let transactions = genesis_transactions(BTreeMap::from([
            (2, Transaction::Regular(bytes(2))),
            (1, Transaction::System(bytes(1))),
        ]));
        let hash = |n| <[u8; 32]>::from(Sha256::digest(bytes(n))).into();

        assert_eq!(
            transactions,
            [
                (
                    Phase::ApplyExtrinsic(1),
                    Transaction::System(bytes(1)),
                    Ok(Applied::Fully { tx_hash: hash(1) })
                ),
                (
                    Phase::ApplyExtrinsic(2),
                    Transaction::Regular(bytes(2)),
                    Ok(Applied::Fully { tx_hash: hash(2) })
                ),
            ]
        );
    }

    /// Builds synthetic blocks, with pallet, call and event indices looked up in the metadata so
    /// that the same block can be decoded by every runtime module.
    struct Synthetic(Metadata);

    impl Synthetic {
        /// An unsigned extrinsic `pallet.call(args)`, length-prefixed as in a block body.
        fn extrinsic(&self, pallet: &str, call: &str, args: &[u8]) -> Vec<u8> {
            let pallet = self.0.pallet_by_name(pallet).unwrap();
            let call = pallet
                .call_variants()
                .unwrap()
                .iter()
                .find(|variant| variant.name == call)
                .unwrap()
                .index;
            let body = [&[0x04, pallet.call_index(), call], args].concat();
            [Compact(body.len() as u32).encode(), body].concat()
        }

        fn timestamp(&self) -> Vec<u8> {
            self.extrinsic("Timestamp", "set", &Compact(1_700_000_000_000u64).encode())
        }

        fn midnight(&self, transaction: &[u8]) -> Vec<u8> {
            self.extrinsic(
                "Midnight",
                "send_mn_transaction",
                &transaction.to_vec().encode(),
            )
        }

        fn midnight_system(&self, transaction: &[u8]) -> Vec<u8> {
            self.extrinsic(
                "MidnightSystem",
                "send_mn_system_transaction",
                &transaction.to_vec().encode(),
            )
        }

        /// Stands for any call that is not in scope, like governance.
        fn other(&self) -> Vec<u8> {
            self.extrinsic("System", "remark", &vec![1u8].encode())
        }

        /// An event record under the given phase.
        fn event(&self, phase: Phase, pallet: &str, variant: &str, fields: &[u8]) -> Vec<u8> {
            use Phase::*;

            let pallet = self.0.pallet_by_name(pallet).unwrap();
            let variant = pallet
                .event_variants()
                .unwrap()
                .iter()
                .find(|v| v.name == variant)
                .unwrap()
                .index;
            let phase = match phase {
                ApplyExtrinsic(extrinsic) => [&[0x00][..], &extrinsic.to_le_bytes()].concat(),
                Finalization => vec![0x01],
                Initialization => vec![0x02],
            };
            [&phase[..], &[pallet.call_index(), variant], fields, &[0x00]].concat()
        }

        /// `System::ExtrinsicSuccess` and `System::ExtrinsicFailed` with `CallFiltered`.
        fn success(&self, extrinsic: u32) -> Vec<u8> {
            self.event(
                Phase::ApplyExtrinsic(extrinsic),
                "System",
                "ExtrinsicSuccess",
                &Self::dispatch_info(),
            )
        }

        fn call_filtered(&self, extrinsic: u32) -> Vec<u8> {
            // `DispatchError::Module` of the System pallet (index 0), error 5.
            let dispatch_error = [3, 0, 5, 0, 0, 0];
            self.event(
                Phase::ApplyExtrinsic(extrinsic),
                "System",
                "ExtrinsicFailed",
                &[&dispatch_error[..], &Self::dispatch_info()].concat(),
            )
        }

        fn dispatch_info() -> Vec<u8> {
            // Zero weight, normal class, pays fee.
            [Compact(0u64).encode(), Compact(0u64).encode(), vec![0, 0]].concat()
        }

        fn tx_applied(&self, extrinsic: u32, tx_hash: [u8; 32]) -> Vec<u8> {
            self.event(
                Phase::ApplyExtrinsic(extrinsic),
                "Midnight",
                "TxApplied",
                &tx_hash,
            )
        }

        fn system_transaction_applied(
            &self,
            phase: Phase,
            tx_hash: [u8; 32],
            transaction: &[u8],
        ) -> Vec<u8> {
            let fields = [&tx_hash[..], &transaction.to_vec().encode()].concat();
            self.event(phase, "MidnightSystem", "SystemTransactionApplied", &fields)
        }

        fn events(records: Vec<Vec<u8>>) -> Vec<u8> {
            [Compact(records.len() as u32).encode(), records.concat()].concat()
        }
    }

    const RUNTIMES: [(&str, u32); 4] = [
        ("0.22.0", 22_000),
        ("1.0.300", 1_000_300),
        ("2.0.0-rc.4", 2_000_000),
        ("2.1.0-rc.4", 2_001_000),
    ];

    #[tokio::test]
    async fn failed_midnight_extrinsic_keeps_its_place() {
        for (dir, spec_version) in RUNTIMES {
            let synthetic = Synthetic(metadata(dir));
            let extrinsics = vec![synthetic.timestamp(), synthetic.midnight(&[7; 8])];
            let events = Synthetic::events(vec![synthetic.success(0), synthetic.call_filtered(1)]);

            let details = decode_raw(block(), spec_version, dir, extrinsics, events)
                .await
                .unwrap();

            let [(phase, transaction, Err(error))] = details.transactions.as_slice() else {
                panic!("{dir}: one failed transaction expected");
            };
            assert_eq!(*phase, Phase::ApplyExtrinsic(1), "{dir}");
            assert_eq!(*transaction, Transaction::Regular(bytes(7)), "{dir}");
            assert!(error.starts_with("Module"), "{dir}: {error}");
        }
    }

    // A governance extrinsic runs among the user transactions and builds a system transaction
    // there, so it stays between them.
    #[tokio::test]
    async fn governance_system_transaction_keeps_its_place_among_user_transactions() {
        for (dir, spec_version) in RUNTIMES {
            let synthetic = Synthetic(metadata(dir));
            let extrinsics = vec![
                synthetic.timestamp(),
                synthetic.midnight(&[1; 8]),
                synthetic.other(),
                synthetic.midnight(&[3; 8]),
            ];
            let events = Synthetic::events(vec![
                synthetic.success(0),
                synthetic.tx_applied(1, [1; 32]),
                synthetic.success(1),
                synthetic.system_transaction_applied(Phase::ApplyExtrinsic(2), [2; 32], &[2; 8]),
                synthetic.success(2),
                synthetic.tx_applied(3, [3; 32]),
                synthetic.success(3),
            ]);

            let details = decode_raw(block(), spec_version, dir, extrinsics, events)
                .await
                .unwrap();

            assert_eq!(
                details.transactions,
                [
                    regular_item(1, 1, Ok(fully(1))),
                    system_item(Phase::ApplyExtrinsic(2), 2),
                    regular_item(3, 3, Ok(fully(3))),
                ],
                "{dir}"
            );
        }
    }

    // System transactions keep the phase the chain stamped, unchecked against the body: the step
    // after the inherents is stamped with the next extrinsic's index, here that of the Midnight
    // extrinsic, whose outcome it does not decide, and in an inherents-only block one past the
    // last extrinsic; a future runtime may emit under `Initialization` or `Finalization`. None of
    // it is a divergence, so this also passes with `divergence-halt`.
    #[tokio::test]
    async fn system_transactions_keep_the_phase_the_chain_stamped() {
        use Phase::*;

        for (dir, spec_version) in RUNTIMES {
            let synthetic = Synthetic(metadata(dir));
            let events = Synthetic::events(vec![
                synthetic.system_transaction_applied(Initialization, [1; 32], &[1; 8]),
                synthetic.success(0),
                synthetic.system_transaction_applied(ApplyExtrinsic(1), [2; 32], &[2; 8]),
                synthetic.tx_applied(1, [3; 32]),
                synthetic.success(1),
                synthetic.system_transaction_applied(ApplyExtrinsic(2), [4; 32], &[4; 8]),
                synthetic.system_transaction_applied(Finalization, [5; 32], &[5; 8]),
            ]);
            let extrinsics = vec![synthetic.timestamp(), synthetic.midnight(&[3; 8])];

            let details = decode_raw(block(), spec_version, dir, extrinsics, events)
                .await
                .unwrap();

            assert_eq!(
                details.transactions,
                [
                    system_item(Initialization, 1),
                    system_item(ApplyExtrinsic(1), 2),
                    regular_item(1, 3, Ok(fully(3))),
                    system_item(ApplyExtrinsic(2), 4),
                    system_item(Finalization, 5),
                ],
                "{dir}"
            );
        }
    }

    // A second outcome for the same extrinsic is a divergence; the first is kept.
    #[tokio::test]
    #[cfg_attr(
        feature = "divergence-halt",
        should_panic(expected = "under ApplyExtrinsic(1), which has no Midnight transaction")
    )]
    async fn several_outcomes_for_a_midnight_extrinsic_keep_the_first() {
        for (dir, spec_version) in RUNTIMES {
            let synthetic = Synthetic(metadata(dir));
            let extrinsics = vec![synthetic.timestamp(), synthetic.midnight(&[1; 8])];
            let events = Synthetic::events(vec![
                synthetic.success(0),
                synthetic.tx_applied(1, [1; 32]),
                synthetic.tx_applied(1, [9; 32]),
                synthetic.success(1),
            ]);

            let details = decode_raw(block(), spec_version, dir, extrinsics, events)
                .await
                .unwrap();

            assert_eq!(
                details.transactions,
                [regular_item(1, 1, Ok(fully(1)))],
                "{dir}"
            );
        }
    }

    // A Midnight extrinsic that succeeds without `TxApplied` or `TxPartialSuccess` is a divergence,
    // but is still indexed as applied, with the hash computed from its bytes.
    #[tokio::test]
    #[cfg_attr(
        feature = "divergence-halt",
        should_panic(
            expected = "Midnight extrinsic 1 succeeded without TxApplied or TxPartialSuccess"
        )
    )]
    async fn midnight_extrinsic_succeeding_without_its_event_is_still_applied() {
        for (dir, spec_version) in RUNTIMES {
            let synthetic = Synthetic(metadata(dir));
            let extrinsics = vec![synthetic.timestamp(), synthetic.midnight(&[7; 8])];
            let events = Synthetic::events(vec![synthetic.success(0), synthetic.success(1)]);

            let details = decode_raw(block(), spec_version, dir, extrinsics, events)
                .await
                .unwrap();

            let tx_hash = Transaction::Regular(bytes(7)).hash();
            assert_eq!(
                details.transactions,
                [regular_item(1, 7, Ok(Applied::Fully { tx_hash }))],
                "{dir}"
            );
        }
    }

    // Top-level `MidnightSystem` extrinsics are `Root`-only, so in an executed block one can only
    // have failed, like any other failed call: it is not a transaction and not a divergence, so
    // this also passes with `divergence-halt`.
    #[tokio::test]
    async fn top_level_midnight_system_extrinsic_is_not_a_transaction() {
        for (dir, spec_version) in RUNTIMES {
            let synthetic = Synthetic(metadata(dir));
            let extrinsics = vec![synthetic.timestamp(), synthetic.midnight_system(&[9; 8])];
            let events = Synthetic::events(vec![synthetic.success(0), synthetic.call_filtered(1)]);

            let details = decode_raw(block(), spec_version, dir, extrinsics, events)
                .await
                .unwrap();

            assert!(details.transactions.is_empty(), "{dir}");
        }
    }
}
