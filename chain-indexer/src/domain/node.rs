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

use crate::domain::{Block, BlockInfo};
use futures::Stream;
use indexer_common::domain::NetworkId;
use std::error::Error as StdError;

/// Node abstraction.
#[trait_variant::make(Send)]
pub trait Node
where
    Self: Clone + Send + Sync + 'static,
{
    /// Error type for items of the stream of finalized [Block]s.
    type Error: StdError + Send + Sync + 'static;

    /// A stream of the latest/highest finalized blocks.
    async fn highest_blocks(
        &self,
    ) -> Result<impl Stream<Item = Result<BlockInfo, Self::Error>> + Send, Self::Error>;

    /// A stream of finalized [Block]s in natural parent-child order without duplicates but possibly
    /// with gaps, starting after the given block.
    fn finalized_blocks(
        &mut self,
        after: Option<BlockInfo>,
        network_id: NetworkId,
    ) -> impl Stream<Item = Result<Block, Self::Error>>;
}
