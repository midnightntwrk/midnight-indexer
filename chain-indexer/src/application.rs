// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
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

mod metrics;

use crate::{
    application::metrics::Metrics,
    domain::{Block, BlockInfo, LedgerState, Node, storage::Storage},
};
use anyhow::{Context, bail};
use async_stream::stream;
use byte_unit::{Byte, UnitType};
use fastrace::{Span, future::FutureExt, prelude::SpanContext, trace};
use futures::{Stream, StreamExt, TryStreamExt, future::ok};
use indexer_common::domain::{
    BlockIndexed, LedgerStateStorage, NetworkId, ProtocolVersion, Publisher, UnshieldedUtxoIndexed,
    ledger,
};
use log::{info, warn};
use parking_lot::RwLock;
use serde::Deserialize;
use std::{collections::HashSet, error::Error as StdError, future::ready, pin::pin, sync::Arc};
use tokio::{
    select,
    signal::unix::Signal,
    task::{self},
};

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Config {
    pub network_id: NetworkId,
    pub blocks_buffer: usize,
    pub save_ledger_state_after: u32,
    pub caught_up_max_distance: u32,
    pub caught_up_leeway: u32,
}

pub async fn run(
    config: Config,
    node: impl Node,
    storage: impl Storage,
    mut ledger_state_storage: impl LedgerStateStorage,
    publisher: impl Publisher,
    mut sigterm: Signal,
) -> anyhow::Result<()> {
    let network_id = config.network_id;

    let highest_block = storage
        .get_highest_block_info()
        .await
        .context("get highest block")?;
    let highest_height = highest_block.map(|b| b.height);
    info!(highest_height:?; "starting indexing");

    let transaction_count = storage
        .get_transaction_count()
        .await
        .context("get transaction count")?;

    let contract_action_count = storage
        .get_contract_action_count()
        .await
        .context("get contract action count")?;

    let metrics = Metrics::new(highest_height, transaction_count, contract_action_count);

    let (mut ledger_state, mut ledger_state_block_height) = ledger_state_storage
        .load_ledger_state()
        .await
        .context("load ledger state")?
        .map(|(ledger_state, block_height, protocol_version)| {
            let ledger_state =
                ledger::LedgerState::deserialize(&ledger_state, network_id, protocol_version)
                    .context("deserialize ledger state")?;
            Ok::<_, anyhow::Error>((ledger_state.into(), Some(block_height)))
        })
        .transpose()?
        .unwrap_or_default();

    // Reset ledger state if storage is behind ledger state storage.
    if ledger_state_block_height > highest_height {
        ledger_state_block_height = None;
        ledger_state = LedgerState::default();
    }

    // Apply the transactions to the ledger state from the saved ledger state height (exclusively,
    // +1) to the highest saved block height (inclusively); also save the ledger state
    // thereafter.
    if let Some(highest_height) = highest_height {
        let ledger_state_block_height = ledger_state_block_height.unwrap_or_default();

        if ledger_state_block_height < highest_height {
            info!(ledger_state_block_height, highest_height; "updating ledger state");

            let mut protocol_version = ProtocolVersion::default();

            for block_height in (ledger_state_block_height + 1)..=highest_height {
                let block_transactions = storage
                    .get_block_transactions(block_height)
                    .await
                    .context("get block transactions")?;

                if block_height == highest_height {
                    protocol_version = block_transactions.protocol_version;
                }

                ledger_state
                    .apply_raw_transactions(
                        block_transactions.transactions.iter(),
                        block_transactions.block_parent_hash,
                        block_transactions.block_timestamp,
                        network_id,
                    )
                    .with_context(|| {
                        format!("apply transactions for block at height {block_height}")
                    })?;
            }

            let raw_ledger_state = ledger_state
                .serialize(network_id)
                .context("serialize ledger state")?;
            ledger_state_storage
                .save(
                    &raw_ledger_state,
                    highest_height,
                    ledger_state.highest_zswap_state_index(),
                    protocol_version,
                )
                .await
                .context("save ledger state")?;
        }
    }

    let highest_block_on_node = Arc::new(RwLock::new(None));

    let highest_block_on_node_task = task::spawn({
        let node = node.clone();
        let highest_block_on_node = highest_block_on_node.clone();

        async move {
            let highest_blocks = node
                .highest_blocks()
                .await
                .context("get stream of highest blocks")?;

            highest_blocks
                .try_for_each(|block_info| {
                    info!(
                        hash:% = block_info.hash,
                        height = block_info.height;
                        "highest finalized block on node"
                    );

                    *highest_block_on_node.write() = Some(block_info);

                    ok(())
                })
                .await
                .context("get next block of highest_blocks")?;

            Ok::<_, anyhow::Error>(())
        }
    });

    let index_blocks_task = task::spawn(async move {
        let blocks = blocks(highest_block, config.network_id, node)
            .map(ready)
            .buffered(config.blocks_buffer);
        let mut blocks = pin!(blocks);
        let mut caught_up = false;

        while let Some(next_ledger_state) = get_and_index_block(
            config,
            &mut blocks,
            ledger_state,
            &highest_block_on_node,
            &mut caught_up,
            &storage,
            &mut ledger_state_storage,
            &publisher,
            &metrics,
        )
        .in_span(Span::root("get-and-index-block", SpanContext::random()))
        .await?
        {
            ledger_state = next_ledger_state
        }

        Ok::<_, anyhow::Error>(())
    });

    select! {
        result = highest_block_on_node_task => result
            .context("highest_block_on_node_task panicked")
            .and_then(|r| r.context("highest_block_on_node_task failed")),

        result = index_blocks_task => result
            .context("index_blocks_task panicked")
            .and_then(|r| r.context("index_blocks_task failed")),

        _ = sigterm.recv() => {
            warn!("SIGTERM received");
            Ok(())
        }
    }
}

/// An infinite stream of [Block]s, neither with duplicates, nor with gaps or otherwise unexpected
/// blocks.
fn blocks<N>(
    mut highest_block: Option<BlockInfo>,
    network_id: NetworkId,
    mut node: N,
) -> impl Stream<Item = Result<Block, N::Error>>
where
    N: Node,
{
    stream! {
        loop {
            let blocks = node.finalized_blocks(highest_block, network_id);
            let mut blocks = pin!(blocks);

            while let Some(block) = blocks.next().await {
                if let Ok(block) = &block {
                    let parent_hash = block.parent_hash;
                    let (highest_hash, highest_height) = highest_block
                        .map(|BlockInfo { hash, height }| (hash, height))
                        .unzip();

                    // In case of unexpected blocks, e.g. because of a gap or the node lagging
                    // behind, break and rerun the `finalized_blocks` stream.
                    if parent_hash != highest_hash.unwrap_or_default() {
                        warn!(
                            parent_hash:%,
                            height = block.height,
                            highest_hash:?,
                            highest_height:?;
                            "unexpected block"
                        );
                        break;
                    }

                    assert_eq!(
                        block.height,
                        highest_height.map(|h| h + 1).unwrap_or_default()
                    );

                    highest_block = Some(block.into());
                }

                yield block;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn get_and_index_block<E>(
    config: Config,
    blocks: &mut (impl Stream<Item = Result<Block, E>> + Unpin),
    ledger_state: LedgerState,
    highest_block_on_node: &Arc<RwLock<Option<BlockInfo>>>,
    caught_up: &mut bool,
    storage: &impl Storage,
    ledger_state_storage: &mut impl LedgerStateStorage,
    publisher: &impl Publisher,
    metrics: &Metrics,
) -> Result<Option<LedgerState>, anyhow::Error>
where
    E: StdError + Send + Sync + 'static,
{
    let block = get_next_block(blocks)
        .await
        .context("get next block for indexing")?;

    match block {
        Some(block) => {
            let ledger_state = index_block(
                config,
                block,
                ledger_state,
                highest_block_on_node,
                caught_up,
                storage,
                ledger_state_storage,
                publisher,
                metrics,
            )
            .await?;

            Ok(Some(ledger_state))
        }

        None => Ok(None),
    }
}

#[trace]
async fn get_next_block<E>(
    blocks: &mut (impl Stream<Item = Result<Block, E>> + Unpin),
) -> Result<Option<Block>, E> {
    blocks.try_next().await
}

#[allow(clippy::too_many_arguments)]
#[trace]
async fn index_block(
    config: Config,
    mut block: Block,
    mut ledger_state: LedgerState,
    highest_block_on_node: &Arc<RwLock<Option<BlockInfo>>>,
    caught_up: &mut bool,
    storage: &impl Storage,
    ledger_state_storage: &mut impl LedgerStateStorage,
    publisher: &impl Publisher,
    metrics: &Metrics,
) -> Result<LedgerState, anyhow::Error> {
    let Config {
        network_id,
        save_ledger_state_after,
        caught_up_max_distance,
        caught_up_leeway,
        ..
    } = config;

    let transactions = block.transactions.iter_mut();
    ledger_state
        .apply_and_update_transactions(transactions, block.parent_hash, block.timestamp, network_id)
        .context("apply and update transactions")?;

    if ledger_state.zswap_merkle_tree_root() != block.zswap_state_root {
        bail!(
            "zswap state root mismatch for block {} at height {}",
            block.hash,
            block.height
        );
    }

    let raw_ledger_state = ledger_state
        .serialize(network_id)
        .context("serialize ledger state")?;

    // Determine whether caught up, also allowing to fall back a little in that state.
    let node_block_height = highest_block_on_node
        .read()
        .map(|BlockInfo { height, .. }| height)
        .unwrap_or_default();
    assert!(node_block_height >= block.height);

    let distance = node_block_height - block.height;
    let max_distance = if *caught_up {
        caught_up_max_distance + caught_up_leeway
    } else {
        caught_up_max_distance
    };

    let old_caught_up = *caught_up;
    *caught_up = distance <= max_distance;
    if old_caught_up != *caught_up {
        info!(caught_up:%; "caught-up status changed")
    }

    // First save and update the block.
    let max_transaction_id = storage.save_block(&mut block).await.context("save block")?;

    // Then save the ledger state. This order is important to maintain consistency.
    if *caught_up || block.height % save_ledger_state_after == 0 {
        ledger_state_storage
            .save(
                &raw_ledger_state,
                block.height,
                ledger_state.highest_zswap_state_index(),
                block.protocol_version,
            )
            .await
            .context("save ledger state")?;
    }

    info!(
        hash:% = block.hash,
        height = block.height,
        parent_hash:% = block.parent_hash,
        protocol_version:% = block.protocol_version,
        distance,
        caught_up = *caught_up,
        ledger_state_size = format_bytes(raw_ledger_state.as_ref().len());
        "block indexed"
    );

    metrics.update(&block, &raw_ledger_state, node_block_height, *caught_up);

    // Publish BlockIndexed.
    publisher
        .publish(&BlockIndexed {
            height: block.height,
            max_transaction_id,
            caught_up: *caught_up,
        })
        .await
        .context("publish BlockIndexed event")?;

    // Publish UnshieldedUtxoIndexed events for affected addresses.
    for transaction in &block.transactions {
        let mut published_addresses = HashSet::new();

        // For created UTXOs
        for utxo in &transaction.created_unshielded_utxos {
            let address = utxo.owner.to_owned();

            if published_addresses.insert(address) {
                publisher
                    .publish(&UnshieldedUtxoIndexed { address })
                    .await
                    .context("publish UnshieldedUtxoIndexed for created")?;
            }
        }

        // For spent UTXOs
        for utxo in &transaction.spent_unshielded_utxos {
            let address = utxo.owner.to_owned();

            if published_addresses.insert(address) {
                publisher
                    .publish(&UnshieldedUtxoIndexed { address })
                    .await
                    .context("publish UnshieldedUtxoIndexed for spent")?;
            }
        }
    }

    Ok(ledger_state)
}

fn format_bytes(value: impl Into<Byte>) -> String {
    let bytes = value.into().get_appropriate_unit(UnitType::Binary);

    let value = bytes.get_value();
    let unit = bytes.get_unit();

    format!("{value:.3} {unit}")
}

#[cfg(test)]
mod tests {
    use crate::{
        application::blocks,
        domain::{Block, BlockInfo, Node},
    };
    use fake::{Fake, Faker};
    use futures::{Stream, StreamExt, TryStreamExt, stream};
    use indexer_common::{
        domain::{BlockHash, ByteArray, NetworkId, ProtocolVersion, ledger::ZswapStateRoot},
        error::BoxError,
    };
    use std::{convert::Infallible, sync::LazyLock};

    #[tokio::test]
    async fn test_blocks() -> Result<(), BoxError> {
        let blocks = blocks(None, NetworkId::Undeployed, MockNode);
        let heights = blocks
            .take(4)
            .map_ok(|block| block.height)
            .try_collect::<Vec<_>>()
            .await?;
        assert_eq!(heights, vec![0, 1, 2, 3]);

        Ok(())
    }

    #[derive(Clone)]
    struct MockNode;

    impl Node for MockNode {
        type Error = Infallible;

        async fn highest_blocks(
            &self,
        ) -> Result<impl Stream<Item = Result<BlockInfo, Self::Error>>, Self::Error> {
            Ok(stream::empty())
        }

        fn finalized_blocks(
            &mut self,
            _highest_block: Option<BlockInfo>,
            _network_id: NetworkId,
        ) -> impl Stream<Item = Result<Block, Self::Error>> {
            stream::iter([&*BLOCK_0, &*BLOCK_1, &*BLOCK_2, &*BLOCK_3])
                .map(|block| Ok(block.to_owned()))
        }
    }

    static BLOCK_0: LazyLock<Block> = LazyLock::new(|| Block {
        hash: BLOCK_0_HASH,
        height: 0,
        protocol_version: PROTOCOL_VERSION,
        parent_hash: ZERO_HASH,
        author: Default::default(),
        timestamp: Default::default(),
        zswap_state_root: ZswapStateRoot::V5(Faker.fake()),
        transactions: Default::default(),
    });

    static BLOCK_1: LazyLock<Block> = LazyLock::new(|| Block {
        hash: BLOCK_1_HASH,
        height: 1,
        protocol_version: PROTOCOL_VERSION,
        parent_hash: BLOCK_0_HASH,
        author: Default::default(),
        timestamp: Default::default(),
        zswap_state_root: ZswapStateRoot::V5(Faker.fake()),
        transactions: Default::default(),
    });

    static BLOCK_2: LazyLock<Block> = LazyLock::new(|| Block {
        hash: BLOCK_2_HASH,
        height: 2,
        protocol_version: PROTOCOL_VERSION,
        parent_hash: BLOCK_1_HASH,
        author: Default::default(),
        timestamp: Default::default(),
        zswap_state_root: ZswapStateRoot::V5(Faker.fake()),
        transactions: Default::default(),
    });

    static BLOCK_3: LazyLock<Block> = LazyLock::new(|| Block {
        hash: BLOCK_3_HASH,
        height: 3,
        protocol_version: PROTOCOL_VERSION,
        parent_hash: BLOCK_2_HASH,
        author: Default::default(),
        timestamp: Default::default(),
        zswap_state_root: ZswapStateRoot::V5(Faker.fake()),
        transactions: Default::default(),
    });

    pub const ZERO_HASH: BlockHash = ByteArray([0; 32]);

    pub const BLOCK_0_HASH: BlockHash = ByteArray([1; 32]);
    pub const BLOCK_1_HASH: BlockHash = ByteArray([2; 32]);
    pub const BLOCK_2_HASH: BlockHash = ByteArray([3; 32]);
    pub const BLOCK_3_HASH: BlockHash = ByteArray([3; 32]);

    pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(1_000);
}
