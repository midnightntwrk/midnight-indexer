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

//! The [Journal] of startup tasks in the `journal` table of the indexer database.

use crate::{
    error::BoxError,
    startup::{Entry, Journal, unix_millis},
};
use indoc::indoc;
use serde_json::Value;
use sqlx::types::Json;
use std::time::SystemTime;

#[cfg(feature = "cloud")]
type Pool = crate::infra::pool::postgres::PostgresPool;
#[cfg(feature = "standalone")]
type Pool = crate::infra::pool::sqlite::SqlitePool;

const APPEND: &str = indoc! {"
    INSERT INTO journal (id, version, state, started_at, ended_at)
    VALUES ($1, $2, $3, $4, $5)
"};
const LATEST: &str = indoc! {"
    SELECT version, state
    FROM journal
    WHERE id = $1
    ORDER BY seq DESC
    LIMIT 1
"};

impl Journal for Pool {
    async fn latest(&self, id: &str) -> Result<Option<Entry<Value>>, BoxError> {
        let row = sqlx::query_as::<_, (i64, Json<Value>)>(LATEST)
            .bind(id)
            .fetch_optional(&**self)
            .await?;

        row.map(|(version, Json(state))| {
            Ok(Entry {
                version: u32::try_from(version)?,
                state,
            })
        })
        .transpose()
    }

    async fn append(
        &self,
        id: &str,
        entry: Entry<Value>,
        started_at: SystemTime,
        ended_at: SystemTime,
    ) -> Result<(), BoxError> {
        sqlx::query(APPEND)
            .bind(id)
            .bind(i64::from(entry.version))
            .bind(Json(entry.state))
            .bind(unix_millis(started_at))
            .bind(unix_millis(ended_at))
            .execute(&**self)
            .await?;

        Ok(())
    }
}

/// The latest entry wins, and entries of other tasks are kept apart. Shared by both suites.
#[cfg(test)]
async fn assert_latest_entry_per_task(pool: &Pool) -> Result<(), BoxError> {
    use serde_json::json;

    assert_eq!(pool.latest("test/a").await?, None);

    let now = SystemTime::now();
    let entry = |version, state| Entry { version, state };
    pool.append("test/a", entry(1, json!({ "cursor": 1 })), now, now)
        .await?;
    pool.append("test/b", entry(1, json!(null)), now, now)
        .await?;
    pool.append("test/a", entry(2, json!({ "cursor": 2 })), now, now)
        .await?;

    assert_eq!(
        pool.latest("test/a").await?,
        Some(entry(2, json!({ "cursor": 2 })))
    );
    assert_eq!(pool.latest("test/b").await?, Some(entry(1, json!(null))));

    // Times are stored as given, in Unix milliseconds.
    let (started_at, ended_at) = (1_000, 2_500);
    let at = |millis| SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(millis);
    pool.append(
        "test/c",
        entry(1, json!(null)),
        at(started_at),
        at(ended_at),
    )
    .await?;
    let times = sqlx::query_as::<_, (i64, i64)>(
        "SELECT started_at, ended_at FROM journal WHERE id = 'test/c'",
    )
    .fetch_one(&**pool)
    .await?;
    assert_eq!(times, (started_at as i64, ended_at as i64));

    Ok(())
}

#[cfg(all(test, feature = "standalone"))]
mod sqlite_tests {
    use super::assert_latest_entry_per_task;
    use crate::{
        error::BoxError,
        infra::{
            migrations::sqlite::run,
            pool::sqlite::{Config, SqlitePool},
        },
    };

    #[tokio::test]
    async fn latest_entry_per_task() -> Result<(), BoxError> {
        let pool = SqlitePool::new(Config::default()).await?;
        run(&pool).await?;
        assert_latest_entry_per_task(&pool).await
    }
}

#[cfg(all(test, feature = "cloud"))]
mod postgres_tests {
    use super::assert_latest_entry_per_task;
    use crate::{
        error::BoxError,
        infra::{
            migrations::postgres::run,
            pool::postgres::{Config, PostgresPool},
        },
    };
    use sqlx::postgres::PgSslMode;
    use std::time::Duration;
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres;

    #[tokio::test]
    async fn latest_entry_per_task() -> Result<(), BoxError> {
        let container = Postgres::default()
            .with_db_name("indexer")
            .with_user("indexer")
            .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
            .with_tag("17.1-alpine")
            .start()
            .await?;
        let pool = PostgresPool::new(Config {
            host: "localhost".to_string(),
            port: container.get_host_port_ipv4(5432).await?,
            dbname: "indexer".to_string(),
            user: "indexer".to_string(),
            password: env!("APP__INFRA__STORAGE__PASSWORD").into(),
            sslmode: PgSslMode::Prefer,
            max_connections: 10,
            idle_timeout: Duration::from_secs(60),
            max_lifetime: Duration::from_secs(5 * 60),
        })
        .await?;
        run(&pool).await?;

        assert_latest_entry_per_task(&pool).await
    }
}
