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
//!
//! A long task, such as a backfill, can be cancelled partway, see [Task::run]: it journals
//! what it did as not [completed](Task::completed), and the next start runs it again from there.

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
/// Cancels running startup tasks, e.g. on SIGTERM; see [run] and [Task::run].
pub use tokio_util::sync::CancellationToken;

/// The future of a task step or of a plan: an `Item`, or the error that stopped it.
pub type BoxFuture<'a, Item = ()> =
    Pin<Box<dyn Future<Output = Result<Item, BoxError>> + Send + 'a>>;

/// What a task's run leaves in the journal: the task version it reached and the state it returned.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry<State> {
    /// The task version the run reached.
    pub version: u32,
    /// The state the run returned.
    pub state: State,
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
    fn read_state(_version: u32, state: Value) -> Result<Self::State, BoxError> {
        Ok(serde_json::from_value(state)?)
    }

    /// Whether a journaled state is of a run that completed the task's work. By default every run
    /// completes; a task that can be cancelled partway, see [Task::run], says whether a run did.
    fn completed(_state: &Self::State) -> bool {
        true
    }

    /// Whether the task needs to run, given its last journaled entry, `None` if it never ran.
    /// By default, if it never ran, ran at an older version or did not [complete](Self::completed).
    /// Override to check live state, as a repair would, or to run on every start, as a hook would.
    fn is_pending<'a>(
        &'a self,
        _context: &'a Context,
        previous: Option<&'a Entry<Self::State>>,
    ) -> BoxFuture<'a, bool> {
        let pending = previous.is_none_or(|previous| {
            previous.version < Self::VERSION || !Self::completed(&previous.state)
        });
        Box::pin(async move { Ok(pending) })
    }

    /// Run the task and return the state to journal. A task that can be cancelled partway checks
    /// `cancel` where it is safe to end early, and once cancelled, returns what it did so far as not
    /// [completed](Self::completed), so the next start resumes it.
    fn run<'a>(
        &'a self,
        context: &'a Context,
        previous: Option<&'a Entry<Self::State>>,
        cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Self::State>;
}

/// The journal: appends an entry per completed run and reads back each task's latest.
pub trait Journal: Send + Sync {
    /// The latest entry of the task with `id`, `None` if it never ran.
    fn latest(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<Option<Entry<Value>>, BoxError>> + Send;

    /// Append the entry of a run of the task with `id` that started and ended at the given times.
    fn append(
        &self,
        id: &str,
        entry: Entry<Value>,
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

/// Run the plan. Takes no lock: if several instances can start at once, take one first. Once
/// `cancel` is cancelled, no further task starts; check it afterwards to tell whether all ran.
///
/// # Errors
///
/// Fails before any task runs if the plan is invalid.
pub fn run<'a, Context: Sync, J: Journal>(
    context: &'a Context,
    journal: &'a J,
    plan: &'a Plan<Context>,
    cancel: &'a CancellationToken,
) -> BoxFuture<'a> {
    Box::pin(async move {
        plan.validate()?;

        match plan {
            Plan::Task(_) if cancel.is_cancelled() => Ok(()),
            Plan::Task(task) => run_task(context, journal, task.as_ref(), cancel).await,
            Plan::Sequential(plans) => {
                for plan in plans {
                    run(context, journal, plan, cancel).await?;
                }
                Ok(())
            }
            Plan::Parallel(plans) => {
                try_join_all(plans.iter().map(|plan| run(context, journal, plan, cancel)))
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
    cancel: &CancellationToken,
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
        let entry = task.execute(context, previous, cancel).await?;
        Ok::<_, BoxError>(entry.map(|entry| (entry, started_at)))
    };

    match executed.await {
        Ok(Some((entry, started_at))) if task.journal() => journal
            .append(task.id(), entry, started_at, SystemTime::now())
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

    /// [Task::FATAL].
    fn fatal(&self) -> bool;

    /// [Task::JOURNAL].
    fn journal(&self) -> bool;

    /// `None` if it was not pending, otherwise the entry to journal.
    fn execute<'a>(
        &'a self,
        context: &'a Context,
        previous: Option<Entry<Value>>,
        cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Option<Entry<Value>>>;
}

impl<Context: Sync, T: Task<Context>> DynTask<Context> for T {
    fn id(&self) -> &'static str {
        T::ID
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
        cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Option<Entry<Value>>> {
        Box::pin(async move {
            // An entry whose state no longer reads counts as none: the task runs again, which it
            // must be safe to do, rather than failing every start. One a newer version journaled,
            // as after a rollback, means a newer build did the work: the task is skipped.
            let previous = match previous {
                None => None,
                Some(Entry { version, state }) => match T::read_state(version, state) {
                    Ok(state) => Some(Entry { version, state }),
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

            // Cancelled meanwhile, e.g. while reading the journal, it does not start.
            if !self.is_pending(context, previous.as_ref()).await? || cancel.is_cancelled() {
                return Ok(None);
            }

            let state = self.run(context, previous.as_ref(), cancel).await?;
            Ok(Some(Entry {
                version: T::VERSION,
                state: serde_json::to_value(state)?,
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BoxFuture, CancellationToken, Entry, Journal, Task, parallel, run, sequential, task,
        unix_millis,
    };
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
            entry: Entry<Value>,
            _started_at: SystemTime,
            _ended_at: SystemTime,
        ) -> Result<(), BoxError> {
            self.0.lock().unwrap().push((id.to_owned(), entry));
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
            _cancel: &'a CancellationToken,
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
            _cancel: &'a CancellationToken,
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
                    _cancel: &'a CancellationToken,
                ) -> BoxFuture<'a, ()> {
                    Box::pin(async { Ok(()) })
                }
            }
        };
    }
    named!(First, "test/first");
    named!(Second, "test/second");
    named!(Third, "test/third");

    /// Takes `STEPS` steps over its runs, recording how many are done, and is cancelled once
    /// `cancel_at` are, as on SIGTERM: it then ends before the next step, not completed.
    struct Resumable {
        cancel_at: u32,
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Steps {
        done: u32,
        completed: bool,
    }

    const STEPS: u32 = 5;

    impl Task<Context> for Resumable {
        const ID: &'static str = "test/resumable";
        type State = Steps;

        fn completed(state: &Steps) -> bool {
            state.completed
        }

        fn run<'a>(
            &'a self,
            context: &'a Context,
            previous: Option<&'a Entry<Steps>>,
            cancel: &'a CancellationToken,
        ) -> BoxFuture<'a, Steps> {
            Box::pin(async move {
                let mut done = previous.map_or(0, |previous| previous.state.done);
                while done < STEPS {
                    if cancel.is_cancelled() {
                        return Ok(Steps {
                            done,
                            completed: false,
                        });
                    }
                    context.runs.fetch_add(1, Relaxed);
                    done += 1;
                    if done == self.cancel_at {
                        cancel.cancel();
                    }
                }
                Ok(Steps {
                    done,
                    completed: true,
                })
            })
        }
    }

    // A cancelled task journals what it did as not completed, and the next start runs it again
    // from there until it completes; then it no longer runs.
    #[tokio::test]
    async fn a_cancelled_task_resumes_until_completed() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());
        let latest = async || -> Result<(u32, bool), BoxError> {
            let entry = journal.latest("test/resumable").await?.expect("journaled");
            let steps = serde_json::from_value::<Steps>(entry.state)?;
            Ok((steps.done, steps.completed))
        };

        let cancel = CancellationToken::new();
        run(
            &context,
            &journal,
            &task(Resumable { cancel_at: 2 }),
            &cancel,
        )
        .await?;
        assert!(cancel.is_cancelled());
        assert_eq!(latest().await?, (2, false), "cancelled after two steps");

        run(
            &context,
            &journal,
            &task(Resumable { cancel_at: 0 }),
            &CancellationToken::new(),
        )
        .await?;
        assert_eq!(latest().await?, (STEPS, true), "resumed and completed");
        assert_eq!(context.runs.load(Relaxed), STEPS, "no step taken twice");

        run(
            &context,
            &journal,
            &task(Resumable { cancel_at: 0 }),
            &CancellationToken::new(),
        )
        .await?;
        assert_eq!(journal.ids().len(), 2, "not run once completed");

        Ok(())
    }

    /// Cancels its token while checking whether it is pending, as a SIGTERM arriving meanwhile.
    struct CancelledWhilePending(CancellationToken);

    impl Task<Context> for CancelledWhilePending {
        const ID: &'static str = "test/cancelled_while_pending";
        type State = ();

        fn is_pending<'a>(
            &'a self,
            _context: &'a Context,
            _previous: Option<&'a Entry<()>>,
        ) -> BoxFuture<'a, bool> {
            self.0.cancel();
            Box::pin(async { Ok(true) })
        }

        fn run<'a>(
            &'a self,
            context: &'a Context,
            _previous: Option<&'a Entry<()>>,
            _cancel: &'a CancellationToken,
        ) -> BoxFuture<'a, ()> {
            Box::pin(async move {
                context.runs.fetch_add(1, Relaxed);
                Ok(())
            })
        }
    }

    // A task cancelled while its entry is read or checked does not start.
    #[tokio::test]
    async fn a_task_cancelled_while_pending_does_not_start() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());

        let cancel = CancellationToken::new();
        run(
            &context,
            &journal,
            &task(CancelledWhilePending(cancel.clone())),
            &cancel,
        )
        .await?;
        assert_eq!(context.runs.load(Relaxed), 0, "not started");
        assert!(journal.ids().is_empty(), "nothing journaled");

        Ok(())
    }

    // Once cancelled, a run of a plan starts no further task; the next start runs them.
    #[tokio::test]
    async fn a_cancelled_run_starts_no_further_task() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());

        let plan = sequential([task(Resumable { cancel_at: 1 }), task(First)]);
        run(&context, &journal, &plan, &CancellationToken::new()).await?;
        assert_eq!(
            journal.ids(),
            vec!["test/resumable"],
            "the first task cancelled the run"
        );

        let cancel = CancellationToken::new();
        cancel.cancel();
        run(&context, &journal, &task(Second), &cancel).await?;
        assert_eq!(
            journal.ids(),
            vec!["test/resumable"],
            "nothing started once cancelled"
        );

        Ok(())
    }

    #[tokio::test]
    async fn runs_once_per_version() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());

        let v1 = task(Counting::<1, true> { fail: false });
        run(&context, &journal, &v1, &CancellationToken::new()).await?;
        run(&context, &journal, &v1, &CancellationToken::new()).await?;
        assert_eq!(context.runs.load(Relaxed), 1, "skipped at the same version");

        let v2 = task(Counting::<2, true> { fail: false });
        run(&context, &journal, &v2, &CancellationToken::new()).await?;
        run(&context, &journal, &v2, &CancellationToken::new()).await?;
        assert_eq!(
            context.runs.load(Relaxed),
            2,
            "ran again at a newer version"
        );
        assert_eq!(
            journal.latest("test/counting").await?,
            Some(Entry {
                version: 2,
                state: Value::from(2)
            }),
            "the newer run saw the older one's state"
        );
        assert_eq!(journal.ids().len(), 2, "one entry per completed run");

        run(&context, &journal, &v1, &CancellationToken::new()).await?;
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
        let entry = Entry {
            version: 1,
            state: Value::from("not a count"),
        };
        journal
            .append("test/counting", entry, SystemTime::now(), SystemTime::now())
            .await?;

        run(
            &context,
            &journal,
            &task(Counting::<1, true> { fail: false }),
            &CancellationToken::new(),
        )
        .await?;
        assert_eq!(context.runs.load(Relaxed), 1);
        assert_eq!(
            journal.latest("test/counting").await?,
            Some(Entry {
                version: 1,
                state: Value::from(1)
            })
        );

        Ok(())
    }

    // After a rollback, an entry a newer version journaled, which this version cannot read, leaves
    // the task skipped: it does not rerun over the newer version's work.
    #[tokio::test]
    async fn unreadable_state_of_a_newer_version_skips() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());
        let entry = Entry {
            version: 2,
            state: Value::from("not a count"),
        };
        journal
            .append("test/counting", entry, SystemTime::now(), SystemTime::now())
            .await?;

        run(
            &context,
            &journal,
            &task(Counting::<1, true> { fail: false }),
            &CancellationToken::new(),
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
            _cancel: &'a CancellationToken,
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
        let entry = Entry {
            version: 1,
            state: Value::from(5),
        };
        journal
            .append(
                "test/migrating",
                entry,
                SystemTime::now(),
                SystemTime::now(),
            )
            .await?;

        run(
            &context,
            &journal,
            &task(Migrating),
            &CancellationToken::new(),
        )
        .await?;
        assert_eq!(context.runs.load(Relaxed), 1);
        assert_eq!(
            journal.latest("test/migrating").await?,
            Some(Entry {
                version: 2,
                state: serde_json::json!([5, 6])
            })
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
                _entry: Entry<Value>,
                _started_at: SystemTime,
                _ended_at: SystemTime,
            ) -> Result<(), BoxError> {
                Err("append failed".into())
            }
        }

        let context = Context::default();
        let counting = task(Counting::<1, false> { fail: false });

        let error = run(
            &context,
            &FailingJournal,
            &counting,
            &CancellationToken::new(),
        )
        .await
        .expect_err("a failed append fails startup");
        assert_eq!(error.to_string(), "startup task test/counting failed");
        assert_eq!(
            error.source().map(ToString::to_string).as_deref(),
            Some("append failed")
        );

        run(
            &context,
            &MemoryJournal::default(),
            &counting,
            &CancellationToken::new(),
        )
        .await?;
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
            _entry: Entry<Value>,
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
        let error = run(
            &context,
            &UnreadableJournal,
            &fatal,
            &CancellationToken::new(),
        )
        .await
        .expect_err("a failed read fails a fatal task");
        assert_eq!(error.to_string(), "startup task test/counting failed");
        assert_eq!(
            error.source().map(ToString::to_string).as_deref(),
            Some("read failed")
        );

        let not_fatal = task(Counting::<1, false> { fail: false });
        run(
            &context,
            &UnreadableJournal,
            &not_fatal,
            &CancellationToken::new(),
        )
        .await?;
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
            let error = run(&context, &journal, plan, &CancellationToken::new())
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

        run(
            &context,
            &UnreadableJournal,
            &task(Hook),
            &CancellationToken::new(),
        )
        .await?;
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
            _cancel: &'a CancellationToken,
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
            _cancel: &'a CancellationToken,
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
            _cancel: &'a CancellationToken,
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
        within(run(&context, &journal, &plan, &CancellationToken::new())).await?;
        assert_eq!(context.finished.load(Relaxed), 2);
        assert_eq!(journal.ids().len(), 2);

        Ok(())
    }

    // A fatal failure in parallel stops its siblings, here one that would never finish.
    #[tokio::test]
    async fn a_fatal_failure_in_parallel_stops_its_siblings() {
        let (context, journal) = (Concurrent::new(1), MemoryJournal::default());

        let plan = parallel([task(Failing::<true>), task(Never)]);
        let result = within(run(&context, &journal, &plan, &CancellationToken::new())).await;
        assert!(result.is_err());
        assert!(journal.ids().is_empty());
    }

    // A failure that is not fatal lets its siblings in parallel finish.
    #[tokio::test]
    async fn a_failure_not_fatal_lets_parallel_siblings_finish() -> Result<(), BoxError> {
        let (context, journal) = (Concurrent::new(1), MemoryJournal::default());

        let plan = parallel([task(Failing::<false>), task(Waiting::<0>)]);
        within(run(&context, &journal, &plan, &CancellationToken::new())).await?;
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
        assert!(
            run(&context, &journal, &fatal, &CancellationToken::new())
                .await
                .is_err()
        );

        let not_fatal = task(Counting::<1, false> { fail: true });
        run(&context, &journal, &not_fatal, &CancellationToken::new()).await?;
        assert!(journal.ids().is_empty());

        // The failed task runs on the next start.
        run(
            &context,
            &journal,
            &task(Counting::<1, true> { fail: false }),
            &CancellationToken::new(),
        )
        .await?;
        assert_eq!(context.runs.load(Relaxed), 1);

        Ok(())
    }

    #[tokio::test]
    async fn hooks_run_every_start_without_entries() -> Result<(), BoxError> {
        let (context, journal) = (Context::default(), MemoryJournal::default());

        let hook = task(Hook);
        run(&context, &journal, &hook, &CancellationToken::new()).await?;
        run(&context, &journal, &hook, &CancellationToken::new()).await?;
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
        run(&context, &journal, &plan, &CancellationToken::new()).await?;
        assert_eq!(journal.ids(), ["test/first", "test/second", "test/third"]);

        // A fatal failure stops what follows it.
        let journal = MemoryJournal::default();
        let plan = sequential([task(Counting::<1, true> { fail: true }), task(First)]);
        assert!(
            run(&context, &journal, &plan, &CancellationToken::new())
                .await
                .is_err()
        );
        assert!(journal.ids().is_empty());

        Ok(())
    }
}
