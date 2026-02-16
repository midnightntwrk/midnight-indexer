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
    domain::{
        Block, BlockRef, LedgerState, SystemParametersChange, Transaction,
        node::{self, Node},
        storage::Storage,
    },
};
use anyhow::{Context, bail};
use async_stream::stream;
use fastrace::{Span, future::FutureExt, prelude::SpanContext, trace};
use futures::{Stream, StreamExt, TryStreamExt, future::ok};
use indexer_common::domain::{
    BlockIndexed, NetworkId, ProtocolVersion, Publisher, UnshieldedUtxoIndexed,
};
use log::{debug, info, warn};
use parking_lot::RwLock;
use serde::Deserialize;
use std::{
    collections::HashSet, error::Error as StdError, future::ready, pin::pin, sync::Arc,
    time::Duration,
};
use tokio::{
    select,
    signal::unix::Signal,
    task::{self},
    time::sleep,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub network_id: NetworkId,
    pub blocks_buffer: usize,
    pub caught_up_max_distance: u32,
    pub caught_up_leeway: u32,
}

pub async fn run(
    config: Config,
    node: impl Node,
    mut storage: impl Storage,
    publisher: impl Publisher,
    mut sigterm: Signal,
) -> anyhow::Result<()> {
    let Config {
        network_id,
        blocks_buffer,
        caught_up_max_distance,
        caught_up_leeway,
    } = config;

    // Get info from highest block.
    let (highest_block_ref, highest_protocol_version_and_ledger_state_key) = match storage
        .get_highest_block()
        .await
        .context("get highest block")?
    {
        Some((r, v, k)) => (Some(r), Some((v, k))),
        None => (None, None),
    };

    let highest_block_height = highest_block_ref.map(|info| info.height);
    info!(highest_block_height:?; "starting indexing");

    // Initialize metrics.
    let transaction_count = storage
        .get_transaction_count()
        .await
        .context("get transaction count")?;
    let contract_action_count = storage
        .get_contract_action_count()
        .await
        .context("get contract action count")?;
    let metrics = Metrics::new(
        highest_block_height,
        transaction_count,
        contract_action_count,
    );

    // Load/initialize ledger state.
    let mut ledger_state = match highest_protocol_version_and_ledger_state_key {
        Some((protocol_version, ledger_state_key)) => {
            LedgerState::load(&ledger_state_key, protocol_version).context("load ledger state")?
        }

        None => LedgerState::new(network_id.clone(), ProtocolVersion::OLDEST)
            .context("create ledger state")?,
    };

    let highest_block_on_node = Arc::new(RwLock::new(None));

    // Spawn task to set info for highest block on node.
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

            warn!("highest_block_on_node_task completed");

            Ok::<_, anyhow::Error>(())
        }
    });

    // Spawn task to index blocks.
    let index_blocks_task = task::spawn({
        let node = node.clone();

        async move {
            let blocks = node_blocks(highest_block_ref, node.clone())
                .map(ready)
                .buffered(blocks_buffer);
            let mut blocks = pin!(blocks);
            let mut caught_up = false;
            let mut parent_block_timestamp = 0;

            loop {
                let next_ledger_state = get_and_index_block(
                    caught_up_max_distance,
                    caught_up_leeway,
                    &mut blocks,
                    ledger_state,
                    &highest_block_on_node,
                    &mut caught_up,
                    &mut parent_block_timestamp,
                    &mut storage,
                    &publisher,
                    &metrics,
                    &node,
                )
                .in_span(Span::root("get-and-index-block", SpanContext::random()))
                .await?;

                ledger_state = next_ledger_state;
            }
        }
    });

    // Handle task completion or SIGTERM termination. "Successful" completion of the tasks is
    // unexpected, hence the above `error!` invocations.
    select! {
        result = highest_block_on_node_task => result
            .context("highest_block_on_node_task panicked")
            .and_then(|r| r.context("highest_block_on_node_task failed")),

        result = index_blocks_task => result
            .context("index_blocks_task panicked")
            .and_then(|r: anyhow::Result<()>| r.context("index_blocks_task failed")),

        _ = sigterm.recv() => {
            warn!("SIGTERM received");
            Ok(())
        }
    }
}

/// An infinite stream of node blocks, neither with duplicates, nor with gaps or otherwise
/// unexpected blocks.
fn node_blocks<N>(
    mut highest_block: Option<BlockRef>,
    mut node: N,
) -> impl Stream<Item = Result<node::Block, N::Error>>
where
    N: Node,
{
    stream! {
        loop {
            let blocks = node.finalized_blocks(highest_block);
            let mut blocks = pin!(blocks);

            while let Some(block) = blocks.next().await {
                if let Ok(block) = &block {
                    let parent_hash = block.parent_hash;
                    let (highest_hash, highest_height) = highest_block
                        .map(|BlockRef { hash, height }| (hash, height))
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

                    highest_block = Some(block.into());
                }

                yield block;
            }

            // Sleep to avoid busy-spin.
            sleep(Duration::from_millis(100)).await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[trace]
async fn get_and_index_block<E, N>(
    caught_up_max_distance: u32,
    caught_up_leeway: u32,
    blocks: &mut (impl Stream<Item = Result<node::Block, E>> + Unpin),
    ledger_state: LedgerState,
    highest_block_on_node: &Arc<RwLock<Option<BlockRef>>>,
    caught_up: &mut bool,
    parent_block_timestamp: &mut u64,
    storage: &mut impl Storage,
    publisher: &impl Publisher,
    metrics: &Metrics,
    node: &N,
) -> anyhow::Result<LedgerState>
where
    E: StdError + Send + Sync + 'static,
    N: Node,
{
    let block = get_next_block(blocks).await?;

    let ledger_state = index_block(
        caught_up_max_distance,
        caught_up_leeway,
        block,
        ledger_state,
        highest_block_on_node,
        caught_up,
        parent_block_timestamp,
        storage,
        publisher,
        metrics,
        node,
    )
    .await?;

    Ok(ledger_state)
}

#[trace]
async fn get_next_block<E>(
    blocks: &mut (impl Stream<Item = Result<node::Block, E>> + Unpin),
) -> anyhow::Result<node::Block>
where
    E: StdError + Send + Sync + 'static,
{
    blocks
        .try_next()
        .await
        .context("get next block from node")
        .and_then(|o| o.context("no more block from node"))
}

#[allow(clippy::too_many_arguments)]
#[trace]
async fn index_block<N>(
    caught_up_max_distance: u32,
    caught_up_leeway: u32,
    block: node::Block,
    mut ledger_state: LedgerState,
    highest_block_on_node: &Arc<RwLock<Option<BlockRef>>>,
    caught_up: &mut bool,
    parent_block_timestamp: &mut u64,
    storage: &mut impl Storage,
    publisher: &impl Publisher,
    metrics: &Metrics,
    node: &N,
) -> anyhow::Result<LedgerState>
where
    N: Node,
{
    let (mut block, transactions) = block.into();

    let ledger_version = block.protocol_version.ledger_version()?;
    ledger_state = ledger_state
        .translate(ledger_version)
        .context("translate ledger state")?;

    if *parent_block_timestamp == 0 {
        *parent_block_timestamp = block.timestamp;
    };

    let apply_transactions = |ledger_state: &mut LedgerState| {
        ledger_state
            .apply_transactions(
                transactions,
                block.parent_hash,
                block.timestamp,
                *parent_block_timestamp,
            )
            .context("apply transactions to ledger state")
    };

    // Apply transactions to ledger state with special handling for genesis block.
    let (transactions, ledger_parameters) = if block.height == 0 {
        // At genesis compare ledger state roots of genesis and block from node to detect whether
        // genesis already includes transactions (post-block-0) or not (pre-block-0).

        let genesis_ledger_state = node
            .fetch_genesis_ledger_state()
            .await
            .context("fetch genesis ledger state")?;
        let genesis_ledger_state =
            LedgerState::from_genesis(genesis_ledger_state, block.protocol_version)
                .context("create ledger state from genesis")?;
        let genesis_ledger_state_root = genesis_ledger_state
            .root()
            .context("compute genesis ledger state root")?;

        if let Some(ledger_state_root) = block.ledger_state_root.as_ref() {
            if *ledger_state_root == genesis_ledger_state_root {
                info!("post-block-0: applying transactions to fresh state, then use genesis state");

                let transactions_ledger_parameters = apply_transactions(&mut ledger_state)?;
                ledger_state = genesis_ledger_state;

                transactions_ledger_parameters
            } else {
                info!("pre-block-0: applying transactions to genesis state");

                ledger_state = genesis_ledger_state;
                apply_transactions(&mut ledger_state)?
            }
        } else {
            // TODO: Remove once support for Node < 0.22 is dropped!
            // Pre Node 0.22: no ledger_state_root RPC! Ignore genesis state.
            apply_transactions(&mut ledger_state)?
        }
    } else {
        // All other blocks, i.e. height > 0.
        apply_transactions(&mut ledger_state)?
    };
    debug!(transactions:?; "transactions applied to ledger state");

    *parent_block_timestamp = block.timestamp;
    block.ledger_parameters = ledger_parameters.serialize()?;

    // Validate ledger state.
    if ledger_state.zswap_merkle_tree_root() != block.zswap_state_root {
        bail!(
            "zswap state root mismatch for block {} at height {}",
            block.hash,
            block.height
        );
    }

    // Determine whether caught up, also allowing to fall back a little in that state.
    // Use saturating subtraction to handle the case where streams are temporarily out of order.
    // The two subscriptions (highest_blocks and finalized_blocks) are independent with no
    // ordering guarantee, so node_block_height < block.height may happen under certain rare
    // conditions. This will produce 0 when node_block_height < block.height, treating it as
    // caught up.
    // Using u32::MAX when node_block_height initially is None obviously results in "not caught up"
    // and hence prevents from prematurely signaling readiness.
    let node_block_height = highest_block_on_node
        .read()
        .map(|BlockRef { height, .. }| height)
        .unwrap_or(u32::MAX);
    let distance = node_block_height.saturating_sub(block.height);
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

    // Persist ledger state.
    let (new_ledger_state, ledger_state_key) =
        ledger_state.0.persist().context("persist ledger state")?;
    ledger_state = new_ledger_state.into();

    // Save and update the block with its related data.
    let max_transaction_id = storage
        .save_block(
            &block,
            &transactions,
            &block.dust_registration_events,
            &ledger_state_key,
        )
        .await
        .context("save block")?;

    // Fetch and store system parameters if changed.
    update_system_parameters(&block, storage, node)
        .await
        .context("update system parameters")?;

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
    let addresses = transactions
        .iter()
        .flat_map(|transaction| match transaction {
            Transaction::Regular(transaction) => transaction
                .created_unshielded_utxos
                .iter()
                .chain(transaction.spent_unshielded_utxos.iter()),

            Transaction::System(transaction) => {
                transaction.created_unshielded_utxos.iter().chain(&[])
            }
        })
        .map(|utxo| utxo.owner)
        .collect::<HashSet<_>>();
    for address in addresses {
        publisher
            .publish(&UnshieldedUtxoIndexed { address })
            .await
            .context("publish UnshieldedUtxoIndexed event")?;
    }

    // Update metrics.
    metrics.update(&block, &transactions, node_block_height, *caught_up);

    info!(
        hash:% = block.hash,
        height = block.height,
        parent_hash:% = block.parent_hash,
        protocol_version:% = block.protocol_version,
        distance,
        caught_up = *caught_up;
        "block indexed"
    );

    Ok(ledger_state)
}

/// Fetch system parameters from the node and store if changed.
#[trace]
async fn update_system_parameters<N>(
    block: &Block,
    storage: &mut impl Storage,
    node: &N,
) -> anyhow::Result<()>
where
    N: Node,
{
    // Fetch current system parameters from the node.
    let current = node
        .fetch_system_parameters(
            block.hash,
            block.height,
            block.timestamp,
            block.protocol_version,
        )
        .await
        .map_err(|error| anyhow::anyhow!("fetch system parameters: {error}"))?;

    // Get the latest stored parameters.
    let stored_d_param = storage
        .get_latest_d_parameter()
        .await
        .context("get latest D-parameter")?;
    let stored_tc = storage
        .get_latest_terms_and_conditions()
        .await
        .context("get latest terms and conditions")?;

    // Determine what has changed.
    let d_param_changed = current.d_parameter.as_ref().is_some_and(|current_d| {
        stored_d_param.as_ref().is_none_or(|stored_d| {
            current_d.num_permissioned_candidates != stored_d.num_permissioned_candidates
                || current_d.num_registered_candidates != stored_d.num_registered_candidates
        })
    });

    let tc_changed = match (&current.terms_and_conditions, &stored_tc) {
        (Some(current_tc), Some(stored_tc)) => {
            current_tc.hash != stored_tc.hash || current_tc.url != stored_tc.url
        }
        (Some(_), None) => true,  // New T&C where none existed.
        (None, Some(_)) => false, // T&C removed - don't record this as a change.
        (None, None) => false,
    };

    // Store changes if any.
    if d_param_changed || tc_changed {
        let change = SystemParametersChange {
            block_height: block.height,
            block_hash: block.hash,
            timestamp: block.timestamp,
            d_parameter: if d_param_changed {
                current.d_parameter
            } else {
                None
            },
            terms_and_conditions: if tc_changed {
                current.terms_and_conditions
            } else {
                None
            },
        };

        storage
            .save_system_parameters_change(&change)
            .await
            .context("save system parameters change")?;

        debug!(
            block_height = block.height,
            d_param_changed,
            tc_changed;
            "system parameters updated"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        application::node_blocks,
        domain::{
            BlockRef, SystemParametersChange,
            node::{self, Node},
        },
    };
    use fake::{Fake, Faker};
    use futures::{Stream, StreamExt, TryStreamExt, stream};
    use indexer_common::{
        domain::{BlockHash, ByteArray, ByteVec, ProtocolVersion, ledger::ZswapStateRoot},
        error::BoxError,
    };
    use std::{convert::Infallible, sync::LazyLock};

    #[tokio::test]
    async fn test_blocks() -> Result<(), BoxError> {
        let blocks = node_blocks(None, MockNode);
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
        ) -> Result<impl Stream<Item = Result<BlockRef, Self::Error>>, Self::Error> {
            Ok(stream::empty())
        }

        fn finalized_blocks(
            &mut self,
            _highest_block: Option<BlockRef>,
        ) -> impl Stream<Item = Result<node::Block, Self::Error>> {
            stream::iter([&*BLOCK_0, &*BLOCK_1, &*BLOCK_2, &*BLOCK_3])
                .map(|block| Ok(block.to_owned()))
        }

        async fn fetch_system_parameters(
            &self,
            block_hash: BlockHash,
            block_height: u32,
            timestamp: u64,
            _protocol_version: ProtocolVersion,
        ) -> Result<SystemParametersChange, Self::Error> {
            Ok(SystemParametersChange {
                block_height,
                block_hash,
                timestamp,
                d_parameter: None,
                terms_and_conditions: None,
            })
        }

        async fn fetch_genesis_ledger_state(&self) -> Result<ByteVec, Self::Error> {
            Ok(Default::default())
        }
    }

    static BLOCK_0: LazyLock<node::Block> = LazyLock::new(|| node::Block {
        hash: BLOCK_0_HASH,
        height: 0,
        protocol_version: PROTOCOL_VERSION,
        parent_hash: ZERO_HASH,
        author: Default::default(),
        timestamp: Default::default(),
        zswap_state_root: ZswapStateRoot::V7(Faker.fake()),
        ledger_state_root: None,
        transactions: Default::default(),
        dust_registration_events: Default::default(),
    });

    static BLOCK_1: LazyLock<node::Block> = LazyLock::new(|| node::Block {
        hash: BLOCK_1_HASH,
        height: 1,
        protocol_version: PROTOCOL_VERSION,
        parent_hash: BLOCK_0_HASH,
        author: Default::default(),
        timestamp: Default::default(),
        zswap_state_root: ZswapStateRoot::V7(Faker.fake()),
        ledger_state_root: None,
        transactions: Default::default(),
        dust_registration_events: Default::default(),
    });

    static BLOCK_2: LazyLock<node::Block> = LazyLock::new(|| node::Block {
        hash: BLOCK_2_HASH,
        height: 2,
        protocol_version: PROTOCOL_VERSION,
        parent_hash: BLOCK_1_HASH,
        author: Default::default(),
        timestamp: Default::default(),
        zswap_state_root: ZswapStateRoot::V7(Faker.fake()),
        ledger_state_root: None,
        transactions: Default::default(),
        dust_registration_events: Default::default(),
    });

    static BLOCK_3: LazyLock<node::Block> = LazyLock::new(|| node::Block {
        hash: BLOCK_3_HASH,
        height: 3,
        protocol_version: PROTOCOL_VERSION,
        parent_hash: BLOCK_2_HASH,
        author: Default::default(),
        timestamp: Default::default(),
        zswap_state_root: ZswapStateRoot::V7(Faker.fake()),
        ledger_state_root: None,
        transactions: Default::default(),
        dust_registration_events: Default::default(),
    });

    pub const ZERO_HASH: BlockHash = ByteArray([0; 32]);

    pub const BLOCK_0_HASH: BlockHash = ByteArray([1; 32]);
    pub const BLOCK_1_HASH: BlockHash = ByteArray([2; 32]);
    pub const BLOCK_2_HASH: BlockHash = ByteArray([3; 32]);
    pub const BLOCK_3_HASH: BlockHash = ByteArray([3; 32]);

    pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(1_000);
}
