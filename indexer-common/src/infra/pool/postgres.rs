// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
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

use log::debug;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_with::{DisplayFromStr, serde_as};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use std::ops::Deref;
use thiserror::Error;

/// New type for `sqlx::PgPool`, allowing for some custom extensions as well as security.
///
/// To use as `&sqlx::PgPool` in `Query::execute`, use its `Deref` implementation: `&*pool` or
/// `pool.deref()`. If an owned `sqlx::PgPool` is needed, use `Into::into`.
#[derive(Debug, Clone)]
pub struct PostgresPool(sqlx::PgPool);

impl PostgresPool {
    /// Try to create a new [PostgresPool] with the given config.
    pub async fn new(config: Config) -> Result<Self, Error> {
        let inner = PgPoolOptions::new().connect_with(config.into()).await?;
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
}

impl From<Config> for PgConnectOptions {
    fn from(config: Config) -> Self {
        PgConnectOptions::new()
            .host(&config.host)
            .database(&config.dbname)
            .username(&config.user)
            .password(config.password.expose_secret())
            .port(config.port)
            .ssl_mode(config.sslmode)
    }
}

#[cfg(test)]
mod tests {
    use crate::infra::pool::postgres::{Config, PostgresPool};
    use anyhow::Context;
    use sqlx::postgres::PgSslMode;
    use std::error::Error as StdError;
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres;

    #[tokio::test]
    async fn test_pool() -> Result<(), Box<dyn StdError>> {
        let postgres_container = Postgres::default()
            .with_db_name("indexer")
            .with_user("indexer")
            .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
            .with_tag("17.1-alpine")
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
