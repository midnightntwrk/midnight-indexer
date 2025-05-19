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
mod contract;
mod wallet;

use crate::domain::Storage;
use async_graphql::MergedSubscription;
use indexer_common::domain::Subscriber;

#[derive(MergedSubscription)]
pub struct Subscription<S, B>(
    block::BlockSubscription<S, B>,
    contract::ContractSubscription<S, B>,
    wallet::WalletSubscription<S, B>,
)
where
    S: Storage,
    B: Subscriber;

impl<S, B> Default for Subscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    fn default() -> Self {
        Subscription(
            block::BlockSubscription::default(),
            contract::ContractSubscription::default(),
            wallet::WalletSubscription::default(),
        )
    }
}
