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

#[cfg(feature = "cloud")]
fn main() {
    use indexer_common::telemetry;
    use log::error;
    use std::panic;

    // Handle `--version` before anything else so it works without a config file.
    indexer_common::handle_version_flag!();

    // Initialize logging.
    telemetry::init_logging();

    // Replace the default panic hook with one that uses structured logging at ERROR level.
    panic::set_hook(Box::new(|panic| error!(panic:%; "process panicked")));

    // Run and log any error.
    if let Err(error) = run() {
        let backtrace = error.backtrace();
        let error = format!("{error:#}");
        error!(error, backtrace:%; "process exited with ERROR");
        std::process::exit(1);
    }
}

#[cfg(feature = "cloud")]
fn run() -> anyhow::Result<()> {
    use anyhow::Context;
    use indexer_api::{
        application,
        config::Config,
        infra::{
            self,
            api::{
                AxumApi,
                ledger_query_limit::{
                    DEFAULT_MAX_BLOCKING_THREADS, LedgerQueryLimiter, default_permits,
                    validate_permits,
                },
            },
        },
    };
    use indexer_common::{
        cipher::make_cipher,
        config::ConfigExt,
        infra::{ledger_db, migrations, pool, pub_sub},
        telemetry,
    };
    use log::info;
    use std::{num::NonZeroUsize, time::Duration};
    use tokio::runtime::Builder;

    // Load configuration.
    let config = Config::load().context("load configuration")?;
    info!(config:?; "starting");
    let Config {
        thread_stack_size,
        max_blocking_threads,
        ledger_query_concurrency,
        application_config,
        infra_config,
        telemetry_config:
            telemetry::Config {
                tracing_config,
                metrics_config,
            },
    } = config;

    let infra::Config {
        run_migrations,
        storage_config,
        ledger_db_config,
        pub_sub_config,
        api_config,
        secret,
    } = infra_config;

    // Cap the blocking pool explicitly (issue #595): a ledger walk occupies a blocking thread for
    // its whole duration via the `block_in_place` core handoff, and tokio's default of 512 at the
    // configured stack size is more thread stacks than this process can afford.
    let max_blocking_threads = max_blocking_threads.unwrap_or(DEFAULT_MAX_BLOCKING_THREADS);

    // The ledger DB shares this pool with every other resolver (`ledger_db::init` below is handed
    // the same pool), so the ledger-query bound is sized off it: admitting more walks than there
    // are connections to serve them starves the rest of the API instead of bounding anything.
    let ledger_db_max_connections = NonZeroUsize::new(storage_config.max_connections as usize)
        .context("storage max_connections must be greater than zero")?;

    let ledger_query_permits = ledger_query_concurrency
        .unwrap_or_else(|| default_permits(ledger_db_max_connections, max_blocking_threads));
    validate_permits(ledger_query_permits, max_blocking_threads)
        .context("validate ledger_query_concurrency")?;
    info!(
        max_blocking_threads = max_blocking_threads.get(),
        ledger_db_max_connections = ledger_db_max_connections.get(),
        ledger_query_permits = ledger_query_permits.get();
        "runtime concurrency"
    );
    let ledger_query_limiter = LedgerQueryLimiter::new(ledger_query_permits);

    let runtime = Builder::new_multi_thread()
        .max_blocking_threads(max_blocking_threads.get())
        .enable_all()
        .thread_stack_size(thread_stack_size as usize)
        .build()
        .context("build Tokio runtime")?;

    let result = runtime.block_on(async {
        telemetry::init_tracing(tracing_config);
        telemetry::init_metrics(metrics_config);

        let pool = pool::postgres::PostgresPool::new(storage_config)
            .await
            .context("create DB pool for Postgres")?;
        if run_migrations {
            migrations::postgres::run(&pool)
                .await
                .context("run Postgres migrations")?;
        }

        let cipher = make_cipher(secret).context("make cipher")?;
        let storage = infra::storage::Storage::new(cipher, pool.clone());

        // One pool serves both GraphQL queries and ledger-arena node reads. Arena reads are on the
        // hot path now that contract states are resolved from keys, and each is many single-row
        // queries, so an arena-heavy request can compete with ordinary queries for the pool's
        // connections. Two things bound that: `pre_fetch` collapses a DAG into one batched query per
        // level, and the contract state cache holds a semaphore across its loads. If those turn out
        // not to be enough under load, the next step is a dedicated pool for the ledger DB here —
        // standalone is unaffected either way, its ledger DB is a separate SQLite file.
        ledger_db::init(ledger_db_config, pool);

        let subscriber = pub_sub::nats::subscriber::NatsSubscriber::new(pub_sub_config).await?;

        let api = AxumApi::new(
            api_config,
            storage,
            subscriber.clone(),
            ledger_query_limiter,
        );

        application::run(application_config, api, subscriber).await
    });

    // The implicit runtime drop hangs indefinitely when spawned tasks are inside
    // block_in_place calls (e.g. ledger DB) that cannot be cancelled by abort().
    runtime.shutdown_timeout(Duration::from_secs(5));

    result
}

#[cfg(not(feature = "cloud"))]
fn main() {
    unimplemented!()
}
