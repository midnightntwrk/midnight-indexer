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

//! The chain indexer's startup tasks; the plan running them is in `application`.

use crate::domain::{LedgerState, storage::Storage};
pub use indexer_common::startup::run;
use indexer_common::{
    domain::{
        SerializedContractStateKey, SerializedZswapStateKey,
        ledger::{ArenaHash, Error},
    },
    error::BoxError,
    startup::{BoxFuture, CancellationToken, Entry, Task, Value},
    version::git_sha,
};
use itertools::Itertools;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::{fmt::Display, time::Instant};
use tokio::task::block_in_place;

/// Pins the contract states and contract zswap states stored before pinning existed, and
/// everything they reach, so gc's sweep skips them and gc no longer marks from them. Contract
/// states written since are pinned as their block is indexed; see `LedgerState::pin_roots`.
///
/// Safe to run again: the walk stops at pinned nodes, so a rerun pins only what is left. Cancelled,
/// as on SIGTERM, it journals its progress, and the next start resumes it.
///
/// A stored key that cannot be read is logged and left unpinned, the others pinned all the same,
/// rather than failing every start: a rerun would not read it either.
pub(super) struct PinContractStates;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct PinnedContractStates {
    /// Whether all roots were processed; if not, it was cancelled and the next start resumes it.
    #[serde(default = "completed_default")]
    completed: bool,
    /// Nodes pinned, counted over the runs since the last that completed.
    nodes: usize,
    /// Roots left unpinned, as a node they reach is missing; their hashes are logged. gc marks them
    /// as before.
    #[serde(default)]
    failed_roots: usize,
    /// Stored keys that cannot be read, left unpinned; they are logged. gc marks their roots as
    /// before.
    #[serde(default)]
    invalid_keys: usize,
}

fn completed_default() -> bool {
    true
}

impl PinContractStates {
    /// The roots the given stored keys refer to, and the number of keys that cannot be read, each
    /// logged with why.
    fn roots(
        state_keys: &[SerializedContractStateKey],
        zswap_state_keys: &[SerializedZswapStateKey],
    ) -> (Vec<ArenaHash>, usize) {
        let (roots, invalid_keys) = state_keys
            .iter()
            .map(|key| ArenaHash::try_from(key).inspect_err(|error| Self::warn_invalid(key, error)))
            .chain(zswap_state_keys.iter().map(|key| {
                ArenaHash::try_from(key).inspect_err(|error| Self::warn_invalid(key, error))
            }))
            .partition_result::<Vec<_>, Vec<_>, _, _>();
        (roots, invalid_keys.len())
    }

    fn warn_invalid(key: impl Display, error: &Error) {
        warn!(key:%, error:%; "stored contract state key that cannot be read left unpinned");
    }
}

impl<S: Storage> Task<S> for PinContractStates {
    const ID: &'static str = "ledger_db/pin_contract_states";
    type State = PinnedContractStates;

    fn completed(state: &Self::State) -> bool {
        state.completed
    }

    fn run<'a>(
        &'a self,
        storage: &'a S,
        previous: Option<&'a Entry<Self::State>>,
        cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Self::State> {
        Box::pin(async move {
            // Nodes pinned by any cancelled runs before this one.
            let pinned_before = previous
                .filter(|previous| !previous.state.completed)
                .map_or(0, |previous| previous.state.nodes);

            let started = Instant::now();
            let (state_keys, zswap_state_keys) = storage.get_contract_state_keys().await?;
            let (roots, invalid_keys) = Self::roots(&state_keys, &zswap_state_keys);

            let roots_count = roots.len();
            info!(
                roots = roots_count,
                invalid_keys,
                elapsed:? = started.elapsed();
                "pinning the contract state history, read the stored keys"
            );
            let started = Instant::now();
            let pinned = block_in_place(|| LedgerState::pin(roots, || cancel.is_cancelled()));
            if !pinned.failed_roots.is_empty() {
                warn!(
                    failed_roots:? = pinned.failed_roots;
                    "contract state roots reaching nodes missing from the ledger DB left unpinned"
                );
            }
            let (completed, nodes, failed_roots) = (
                !pinned.cancelled,
                pinned_before + pinned.nodes,
                pinned.failed_roots.len(),
            );
            info!(
                completed,
                roots = roots_count,
                nodes,
                failed_roots,
                invalid_keys,
                elapsed:? = started.elapsed();
                "pinned the contract state history"
            );

            Ok(PinnedContractStates {
                completed,
                nodes,
                failed_roots,
                invalid_keys,
            })
        })
    }
}

/// Records the indexer build that writes the blocks after the given height, when its version or
/// git commit differs from the last recorded: a newer entry's height ends the previous build's
/// range, so data can be traced back to the build that wrote it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Build {
    /// The indexer version.
    pub version: String,
    /// The git commit it was built from, if known at build time. Changes not committed are not
    /// marked: a local build from a changed tree records the commit it started from.
    pub git_sha: Option<String>,
    /// The highest block written before it started, `None` if none was; it writes the ones after.
    pub highest_block_height: Option<u64>,
}

impl Build {
    /// This build, writing the blocks after `highest_block_height`.
    pub fn current(highest_block_height: Option<u64>) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            git_sha: git_sha().map(ToOwned::to_owned),
            highest_block_height,
        }
    }
}

impl<S: Sync> Task<S> for Build {
    const ID: &'static str = "indexer/build";
    const VERSION: u32 = 1;
    /// `None` for an entry this version does not read, as one a newer version journaled; always
    /// journaled as `Some`.
    type State = Option<Build>;

    /// An entry of another version, or one that does not read, is `None`, another build's, so this
    /// build journals its own.
    fn read_state(version: u32, state: Value) -> Result<Self::State, BoxError> {
        // A version that changes the shape migrates older versions' entries here.
        if version == <Self as Task<S>>::VERSION {
            Ok(Build::deserialize(state).ok())
        } else {
            Ok(None)
        }
    }

    /// Pending when the version or git commit differs from the last recorded, so a restart of the
    /// same build records nothing. A commit not known at build time differs from a known one.
    fn is_pending<'a>(
        &'a self,
        _context: &'a S,
        previous: Option<&'a Entry<Self::State>>,
    ) -> BoxFuture<'a, bool> {
        let changed = previous
            .and_then(|previous| previous.state.as_ref())
            .is_none_or(|previous| {
                previous.version != self.version || previous.git_sha != self.git_sha
            });
        Box::pin(async move { Ok(changed) })
    }

    fn run<'a>(
        &'a self,
        _context: &'a S,
        _previous: Option<&'a Entry<Self::State>>,
        _cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Self::State> {
        Box::pin(async move { Ok(Some(self.clone())) })
    }
}

/// Fails if contract actions without contract state keys exist. Checking scans every contract
/// action, so once passed it is journaled and not run again: such actions can only predate it.
pub(super) struct EnsureContractStateKeys;

impl<S: Storage> Task<S> for EnsureContractStateKeys {
    const ID: &'static str = "contract_actions/state_keys";
    type State = ();

    fn run<'a>(
        &'a self,
        storage: &'a S,
        _previous: Option<&'a Entry<Self::State>>,
        _cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Self::State> {
        Box::pin(async move {
            if storage.contract_actions_without_state_keys_exist().await? {
                return Err("found contract actions with no contract state key; they were indexed \
                     by a version that stored contract states as blobs, which cannot be converted. \
                     Wipe both the indexer database and the ledger DB and reindex from genesis; see \
                     docs/reindexing.md"
                    .into());
            }

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Build, PinContractStates, PinnedContractStates};
    use indexer_common::{
        error::BoxError,
        startup::{CancellationToken, Entry, Task},
    };

    #[tokio::test]
    async fn build_is_recorded_when_its_version_changes() -> Result<(), BoxError> {
        let build = Build::current(Some(42));
        assert_eq!(build.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(build.highest_block_height, Some(42));

        let current = &build;
        let pending = |previous: Option<Entry<Option<Build>>>| async move {
            Task::<()>::is_pending(current, &(), previous.as_ref()).await
        };
        assert!(pending(None).await?, "first start");
        assert!(
            !pending(Some(Entry {
                version: 1,
                state: Some(Build::current(Some(7)))
            }))
            .await?,
            "same build restarted"
        );
        assert!(
            pending(Some(Entry {
                version: 1,
                state: Some(Build {
                    version: "0.0.0".to_owned(),
                    ..Build::current(Some(7))
                })
            }))
            .await?,
            "different version"
        );
        assert!(
            pending(Some(Entry {
                version: 1,
                state: Some(Build {
                    git_sha: Some("0000000".to_owned()),
                    ..Build::current(Some(7))
                })
            }))
            .await?,
            "same version from another commit"
        );
        assert!(
            pending(Some(Entry {
                version: 2,
                state: None
            }))
            .await?,
            "an entry that does not read"
        );

        assert_eq!(
            Task::<()>::run(&build, &(), None, &CancellationToken::new()).await?,
            Some(build)
        );

        Ok(())
    }

    // An entry journaled before the backfill could be cancelled reads as completed, so it does not
    // run again.
    #[test]
    fn pinned_contract_states_read_as_completed_without_the_flag() {
        let state = serde_json::from_value::<PinnedContractStates>(serde_json::json!({
            "nodes": 7,
            "failed_roots": 0,
            "invalid_keys": 0,
        }))
        .expect("entry reads");
        assert!(state.completed);
    }

    /// Stored keys that cannot be read are counted, and leave the readable ones' roots to be
    /// pinned all the same.
    #[test]
    fn pin_contract_states_keeps_the_readable_keys() {
        use indexer_common::domain::{SerializedContractStateKey, SerializedZswapStateKey};

        // A ledger-9 contract state key: its tag, then a by-reference key, a discriminant and the
        // hash.
        let readable = [
            b"midnight:storage-key(contract-state[v8]):".as_slice(),
            &[0],
            &[7; 32],
        ]
        .concat();
        let state_keys = [vec![0xff], readable].map(SerializedContractStateKey::from);
        let zswap_state_keys = [SerializedZswapStateKey::from(vec![0xfe])];

        let (roots, invalid_keys) = PinContractStates::roots(&state_keys, &zswap_state_keys);
        assert_eq!(
            roots
                .iter()
                .map(|root| root.0.as_slice())
                .collect::<Vec<_>>(),
            [[7; 32].as_slice()]
        );
        assert_eq!(invalid_keys, 2);
    }
}

// The startup plan journals the build in the indexer database once per build: a restart of the
// same build adds nothing, and another build adds its own entry. SQLite only, needing no container.
#[cfg(all(test, feature = "standalone"))]
mod storage_tests {
    use super::{Build, EnsureContractStateKeys};
    use crate::infra::storage::Storage;
    use indexer_common::{
        error::BoxError,
        infra::{
            migrations,
            pool::sqlite::{self, SqlitePool},
        },
        startup::{CancellationToken, Journal, Task, run, sequential, task},
    };
    use indoc::indoc;

    #[tokio::test]
    async fn build_is_journaled_once_per_build() -> Result<(), BoxError> {
        let dir = tempfile::tempdir()?;
        let url = dir.path().join("indexer.sqlite").display().to_string();
        let pool = SqlitePool::new(sqlite::Config::with_url(url)).await?;
        migrations::sqlite::run(&pool).await?;
        let storage = Storage::new(pool.clone());

        let build = Build::current(Some(7));
        let plan = sequential([task(build.clone())]);
        run(&storage, &storage, &plan, &CancellationToken::new()).await?;
        run(&storage, &storage, &plan, &CancellationToken::new()).await?;
        let entry = storage.latest("indexer/build").await?.expect("journaled");
        assert_eq!(serde_json::from_value::<Build>(entry.state)?, build);

        let other = Build {
            git_sha: Some("00000000".to_owned()),
            ..Build::current(Some(9))
        };
        run(
            &storage,
            &storage,
            &sequential([task(other)]),
            &CancellationToken::new(),
        )
        .await?;
        let entries =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM journal WHERE id = 'indexer/build'")
                .fetch_one(&*pool)
                .await?;
        assert_eq!(entries, 2, "one entry per build");

        Ok(())
    }

    // An entry a newer version of the task journaled, which this version cannot read, does not
    // stop this build from journaling its own.
    #[tokio::test]
    async fn build_is_journaled_after_an_unreadable_newer_entry() -> Result<(), BoxError> {
        let dir = tempfile::tempdir()?;
        let url = dir.path().join("indexer.sqlite").display().to_string();
        let pool = SqlitePool::new(sqlite::Config::with_url(url)).await?;
        migrations::sqlite::run(&pool).await?;
        let storage = Storage::new(pool.clone());

        sqlx::query(indoc! {r#"
            INSERT INTO journal (id, version, state, started_at, ended_at)
            VALUES ('indexer/build', 99, '"not a build"', 0, 0)
        "#})
        .execute(&*pool)
        .await?;

        let build = Build::current(Some(7));
        run(
            &storage,
            &storage,
            &sequential([task(build.clone())]),
            &CancellationToken::new(),
        )
        .await?;
        let entry = storage.latest("indexer/build").await?.expect("journaled");
        assert_eq!(entry.version, 1);
        assert_eq!(serde_json::from_value::<Build>(entry.state)?, build);

        Ok(())
    }

    // The check fails, journaling nothing, while a contract action without a state key exists, and
    // once passed is not run again.
    #[tokio::test]
    async fn contract_state_keys_are_checked_until_they_pass() -> Result<(), BoxError> {
        const ID: &str = <EnsureContractStateKeys as Task<Storage>>::ID;

        let dir = tempfile::tempdir()?;
        let url = dir.path().join("indexer.sqlite").display().to_string();
        let pool = SqlitePool::new(sqlite::Config::with_url(url)).await?;
        migrations::sqlite::run(&pool).await?;
        let storage = Storage::new(pool.clone());
        let plan = sequential([task(EnsureContractStateKeys)]);

        // Insert a contract action with the given contract state and zswap state key. One without,
        // as a version storing contract states as blobs left it once migrated, is what the check
        // refuses. It references no transaction, so foreign keys are off while inserting it.
        let insert_action = async |state_key: Option<&[u8]>| -> Result<(), BoxError> {
            let mut connection = pool.acquire().await?;
            sqlx::query("PRAGMA foreign_keys = OFF")
                .execute(&mut *connection)
                .await?;
            sqlx::query(indoc! {"
                INSERT INTO contract_actions (
                    transaction_id, variant, address, attributes, state_key, zswap_state_key
                )
                VALUES (1, 'Deploy', x'01', '{}', $1, $1)
            "})
            .bind(state_key)
            .execute(&mut *connection)
            .await?;
            sqlx::query("PRAGMA foreign_keys = ON")
                .execute(&mut *connection)
                .await?;
            Ok(())
        };

        // An action without keys fails the check, and nothing is journaled.
        insert_action(None).await?;
        let error = run(&storage, &storage, &plan, &CancellationToken::new())
            .await
            .expect_err("action without keys");
        let reason = std::error::Error::source(&*error).map(ToString::to_string);
        assert!(
            reason.is_some_and(|reason| reason.contains("reindex")),
            "{error:?}"
        );
        assert!(storage.latest(ID).await?.is_none());

        // Only actions with keys pass the check, which is journaled.
        sqlx::query("DELETE FROM contract_actions")
            .execute(&*pool)
            .await?;
        insert_action(Some(&[0x02])).await?;
        run(&storage, &storage, &plan, &CancellationToken::new()).await?;
        let entries = async || {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM journal WHERE id = $1")
                .bind(ID)
                .fetch_one(&*pool)
                .await
        };
        assert_eq!(entries().await?, 1);

        // Once journaled, the check is not run again, so a later start journals nothing more.
        insert_action(Some(&[0x03])).await?;
        run(&storage, &storage, &plan, &CancellationToken::new()).await?;
        assert_eq!(entries().await?, 1, "not run again");

        Ok(())
    }
}
