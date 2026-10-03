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

/// An in-memory node; clones share their state.
#[derive(Clone)]
pub struct FakeNode(Arc<Inner>);

struct Inner {
    respond: Box<Respond>,
    respond_subscribe: Option<Box<RespondSubscribe>>,
    delay: Duration,
    delay_for: Option<Box<DelayFor>>,
    live_subscriptions: Arc<AtomicUsize>,
    max_batch_size: Option<usize>,
    disconnects: AtomicUsize,
    failing_reconnects: AtomicBool,
    reconnects: AtomicUsize,
    batch_sizes: Mutex<Vec<usize>>,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
    subscriptions: Mutex<Vec<Vec<Value>>>,
    notification_interval: Duration,
    subscribes: AtomicUsize,
    subscribed: Mutex<Vec<&'static str>>,
}

impl FakeNode {
    /// A node answering every call with `respond`.
    pub fn new(respond: impl Fn(&Call) -> CallResult + Send + Sync + 'static) -> Self {
        Self(Arc::new(Inner {
            respond: Box::new(respond),
            respond_subscribe: None,
            delay: Duration::ZERO,
            delay_for: None,
            live_subscriptions: Arc::default(),
            max_batch_size: None,
            disconnects: AtomicUsize::new(0),
            failing_reconnects: AtomicBool::new(false),
            reconnects: AtomicUsize::new(0),
            batch_sizes: Mutex::default(),
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
            subscriptions: Mutex::default(),
            notification_interval: Duration::ZERO,
            subscribes: AtomicUsize::new(0),
            subscribed: Mutex::default(),
        }))
    }

    /// Answer each batch after the given delay.
    pub fn with_delay(self, delay: Duration) -> Self {
        self.map(|inner| inner.delay = delay)
    }

    /// Answer each batch after a delay that depends on its calls.
    pub fn with_delay_for(
        self,
        delay_for: impl Fn(&[Call]) -> Duration + Send + Sync + 'static,
    ) -> Self {
        self.map(|inner| inner.delay_for = Some(Box::new(delay_for)))
    }

    /// The number of subscriptions not yet dropped.
    pub fn live_subscriptions(&self) -> usize {
        self.0.live_subscriptions.load(Ordering::SeqCst)
    }

    /// Refuse batches with more calls than the given size.
    pub fn with_max_batch_size(self, max_batch_size: usize) -> Self {
        self.map(|inner| inner.max_batch_size = Some(max_batch_size))
    }

    /// Lose the connection on the next `n` batches.
    pub fn with_disconnects(self, n: usize) -> Self {
        self.0.disconnects.store(n, Ordering::SeqCst);
        self
    }

    /// Fail every reconnect.
    pub fn with_failing_reconnects(self) -> Self {
        self.0.failing_reconnects.store(true, Ordering::SeqCst);
        self
    }

    /// Answer the next subscriptions with the given notifications, one list per subscription; after
    /// its notifications a subscription stays open and silent.
    pub fn with_subscriptions(self, subscriptions: Vec<Vec<Value>>) -> Self {
        *self.0.subscriptions.lock() = subscriptions;
        self
    }

    /// Answer subscriptions with the notifications `respond` returns for their method and
    /// parameters; scripted subscriptions answer where it returns `None`.
    pub fn with_subscribe(
        self,
        respond: impl Fn(&'static str, &[Value]) -> Option<Vec<Value>> + Send + Sync + 'static,
    ) -> Self {
        self.map(|inner| inner.respond_subscribe = Some(Box::new(respond)))
    }

    /// Send subscription notifications at the given interval.
    pub fn with_notification_interval(self, interval: Duration) -> Self {
        self.map(|inner| inner.notification_interval = interval)
    }

    /// The method of every subscription made, in order.
    pub fn subscribed(&self) -> Vec<&'static str> {
        self.0.subscribed.lock().clone()
    }

    /// The number of subscriptions made.
    pub fn subscribes(&self) -> usize {
        self.0.subscribes.load(Ordering::SeqCst)
    }

    /// The size of every batch received, in order.
    pub fn batch_sizes(&self) -> Vec<usize> {
        self.0.batch_sizes.lock().clone()
    }

    /// The most batches answered at the same time.
    pub fn max_in_flight(&self) -> usize {
        self.0.max_in_flight.load(Ordering::SeqCst)
    }

    /// The number of reconnects.
    pub fn reconnects(&self) -> usize {
        self.0.reconnects.load(Ordering::SeqCst)
    }

    fn map(self, f: impl FnOnce(&mut Inner)) -> Self {
        let mut inner = Arc::into_inner(self.0).expect("configured before being shared");
        f(&mut inner);
        Self(Arc::new(inner))
    }
}

impl Transport for FakeNode {
    async fn batch(&self, calls: Vec<Call>) -> Result<Vec<CallResult>, TransportError> {
        let inner = &self.0;

        let disconnect = inner
            .disconnects
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if disconnect {
            return Err(TransportError::Disconnected("fake disconnect".into()));
        }

        if inner
            .max_batch_size
            .is_some_and(|max_batch_size| calls.len() > max_batch_size)
        {
            return Err(TransportError::Other("batch too large".into()));
        }

        inner.batch_sizes.lock().push(calls.len());
        let in_flight = inner.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        inner.max_in_flight.fetch_max(in_flight, Ordering::SeqCst);
        let delay = inner
            .delay_for
            .as_ref()
            .map(|delay_for| delay_for(&calls))
            .unwrap_or(inner.delay);
        sleep(delay).await;
        inner.in_flight.fetch_sub(1, Ordering::SeqCst);

        Ok(calls.iter().map(|call| (inner.respond)(call)).collect())
    }

    async fn subscribe(
        &self,
        method: &'static str,
        params: Vec<Value>,
        _unsubscribe: &'static str,
    ) -> Result<Subscription, TransportError> {
        let n = self.0.subscribes.fetch_add(1, Ordering::SeqCst);
        self.0.subscribed.lock().push(method);
        let responded = self
            .0
            .respond_subscribe
            .as_ref()
            .and_then(|respond| respond(method, &params));
        let notifications = if let Some(notifications) = responded {
            notifications
        } else {
            let mut subscriptions = self.0.subscriptions.lock();
            if subscriptions.is_empty() {
                vec![]
            } else {
                subscriptions.remove(0)
            }
        };
        let interval = self.0.notification_interval;
        let live = LiveSubscription::new(self.0.live_subscriptions.clone());
        let notifications = stream::iter(notifications)
            .then(move |notification| async move {
                sleep(interval).await;
                Ok(notification)
            })
            .chain(stream::pending())
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
        self.0.reconnects.fetch_add(1, Ordering::SeqCst);
        if self.0.failing_reconnects.load(Ordering::SeqCst) {
            Err(TransportError::Disconnected(
                "fake reconnect failure".into(),
            ))
        } else {
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
