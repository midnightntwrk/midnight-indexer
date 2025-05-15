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

use crate::domain::SessionId;
use derive_more::derive::From;
use futures::{Stream, stream};
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, error::Error as StdError, fmt::Debug};

macro_rules! message {
    ($name:ident) => {
        impl Message for $name {
            const TOPIC: Topic = Topic(stringify!($name));
        }

        impl sealed::Sealed for $name {}
    };
}

/// A pub-sub message. Restricted to implementations in this module.
pub trait Message
where
    Self: sealed::Sealed + Debug + Clone + Eq + Serialize + for<'de> Deserialize<'de> + Send,
{
    const TOPIC: Topic;
}

#[derive(Debug, Clone, Copy)]
pub struct Topic(pub &'static str);

/// Message/event signaling that a block has been indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, From)]
pub struct BlockIndexed {
    pub height: u32,
    pub caught_up: bool,
}
message!(BlockIndexed);

/// Message/event signaling that a wallet has been indexed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, From)]
pub struct WalletIndexed {
    pub session_id: SessionId,
}
message!(WalletIndexed);

/// A pub-sub publisher.
#[trait_variant::make(Send)]
pub trait Publisher
where
    Self: Clone + Send + Sync + 'static,
{
    /// Error type for the [Publisher::publish] method.
    type Error: StdError + Send + Sync + 'static;

    /// Publish the given message.
    async fn publish<T>(&self, message: &T) -> Result<(), Self::Error>
    where
        T: Message + Send + Sync;
}

/// A pub-sub subscriber.
#[trait_variant::make(Send)]
pub trait Subscriber
where
    Self: Clone + Send + Sync + 'static,
{
    /// Error type for the [Subscriber::subscribe] method.
    type Error: StdError + Send + Sync + 'static;

    /// Subscribe to the given messages.
    async fn subscribe<T>(
        &self,
    ) -> Result<impl Stream<Item = Result<T, Self::Error>> + Send, Self::Error>
    where
        T: Message;
}

/// A [Subscriber] implementation that "does nothing".
#[derive(Debug, Clone, Default)]
pub struct NoopSubscriber;

impl Subscriber for NoopSubscriber {
    type Error = Infallible;

    async fn subscribe<T>(&self) -> Result<impl Stream<Item = Result<T, Self::Error>>, Self::Error>
    where
        T: Message,
    {
        Ok(stream::empty())
    }
}

mod sealed {
    pub trait Sealed {}
}
