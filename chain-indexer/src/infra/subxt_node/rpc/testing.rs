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

//! An in-memory [Transport] answering calls with a given function, with switches for the failure
//! modes of a real node: lost connections, refused batches and slow responses.

use crate::infra::subxt_node::rpc::{Call, CallResult, Subscription, Transport, TransportError};
use futures::{StreamExt, stream};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::time::sleep;

type Respond = dyn Fn(&Call) -> CallResult + Send + Sync;
type DelayFor = dyn Fn(&[Call]) -> Duration + Send + Sync;
type RespondSubscribe = dyn Fn(&'static str, &[Value]) -> Option<Vec<Value>> + Send + Sync;

/// An in-memory node, configured before being shared and then shared as `Arc<FakeNode>`.
pub struct FakeNode {
    respond: Box<Respond>,
    respond_subscribe: Option<Box<RespondSubscribe>>,
    delay: Duration,
    delay_for: Option<Box<DelayFor>>,
    live_subscriptions: Arc<AtomicUsize>,
    max_batch_size: Option<usize>,
    disconnects: AtomicUsize,
    connection_lost: AtomicBool,
    timeouts: AtomicUsize,
    failing_reconnects: AtomicBool,
    reconnects: AtomicUsize,
    batch_sizes: Mutex<Vec<usize>>,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
    subscriptions: Mutex<Vec<Vec<Value>>>,
    notification_interval: Duration,
    ending_subscriptions: bool,
    subscribes: AtomicUsize,
    subscribed: Mutex<Vec<&'static str>>,
}

impl FakeNode {
    /// A node answering every call with `respond`.
    pub fn new(respond: impl Fn(&Call) -> CallResult + Send + Sync + 'static) -> Self {
        Self {
            respond: Box::new(respond),
            respond_subscribe: None,
            delay: Duration::ZERO,
            delay_for: None,
            live_subscriptions: Arc::default(),
            max_batch_size: None,
            disconnects: AtomicUsize::new(0),
            connection_lost: AtomicBool::new(false),
            timeouts: AtomicUsize::new(0),
            failing_reconnects: AtomicBool::new(false),
            reconnects: AtomicUsize::new(0),
            batch_sizes: Mutex::default(),
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
            subscriptions: Mutex::default(),
            notification_interval: Duration::ZERO,
            ending_subscriptions: false,
            subscribes: AtomicUsize::new(0),
            subscribed: Mutex::default(),
        }
    }

    /// Answer each batch after the given delay.
    pub fn with_delay(self, delay: Duration) -> Self {
        Self { delay, ..self }
    }

    /// Answer each batch after a delay that depends on its calls.
    pub fn with_delay_for(
        self,
        delay_for: impl Fn(&[Call]) -> Duration + Send + Sync + 'static,
    ) -> Self {
        Self {
            delay_for: Some(Box::new(delay_for)),
            ..self
        }
    }

    /// The number of subscriptions not yet dropped.
    pub fn live_subscriptions(&self) -> usize {
        self.live_subscriptions.load(Ordering::SeqCst)
    }

    /// Refuse batches with more calls than the given size.
    pub fn with_max_batch_size(self, max_batch_size: usize) -> Self {
        Self {
            max_batch_size: Some(max_batch_size),
            ..self
        }
    }

    /// Lose the connection on the next `n` batches.
    pub fn with_disconnects(self, n: usize) -> Self {
        Self {
            disconnects: AtomicUsize::new(n),
            ..self
        }
    }

    /// Lose the connection: every call fails until a reconnect.
    pub fn with_connection_lost(self) -> Self {
        Self {
            connection_lost: AtomicBool::new(true),
            ..self
        }
    }

    /// Time out on the next `n` batches.
    pub fn with_timeouts(self, n: usize) -> Self {
        Self {
            timeouts: AtomicUsize::new(n),
            ..self
        }
    }

    /// Fail every reconnect.
    pub fn with_failing_reconnects(self) -> Self {
        Self {
            failing_reconnects: AtomicBool::new(true),
            ..self
        }
    }

    /// Answer the next subscriptions with the given notifications, one list per subscription; after
    /// its notifications a subscription stays open and silent.
    pub fn with_subscriptions(self, subscriptions: Vec<Vec<Value>>) -> Self {
        Self {
            subscriptions: Mutex::new(subscriptions),
            ..self
        }
    }

    /// Answer subscriptions with the notifications `respond` returns for their method and
    /// parameters; scripted subscriptions answer where it returns `None`.
    pub fn with_subscribe(
        self,
        respond: impl Fn(&'static str, &[Value]) -> Option<Vec<Value>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            respond_subscribe: Some(Box::new(respond)),
            ..self
        }
    }

    /// End each subscription after its notifications, rather than leaving it open and silent.
    pub fn with_ending_subscriptions(self) -> Self {
        Self {
            ending_subscriptions: true,
            ..self
        }
    }

    /// Send subscription notifications at the given interval.
    pub fn with_notification_interval(self, interval: Duration) -> Self {
        Self {
            notification_interval: interval,
            ..self
        }
    }

    /// The method of every subscription made, in order.
    pub fn subscribed(&self) -> Vec<&'static str> {
        self.subscribed.lock().clone()
    }

    /// The number of subscriptions made.
    pub fn subscribes(&self) -> usize {
        self.subscribes.load(Ordering::SeqCst)
    }

    /// The size of every batch received, in order.
    pub fn batch_sizes(&self) -> Vec<usize> {
        self.batch_sizes.lock().clone()
    }

    /// The most batches answered at the same time.
    pub fn max_in_flight(&self) -> usize {
        self.max_in_flight.load(Ordering::SeqCst)
    }

    /// The number of reconnects.
    pub fn reconnects(&self) -> usize {
        self.reconnects.load(Ordering::SeqCst)
    }
}

impl Transport for Arc<FakeNode> {
    async fn batch(&self, calls: Vec<Call>) -> Result<Vec<CallResult>, TransportError> {
        let disconnect = self
            .disconnects
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if disconnect || self.connection_lost.load(Ordering::SeqCst) {
            return Err(TransportError::Disconnected("fake disconnect".into()));
        }

        let timeout = self
            .timeouts
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if timeout {
            return Err(TransportError::Timeout("fake timeout".into()));
        }

        if self
            .max_batch_size
            .is_some_and(|max_batch_size| calls.len() > max_batch_size)
        {
            return Err(TransportError::Other("batch too large".into()));
        }

        self.batch_sizes.lock().push(calls.len());
        let in_flight = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(in_flight, Ordering::SeqCst);
        let delay = self
            .delay_for
            .as_ref()
            .map(|delay_for| delay_for(&calls))
            .unwrap_or(self.delay);
        sleep(delay).await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);

        Ok(calls.iter().map(|call| (self.respond)(call)).collect())
    }

    async fn subscribe(
        &self,
        method: &'static str,
        params: Vec<Value>,
        _unsubscribe: &'static str,
    ) -> Result<Subscription, TransportError> {
        if self.connection_lost.load(Ordering::SeqCst) {
            return Err(TransportError::Disconnected("fake disconnect".into()));
        }

        let n = self.subscribes.fetch_add(1, Ordering::SeqCst);
        self.subscribed.lock().push(method);
        let responded = self
            .respond_subscribe
            .as_ref()
            .and_then(|respond| respond(method, &params));
        let notifications = if let Some(notifications) = responded {
            notifications
        } else {
            let mut subscriptions = self.subscriptions.lock();
            if subscriptions.is_empty() {
                vec![]
            } else {
                subscriptions.remove(0)
            }
        };
        let interval = self.notification_interval;
        let live = LiveSubscription::new(self.live_subscriptions.clone());
        let notifications = stream::iter(notifications)
            .then(move |notification| async move {
                sleep(interval).await;
                Ok(notification)
            })
            .chain(if self.ending_subscriptions {
                stream::empty().boxed()
            } else {
                stream::pending().boxed()
            })
            .map(move |notification| {
                let _live = &live;
                notification
            })
            .boxed();

        Ok(Subscription {
            id: format!("subscription-{n}").into(),
            notifications,
        })
    }

    async fn reconnect(&self) -> Result<(), TransportError> {
        self.reconnects.fetch_add(1, Ordering::SeqCst);
        if self.failing_reconnects.load(Ordering::SeqCst) {
            Err(TransportError::Disconnected(
                "fake reconnect failure".into(),
            ))
        } else {
            self.connection_lost.store(false, Ordering::SeqCst);
            Ok(())
        }
    }
}

/// Counts a subscription as live until dropped.
struct LiveSubscription(Arc<AtomicUsize>);

impl LiveSubscription {
    fn new(live: Arc<AtomicUsize>) -> Self {
        live.fetch_add(1, Ordering::SeqCst);
        Self(live)
    }
}

impl Drop for LiveSubscription {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
