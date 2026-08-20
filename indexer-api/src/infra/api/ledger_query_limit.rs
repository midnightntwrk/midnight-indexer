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

//! Process-global concurrency limit for ledger-DB-backed GraphQL resolvers.
//!
//! Resolvers that materialize ledger state drive storage-core's synchronous `DB`
//! (`indexer-common/src/infra/ledger_db/v1_1.rs`) via `block_in_place`, which converts the current
//! Tokio worker into a blocking thread and hands its run queue to another worker. With no bound, N
//! concurrent ledger queries occupy N workers at once; once N reaches the worker count there is no
//! worker left to take the handoff, the runtime stops making progress, and even `/live` stops
//! answering (issue #595, "amber-heron"). Unauthenticated queries like `dustGenerationMerkleTreeUpdate`
//! reach this with no credentials and pass every existing limit (`max_complexity`, `limit_depth`,
//! the WebSocket-only subscription quota).
//!
//! [`LedgerQueryLimiter`] is a shared [`Semaphore`] sized to `worker_threads - k` (see
//! [`permits_for_worker_threads`]). Every ledger-DB-touching resolver acquires one permit *for the
//! duration of its ledger work and no longer* — one-shot queries hold it across their single walk,
//! and subscriptions acquire per emitted item rather than once per stream, so a long-lived
//! subscription never pins a permit while idle. Because at most `worker_threads - k` permits exist,
//! at least `k` workers are always free to run liveness probes and take `block_in_place` handoffs.

use metrics::{Counter, Gauge, counter, gauge};
use std::{num::NonZeroUsize, sync::Arc};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Tokio worker threads kept out of the ledger-query budget so liveness/readiness probes, the
/// accept loop, and `block_in_place` handoffs always have a worker even while every permit is held
/// in a synchronous ledger walk.
pub const LIVENESS_WORKER_RESERVE: usize = 1;

/// Default number of concurrent ledger queries for a runtime with `worker_threads` workers:
/// `worker_threads - LIVENESS_WORKER_RESERVE`, clamped to at least one so single/dual-core hosts
/// still serve ledger queries (with one in flight at a time).
pub fn permits_for_worker_threads(worker_threads: usize) -> NonZeroUsize {
    NonZeroUsize::new(worker_threads.saturating_sub(LIVENESS_WORKER_RESERVE))
        .unwrap_or(NonZeroUsize::MIN)
}

/// Process-global limit on concurrent ledger-DB-backed resolvers. Cheaply cloneable; every clone
/// shares one semaphore and one set of metrics.
#[derive(Debug, Clone)]
pub struct LedgerQueryLimiter {
    semaphore: Arc<Semaphore>,
    metrics: Arc<LedgerQueryMetrics>,
}

impl LedgerQueryLimiter {
    /// Create a limiter allowing `permits` concurrent ledger queries.
    pub fn new(permits: NonZeroUsize) -> Self {
        let metrics = LedgerQueryMetrics::default();
        metrics.permits_total.set(permits.get() as f64);

        Self {
            semaphore: Arc::new(Semaphore::new(permits.get())),
            metrics: Arc::new(metrics),
        }
    }

    /// Acquire one permit, waiting (without occupying a worker) if all are in use. Hold the
    /// returned [`LedgerQueryPermit`] only for the ledger-DB work itself and drop it immediately
    /// after; in a subscription, acquire once per emitted item, never once per stream.
    pub async fn acquire(&self) -> LedgerQueryPermit {
        self.metrics.waiting.increment(1.0);
        // The semaphore is never closed, so `acquire_owned` cannot fail.
        let permit = Arc::clone(&self.semaphore)
            .acquire_owned()
            .await
            .expect("ledger query semaphore is never closed");
        self.metrics.waiting.decrement(1.0);

        self.metrics.in_flight.increment(1.0);
        self.metrics.acquired_total.increment(1);

        LedgerQueryPermit {
            _permit: permit,
            in_flight: self.metrics.in_flight.clone(),
        }
    }

    #[cfg(test)]
    fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }
}

/// RAII guard for one in-flight ledger query. On drop it releases the semaphore permit and
/// decrements the in-flight gauge.
#[derive(Debug)]
pub struct LedgerQueryPermit {
    _permit: OwnedSemaphorePermit,
    in_flight: Gauge,
}

impl Drop for LedgerQueryPermit {
    fn drop(&mut self) {
        self.in_flight.decrement(1.0);
    }
}

#[derive(Debug)]
struct LedgerQueryMetrics {
    /// Configured permit count (the concurrency bound).
    permits_total: Gauge,
    /// Ledger queries currently holding a permit.
    in_flight: Gauge,
    /// Callers currently waiting for a permit.
    waiting: Gauge,
    /// Total permits handed out since start.
    acquired_total: Counter,
}

impl Default for LedgerQueryMetrics {
    fn default() -> Self {
        Self {
            permits_total: gauge!("indexer_ledger_query_permits_total"),
            in_flight: gauge!("indexer_ledger_query_in_flight"),
            waiting: gauge!("indexer_ledger_query_waiting"),
            acquired_total: counter!("indexer_ledger_query_acquired_total"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_reserve_one_worker_for_liveness() {
        assert_eq!(permits_for_worker_threads(8).get(), 7);
        assert_eq!(permits_for_worker_threads(2).get(), 1);
    }

    #[test]
    fn permits_never_zero_on_tiny_hosts() {
        // A single-core host still gets one permit rather than a deadlocked zero.
        assert_eq!(permits_for_worker_threads(1).get(), 1);
        assert_eq!(permits_for_worker_threads(0).get(), 1);
    }

    #[tokio::test]
    async fn acquire_bounds_concurrency_and_releases_on_drop() {
        let limiter = LedgerQueryLimiter::new(NonZeroUsize::new(2).unwrap());
        assert_eq!(limiter.available_permits(), 2);

        let p1 = limiter.acquire().await;
        let p2 = limiter.acquire().await;
        assert_eq!(limiter.available_permits(), 0);

        // A third acquire cannot complete while both permits are held.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), limiter.acquire())
                .await
                .is_err(),
            "third acquire must block while the two permits are in flight"
        );

        // Dropping a permit frees a slot and unblocks a waiter.
        drop(p1);
        assert_eq!(limiter.available_permits(), 1);
        let _p3 = limiter.acquire().await;
        assert_eq!(limiter.available_permits(), 0);

        drop(p2);
        assert_eq!(limiter.available_permits(), 1);
    }
}
