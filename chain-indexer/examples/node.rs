use anyhow::Context;
use chain_indexer::{
    domain::Node,
    infra::node::{self, SubxtNode},
};
use futures::{StreamExt, TryStreamExt};
use indexer_common::domain::NetworkId;
use std::pin::pin;

/// This program connects to a local node and prints some first blocks and their transactions.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let node_config = node::Config::default();
    let mut node = SubxtNode::new(node_config)
        .await
        .context("create SubxtNode")?;

    let blocks = node.finalized_blocks(None, NetworkId::Undeployed).take(60);
    let mut blocks = pin!(blocks);
    while let Some(block) = blocks.try_next().await.context("get next block")? {
        println!("## BLOCK: height={}, \thash={}", block.height, block.hash);
        for transaction in block.transactions {
            println!(
                "    ## TRANSACTION: hash={}, \t{transaction:?}",
                transaction.hash
            );
        }
    }

    Ok(())
}
