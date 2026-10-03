use anyhow::Context;
use chain_indexer::{
    domain::BlockRef,
    infra::subxt_node::Config,
    pipeline::{self, decode::CpuPool, sourcing::Source},
};
use clap::Parser;
use futures::TryStreamExt;
use indexer_common::domain::BlockNumber;
use std::{fs, num::NonZeroUsize, path::PathBuf, pin::pin, sync::Arc, time::Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Cli::parse().run().await
}

/// This program sources and decodes blocks from a node and prints them with their transactions.
#[derive(Debug, Parser)]
#[command()]
struct Cli {
    /// The node URL; defaults to "ws://localhost:9944".
    #[arg(long, default_value = "ws://localhost:9944")]
    node: String,
    /// The height of the first block; genesis if omitted.
    #[arg(long)]
    from: Option<BlockNumber>,
    /// How many blocks to print; unlimited if omitted.
    #[arg(long)]
    count: Option<BlockNumber>,
    /// A directory to write each transaction's bytes to, as `<height>-<index>-<kind>.raw`.
    #[arg(long)]
    save_transactions: Option<PathBuf>,
}

impl Cli {
    async fn run(self) -> anyhow::Result<()> {
        let config = Config {
            url: self.node,
            reconnect_max_delay: Duration::from_secs(1),
            reconnect_max_attempts: 1,
            subscription_recovery_timeout: Duration::from_secs(30),
            source_chunk_size: NonZeroUsize::new(64).unwrap(),
            source_chunks_ahead: NonZeroUsize::new(8).unwrap(),
            rpc_batch_size: NonZeroUsize::new(64).unwrap(),
            rpc_batches_in_flight: NonZeroUsize::new(16).unwrap(),
        };
        let source = Source::connect(&config.url, (&config).into())
            .await
            .context("connect block source")?;
        let pool = Arc::new(CpuPool::new(NonZeroUsize::MIN).context("create decode pool")?);

        let from = self.from.unwrap_or(0);
        let start = match from {
            0 => None,
            from => {
                let hash = pipeline::sourcing::resolve(source.rpc(), from - 1..=from - 1)
                    .await
                    .context("resolve the block before the first")?
                    .pop()
                    .flatten()
                    .context("one block at the height before the first")?;
                Some(BlockRef {
                    hash,
                    height: (from - 1).into(),
                })
            }
        };
        let end = self.count.map(|count| from + count - 1);

        let (blocks, _) = pipeline::finalized_blocks(&source, pool, start, end);
        let mut blocks = pin!(blocks);
        while let Some(block) = blocks.try_next().await.context("get next block")? {
            println!(
                "## BLOCK: height={}, hash={}, protocol version={}, author={:?}",
                block.height,
                block.hash,
                u32::from(block.protocol_version),
                block.author
            );
            for (index, (hash, transaction)) in block.transactions.iter().enumerate() {
                let kind = if transaction.is_system() {
                    "system"
                } else {
                    "regular"
                };
                let bytes = transaction.bytes();
                println!("\t## {kind} transaction {hash}, {} bytes", bytes.len());
                if let Some(dir) = &self.save_transactions {
                    let file = dir.join(format!("{}-{index}-{kind}.raw", block.height));
                    fs::write(&file, bytes).with_context(|| format!("write {}", file.display()))?;
                }
            }
        }

        Ok(())
    }
}
