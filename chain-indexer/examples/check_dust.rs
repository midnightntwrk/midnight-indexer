use anyhow::Context;
use chain_indexer::{
    domain::node::{self, Node},
    infra::subxt_node::{Config, SubxtNode},
};
use futures::{StreamExt, TryStreamExt};
use indexer_common::domain::{
    PROTOCOL_VERSION_000_016_000, ledger::SystemTransaction as LedgerSystemTransaction,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config {
        url: "ws://localhost:9944".to_string(),
        genesis_protocol_version: PROTOCOL_VERSION_000_016_000,
        reconnect_max_delay: Duration::from_secs(1),
        reconnect_max_attempts: 1,
    };
    let mut node = SubxtNode::new(config).await.context("create SubxtNode")?;

    let blocks = node.finalized_blocks(None);
    let mut blocks = Box::pin(blocks.take(30)); // Check first 30 blocks to see fee transactions

    let mut cnight_count = 0;
    let mut distribute_reserve_count = 0;
    let mut other_count = 0;
    let mut total_fees_paid = 0u128;
    let mut fee_paying_tx_count = 0;

    while let Some(block) = blocks.try_next().await.context("get next block")? {
        println!(
            "Block {}: {} transactions",
            block.height,
            block.transactions.len()
        );

        for transaction in block.transactions {
            if let node::Transaction::System(sys_tx) = transaction {
                // Try to deserialize the system transaction
                match LedgerSystemTransaction::deserialize(&sys_tx.raw, sys_tx.protocol_version) {
                    Ok(tx) => {
                        // Extract the V6 variant
                        match tx {
                            LedgerSystemTransaction::V6(v6_tx) => {
                                // Now we can use the actual ledger types
                                use midnight_ledger_v6::structure::SystemTransaction as V6SystemTx;
                                match v6_tx {
                                    V6SystemTx::CNightGeneratesDustUpdate { events } => {
                                        cnight_count += 1;
                                        println!(
                                            "  CNightGeneratesDust with {} events",
                                            events.len()
                                        );
                                    }
                                    V6SystemTx::DistributeReserve(amount) => {
                                        distribute_reserve_count += 1;
                                        println!("  DistributeReserve: {} NIGHT", amount);
                                    }
                                    V6SystemTx::OverwriteParameters(_) => {
                                        other_count += 1;
                                        println!("  OverwriteParameters");
                                    }
                                    V6SystemTx::DistributeNight(_, _) => {
                                        other_count += 1;
                                        println!("  DistributeNight");
                                    }
                                    V6SystemTx::PayBlockRewardsToTreasury { .. } => {
                                        other_count += 1;
                                        println!("  PayBlockRewardsToTreasury");
                                    }
                                    V6SystemTx::PayFromTreasuryShielded { .. } => {
                                        other_count += 1;
                                        println!("  PayFromTreasuryShielded");
                                    }
                                    V6SystemTx::PayFromTreasuryUnshielded { .. } => {
                                        other_count += 1;
                                        println!("  PayFromTreasuryUnshielded");
                                    }
                                    _ => {
                                        other_count += 1;
                                        println!("  Unknown system transaction type");
                                    }
                                }
                            }
                        }
                    }
                    Err(error) => {
                        println!("  Failed to deserialize system tx: {}", error);
                    }
                }
            } else if let node::Transaction::Regular(reg_tx) = transaction {
                // Check if regular transaction paid fees
                if reg_tx.paid_fees > 0 {
                    fee_paying_tx_count += 1;
                    total_fees_paid += reg_tx.paid_fees;
                    println!("  Regular tx paid fees: {} DUST", reg_tx.paid_fees);
                }
            }
        }
    }

    println!("\nSummary:");
    println!("  CNightGeneratesDust transactions: {}", cnight_count);
    println!(
        "  DistributeReserve transactions: {}",
        distribute_reserve_count
    );
    println!("  Other system transactions: {}", other_count);
    println!("  Fee-paying regular transactions: {}", fee_paying_tx_count);
    println!("  Total fees paid: {} DUST", total_fees_paid);

    if fee_paying_tx_count > 0 && cnight_count == 0 {
        println!("\nNote: Fees were paid but no CNightGeneratesDust occurred.");
        println!("This is expected without cNIGHT token holders.");
    }

    Ok(())
}
