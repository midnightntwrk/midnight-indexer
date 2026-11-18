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

pub use indexer_common::startup::run;
use indexer_common::{
    error::BoxError,
    startup::{BoxFuture, Entry, Task, Value},
    version::git_sha,
};
use serde::{Deserialize, Serialize};

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
    ) -> BoxFuture<'a, Self::State> {
        Box::pin(async move { Ok(Some(self.clone())) })
    }
}

#[cfg(test)]
mod tests {
    use super::Build;
    use indexer_common::{
        error::BoxError,
        startup::{Entry, Task},
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

        assert_eq!(Task::<()>::run(&build, &(), None).await?, Some(build));

        Ok(())
    }
}

// The startup plan journals the build in the indexer database once per build: a restart of the
// same build adds nothing, and another build adds its own entry. SQLite only, needing no container.
#[cfg(all(test, feature = "standalone"))]
mod storage_tests {
    use super::Build;
    use crate::infra::storage::Storage;
    use indexer_common::{
        error::BoxError,
        infra::{
            migrations,
            pool::sqlite::{self, SqlitePool},
        },
        startup::{Journal, run, sequential, task},
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
        run(&storage, &storage, &plan).await?;
        run(&storage, &storage, &plan).await?;
        let entry = storage.latest("indexer/build").await?.expect("journaled");
        assert_eq!(serde_json::from_value::<Build>(entry.state)?, build);

        let other = Build {
            git_sha: Some("00000000".to_owned()),
            ..Build::current(Some(9))
        };
        run(&storage, &storage, &sequential([task(other)])).await?;
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
        run(&storage, &storage, &sequential([task(build.clone())])).await?;
        let entry = storage.latest("indexer/build").await?.expect("journaled");
        assert_eq!(entry.version, 1);
        assert_eq!(serde_json::from_value::<Build>(entry.state)?, build);

        Ok(())
    }
}
