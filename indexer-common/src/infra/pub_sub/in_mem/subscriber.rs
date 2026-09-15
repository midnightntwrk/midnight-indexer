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

use crate::{
    domain::{Message, Subscriber},
    infra::pub_sub::in_mem::InMemPubSub,
};
use futures::{Stream, StreamExt};
use log::warn;
use std::{fmt::Debug, future::ready};
use thiserror::Error;
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};

/// In memory based implementations for [Subscriber].
#[derive(Clone)]
pub struct InMemSubscriber(InMemPubSub);

impl InMemSubscriber {
    #[allow(missing_docs)]
    pub fn new(in_mem_pub_sub: InMemPubSub) -> Self {
        Self(in_mem_pub_sub)
    }
}

impl Subscriber for InMemSubscriber {
    type Error = SubscriberError;

    fn subscribe<T>(&self) -> impl Stream<Item = Result<T, Self::Error>>
    where
        T: Message,
    {
        let receiver = self.0.sender(T::TOPIC).subscribe();
        let values = BroadcastStream::new(receiver);

        // `Lagged` leaves the receiver usable and `recv` resumes from the oldest message still
        // buffered, so the skipped count is logged and the stream continues.
        let values = values.filter_map(|value| {
            ready(match value {
                Ok(value) => Some(value),

                Err(BroadcastStreamRecvError::Lagged(skipped)) => {
                    warn!(topic:% = T::TOPIC, skipped; "subscriber lagged");
                    None
                }
            })
        });

        values.map(|value| {
            let message = serde_json::from_value::<T>(value)?;
            Ok(message)
        })
    }
}

#[derive(Debug, Error)]
pub enum SubscriberError {
    #[error("cannot JSON deserialize message")]
    Deserialize(#[from] serde_json::Error),
}
