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

//! Startup tasks: migrations of data rather than schema, backfills, repairs and hooks, composed
//! with [sequential] and [parallel]. Each task decides whether it needs to run from its last
//! journaled entry and, if it likes, from live state. Every run that completes is appended to the
//! [Journal], so the journal is also a log of what was done to the database, in order.

use crate::error::BoxError;
use futures::future::try_join_all;
use log::warn;
use serde::{Serialize, de::DeserializeOwned};
/// The journaled form of a task's state.
pub use serde_json::Value;
use std::{
    collections::HashSet,
    future::Future,
    pin::Pin,
    time::{SystemTime, UNIX_EPOCH},
};

/// The future of a task step or of a plan: an `Item`, or the error that stopped it.
pub type BoxFuture<'a, Item = ()> =
    Pin<Box<dyn Future<Output = Result<Item, BoxError>> + Send + 'a>>;

/// What a task's run left in the journal: where in the journal, the task version the run reached
/// and the state it returned.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry<State> {
    /// Orders the journal, increasing with each entry appended, so it tells apart the entries of
    /// two runs that are otherwise equal.
    pub seq: u64,
    /// The task version the run reached.
    pub version: u32,
    /// The state the run returned.
    pub state: State,
}

#[cfg(any(test, feature = "testing"))]
impl<State> Entry<State> {
    /// The entry at `seq` of a run that reached `version` and returned `state`.
    pub fn new(seq: u64, version: u32, state: State) -> Self {
        Self {
            seq,
            version,
            state,
        }
    }
}

/// A startup task. `Context` is whatever the tasks of an application need, e.g. its storage.
///
/// Its journal entry is appended once it has run, apart from its own work, so a task stopped
/// before then runs again on the next start: it must be safe to run again over what it already did.
pub trait Task<Context>: Send + Sync {
    /// Unique, stable across versions, e.g. `"ledger_db/pin_contract_states"`.
    const ID: &'static str;
    /// Bump to run the task again where it ran at an older version.
    const VERSION: u32 = 1;
    /// Whether a failure, of the task or of reading its entry, stops startup; if not, it is logged
    /// and startup carries on. A failure to append its entry always stops startup.
    const FATAL: bool = true;
    /// Whether a run is journaled. Hooks that run on every start and record nothing can opt out.
    const JOURNAL: bool = true;

    /// Kept between runs, e.g. a checkpoint; `()` if unused. Should stay readable across `VERSION`
    /// bumps, e.g. with `#[serde(default)]`, or be migrated by [Task::read_state]: an entry whose
    /// state no longer reads counts as none, so the task runs as if it never ran. One a newer
    /// version journaled, as after a rollback, leaves the task skipped instead, as the newer
    /// version did its work.
    type State: Serialize + DeserializeOwned + Send + Sync + 'static;

    /// The state journaled by `version` of this task, as this version reads it. By default it is
    /// deserialized as it is; override to migrate the shapes older versions wrote.
    fn read_state(version: u32, state: Value) -> Result<Self::State, BoxError> {
        let _ = version;
        Ok(serde_json::from_value(state)?)
    }

    /// Whether the task needs to run, given its last journaled entry, `None` if it never ran.
    /// By default, if it never ran or ran at an older version. Override to check live state, as a
    /// repair would, or to run on every start, as a hook would.
    fn is_pending<'a>(
        &'a self,
        _context: &'a Context,
        previous: Option<&'a Entry<Self::State>>,
    ) -> BoxFuture<'a, bool> {
        let pending = previous.is_none_or(|previous| previous.version < Self::VERSION);
        Box::pin(async move { Ok(pending) })
    }

    /// Run the task and return the state to journal.
    fn run<'a>(
        &'a self,
        context: &'a Context,
        previous: Option<&'a Entry<Self::State>>,
    ) -> BoxFuture<'a, Self::State>;
}

/// The journal: appends an entry per completed run and reads back each task's latest.
pub trait Journal: Send + Sync {
    /// The latest entry of the task with `id`, `None` if it never ran.
    fn latest(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<Option<Entry<Value>>, BoxError>> + Send;

    /// Append the entry of a run of the task with `id`, at the version it reached, with the state
    /// it returned, that started and ended at the given times.
    fn append(
        &self,
        id: &str,
        version: u32,
        state: Value,
        started_at: SystemTime,
        ended_at: SystemTime,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
}

/// A plan of tasks.
pub enum Plan<Context> {
    /// A single task; see [task].
    Task(Box<dyn DynTask<Context>>),
    /// In order, each after the previous completed.
    Sequential(Vec<Plan<Context>>),
    /// Concurrently. A fatal failure of one cancels the others.
    Parallel(Vec<Plan<Context>>),
}

impl<Context> Plan<Context> {
    /// The IDs of the plan's tasks, in plan order.
    fn ids(&self) -> Vec<&'static str> {
        use Plan::*;
        match self {
            Task(task) => vec![task.id()],
            Sequential(plans) | Parallel(plans) => plans.iter().flat_map(Plan::ids).collect(),
        }
    }

    /// Validates the plan.
    ///
    /// # Errors
    ///
    /// Fails if two tasks of the plan share an ID.
    fn validate(&self) -> Result<(), DuplicateTaskId> {
        let mut ids = HashSet::new();
        match self.ids().into_iter().find(|id| !ids.insert(*id)) {
            Some(id) => Err(DuplicateTaskId(id)),
            None => Ok(()),
        }
    }
}

/// A plan of the given task alone.
pub fn task<Context: Sync, T: Task<Context> + 'static>(task: T) -> Plan<Context> {
    Plan::Task(Box::new(task))
}

/// A plan running the given ones in order, each after the previous completed.
pub fn sequential<Context>(plans: impl IntoIterator<Item = Plan<Context>>) -> Plan<Context> {
    Plan::Sequential(plans.into_iter().collect())
}

/// A plan running the given ones concurrently. A fatal failure of one cancels the others.
pub fn parallel<Context>(plans: impl IntoIterator<Item = Plan<Context>>) -> Plan<Context> {
    Plan::Parallel(plans.into_iter().collect())
}

/// Run the plan. Takes no lock: if several instances can start at once, take one first.
///
/// # Errors
///
/// Fails before any task runs if the plan is invalid.
pub fn run<'a, Context: Sync, J: Journal>(
    context: &'a Context,
    journal: &'a J,
    plan: &'a Plan<Context>,
) -> BoxFuture<'a> {
    Box::pin(async move {
        plan.validate()?;

        match plan {
            Plan::Task(task) => run_task(context, journal, task.as_ref()).await,
            Plan::Sequential(plans) => {
                for plan in plans {
                    run(context, journal, plan).await?;
                }
                Ok(())
            }
            Plan::Parallel(plans) => {
                try_join_all(plans.iter().map(|plan| run(context, journal, plan)))
                    .await
                    .map(drop)
            }
        }
    })
}

async fn run_task<Context: Sync, J: Journal>(
    context: &Context,
    journal: &J,
    task: &dyn DynTask<Context>,
) -> Result<(), BoxError> {
    let failed = |source| TaskError {
        task: task.id(),
        source,
    };

    // A failed read of the task's entry is a failure of the task, so it follows FATAL. A task that
    // is not journaled has no entry to read.
    let executed = async {
        let previous = match task.journal() {
            true => journal.latest(task.id()).await?,
            false => None,
        };
        let started_at = SystemTime::now();
        let state = task.execute(context, previous).await?;
        Ok::<_, BoxError>(state.map(|state| (state, started_at)))
    };

    match executed.await {
        Ok(Some((state, started_at))) if task.journal() => journal
            .append(
                task.id(),
                task.version(),
                state,
                started_at,
                SystemTime::now(),
            )
            .await
            .map_err(|source| failed(source).into()),
        Ok(_) => Ok(()),
        Err(error) if !task.fatal() => {
            warn!(task = task.id(), error:%; "startup task failed, continuing");
            Ok(())
        }
        Err(source) => Err(failed(source).into()),
    }
}

/// A startup task failed, or its journal entry could not be read or written.
#[derive(Debug, thiserror::Error)]
#[error("startup task {task} failed")]
pub struct TaskError {
    /// [Task::ID].
    pub task: &'static str,
    #[source]
    pub source: BoxError,
}

/// Two tasks of a plan share an ID.
#[derive(Debug, thiserror::Error)]
#[error("startup task ID {0} is not unique in the plan")]
struct DuplicateTaskId(&'static str);

/// Milliseconds since the Unix epoch, as the journal stores times.
pub fn unix_millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

/// An object-safe view of [Task], implemented for every task.
pub trait DynTask<Context>: Send + Sync {
    /// [Task::ID].
    fn id(&self) -> &'static str;

    /// [Task::VERSION].
    fn version(&self) -> u32;

    /// [Task::FATAL].
    fn fatal(&self) -> bool;

    /// [Task::JOURNAL].
    fn journal(&self) -> bool;

    /// `None` if it was not pending, otherwise the state to journal.
    fn execute<'a>(
        &'a self,
        context: &'a Context,
        previous: Option<Entry<Value>>,
    ) -> BoxFuture<'a, Option<Value>>;
}

impl<Context: Sync, T: Task<Context>> DynTask<Context> for T {
    fn id(&self) -> &'static str {
        T::ID
    }

    fn version(&self) -> u32 {
        T::VERSION
    }

    fn fatal(&self) -> bool {
        T::FATAL
    }

    fn journal(&self) -> bool {
        T::JOURNAL
    }

    fn execute<'a>(
        &'a self,
        context: &'a Context,
        previous: Option<Entry<Value>>,
    ) -> BoxFuture<'a, Option<Value>> {
        Box::pin(async move {
            // An entry whose state no longer reads counts as none: the task runs again, which it
            // must be safe to do, rather than failing every start. One a newer version journaled,
            // as after a rollback, means a newer build did the work: the task is skipped.
            let previous = match previous {
                None => None,
                Some(Entry {
                    seq,
                    version,
                    state,
                }) => match T::read_state(version, state) {
                    Ok(state) => Some(Entry {
                        seq,
                        version,
                        state,
                    }),
                    Err(error) if version > T::VERSION => {
                        warn!(
                            task = T::ID,
                            version,
                            error:%;
                            "journaled state of a newer version does not read, skipping"
                        );
                        return Ok(None);
                    }
                    Err(error) => {
                        warn!(
                            task = T::ID,
                            version,
                            error:%;
                            "journaled state does not read, running as if never run"
                        );
                        None
                    }
                },
            };

            if !self.is_pending(context, previous.as_ref()).await? {
                return Ok(None);
            }

            let state = self.run(context, previous.as_ref()).await?;
            Ok(Some(serde_json::to_value(state)?))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{BoxFuture, Entry, Journal, Task, parallel, run, sequential, task, unix_millis};
    use crate::error::BoxError;
    use serde_json::Value;
    use std::{
        sync::{
            Mutex,
            atomic::{AtomicU32, Ordering::Relaxed},
        },
        time::SystemTime,
    };

    /// Appends to a vector.
    #[derive(Default)]
    struct MemoryJournal(Mutex<Vec<(String, Entry<Value>)>>);

    impl MemoryJournal {
        fn ids(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .map(|(id, _)| id.clone())
                .collect()
        }
    }

    impl Journal for MemoryJournal {
        async fn latest(&self, id: &str) -> Result<Option<Entry<Value>>, BoxError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|(entry_id, _)| entry_id == id)
                .map(|(_, entry)| entry.clone()))
        }

        async fn append(
            &self,
            id: &str,
            version: u32,
            state: Value,
            _started_at: SystemTime,
            _ended_at: SystemTime,
        ) -> Result<(), BoxError> {
            let mut entries = self.0.lock().unwrap();
            let seq = entries.len() as u64 + 1;
            entries.push((
                id.to_owned(),
                Entry {
                    seq,
                    version,
                    state,
                },
            ));
            Ok(())
        }
    }

    /// Counts its runs in the context, records the previous run count as its state, and fails if
    /// told to.
    struct Counting<const V: u32, const F: bool> {
        fail: bool,
    }

    #[derive(Default)]
    struct Context {
        runs: AtomicU32,
    }

    impl<const V: u32, const F: bool> Task<Context> for Counting<V, F> {
        const ID: &'static str = "test/counting";
        const VERSION: u32 = V;
        const FATAL: bool = F;
        type State = u32;

        fn run<'a>(
            &'a self,
            context: &'a Context,
            previous: Option<&'a Entry<u32>>,
        ) -> BoxFuture<'a, u32> {
            Box::pin(async move {
                if self.fail {
                    return Err("failed".into());
                }
                context.runs.fetch_add(1, Relaxed);
                Ok(previous.map_or(1, |previous| previous.state + 1))
            })
        }
    }

    /// Runs on every start without journaling.
    struct Hook;

    impl Task<Context> for Hook {
        const ID: &'static str = "test/hook";
        const JOURNAL: bool = false;
        type State = ();

        fn is_pending<'a>(
            &'a self,
            _context: &'a Context,
            _previous: Option<&'a Entry<()>>,
        ) -> BoxFuture<'a, bool> {
            Box::pin(async { Ok(true) })
        }

        fn run<'a>(
            &'a self,
            context: &'a Context,
            _previous: Option<&'a Entry<()>>,
        ) -> BoxFuture<'a, ()> {
            Box::pin(async move {
                context.runs.fetch_add(1, Relaxed);
                Ok(())
            })
        }
    }

    /// Does nothing but get journaled, to show the order of runs.
    macro_rules! named {
        ($name:ident, $id:literal) => {
            struct $name;
            impl Task<Context> for $name {
                const ID: &'static str = $id;
                type State = ();
                fn run<'a>(
                    &'a self,
                    _context: &'a Context,
                    _previous: Option<&'a Entry<()>>,
                ) -> BoxFuture<'a, ()> {
                    Box::pin(async { Ok(()) })
                }
            }
        };
    }
    named!(First, "test/first");
    named!(Second, "test/second");
    named!(Third, "test/third");

    #[tokio::test]
    async fn runs_once_per_version() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());

        let v1 = task(Counting::<1, true> { fail: false });
        run(&context, &journal, &v1).await?;
        run(&context, &journal, &v1).await?;
        assert_eq!(context.runs.load(Relaxed), 1, "skipped at the same version");

        let v2 = task(Counting::<2, true> { fail: false });
        run(&context, &journal, &v2).await?;
        run(&context, &journal, &v2).await?;
        assert_eq!(
            context.runs.load(Relaxed),
            2,
            "ran again at a newer version"
        );
        assert_eq!(
            journal.latest("test/counting").await?,
            Some(Entry::new(2, 2, Value::from(2))),
            "the newer run saw the older one's state"
        );
        assert_eq!(journal.ids().len(), 2, "one entry per completed run");

        run(&context, &journal, &v1).await?;
        assert_eq!(
            context.runs.load(Relaxed),
            2,
            "an older version does not run after a newer one"
        );

        Ok(())
    }

    // An entry whose state no longer reads counts as none, rather than failing every start.
    #[tokio::test]
    async fn unreadable_state_runs_as_if_never_run() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());
        journal
            .append(
                "test/counting",
                1,
                Value::from("not a count"),
                SystemTime::now(),
                SystemTime::now(),
            )
            .await?;

        run(
            &context,
            &journal,
            &task(Counting::<1, true> { fail: false }),
        )
        .await?;
        assert_eq!(context.runs.load(Relaxed), 1);
        assert_eq!(
            journal.latest("test/counting").await?,
            Some(Entry::new(2, 1, Value::from(1)))
        );

        Ok(())
    }

    // After a rollback, an entry a newer version journaled, which this version cannot read, leaves
    // the task skipped: it does not rerun over the newer version's work.
    #[tokio::test]
    async fn unreadable_state_of_a_newer_version_skips() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());
        journal
            .append(
                "test/counting",
                2,
                Value::from("not a count"),
                SystemTime::now(),
                SystemTime::now(),
            )
            .await?;

        run(
            &context,
            &journal,
            &task(Counting::<1, true> { fail: false }),
        )
        .await?;
        assert_eq!(context.runs.load(Relaxed), 0);
        assert_eq!(journal.ids().len(), 1, "nothing journaled");

        Ok(())
    }

    /// Version 2 of a task whose version 1 journaled a single count: it journals every count, and
    /// reads version 1's count as the first.
    struct Migrating;

    impl Task<Context> for Migrating {
        const ID: &'static str = "test/migrating";
        const VERSION: u32 = 2;
        type State = Vec<u32>;

        fn read_state(version: u32, state: Value) -> Result<Vec<u32>, BoxError> {
            match version {
                1 => Ok(vec![serde_json::from_value(state)?]),
                _ => Ok(serde_json::from_value(state)?),
            }
        }

        fn run<'a>(
            &'a self,
            context: &'a Context,
            previous: Option<&'a Entry<Vec<u32>>>,
        ) -> BoxFuture<'a, Vec<u32>> {
            Box::pin(async move {
                context.runs.fetch_add(1, Relaxed);
                let mut counts = previous.map_or_else(Vec::new, |previous| previous.state.clone());
                counts.push(counts.last().map_or(1, |count| count + 1));
                Ok(counts)
            })
        }
    }

    // A newer version reads the state an older one journaled, migrating its shape.
    #[tokio::test]
    async fn a_newer_version_migrates_older_state() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());
        journal
            .append(
                "test/migrating",
                1,
                Value::from(5),
                SystemTime::now(),
                SystemTime::now(),
            )
            .await?;

        run(&context, &journal, &task(Migrating)).await?;
        assert_eq!(context.runs.load(Relaxed), 1);
        assert_eq!(
            journal.latest("test/migrating").await?,
            Some(Entry::new(2, 2, serde_json::json!([5, 6])))
        );

        Ok(())
    }

    // A run whose entry is not appended is not journaled, so it runs again on the next start, and
    // the error names the task.
    #[tokio::test]
    async fn a_run_not_journaled_runs_again() -> Result<(), BoxError> {
        /// Reads like an empty journal and fails every append.
        struct FailingJournal;

        impl Journal for FailingJournal {
            async fn latest(&self, _id: &str) -> Result<Option<Entry<Value>>, BoxError> {
                Ok(None)
            }

            async fn append(
                &self,
                _id: &str,
                _version: u32,
                _state: Value,
                _started_at: SystemTime,
                _ended_at: SystemTime,
            ) -> Result<(), BoxError> {
                Err("append failed".into())
            }
        }

        let context = Context::default();
        let counting = task(Counting::<1, false> { fail: false });

        let error = run(&context, &FailingJournal, &counting)
            .await
            .expect_err("a failed append fails startup");
        assert_eq!(error.to_string(), "startup task test/counting failed");
        assert_eq!(
            error.source().map(ToString::to_string).as_deref(),
            Some("append failed")
        );

        run(&context, &MemoryJournal::default(), &counting).await?;
        assert_eq!(context.runs.load(Relaxed), 2, "ran again");

        Ok(())
    }

    /// Fails every read.
    struct UnreadableJournal;

    impl Journal for UnreadableJournal {
        async fn latest(&self, _id: &str) -> Result<Option<Entry<Value>>, BoxError> {
            Err("read failed".into())
        }

        async fn append(
            &self,
            _id: &str,
            _version: u32,
            _state: Value,
            _started_at: SystemTime,
            _ended_at: SystemTime,
        ) -> Result<(), BoxError> {
            Err("append failed".into())
        }
    }

    // A failed read of the journal stops startup for a fatal task, and skips a task that is not.
    #[tokio::test]
    async fn a_failed_journal_read_follows_fatal() -> Result<(), BoxError> {
        let context = Context::default();

        let fatal = task(Counting::<1, true> { fail: false });
        let error = run(&context, &UnreadableJournal, &fatal)
            .await
            .expect_err("a failed read fails a fatal task");
        assert_eq!(error.to_string(), "startup task test/counting failed");
        assert_eq!(
            error.source().map(ToString::to_string).as_deref(),
            Some("read failed")
        );

        let not_fatal = task(Counting::<1, false> { fail: false });
        run(&context, &UnreadableJournal, &not_fatal).await?;
        assert_eq!(context.runs.load(Relaxed), 0, "skipped");

        Ok(())
    }

    // A plan whose tasks share an ID fails before any task runs, nested or not.
    #[tokio::test]
    async fn duplicate_task_ids_fail_the_plan() {
        let (context, journal) = (Context::default(), MemoryJournal::default());

        let plans = [
            parallel([
                task(Counting::<1, true> { fail: false }),
                task(Counting::<1, true> { fail: false }),
            ]),
            sequential([
                task(Counting::<1, true> { fail: false }),
                parallel([task(First), task(Counting::<2, true> { fail: false })]),
            ]),
        ];
        for plan in &plans {
            let error = run(&context, &journal, plan)
                .await
                .expect_err("a duplicate ID fails the plan");
            assert_eq!(
                error.to_string(),
                "startup task ID test/counting is not unique in the plan"
            );
        }
        assert_eq!(context.runs.load(Relaxed), 0);
        assert!(journal.ids().is_empty());
    }

    // A task that is not journaled does not read the journal, so it runs even when the journal
    // cannot be read.
    #[tokio::test]
    async fn a_hook_runs_without_reading_the_journal() -> Result<(), BoxError> {
        let context = Context::default();

        run(&context, &UnreadableJournal, &task(Hook)).await?;
        assert_eq!(context.runs.load(Relaxed), 1);

        Ok(())
    }

    /// The context of the parallel tests: a barrier that only lets its waiters through together.
    struct Concurrent {
        barrier: tokio::sync::Barrier,
        finished: AtomicU32,
    }

    impl Concurrent {
        fn new(waiters: usize) -> Self {
            Self {
                barrier: tokio::sync::Barrier::new(waiters),
                finished: AtomicU32::new(0),
            }
        }
    }

    /// Waits at the barrier, then counts itself finished.
    struct Waiting<const N: u8>;

    impl<const N: u8> Task<Concurrent> for Waiting<N> {
        const ID: &'static str = if N == 0 {
            "test/waiting/0"
        } else {
            "test/waiting/1"
        };
        type State = ();

        fn run<'a>(
            &'a self,
            context: &'a Concurrent,
            _previous: Option<&'a Entry<()>>,
        ) -> BoxFuture<'a, ()> {
            Box::pin(async move {
                context.barrier.wait().await;
                context.finished.fetch_add(1, Relaxed);
                Ok(())
            })
        }
    }

    /// Fails at once.
    struct Failing<const F: bool>;

    impl<const F: bool> Task<Concurrent> for Failing<F> {
        const ID: &'static str = "test/failing";
        const FATAL: bool = F;
        type State = ();

        fn run<'a>(
            &'a self,
            _context: &'a Concurrent,
            _previous: Option<&'a Entry<()>>,
        ) -> BoxFuture<'a, ()> {
            Box::pin(async { Err("failed".into()) })
        }
    }

    /// Never finishes.
    struct Never;

    impl Task<Concurrent> for Never {
        const ID: &'static str = "test/never";
        type State = ();

        fn run<'a>(
            &'a self,
            _context: &'a Concurrent,
            _previous: Option<&'a Entry<()>>,
        ) -> BoxFuture<'a, ()> {
            Box::pin(std::future::pending())
        }
    }

    /// Bounds a run, so a test that would hang fails instead.
    async fn within<T>(run: impl Future<Output = T>) -> T {
        tokio::time::timeout(std::time::Duration::from_secs(10), run)
            .await
            .expect("finishes in time")
    }

    // Parallel tasks run concurrently: each waits for the other at the barrier.
    #[tokio::test]
    async fn parallel_tasks_run_concurrently() -> Result<(), BoxError> {
        let (context, journal) = (Concurrent::new(2), MemoryJournal::default());

        let plan = parallel([task(Waiting::<0>), task(Waiting::<1>)]);
        within(run(&context, &journal, &plan)).await?;
        assert_eq!(context.finished.load(Relaxed), 2);
        assert_eq!(journal.ids().len(), 2);

        Ok(())
    }

    // A fatal failure in parallel stops its siblings, here one that would never finish.
    #[tokio::test]
    async fn a_fatal_failure_in_parallel_stops_its_siblings() {
        let (context, journal) = (Concurrent::new(1), MemoryJournal::default());

        let plan = parallel([task(Failing::<true>), task(Never)]);
        let result = within(run(&context, &journal, &plan)).await;
        assert!(result.is_err());
        assert!(journal.ids().is_empty());
    }

    // A failure that is not fatal lets its siblings in parallel finish.
    #[tokio::test]
    async fn a_failure_not_fatal_lets_parallel_siblings_finish() -> Result<(), BoxError> {
        let (context, journal) = (Concurrent::new(1), MemoryJournal::default());

        let plan = parallel([task(Failing::<false>), task(Waiting::<0>)]);
        within(run(&context, &journal, &plan)).await?;
        assert_eq!(context.finished.load(Relaxed), 1);
        assert_eq!(journal.ids(), ["test/waiting/0"]);

        Ok(())
    }

    #[test]
    fn unix_millis_counts_from_the_epoch() {
        use std::time::{Duration, UNIX_EPOCH};

        assert_eq!(
            unix_millis(UNIX_EPOCH + Duration::from_millis(1_234)),
            1_234
        );
        assert_eq!(
            unix_millis(UNIX_EPOCH - Duration::from_millis(1)),
            0,
            "before the epoch"
        );
    }

    #[tokio::test]
    async fn failures_are_not_journaled() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());

        let fatal = task(Counting::<1, true> { fail: true });
        assert!(run(&context, &journal, &fatal).await.is_err());

        let not_fatal = task(Counting::<1, false> { fail: true });
        run(&context, &journal, &not_fatal).await?;
        assert!(journal.ids().is_empty());

        // The failed task runs on the next start.
        run(
            &context,
            &journal,
            &task(Counting::<1, true> { fail: false }),
        )
        .await?;
        assert_eq!(context.runs.load(Relaxed), 1);

        Ok(())
    }

    #[tokio::test]
    async fn hooks_run_every_start_without_entries() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());

        let hook = task(Hook);
        run(&context, &journal, &hook).await?;
        run(&context, &journal, &hook).await?;
        assert_eq!(context.runs.load(Relaxed), 2);
        assert!(journal.ids().is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn plans_compose() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());

        let plan = sequential([
            task(First),
            parallel([task(Second), task(Hook)]),
            task(Third),
        ]);
        run(&context, &journal, &plan).await?;
        assert_eq!(journal.ids(), ["test/first", "test/second", "test/third"]);

        // A fatal failure stops what follows it.
        let journal = MemoryJournal::default();
        let plan = sequential([task(Counting::<1, true> { fail: true }), task(First)]);
        assert!(run(&context, &journal, &plan).await.is_err());
        assert!(journal.ids().is_empty());

        Ok(())
    }
}
