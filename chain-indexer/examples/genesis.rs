use anyhow::Context;
use chain_indexer::{
    domain::node::{self, Node},
    infra::subxt_node::{Config, SubxtNode},
};
use clap::Parser;
use futures::{StreamExt, TryStreamExt};
use indexer_common::domain::{
    BlockHash, NetworkId, PROTOCOL_VERSION_000_016_000,
    ledger::{LedgerState, Transaction},
};
use std::{pin::pin, time::Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Cli::parse().run().await
}

/// This program analyzes the genesis block for transaction types and unshielded tokens.
///
/// Background:
/// - Genesis blocks don't emit UnshieldedTokens events due to Substrate PR #5463.
/// - Genesis UTXO extraction happens during transaction processing.
/// - The indexer aggregates all genesis UTXOs to first regular transaction.
#[derive(Debug, Parser)]
#[command()]
struct Cli {
    /// The node URL; defaults to "ws://localhost:9944".
    #[arg(long, default_value = "ws://localhost:9944")]
    node: String,

    /// Show detailed output for all transactions; defaults to summary only.
    #[arg(long)]
    verbose: bool,
}

impl Cli {
    async fn run(self) -> anyhow::Result<()> {
        let config = Config {
            url: self.node.clone(),
            genesis_protocol_version: PROTOCOL_VERSION_000_016_000,
            reconnect_max_delay: Duration::from_secs(1),
            reconnect_max_attempts: 1,
        };
        let mut node = SubxtNode::new(config).await.context("create SubxtNode")?;

        let blocks = node.finalized_blocks(None).take(1);
        let mut blocks = pin!(blocks);

        if let Some(block) = blocks.try_next().await.context("get genesis block")? {
            self.analyze_genesis_block(block)?;
        } else {
            println!("## ERROR: Failed to retrieve genesis block");
        }

        Ok(())
    }

    fn analyze_genesis_block(&self, block: node::Block) -> anyhow::Result<()> {
        println!("## GENESIS BLOCK ANALYSIS");
        println!("## Block: height={}, hash={}", block.height, block.hash);
        println!("## Parent hash: {}", block.parent_hash);
        println!("## Timestamp: {}", block.timestamp);
        println!("## Total transactions: {}", block.transactions.len());
        println!();

        let mut ledger_state = LedgerState::new(NetworkId::Undeployed);
        let mut regular_count = 0;
        let mut system_count = 0;
        let mut total_utxos = 0;
        let mut utxo_details = Vec::new();

        for (i, transaction) in block.transactions.iter().enumerate() {
            match transaction {
                node::Transaction::Regular(tx) => {
                    regular_count += 1;
                    self.process_regular_transaction(
                        i,
                        tx,
                        &block,
                        &mut ledger_state,
                        &mut total_utxos,
                        &mut utxo_details,
                    )?;
                }

                node::Transaction::System(tx) => {
                    system_count += 1;
                    if self.verbose && i < 5 {
                        println!("    ## [{}] SYSTEM TRANSACTION: hash={}", i, tx.hash);
                    }
                }
            }
        }

        self.print_summary(system_count, regular_count, total_utxos, &utxo_details);
        self.verify_genesis_detection(&block);

        Ok(())
    }

    fn process_regular_transaction(
        &self,
        index: usize,
        tx: &node::RegularTransaction,
        block: &node::Block,
        ledger_state: &mut LedgerState,
        total_utxos: &mut usize,
        utxo_details: &mut Vec<(usize, u128, String, String)>,
    ) -> anyhow::Result<()> {
        let tx_type = match Transaction::deserialize(&tx.raw, tx.protocol_version) {
            Ok(Transaction::V6(tx_v6)) => {
                if format!("{:?}", tx_v6).contains("ClaimRewards") {
                    "ClaimRewards"
                } else {
                    "Standard"
                }
            }
            Err(_) => "Unknown",
        };

        match ledger_state.apply_regular_transaction(&tx.raw, block.parent_hash, block.timestamp) {
            Ok((_result, created_utxos, spent_utxos)) => {
                let should_print = self.verbose
                    || !created_utxos.is_empty()
                    || index < 5
                    || index == block.transactions.len() - 1;

                if should_print {
                    println!(
                        "    ## [{}] REGULAR TRANSACTION ({}): hash={}",
                        index, tx_type, tx.hash
                    );
                    if !created_utxos.is_empty() || !spent_utxos.is_empty() {
                        println!(
                            "        Created UTXOs: {}, Spent UTXOs: {}",
                            created_utxos.len(),
                            spent_utxos.len()
                        );
                    }

                    if !created_utxos.is_empty() {
                        *total_utxos += created_utxos.len();
                        for utxo in &created_utxos {
                            utxo_details.push((
                                index,
                                utxo.value,
                                format!("{:?}", utxo.owner),
                                format!("{:?}", utxo.token_type),
                            ));
                            if self.verbose {
                                println!(
                                    "        UTXO: value={}, owner={:?}",
                                    utxo.value, utxo.owner
                                );
                            }
                        }
                    }
                }
            }
            Err(e) => {
                println!(
                    "    ## [{}] ERROR: Failed to apply transaction: {}",
                    index, e
                );
            }
        }

        Ok(())
    }

    fn print_summary(
        &self,
        system_count: usize,
        regular_count: usize,
        total_utxos: usize,
        utxo_details: &[(usize, u128, String, String)],
    ) {
        println!();
        println!("## SUMMARY");
        println!("    System transactions: {}", system_count);
        println!("    Regular transactions: {}", regular_count);
        println!("    Total unshielded UTXOs found: {}", total_utxos);

        if total_utxos > 0 {
            println!();
            println!("## UTXO DISTRIBUTION");
            let mut current_tx = None;
            for (tx_idx, value, _, _) in utxo_details {
                if current_tx != Some(*tx_idx) {
                    current_tx = Some(*tx_idx);
                    println!("    Transaction {}: {} units", tx_idx, value);
                }
            }

            println!();
            println!("## RESULT: ✅ GENESIS CONTAINS UNSHIELDED TOKENS");
            println!(
                "    Genesis block contains {} unshielded token transactions",
                total_utxos
            );
            println!();
            println!("    Note: The indexer aggregates all genesis UTXOs to the first regular");
            println!("    transaction for compatibility with tests and API expectations.");
        } else {
            println!();
            println!("## RESULT: ⚠️ NO UNSHIELDED TOKENS FOUND IN GENESIS");
        }
    }

    fn verify_genesis_detection(&self, block: &node::Block) {
        println!();
        if block.parent_hash == BlockHash::default() {
            println!("## GENESIS DETECTION: ✅ Properly detected (parent_hash == 0x00...)");
        } else {
            println!("## GENESIS DETECTION: ❌ Issue - parent_hash is not zeros!");
        }
    }
}
