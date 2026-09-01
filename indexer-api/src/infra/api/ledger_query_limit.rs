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
//! (`indexer-common/src/infra/ledger_db/v1_1.rs`) via `block_in_place`. That call does *not*
//! consume the worker: tokio hands the worker's core to a freshly spawned blocking-pool thread
//! (`spawn_blocking(move || run(worker))`, `multi_thread/worker.rs`) and the calling thread then
//! sits in `Handle::current().block_on` for the duration of the ledger walk. Live cores therefore
//! stay at `worker_threads` no matter how many walks are in flight; what grows is the **blocking
//! pool**, roughly one thread per concurrent walk.
//!
//! The pool is capped at `max_blocking_threads + worker_threads`
//! (`runtime/builder.rs`, `build_threaded_runtime`). At the cap the next core handoff is queued with
//! no thread to run it (`runtime/blocking/pool.rs`), so cores go dark one by one until the runtime
//! stops making progress and even `/live` stops answering. Nothing else in either binary is a
//! meaningful user of the blocking pool, so ledger walks are the only realistic way to reach the
//! cap — and unauthenticated queries such as `dustGenerationMerkleTreeUpdate` reach the walk with no
//! credentials, passing every existing limit (`max_complexity`, `limit_depth`, the WebSocket-only
//! subscription quota). Issue #595, "amber-heron".
//!
//! Memory bites before the cap does: blocking threads inherit `thread_stack_size` (24 MiB in both
//! `config.yaml`s), and ledger deserialization is deliberately deep-recursive, so hundreds of
//! concurrent walks commit stacks in the gigabytes.
//!
//! [`LedgerQueryLimiter`] bounds concurrent walks with a shared [`Semaphore`], sized off the
//! ledger DB's connection pool rather than off the core count (see [`default_permits`]) — a walk
//! is a chain of dependent round-trips, so connections are what it contends for, and the blocking
//! pool only caps how many can be resident at all. Every ledger-DB-touching resolver acquires one permit *for
//! the duration of its ledger work and no longer*: one-shot queries hold it across their single
//! walk, and subscriptions acquire per emitted item rather than once per stream, so a long-lived
//! subscription never pins a permit while idle.

use metrics::{Counter, Gauge, counter, gauge};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Cap for the Tokio blocking pool, applied explicitly rather than left at tokio's default of 512:
/// at the configured 24 MiB stack size that default is ~12 GiB of thread stacks, and the pool's
/// only real user is the `block_in_place` core handoff of a ledger walk.
pub const DEFAULT_MAX_BLOCKING_THREADS: NonZeroUsize = NonZeroUsize::new(64).unwrap();

/// Fraction of the ledger DB's connection pool that ledger queries may occupy by default. Walks
/// get half; the rest stays available to every other resolver sharing that pool, so a ledger query
/// storm degrades into latency for ledger queries rather than connection starvation for the whole
/// API — readiness probes included.
const LEDGER_POOL_SHARE: usize = 2;

/// Blocking threads left outside the ledger-query budget when the connection pool would otherwise
/// justify more permits than the pool cap allows, so a small `max_blocking_threads` still leaves
/// room for the core handoffs of unrelated blocking work.
const BLOCKING_POOL_RESERVE: usize = 1;

/// Default number of concurrent ledger queries: half the ledger DB's connection pool, capped below
/// `max_blocking_threads` and clamped to at least one.
///
/// Sized off connections, not cores and not the blocking pool. A walk is I/O-bound — a long chain
/// of dependent round-trips — so connections are the resource it contends for, shared in cloud
/// with every other resolver (`max_connections: 25`, halved here to 12). Sizing off
/// `worker_threads` would serialize every ledger query on a two-core pod; sizing off the blocking
/// pool would admit more walks than there are connections to serve them.
///
/// In standalone the ledger DB is SQLite with a single connection, so the same rule lands on one
/// permit — walks already serialize on that connection, and admitting more would only pin blocking
/// threads waiting for it.
pub fn default_permits(
    ledger_db_max_connections: NonZeroUsize,
    max_blocking_threads: NonZeroUsize,
) -> NonZeroUsize {
    let by_connections = ledger_db_max_connections.get() / LEDGER_POOL_SHARE;
    let by_blocking_pool = max_blocking_threads
        .get()
        .saturating_sub(BLOCKING_POOL_RESERVE);

    NonZeroUsize::new(by_connections.min(by_blocking_pool)).unwrap_or(NonZeroUsize::MIN)
}

/// Reject a configured bound that cannot bound anything: at or above the blocking-pool cap, ledger
/// queries can still drive the pool to exhaustion and wedge the runtime.
pub fn validate_permits(
    permits: NonZeroUsize,
    max_blocking_threads: NonZeroUsize,
) -> Result<(), InvalidLedgerQueryConcurrency> {
    if permits >= max_blocking_threads {
        Err(InvalidLedgerQueryConcurrency {
            permits,
            max_blocking_threads,
        })
    } else {
        Ok(())
    }
}

/// `ledger_query_concurrency` is large enough to defeat its own purpose.
#[derive(Debug, thiserror::Error)]
#[error(
    "ledger_query_concurrency ({permits}) must stay below max_blocking_threads \
     ({max_blocking_threads}); at or above it, ledger queries can exhaust the blocking pool and \
     wedge the runtime"
)]
pub struct InvalidLedgerQueryConcurrency {
    permits: NonZeroUsize,
    max_blocking_threads: NonZeroUsize,
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
        metrics.permits.set(permits.get() as f64);

        Self {
            semaphore: Arc::new(Semaphore::new(permits.get())),
            metrics: Arc::new(metrics),
        }
    }

    /// Acquire one permit, waiting (without occupying a worker) if all are in use. Hold the
    /// returned [`LedgerQueryPermit`] only for the ledger-DB work itself and drop it immediately
    /// after; in a subscription, acquire once per emitted item, never once per stream.
    pub async fn acquire(&self) -> LedgerQueryPermit {
        // Count the wait through a guard rather than a bare increment/decrement pair: this future
        // is dropped whenever the client disconnects, which under exactly the load this bounds is
        // the common case, and a dropped future must not leave the gauge incremented forever.
        let waiting = WaitingGuard::new(&self.metrics);

        // The semaphore is never closed, so `acquire_owned` cannot fail.
        let permit = Arc::clone(&self.semaphore)
            .acquire_owned()
            .await
            .expect("ledger query semaphore is never closed");
        drop(waiting);

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

    #[cfg(test)]
    fn waiting(&self) -> usize {
        self.metrics.waiting_count.load(Ordering::Relaxed)
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

/// Counts one caller queued for a permit for as long as it lives, including when the awaiting
/// future is dropped rather than completed.
struct WaitingGuard {
    metrics: Arc<LedgerQueryMetrics>,
}

impl WaitingGuard {
    fn new(metrics: &Arc<LedgerQueryMetrics>) -> Self {
        let metrics = Arc::clone(metrics);
        metrics.set_waiting(metrics.waiting_count.fetch_add(1, Ordering::Relaxed) + 1);

        Self { metrics }
    }
}

impl Drop for WaitingGuard {
    fn drop(&mut self) {
        let waiting = self.metrics.waiting_count.fetch_sub(1, Ordering::Relaxed) - 1;
        self.metrics.set_waiting(waiting);
    }
}

#[derive(Debug)]
struct LedgerQueryMetrics {
    /// Configured permit count (the concurrency bound).
    permits: Gauge,
    /// Ledger queries currently holding a permit.
    in_flight: Gauge,
    /// Callers currently waiting for a permit.
    waiting: Gauge,
    /// Total permits handed out since start.
    acquired_total: Counter,
    /// Source of truth behind `waiting`: the gauge is set from it so a dropped waiter cannot leave
    /// the two out of step.
    waiting_count: AtomicUsize,
}

impl LedgerQueryMetrics {
    fn set_waiting(&self, waiting: usize) {
        self.waiting.set(waiting as f64);
    }
}

impl Default for LedgerQueryMetrics {
    fn default() -> Self {
        Self {
            permits: gauge!("indexer_ledger_query_permits"),
            in_flight: gauge!("indexer_ledger_query_in_flight"),
            waiting: gauge!("indexer_ledger_query_waiting"),
            acquired_total: counter!("indexer_ledger_query_acquired_total"),
            waiting_count: AtomicUsize::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The cloud ledger DB shares the API's Postgres pool; see `indexer-api/config.yaml`.
    const CLOUD_MAX_CONNECTIONS: NonZeroUsize = NonZeroUsize::new(25).unwrap();

    #[test]
    fn permits_default_to_half_the_connection_pool() {
        assert_eq!(
            default_permits(CLOUD_MAX_CONNECTIONS, DEFAULT_MAX_BLOCKING_THREADS).get(),
            12
        );
        assert_eq!(
            default_permits(NonZeroUsize::new(10).unwrap(), DEFAULT_MAX_BLOCKING_THREADS).get(),
            5
        );
    }

    #[test]
    fn permits_leave_connections_for_the_rest_of_the_api() {
        // The regression this guards: sizing off the blocking pool admitted 32 concurrent walks
        // against a 25-connection pool shared with every other resolver and the readiness probe.
        let permits = default_permits(CLOUD_MAX_CONNECTIONS, DEFAULT_MAX_BLOCKING_THREADS);
        assert!(
            permits.get() < CLOUD_MAX_CONNECTIONS.get(),
            "ledger walks must not be able to claim the whole connection pool"
        );
    }

    #[test]
    fn permits_never_zero_on_a_single_connection_pool() {
        // Standalone: SQLite with one connection, which already serializes walks.
        assert_eq!(
            default_permits(NonZeroUsize::MIN, DEFAULT_MAX_BLOCKING_THREADS).get(),
            1
        );
    }

    #[test]
    fn permits_stay_below_a_small_blocking_pool() {
        // A hand-lowered `max_blocking_threads` must not leave the default failing its own
        // validation: the connection pool would justify 12 permits, the pool cap allows 3.
        let max_blocking_threads = NonZeroUsize::new(4).unwrap();
        let permits = default_permits(CLOUD_MAX_CONNECTIONS, max_blocking_threads);

        assert_eq!(permits.get(), 3);
        assert!(validate_permits(permits, max_blocking_threads).is_ok());
    }

    #[test]
    fn permits_are_independent_of_the_core_count() {
        // The regression this guards: sizing off `worker_threads` gave a two-core pod exactly one
        // concurrent ledger query, serializing every wallet sync.
        let permits = default_permits(CLOUD_MAX_CONNECTIONS, DEFAULT_MAX_BLOCKING_THREADS);
        assert!(
            permits.get() > 1,
            "the default bound must not serialize ledger queries"
        );
    }

    #[test]
    fn a_bound_at_or_above_the_pool_cap_is_rejected() {
        let pool = NonZeroUsize::new(64).unwrap();
        assert!(validate_permits(NonZeroUsize::new(63).unwrap(), pool).is_ok());
        assert!(validate_permits(NonZeroUsize::new(64).unwrap(), pool).is_err());
        assert!(validate_permits(NonZeroUsize::new(4096).unwrap(), pool).is_err());
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
            tokio::time::timeout(Duration::from_millis(50), limiter.acquire())
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

    #[tokio::test]
    async fn waiting_is_released_when_the_awaiting_future_is_dropped() {
        let limiter = LedgerQueryLimiter::new(NonZeroUsize::MIN);
        let _held = limiter.acquire().await;
        assert_eq!(limiter.waiting(), 0);

        // A client that disconnects while queued drops the resolver future mid-await. The wait
        // count must not drift up: this is the leak the drop guard exists for.
        for _ in 0..10 {
            assert!(
                tokio::time::timeout(Duration::from_millis(10), limiter.acquire())
                    .await
                    .is_err()
            );
        }

        assert_eq!(limiter.waiting(), 0, "dropped waiters must not leak");
    }
}
