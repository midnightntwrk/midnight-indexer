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

//! GraphQL WebSocket subscription quotas.
//!
//! Implements two caps:
//!
//! - Per WebSocket connection, a concurrent count of active subscriptions across all 9 GraphQL
//!   subscription types. Enforced via a per-connection `AtomicUsize` injected at
//!   `on_connection_init` and decremented by [`SubscriptionGuard`] on drop.
//! - Per session id, a creation rate via a token bucket. Only `shielded_transactions` takes a
//!   `session_id`, so this layer applies there. Rejected attempts do not consume a token. Buckets
//!   that have refilled completely behave the same as missing ones, so they are pruned at most
//!   once per minute, keeping the map bounded by the sessions recently used.
//!
//! Cap hits return [`QuotaError`] which API resolvers convert to `ApiError::client`. The
//! WebSocket connection itself remains open.

use dashmap::DashMap;
use indexer_common::domain::SessionId;
use metrics::{Counter, Gauge, counter, gauge};
use parking_lot::Mutex;
use serde::Deserialize;
use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use thiserror::Error;

const REJECTION_KIND_PER_CONNECTION: &str = "per_connection";
const REJECTION_KIND_PER_SESSION_RATE: &str = "per_session_rate";

/// Minimum time between two prunes of the per-session buckets.
const PRUNE_INTERVAL: Duration = Duration::from_secs(60);

/// Per-WebSocket-connection counter for active subscriptions. Attached to the connection's
/// async-graphql `Data` from the `on_connection_init` callback so every subscription resolver on
/// that connection can increment and check against the per-connection cap.
#[derive(Debug, Default)]
pub struct PerConnectionCounter(pub(super) Arc<AtomicUsize>);

/// Configuration for the subscription quota layer.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct QuotaConfig {
    pub max_concurrent_per_connection: NonZeroUsize,

    /// Maximum new `shielded_transactions` subscriptions per `session_id` per minute. Implemented
    /// as a token bucket whose capacity equals this value and which refills at the same rate
    /// across one minute.
    pub max_session_subscriptions_per_minute: NonZeroU32,
}

/// State shared across the entire `indexer-api` instance, held in the async-graphql Schema data.
pub struct SubscriptionQuotas {
    config: QuotaConfig,
    per_session_buckets: DashMap<SessionId, Arc<Mutex<TokenBucket>>>,
    last_prune: Mutex<Instant>,
    metrics: QuotaMetrics,
}

impl SubscriptionQuotas {
    pub fn new(config: QuotaConfig) -> Self {
        Self {
            config,
            per_session_buckets: DashMap::new(),
            last_prune: Mutex::new(Instant::now()),
            metrics: QuotaMetrics::default(),
        }
    }

    /// Try to register a new active subscription. The per-connection counter must be obtained from
    /// the per-WebSocket-connection async-graphql `Data` populated by the `on_connection_init`
    /// callback. If `session` is provided, also consume a token from that session id's rate
    /// bucket.
    pub fn try_acquire(
        &self,
        per_connection_counter: &Arc<AtomicUsize>,
        session: Option<&SessionId>,
    ) -> Result<SubscriptionGuard, QuotaError> {
        let max_concurrent = self.config.max_concurrent_per_connection.get();
        if per_connection_counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < max_concurrent).then_some(current + 1)
            })
            .is_err()
        {
            self.metrics.rejected_per_connection.increment(1);
            return Err(QuotaError::PerConnection(max_concurrent));
        }

        if let Some(session) = session {
            self.prune_idle_if_due(Instant::now());

            let bucket = self
                .per_session_buckets
                .entry(session.clone())
                .or_insert_with(|| Arc::new(Mutex::new(TokenBucket::new(&self.config))))
                .clone();
            let allowed = bucket.lock().try_take();
            if !allowed {
                per_connection_counter.fetch_sub(1, Ordering::AcqRel);
                self.metrics.rejected_per_session_rate.increment(1);
                return Err(QuotaError::PerSessionRate(
                    self.config.max_session_subscriptions_per_minute.get(),
                ));
            }
        }

        self.metrics.active.increment(1);
        Ok(SubscriptionGuard {
            per_connection_counter: per_connection_counter.clone(),
            active_gauge: self.metrics.active.clone(),
        })
    }

    /// Prune idle buckets if at least `PRUNE_INTERVAL` has passed since the last prune. If
    /// another caller is currently checking, skip: it will prune if due.
    fn prune_idle_if_due(&self, now: Instant) {
        let due = self.last_prune.try_lock().is_some_and(|mut last_prune| {
            let due = now.saturating_duration_since(*last_prune) >= PRUNE_INTERVAL;
            if due {
                *last_prune = now;
            }
            due
        });

        if due {
            self.prune_idle_at(now);
        }
    }

    /// Drop per-session buckets that are full again at `now`; a full bucket behaves the same as a
    /// missing one. Buckets are only cloned out of the map while its shard lock is held, which
    /// `retain` holds too, so a strong count of 1 means no caller is about to use the bucket.
    fn prune_idle_at(&self, now: Instant) {
        self.per_session_buckets
            .retain(|_, bucket| Arc::strong_count(bucket) > 1 || !bucket.lock().is_full_at(now));
    }
}

/// RAII handle held by an active subscription. On drop, decrements the per-connection counter and
/// the active gauge.
#[derive(Debug)]
pub struct SubscriptionGuard {
    per_connection_counter: Arc<AtomicUsize>,
    active_gauge: Gauge,
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        self.per_connection_counter.fetch_sub(1, Ordering::AcqRel);
        self.active_gauge.decrement(1);
    }
}

#[derive(Debug, Error)]
pub enum QuotaError {
    #[error("per-connection limit exceeded ({0})")]
    PerConnection(usize),

    #[error("per-session rate limit exceeded ({0}/min)")]
    PerSessionRate(u32),
}

/// Token bucket for per-session creation rate limiting.
///
/// Capacity equals the configured per-minute rate, allowing a fresh session to issue a burst up to
/// the cap before throttling. Refill is the same number of tokens spread across 60 seconds.
struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_per_sec: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(config: &QuotaConfig) -> Self {
        let capacity = f64::from(config.max_session_subscriptions_per_minute.get());
        Self {
            tokens: capacity,
            capacity,
            refill_per_sec: capacity / 60.0,
            last_refill: Instant::now(),
        }
    }

    fn try_take(&mut self) -> bool {
        self.try_take_at(Instant::now())
    }

    fn try_take_at(&mut self, now: Instant) -> bool {
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        self.last_refill = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn is_full_at(&self, now: Instant) -> bool {
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens + elapsed * self.refill_per_sec >= self.capacity
    }
}

struct QuotaMetrics {
    active: Gauge,
    rejected_per_connection: Counter,
    rejected_per_session_rate: Counter,
}

impl Default for QuotaMetrics {
    fn default() -> Self {
        Self {
            active: gauge!("indexer_subscriptions_active"),
            rejected_per_connection: counter!(
                "indexer_subscriptions_rejected_total",
                "kind" => REJECTION_KIND_PER_CONNECTION,
            ),
            rejected_per_session_rate: counter!(
                "indexer_subscriptions_rejected_total",
                "kind" => REJECTION_KIND_PER_SESSION_RATE,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn config(per_connection: usize, per_session_per_minute: u32) -> QuotaConfig {
        QuotaConfig {
            max_concurrent_per_connection: NonZeroUsize::new(per_connection).unwrap(),
            max_session_subscriptions_per_minute: NonZeroU32::new(per_session_per_minute).unwrap(),
        }
    }

    fn session(byte: u8) -> SessionId {
        [byte; 32].into()
    }

    #[test]
    fn per_connection_cap_blocks_after_limit() {
        let quotas = SubscriptionQuotas::new(config(3, 1000));
        let counter = Arc::new(AtomicUsize::new(0));

        let g1 = quotas.try_acquire(&counter, None).expect("1st");
        let g2 = quotas.try_acquire(&counter, None).expect("2nd");
        let g3 = quotas.try_acquire(&counter, None).expect("3rd");
        assert_eq!(counter.load(Ordering::Acquire), 3);

        let err = quotas.try_acquire(&counter, None).unwrap_err();
        assert!(matches!(err, QuotaError::PerConnection(3)));
        assert_eq!(counter.load(Ordering::Acquire), 3);

        drop(g1);
        let g4 = quotas.try_acquire(&counter, None).expect("after drop");
        assert_eq!(counter.load(Ordering::Acquire), 3);
        drop((g2, g3, g4));
        assert_eq!(counter.load(Ordering::Acquire), 0);
    }

    #[test]
    fn per_session_rate_blocks_after_capacity() {
        let quotas = SubscriptionQuotas::new(config(1000, 5));
        let counter = Arc::new(AtomicUsize::new(0));
        let s = session(1);

        let mut guards = Vec::new();
        for _ in 0..5 {
            guards.push(quotas.try_acquire(&counter, Some(&s)).unwrap());
        }
        let err = quotas.try_acquire(&counter, Some(&s)).unwrap_err();
        assert!(matches!(err, QuotaError::PerSessionRate(5)));
    }

    #[test]
    fn rejected_session_rate_does_not_consume_per_connection_slot() {
        let quotas = SubscriptionQuotas::new(config(10, 1));
        let counter = Arc::new(AtomicUsize::new(0));
        let s = session(2);

        let _g = quotas.try_acquire(&counter, Some(&s)).unwrap();
        assert_eq!(counter.load(Ordering::Acquire), 1);

        let err = quotas.try_acquire(&counter, Some(&s)).unwrap_err();
        assert!(matches!(err, QuotaError::PerSessionRate(1)));
        assert_eq!(
            counter.load(Ordering::Acquire),
            1,
            "rejected session-rate attempt must roll back the per-connection slot"
        );
    }

    #[test]
    fn rejected_per_connection_does_not_consume_session_token() {
        let quotas = SubscriptionQuotas::new(config(1, 60));
        let counter = Arc::new(AtomicUsize::new(0));
        let s = session(3);

        let _g = quotas.try_acquire(&counter, Some(&s)).unwrap();
        let err = quotas.try_acquire(&counter, Some(&s)).unwrap_err();
        assert!(matches!(err, QuotaError::PerConnection(1)));

        let bucket = quotas.per_session_buckets.get(&s).unwrap().clone();
        let tokens = bucket.lock().tokens;
        assert!(
            tokens > 58.0,
            "session bucket should still be near full ({tokens})"
        );
    }

    #[test]
    fn distinct_sessions_have_independent_buckets() {
        let quotas = SubscriptionQuotas::new(config(1000, 1));
        let counter = Arc::new(AtomicUsize::new(0));

        quotas.try_acquire(&counter, Some(&session(10))).unwrap();
        quotas.try_acquire(&counter, Some(&session(11))).unwrap();
        quotas.try_acquire(&counter, Some(&session(12))).unwrap();

        let err = quotas
            .try_acquire(&counter, Some(&session(10)))
            .unwrap_err();
        assert!(matches!(err, QuotaError::PerSessionRate(1)));
    }

    #[test]
    fn idle_full_buckets_are_pruned() {
        let quotas = SubscriptionQuotas::new(config(1000, 1));
        let counter = Arc::new(AtomicUsize::new(0));

        let guards = (0..100)
            .map(|n| quotas.try_acquire(&counter, Some(&session(n))).unwrap())
            .collect::<Vec<_>>();
        drop(guards);
        assert_eq!(quotas.per_session_buckets.len(), 100);

        // After a full refill period every bucket is full again, i.e. indistinguishable from a
        // missing one.
        quotas.prune_idle_at(Instant::now() + Duration::from_secs(120));
        assert!(
            quotas.per_session_buckets.is_empty(),
            "{} idle buckets were not pruned",
            quotas.per_session_buckets.len()
        );
    }

    #[test]
    fn prune_runs_at_most_once_per_interval() {
        // A bucket of capacity 60 refills one token per second, so it is full again one second
        // after a take; whether it is pruned then depends only on the prune interval.
        let quotas = SubscriptionQuotas::new(config(1000, 60));
        let counter = Arc::new(AtomicUsize::new(0));
        drop(quotas.try_acquire(&counter, Some(&session(1))).unwrap());
        let start = *quotas.last_prune.lock();

        let not_due = start + PRUNE_INTERVAL - Duration::from_secs(1);
        quotas.prune_idle_if_due(not_due);
        assert_eq!(quotas.per_session_buckets.len(), 1);
        assert_eq!(*quotas.last_prune.lock(), start);

        let due = start + PRUNE_INTERVAL + Duration::from_secs(1);
        quotas.prune_idle_if_due(due);
        assert!(quotas.per_session_buckets.is_empty());
        assert_eq!(*quotas.last_prune.lock(), due);
    }

    #[test]
    fn prune_skips_while_another_caller_checks() {
        let quotas = SubscriptionQuotas::new(config(1000, 60));
        let counter = Arc::new(AtomicUsize::new(0));
        drop(quotas.try_acquire(&counter, Some(&session(1))).unwrap());

        let last_prune = quotas.last_prune.lock();
        let due = *last_prune + PRUNE_INTERVAL + Duration::from_secs(1);
        quotas.prune_idle_if_due(due);
        assert_eq!(quotas.per_session_buckets.len(), 1);
        drop(last_prune);
    }

    #[test]
    fn prune_keeps_drained_and_referenced_buckets() {
        // A bucket of capacity 1 takes 60 seconds to refill, so it is reliably not full right
        // after a take.
        let quotas = SubscriptionQuotas::new(config(1000, 1));
        let counter = Arc::new(AtomicUsize::new(0));
        drop(quotas.try_acquire(&counter, Some(&session(1))).unwrap());

        // Right after a take, the bucket is a token short of full.
        quotas.prune_idle_at(Instant::now());
        assert_eq!(quotas.per_session_buckets.len(), 1);

        // Once refilled, the bucket is kept while a caller holds it and pruned afterwards.
        let held = quotas
            .per_session_buckets
            .iter()
            .next()
            .map(|entry| entry.value().clone())
            .unwrap();
        let refilled = Instant::now() + Duration::from_secs(120);
        quotas.prune_idle_at(refilled);
        assert_eq!(quotas.per_session_buckets.len(), 1);

        drop(held);
        quotas.prune_idle_at(refilled);
        assert!(quotas.per_session_buckets.is_empty());
    }

    #[test]
    fn token_bucket_refills_over_time() {
        let cfg = config(1000, 60);
        let mut bucket = TokenBucket::new(&cfg);
        let start = bucket.last_refill;

        for _ in 0..60 {
            assert!(bucket.try_take_at(start));
        }
        assert!(!bucket.try_take_at(start));

        // 6 seconds later, 6 tokens should be available again.
        let later = start + Duration::from_secs(6);
        for _ in 0..6 {
            assert!(bucket.try_take_at(later));
        }
        assert!(!bucket.try_take_at(later));
    }
}
