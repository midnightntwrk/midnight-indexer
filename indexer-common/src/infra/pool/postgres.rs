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
use log::{debug, warn};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
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
    ///
    /// TLS is mandatory (matching midnight-node): [PgSslMode::VerifyFull] when a `ssl_root_cert`
    /// is configured, otherwise [PgSslMode::Require] (encrypted, but the server certificate is not
    /// validated). TLS is never disabled — a deployed binary always encrypts its database
    /// connection. Tests against a non-TLS Postgres use [PostgresPool::new_without_tls].
    pub async fn new(config: Config) -> Result<Self, Error> {
        let ssl_mode = ssl_mode(config.ssl_root_cert.as_deref());
        Self::connect(config, ssl_mode).await
    }

    /// Create a pool with TLS disabled. Test-only: unreachable from shipped configuration (a
    /// deployed binary always calls [PostgresPool::new] with mandatory TLS). Exists so tests can
    /// connect to a local Postgres that does not serve TLS.
    pub async fn new_without_tls(config: Config) -> Result<Self, Error> {
        Self::connect(config, PgSslMode::Disable).await
    }

    async fn connect(config: Config, ssl_mode: PgSslMode) -> Result<Self, Error> {
        let Config {
            host,
            port,
            dbname,
            user,
            password,
            ssl_root_cert,
            max_connections,
            idle_timeout,
            max_lifetime,
        } = config;

        let mut connect_options = PgConnectOptions::new()
            .host(&host)
            .database(&dbname)
            .username(&user)
            .password(password.expose_secret())
            .port(port)
            .ssl_mode(ssl_mode);
        if let Some(ssl_root_cert) = ssl_root_cert {
            connect_options = connect_options.ssl_root_cert(ssl_root_cert);
        }

        let inner = PgPoolOptions::new()
            .max_connections(max_connections)
            // Validate a pooled connection before handing it out and, if it is dead, discard it and
            // open a fresh one. Necessary since rustls 0.23 (sqlx 0.9) turns a connection the server
            // closed without a TLS `close_notify` into a hard error rather than tolerating it.
            .test_before_acquire(true)
            // Fail fast instead of blocking on the 30s default: a caught error is retried by the
            // caller, so a brief unavailability should not stall the whole indexing cycle.
            .acquire_timeout(Duration::from_secs(10))
            .idle_timeout(Some(idle_timeout))
            .max_lifetime(max_lifetime)
            .connect_with(connect_options)
            .await?;
        let pool = PostgresPool(inner);
        debug!(pool:?; "created pool");

        Ok(pool)
    }
}

/// Select the mandatory TLS mode. Never returns [PgSslMode::Disable].
fn ssl_mode(ssl_root_cert: Option<&str>) -> PgSslMode {
    match ssl_root_cert {
        Some(_) => PgSslMode::VerifyFull,
        None => {
            warn!(
                "no ssl_root_cert configured: using PgSslMode::Require (encrypted but no \
                 certificate validation); set ssl_root_cert for full MITM protection"
            );
            PgSslMode::Require
        }
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
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub host: String,

    pub port: u16,

    pub dbname: String,

    pub user: String,

    pub password: SecretString,

    /// Path to a PEM root certificate. When set, the database connection uses
    /// [PgSslMode::VerifyFull] (full certificate validation); when absent, [PgSslMode::Require]
    /// (encrypted, no validation). TLS is always required — there is no way to disable it via
    /// configuration.
    #[serde(default)]
    pub ssl_root_cert: Option<String>,

    pub max_connections: u32,

    #[serde(with = "humantime_serde")]
    pub idle_timeout: Duration,

    #[serde(with = "humantime_serde")]
    pub max_lifetime: Duration,
}

#[cfg(test)]
mod tests {
    use crate::infra::pool::postgres::{Config, PostgresPool, ssl_mode};
    use anyhow::Context;
    use sqlx::postgres::PgSslMode;
    use std::{error::Error as StdError, time::Duration};
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres;

    #[test]
    fn ssl_mode_requires_tls_and_is_never_disabled() {
        assert!(matches!(ssl_mode(None), PgSslMode::Require));
        assert!(matches!(
            ssl_mode(Some("/path/to/ca.pem")),
            PgSslMode::VerifyFull
        ));
        for cert in [None, Some("/path/to/ca.pem")] {
            assert!(!matches!(ssl_mode(cert), PgSslMode::Disable));
        }
    }

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
            ssl_root_cert: None,
            max_connections: 10,
            idle_timeout: Duration::from_secs(60),
            max_lifetime: Duration::from_secs(5 * 60),
        };

        // The test container does not serve TLS, so use the TLS-disabled test constructor.
        let pool = PostgresPool::new_without_tls(config).await;
        assert!(pool.is_ok());
        let pool = pool.unwrap();

        let result = sqlx::query("CREATE TABLE test (id integer PRIMARY KEY)")
            .execute(&*pool)
            .await;
        assert!(result.is_ok());

        Ok(())
    }
}
