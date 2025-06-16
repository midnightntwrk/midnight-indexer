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

use anyhow::Context;
use chain_indexer::{
    domain::Node,
    infra::node::{Config, SubxtNode},
};
use futures::{StreamExt, TryStreamExt};
use indexer_common::domain::{NetworkId, PROTOCOL_VERSION_000_013_000};
use std::{pin::pin, time::Duration};

/// This program connects to a local node and prints some first blocks and their transactions.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config {
        url: "ws://localhost:9944".to_string(),
        genesis_protocol_version: PROTOCOL_VERSION_000_013_000,
        reconnect_max_delay: Duration::from_secs(1),
        reconnect_max_attempts: 3,
    };
    let mut node = SubxtNode::new(config).await.context("create SubxtNode")?;

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
