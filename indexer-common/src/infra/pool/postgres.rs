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

use derive_more::Into;
use log::debug;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_with::{DisplayFromStr, serde_as};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use std::{ops::Deref, time::Duration};
use thiserror::Error;

/// New type for `sqlx::PgPool`, allowing for some custom extensions as well as security.
///
/// To use as `&sqlx::PgPool` in `Query::execute`, use its `Deref` implementation: `&*pool` or
/// `pool.deref()`. If an owned `sqlx::PgPool` is needed, use `Into::into`.
#[derive(Debug, Clone, Into)]
pub struct PostgresPool(sqlx::PgPool);

impl PostgresPool {
    /// Try to create a new [PostgresPool] with the given config.
    pub async fn new(config: Config) -> Result<Self, Error> {
        let Config {
            host,
            port,
            dbname,
            user,
            password,
            sslmode,
            max_connections,
            idle_timeout,
            max_lifetime,
            test_before_acquire,
        } = config;

        let connect_options = PgConnectOptions::new()
            .host(&host)
            .database(&dbname)
            .username(&user)
            .password(password.expose_secret())
            .port(port)
            .ssl_mode(sslmode);

        let inner = PgPoolOptions::new()
            .max_connections(max_connections)
            .idle_timeout(Some(idle_timeout))
            .max_lifetime(max_lifetime)
            .test_before_acquire(test_before_acquire)
            .connect_with(connect_options)
            .await?;
        let pool = PostgresPool(inner);
        debug!(pool:?; "created pool");

        Ok(pool)
    }
}

impl Deref for PostgresPool {
    type Target = sqlx::PgPool;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Error possibly returned by [PostgresPool::new].
#[derive(Debug, Error)]
#[error("cannot create Postgres connection pool")]
pub struct Error(#[from] sqlx::Error);

/// Configuration for [PostgresPool].
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub host: String,

    pub port: u16,

    pub dbname: String,

    pub user: String,

    pub password: SecretString,

    #[serde_as(as = "DisplayFromStr")]
    pub sslmode: PgSslMode,

    pub max_connections: u32,

    #[serde(with = "humantime_serde")]
    pub idle_timeout: Duration,

    #[serde(with = "humantime_serde")]
    pub max_lifetime: Duration,

    /// Ping each connection as it is taken from the pool, so one broken while idle is replaced
    /// rather than failing a query. It costs a round trip per acquisition; with connections
    /// recycled after `idle_timeout` and `max_lifetime`, a caller that retries or restarts on a
    /// failed query can do without it.
    #[serde(default = "test_before_acquire_default")]
    pub test_before_acquire: bool,
}

fn test_before_acquire_default() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use crate::infra::pool::postgres::{Config, PostgresPool};
    use crate::testing::POSTGRES_IMAGE_TAG;
    use anyhow::Context;
    use sqlx::postgres::PgSslMode;
    use std::{error::Error as StdError, time::Duration};
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres;

    // Leaving `test_before_acquire` out keeps the connection test, which every service but the
    // chain indexer relies on.
    #[test]
    fn test_before_acquire_defaults_to_true() {
        let config = |test_before_acquire: &str| {
            serde_json::from_str::<Config>(&format!(
                r#"{{
                    "host": "localhost", "port": 5432, "dbname": "indexer", "user": "indexer",
                    "password": "postgres", "sslmode": "prefer", "max_connections": 10,
                    "idle_timeout": "1m", "max_lifetime": "5m"{test_before_acquire}
                }}"#
            ))
            .expect("config deserializes")
        };

        assert!(config("").test_before_acquire);
        assert!(!config(r#", "test_before_acquire": false"#).test_before_acquire);
    }

    #[tokio::test]
    async fn test_pool() -> Result<(), Box<dyn StdError>> {
        let postgres_container = Postgres::default()
            .with_db_name("indexer")
            .with_user("indexer")
            .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
            .with_tag(POSTGRES_IMAGE_TAG)
            .start()
            .await
            .context("start Postgres container")?;
        let postgres_port = postgres_container
            .get_host_port_ipv4(5432)
            .await
            .context("get Postgres port")?;

        let config = Config {
            host: "localhost".to_string(),
            port: postgres_port,
            dbname: "indexer".to_string(),
            user: "indexer".to_string(),
            password: env!("APP__INFRA__STORAGE__PASSWORD").into(),
            sslmode: PgSslMode::Prefer,
            max_connections: 10,
            idle_timeout: Duration::from_secs(60),
            max_lifetime: Duration::from_secs(5 * 60),
            test_before_acquire: true,
        };

        let pool = PostgresPool::new(config).await;
        assert!(pool.is_ok());
        let pool = pool.unwrap();

        let result = sqlx::query("CREATE TABLE test (id integer PRIMARY KEY)")
            .execute(&*pool)
            .await;
        assert!(result.is_ok());

        Ok(())
    }
}
