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

mod block;
mod contract_action;
mod wallet;

use crate::domain::Storage;
use async_graphql::MergedSubscription;
use indexer_common::domain::{Subscriber, ZswapStateStorage};

#[derive(MergedSubscription)]
pub struct Subscription<S, B, Z>(
    block::BlockSubscription<S, B>,
    contract_action::ContractActionSubscription<S, B>,
    wallet::WalletSubscription<S, B, Z>,
)
where
    S: Storage,
    B: Subscriber,
    Z: ZswapStateStorage;

impl<S, B, Z> Default for Subscription<S, B, Z>
where
    S: Storage,
    B: Subscriber,
    Z: ZswapStateStorage,
{
    fn default() -> Self {
        Subscription(
            block::BlockSubscription::default(),
            contract_action::ContractActionSubscription::default(),
            wallet::WalletSubscription::default(),
        )
    }
}
