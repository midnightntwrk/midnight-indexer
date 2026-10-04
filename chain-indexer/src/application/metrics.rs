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

use crate::domain::{Block, ContractAction, Transaction};
use indexer_common::domain::ContractAttributes;
use metrics::{Counter, Gauge, Histogram, counter, gauge, histogram};
use std::time::Duration;

pub struct Metrics {
    /// The height of the last indexed block.
    block_height: Counter,
    /// The height of the highest block on the node.
    node_block_height: Counter,
    /// 1 when indexing is within the caught-up distance of the node, else 0.
    caught_up: Gauge,
    /// Transactions indexed.
    transaction_count: Counter,
    /// Contract deploys indexed.
    contract_deploy_count: Counter,
    /// Contract calls indexed.
    contract_call_count: Counter,
    /// Contract maintenance updates indexed.
    contract_update_count: Counter,
    /// Ledger arena garbage collection passes.
    gc_run_count: Counter,
    /// Ledger arena nodes removed by garbage collection.
    gc_culled_node_count: Counter,
    /// Duration of each garbage collection pass.
    gc_duration_seconds: Histogram,
    /// Ledger arena roots after the last garbage collection pass.
    gc_root_count: Gauge,
    /// Rows in `ledger_db_nodes`: the size of the ledger arena.
    arena_node_count: Gauge,
    /// Contract addresses whose state could not be captured from the ledger state.
    uncaptured_contract_state_count: Counter,
    /// Time waiting for the next block to index.
    index_wait_duration_seconds: Histogram,
    /// Time converting a block into its domain types.
    index_convert_duration_seconds: Histogram,
    /// Time applying a block to the ledger state and validating its roots.
    index_ledger_update_duration_seconds: Histogram,
    /// Time persisting the ledger state.
    index_ledger_persist_duration_seconds: Histogram,
    /// Time determining a block's system parameter changes.
    index_system_parameters_duration_seconds: Histogram,
    /// Time saving a block to storage.
    index_storage_duration_seconds: Histogram,
    /// Time publishing a block's events.
    index_publish_duration_seconds: Histogram,
    /// Time indexing a block, from conversion to publishing.
    index_block_duration_seconds: Histogram,
}

impl Metrics {
    pub fn new(
        block_height: Option<u64>,
        transaction_count: u64,
        (contract_deploy_count, contract_call_count, contract_update_count): (u64, u64, u64),
    ) -> Self {
        let metrics = Self {
            block_height: counter!("indexer_block_height"),
            node_block_height: counter!("indexer_node_block_height"),
            caught_up: gauge!("indexer_caught_up"),
            transaction_count: counter!("indexer_transaction_count"),
            contract_deploy_count: counter!("indexer_contract_deploy_count"),
            contract_call_count: counter!("indexer_contract_call_count"),
            contract_update_count: counter!("indexer_contract_update_count"),
            gc_run_count: counter!("indexer_gc_run_count"),
            gc_culled_node_count: counter!("indexer_gc_culled_node_count"),
            gc_duration_seconds: histogram!("indexer_gc_duration_seconds"),
            gc_root_count: gauge!("indexer_gc_root_count"),
            arena_node_count: gauge!("indexer_arena_node_count"),
            uncaptured_contract_state_count: counter!("indexer_uncaptured_contract_state_count"),
            index_wait_duration_seconds: histogram!("indexer_index_wait_duration_seconds"),
            index_convert_duration_seconds: histogram!("indexer_index_convert_duration_seconds"),
            index_ledger_update_duration_seconds: histogram!(
                "indexer_index_ledger_update_duration_seconds"
            ),
            index_ledger_persist_duration_seconds: histogram!(
                "indexer_index_ledger_persist_duration_seconds"
            ),
            index_system_parameters_duration_seconds: histogram!(
                "indexer_index_system_parameters_duration_seconds"
            ),
            index_storage_duration_seconds: histogram!("indexer_index_storage_duration_seconds"),
            index_publish_duration_seconds: histogram!("indexer_index_publish_duration_seconds"),
            index_block_duration_seconds: histogram!("indexer_index_block_duration_seconds"),
        };

        if let Some(block_height) = block_height {
            metrics.block_height.absolute(block_height);
        }
        metrics.transaction_count.absolute(transaction_count);
        metrics
            .contract_deploy_count
            .absolute(contract_deploy_count);
        metrics.contract_call_count.absolute(contract_call_count);
        metrics
            .contract_update_count
            .absolute(contract_update_count);

        metrics
    }

    pub fn update(
        &self,
        block: &Block,
        transactions: &[Transaction],
        node_block_height: u64,
        caught_up: bool,
    ) {
        self.block_height.absolute(block.height);

        self.node_block_height.absolute(node_block_height);

        self.caught_up.set(f64::from(caught_up));

        self.transaction_count.increment(transactions.len() as u64);

        self.contract_call_count.increment(
            transactions
                .iter()
                .filter_map(|t| match t {
                    Transaction::Regular(t) => Some(t),
                    Transaction::System(_) => None,
                })
                .flat_map(|t| {
                    t.contract_actions.iter().filter(|a| {
                        matches!(
                            a,
                            ContractAction {
                                attributes: ContractAttributes::Call { .. },
                                ..
                            }
                        )
                    })
                })
                .count() as u64,
        );

        self.contract_deploy_count.increment(
            transactions
                .iter()
                .filter_map(|t| match t {
                    Transaction::Regular(t) => Some(t),
                    Transaction::System(_) => None,
                })
                .flat_map(|t| {
                    t.contract_actions.iter().filter(|a| {
                        matches!(
                            a,
                            ContractAction {
                                attributes: ContractAttributes::Deploy,
                                ..
                            }
                        )
                    })
                })
                .count() as u64,
        );

        self.contract_update_count.increment(
            transactions
                .iter()
                .filter_map(|t| match t {
                    Transaction::Regular(t) => Some(t),
                    Transaction::System(_) => None,
                })
                .flat_map(|t| {
                    t.contract_actions.iter().filter(|a| {
                        matches!(
                            a,
                            ContractAction {
                                attributes: ContractAttributes::Update,
                                ..
                            }
                        )
                    })
                })
                .count() as u64,
        );
    }

    /// Record one storage-core gc-v1 mark-and-sweep pass, along with the gc root count observed
    /// after it.
    ///
    /// The root count is the growth number to watch: per-action contract state roots are never
    /// unpersisted, so they accumulate with the number of *distinct* contract states. It is also the
    /// only live-set proxy available from outside storage-core — the mark set that actually bounds
    /// gc's memory is a `pub(crate)` field of `GcState`, so its size cannot be observed from here.
    pub fn record_gc(&self, duration: Duration, nodes_culled: usize, root_count: usize) {
        self.gc_run_count.increment(1);
        self.gc_culled_node_count.increment(nodes_culled as u64);
        self.gc_duration_seconds.record(duration.as_secs_f64());
        self.gc_root_count.set(root_count as f64);
    }

    /// Record the number of rows in `ledger_db_nodes`, i.e. the size of the arena.
    pub fn record_arena_node_count(&self, node_count: usize) {
        self.arena_node_count.set(node_count as f64);
    }

    /// Record contract addresses in a block for which no contract state could be captured from the
    /// ledger state. Expected to be non-zero only for failed actions, so a rising rate means states
    /// are silently not being captured and `state` is reading back empty.
    pub fn record_uncaptured_contract_states(&self, count: usize) {
        self.uncaptured_contract_state_count.increment(count as u64);
    }

    /// Record the time waiting for the next block to index.
    pub fn record_index_wait(&self, duration: Duration) {
        self.index_wait_duration_seconds
            .record(duration.as_secs_f64());
    }

    /// Record the time converting a block into its domain types.
    pub fn record_index_convert(&self, duration: Duration) {
        self.index_convert_duration_seconds
            .record(duration.as_secs_f64());
    }

    /// Record the time applying a block to the ledger state and validating its roots.
    pub fn record_index_ledger_update(&self, duration: Duration) {
        self.index_ledger_update_duration_seconds
            .record(duration.as_secs_f64());
    }

    /// Record the time persisting the ledger state.
    pub fn record_index_ledger_persist(&self, duration: Duration) {
        self.index_ledger_persist_duration_seconds
            .record(duration.as_secs_f64());
    }

    /// Record the time determining a block's system parameter changes.
    pub fn record_index_system_parameters(&self, duration: Duration) {
        self.index_system_parameters_duration_seconds
            .record(duration.as_secs_f64());
    }

    /// Record the time saving a block to storage.
    pub fn record_index_storage(&self, duration: Duration) {
        self.index_storage_duration_seconds
            .record(duration.as_secs_f64());
    }

    /// Record the time publishing a block's events.
    pub fn record_index_publish(&self, duration: Duration) {
        self.index_publish_duration_seconds
            .record(duration.as_secs_f64());
    }

    /// Record the time indexing a block, from conversion to publishing.
    pub fn record_index_block(&self, duration: Duration) {
        self.index_block_duration_seconds
            .record(duration.as_secs_f64());
    }
}
