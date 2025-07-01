// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Example demonstrating DUST event processing with mock data.
//! TEMPORARY: This example uses mock data and will be updated once we have a node image
//! with ledger-5.0.0-alpha.3+ that provides real DUST events.
//!
//! Run with: cargo run --example dust_processing --features cloud

use chain_indexer::domain::{Block, DustEventHandler, Transaction};
use indexer_common::domain::{
    BlockHash, ByteArray, ProtocolVersion, TransactionHash,
    dust::{DustEvent, DustEventDetails, DustGenerationInfo, DustParameters, QualifiedDustOutput},
};
use log::info;

/// Generate mock DUST events for testing.
/// TEMPORARY: Will be replaced with real events from node.
fn generate_mock_dust_events() -> Vec<DustEvent> {
    let tx_hash = TransactionHash([1u8; 32]);
    let owner = ByteArray([10u8; 32]);

    vec![
        // Event 1: Initial DUST UTXO creation.
        DustEvent {
            transaction_hash: tx_hash,
            logical_segment: 0,
            physical_segment: 0,
            event_details: DustEventDetails::DustInitialUtxo {
                output: QualifiedDustOutput {
                    initial_value: 0,
                    owner,
                    nonce: ByteArray([1u8; 32]),
                    seq: 0,
                    ctime: 1000,
                    backing_night: ByteArray([2u8; 32]),
                    mt_index: 0,
                },
                generation: DustGenerationInfo {
                    value: 1000,
                    owner,
                    nonce: ByteArray([3u8; 32]),
                    ctime: 1000,
                    dtime: 0,
                },
                generation_index: 0,
            },
        },
        // Event 2: DUST spend for fees.
        DustEvent {
            transaction_hash: tx_hash,
            logical_segment: 1,
            physical_segment: 1,
            event_details: DustEventDetails::DustSpendProcessed {
                commitment: owner,
                commitment_index: 0,
                nullifier: ByteArray([4u8; 32]),
                v_fee: 50,
                time: 2000,
                params: DustParameters {
                    night_dust_ratio: 10,
                    generation_decay_rate: 3600,
                    dust_grace_period: 300,
                },
            },
        },
    ]
}

/// Create a mock block with DUST events.
/// TEMPORARY: Will be replaced with real blocks from node.
fn create_mock_block_with_dust() -> Block {
    let mut transaction = Transaction {
        id: 1,
        hash: TransactionHash([1u8; 32]),
        transaction_result: Default::default(),
        protocol_version: ProtocolVersion(0x000D0000),
        identifiers: vec![],
        contract_actions: vec![],
        raw: vec![].into(),
        merkle_tree_root: Default::default(),
        start_index: Default::default(),
        end_index: Default::default(),
        created_unshielded_utxos: vec![],
        spent_unshielded_utxos: vec![],
        paid_fees: 50,
        estimated_fees: 50,
        dust_events: generate_mock_dust_events(),
    };

    Block {
        hash: BlockHash([100u8; 32]),
        height: 1000,
        parent_hash: BlockHash([99u8; 32]),
        protocol_version: ProtocolVersion(0x000D0000),
        author: None,
        timestamp: 1000,
        zswap_state_root: Default::default(),
        transactions: vec![transaction],
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    info!("DUST Processing Example");
    info!("========================");

    // Create mock block with DUST events.
    let block = create_mock_block_with_dust();
    info!("Created mock block at height {}", block.height);

    // Log event details.
    for tx in &block.transactions {
        info!(
            "Transaction {} has {} DUST events",
            const_hex::encode(tx.hash.0),
            tx.dust_events.len()
        );

        for (i, event) in tx.dust_events.iter().enumerate() {
            match &event.event_details {
                DustEventDetails::DustInitialUtxo {
                    output, generation, ..
                } => {
                    info!(
                        "  Event {}: Initial DUST UTXO - Night value: {}, Owner: {}",
                        i,
                        generation.value,
                        const_hex::encode(output.owner.0)
                    );
                }
                DustEventDetails::DustSpendProcessed {
                    v_fee, nullifier, ..
                } => {
                    info!(
                        "  Event {}: DUST Spend - Fee: {}, Nullifier: {}",
                        i,
                        v_fee,
                        const_hex::encode(nullifier.0)
                    );
                }
                _ => {}
            }
        }
    }

    info!("\nNote: Full processing requires database storage implementation.");
    info!("This example demonstrates the event structure that will be processed.");

    // Assumes when ledger integration is complete, we would do:
    // let storage = PostgresStorage::new(pool);
    // let handler = DustEventHandler::new(storage, 1000);
    // handler.process_block_dust_events(&block).await?;

    Ok(())
}
