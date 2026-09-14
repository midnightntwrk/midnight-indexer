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

use crate::domain::{UnshieldedAddress, bridge::BridgeEvent};
use derive_more::derive::{Display, From};
use futures::{Stream, stream};
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, error::Error as StdError, fmt::Debug};
use uuid::Uuid;

/// A pub-sub message. Restricted to implementations in this module.
pub trait Message
where
    Self: sealed::Sealed + Debug + Clone + Eq + Serialize + for<'de> Deserialize<'de> + Send,
{
    const TOPIC: Topic;
}

// Declares `Topic` over the given message types and implements `Message` for each, pairing every
// type with its same-named variant.
macro_rules! topics {
    ($($name:ident),+ $(,)?) => {
        /// The channel a [Message] travels on, one variant per implementation.
        #[derive(Debug, Display, Clone, Copy, PartialEq, Eq)]
        pub enum Topic {
            $($name),+
        }

        impl Topic {
            /// Every topic in declaration order. No variant carries an explicit discriminant, so
            /// `topic as usize` indexes this slice.
            #[cfg(any(test, feature = "standalone"))]
            pub(crate) const VARIANTS: &'static [Topic] = &[$(Topic::$name),+];
        }

        $(
            // Binds the message type to the topic it travels on.
            impl Message for $name {
                const TOPIC: Topic = Topic::$name;
            }

            // Satisfies the supertrait bound on `Message`.
            impl sealed::Sealed for $name {}
        )+
    };
}

// The complete set of pub-sub message types.
topics!(
    BlockIndexed,
    WalletIndexed,
    UnshieldedUtxoIndexed,
    BridgeEventIndexed,
);

/// Message/event signaling that a block has been indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, From)]
pub struct BlockIndexed {
    pub height: u64,
    pub max_transaction_id: Option<u64>,
    pub caught_up: bool,
}

/// Message/event signaling that a wallet has been indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, From)]
pub struct WalletIndexed {
    pub wallet_id: Uuid,
}

/// Emitted when a transaction affecting unshielded UTXOs for a concrete address
/// has been stored in the DB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnshieldedUtxoIndexed {
    pub address: UnshieldedAddress,
}

/// Emitted when a c2m-bridge event (any of the 5 variants) is indexed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeEventIndexed {
    pub block_height: u64,
    pub event: BridgeEvent,
}

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
    /// Conversion errors into the message type of the [Subscriber::subscribe] method.
    type Error: StdError + Send + Sync + 'static;

    /// Subscribe to the messages of the given type. Implementations must return an infinite stream
    /// that can handle any underlying errors transparently, i.e. without leaking into the
    /// `Self::Error` type which is reserved for conversion errors.
    fn subscribe<T>(&self) -> impl Stream<Item = Result<T, Self::Error>> + Send
    where
        T: Message;
}

/// A [Subscriber] implementation that "does nothing".
#[derive(Debug, Clone, Default)]
pub struct NoopSubscriber;

impl Subscriber for NoopSubscriber {
    type Error = Infallible;

    fn subscribe<T>(&self) -> impl Stream<Item = Result<T, Self::Error>> + Send
    where
        T: Message,
    {
        stream::empty()
    }
}

// Private, so only this module can name `Sealed` and thus satisfy the supertrait bound on
// `Message`.
mod sealed {
    pub trait Sealed {}
}

#[cfg(test)]
mod tests {
    use crate::domain::Topic;

    /// Every topic renders as its own name. The match is exhaustive, so a new topic must be added
    /// here too.
    #[test]
    fn test_topic_display() {
        use Topic::*;
        for topic in Topic::VARIANTS {
            let name = match topic {
                BlockIndexed => "BlockIndexed",
                WalletIndexed => "WalletIndexed",
                UnshieldedUtxoIndexed => "UnshieldedUtxoIndexed",
                BridgeEventIndexed => "BridgeEventIndexed",
            };

            assert_eq!(topic.to_string(), name);
        }
    }

    /// Each topic's discriminant is its own position in `VARIANTS`. Giving a variant an explicit
    /// discriminant breaks this, and with it every lookup keyed on `topic as usize`.
    #[test]
    fn test_variants_are_indexed_by_discriminant() {
        for &topic in Topic::VARIANTS {
            assert_eq!(Topic::VARIANTS[topic as usize], topic);
        }
    }
}
