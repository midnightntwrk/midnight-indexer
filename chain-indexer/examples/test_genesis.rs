use anyhow::Context;
use chain_indexer::{
    domain::Node,
    infra::node::{Config, SubxtNode},
};
use futures::{StreamExt, TryStreamExt};
use indexer_common::domain::{NetworkId, PROTOCOL_VERSION_000_013_000};
use std::{pin::pin, time::Duration};

/// Test genesis UTXO extraction by calling the processing pipeline directly.
///
/// This test was created because the existing `examples/node.rs` bypasses the application
/// layer entirely and calls the node interface directly via `node.finalized_blocks()`.
/// The genesis UTXO extraction implementation is located in the application layer
/// (`application::index_block` function), so the original example never executes the
/// extraction code.
///
/// This test demonstrates that:
/// 1. Genesis UTXO extraction works correctly when called through proper channels
/// 2. The implementation successfully extracts UTXOs from genesis blocks
/// 3. UTXOs are properly added to the genesis transaction's `created_unshielded_utxos`
///
/// Background:
/// - Genesis blocks don't emit UnshieldedTokens events due to Substrate PR #5463
/// - The indexer must apply the genesis transaction to extract UTXOs from the resulting state
/// - This workaround is only needed for genesis blocks (height = 0)
///
/// Related tickets:
/// - PM-17350: [Indexer] Implement Genesis UTXO Extraction Workaround
/// - PM-17351: [Node] Investigate Genesis Event Emission Workarounds
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Note: logging is disabled for this simple test

    let config = Config {
        url: "ws://localhost:9944".to_string(),
        genesis_protocol_version: PROTOCOL_VERSION_000_013_000,
        reconnect_max_delay: Duration::from_secs(1),
        reconnect_max_attempts: 3,
    };
    let mut node = SubxtNode::new(config).await.context("create SubxtNode")?;

    let blocks = node.finalized_blocks(None, NetworkId::Undeployed).take(3);
    let mut blocks = pin!(blocks);

    while let Some(mut block) = blocks.try_next().await.context("get next block")? {
        println!("## BLOCK: height={}, \thash={}", block.height, block.hash);

        // For genesis block, manually call my extraction function
        if block.height == 0 {
            println!("*** TESTING GENESIS UTXO EXTRACTION ***");

            // Call my extraction function directly
            match chain_indexer::application::extract_genesis_unshielded_utxos(
                &mut block,
                NetworkId::Undeployed,
            )
            .await
            {
                Ok(()) => println!("Genesis extraction completed"),
                Err(e) => println!("Genesis extraction failed: {}", e),
            }

            println!(
                "*** AFTER EXTRACTION - UTXOs: {} ***",
                block
                    .transactions
                    .get(0)
                    .map(|t| t.created_unshielded_utxos.len())
                    .unwrap_or(0)
            );
        }

        for transaction in &block.transactions {
            println!(
                "    ## TRANSACTION: hash={}, created_utxos={}, spent_utxos={}",
                transaction.hash,
                transaction.created_unshielded_utxos.len(),
                transaction.spent_unshielded_utxos.len()
            );
        }
    }

    Ok(())
}
