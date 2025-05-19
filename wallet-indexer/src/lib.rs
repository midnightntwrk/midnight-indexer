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

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod application;
pub mod domain;
pub mod infra;

#[cfg(feature = "cloud")]
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    pub run_migrations: bool,

    #[serde(rename = "application")]
    pub application_config: application::Config,

    #[serde(rename = "infra")]
    pub infra_config: infra::Config,

    #[serde(rename = "telemetry")]
    pub telemetry_config: indexer_common::telemetry::Config,
}

#[cfg(feature = "cloud")]
pub async fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use indexer_common::{config::ConfigExt, telemetry};
    use log::error;
    use std::panic;

    // Initialize logging.
    telemetry::init_logging();

    // Replace the default panic hook with one that uses structured logging at ERROR level.
    panic::set_hook(Box::new(|panic| error!(panic:%; "process panicked")));

    // Load configuration.
    let Config {
        run_migrations,
        application_config,
        infra_config,
        telemetry_config:
            telemetry::Config {
                tracing_config,
                metrics_config,
            },
    } = Config::load()
        .context("load configuration")
        .inspect_err(|error| {
            let backtrace = error.backtrace();
            let error = format!("{error:#}");
            error!(error, backtrace:%; "process exited with ERROR")
        })?;

    // Initialize tracing and metrics.
    telemetry::init_tracing(tracing_config);
    telemetry::init_metrics(metrics_config);

    // Run and log any error.
    run(run_migrations, application_config, infra_config)
        .await
        .inspect_err(|error| {
            let backtrace = error.backtrace();
            let error = format!("{error:#}");
            error!(error, backtrace:%; "process exited with ERROR")
        })
}

#[cfg(feature = "cloud")]
async fn run(
    run_migrations: bool,
    application_config: application::Config,
    infra_config: infra::Config,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use indexer_common::{
        cipher::make_cipher,
        infra::{migrations, pool, pub_sub},
    };
    use log::{error, info};

    info!(
        run_migrations,
        application_config:?,
        infra_config:?;
        "starting"
    );

    let infra::Config {
        secret,
        storage_config,
        pub_sub_config,
    } = infra_config;

    let pool = pool::postgres::PostgresPool::new(storage_config)
        .await
        .context("create DB pool for Postgres")?;
    if run_migrations {
        migrations::postgres::run(&pool)
            .await
            .context("run Postgres migrations")?;
    }
    let cipher = make_cipher(secret).context("make cipher")?;
    let storage = infra::storage::postgres::PostgresStorage::new(cipher, pool);

    let publisher = pub_sub::nats::publisher::NatsPublisher::new(pub_sub_config)
        .await
        .context("create NatsPublisher")?;

    // Run indexing.
    application::run(application_config, storage, publisher)
        .await
        .context("run application")?;

    error!("wallet-indexer terminated");

    Ok(())
}
