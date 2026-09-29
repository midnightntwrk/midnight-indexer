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

#[cfg(feature = "standalone")]
mod config;

#[cfg(feature = "standalone")]
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

#[cfg(feature = "standalone")]
fn run() -> anyhow::Result<()> {
    use crate::config::{Config, InfraConfig};
    use anyhow::Context;
    use chain_indexer::{
        application as chain_app,
        infra::{storage as chain_storage, subxt_node::SubxtNode},
    };
    use indexer_api::{
        application as api_app,
        infra::{
            api::{
                AxumApi,
                ledger_query_limit::{
                    DEFAULT_MAX_BLOCKING_THREADS, LedgerQueryLimiter, default_permits,
                    validate_permits,
                },
            },
            storage as api_storage,
        },
    };
    use indexer_common::{
        cipher::make_cipher,
        config::ConfigExt,
        infra::{ledger_db, migrations, pool, pub_sub},
        telemetry,
    };
    use log::info;
    use spo_indexer::{
        application as spo_app,
        infra::{spo_client::SPOClient, storage as spo_storage},
    };
    use std::{num::NonZeroUsize, panic};
    use tokio::{
        runtime::Builder,
        select,
        signal::unix::{SignalKind, signal},
        task,
    };
    use wallet_indexer::{application as wallet_app, infra::storage as wallet_storage};

    // Load configuration.
    let Config {
        thread_stack_size,
        max_blocking_threads,
        ledger_query_concurrency,
        application_config,
        spo_config,
        infra_config,
        telemetry_config:
            telemetry::Config {
                tracing_config,
                metrics_config,
            },
    } = Config::load().context("load configuration")?;

    info!(
        application_config:?,
        infra_config:?;
        "starting"
    );

    // Retention/freshness invariant, enforceable only here where both configs share a process
    // (in cloud they live in separate binaries, so there it is an operator responsibility): the
    // dust generations subscription accepts snapshots up to `max_snapshot_age` blocks old, but
    // the chain-indexer only keeps the newest `ledger_state_retention` blocks' ledger states
    // loadable. If retention does not exceed the freshness window, an accepted snapshot can
    // resolve to garbage-collected state and panic inside storage-core on load.
    let ledger_state_retention = application_config.ledger_state_retention.get();
    let max_snapshot_age = infra_config
        .api_config
        .subscription_config
        .dust_generations
        .max_snapshot_age;
    assert!(
        u64::try_from(ledger_state_retention).unwrap_or(u64::MAX) > u64::from(max_snapshot_age),
        "ledger_state_retention ({ledger_state_retention}) must exceed \
         dust_generations.max_snapshot_age ({max_snapshot_age}): otherwise a snapshot can pass \
         the freshness check yet resolve to garbage-collected ledger state and panic on load"
    );

    let InfraConfig {
        run_migrations,
        storage_config,
        ledger_db_config,
        node_config,
        spo_node_config,
        api_config,
        secret,
    } = infra_config;

    // Cap the blocking pool explicitly (issue #595): a ledger walk occupies a blocking thread for
    // its whole duration via the `block_in_place` core handoff. In standalone the chain-indexer,
    // wallet-indexer, spo-indexer and API share this one runtime and this one pool, so bounding
    // API-driven ledger queries also keeps the indexing tasks scheduled under a query storm.
    let max_blocking_threads = max_blocking_threads.unwrap_or(DEFAULT_MAX_BLOCKING_THREADS);

    // The standalone ledger DB is SQLite pinned to a single connection — storage-core assumes a
    // single writer, see `ledger_db::init` — so the default lands on one permit: walks already
    // serialize on that connection and admitting more would only pin blocking threads waiting for
    // it. Note this is the ledger DB's own pool, not the main storage pool, which is wider.
    let ledger_db_max_connections = NonZeroUsize::new(ledger_db::MAX_CONNECTIONS as usize)
        .expect("ledger DB pool holds at least one connection");

    // The contract state cache bounds its own arena loads against this same single-connection
    // ledger DB, so the ledger bound budgets against it rather than alongside it.
    let contract_state_loads = api_config
        .contract_state_cache_config
        .max_concurrent_loads();

    let ledger_query_permits = ledger_query_concurrency.unwrap_or_else(|| {
        default_permits(
            ledger_db_max_connections,
            contract_state_loads,
            max_blocking_threads,
        )
    });
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

    runtime.block_on(async {
        telemetry::init_tracing(tracing_config);
        telemetry::init_metrics(metrics_config);

        let pool = pool::sqlite::SqlitePool::new(storage_config)
            .await
            .context("create DB pool for Sqlite")?;
        if run_migrations {
            migrations::sqlite::run(&pool)
                .await
                .context("run Sqlite migrations")?;
        }

        let cipher = make_cipher(secret).context("make cipher")?;

        let pub_sub = pub_sub::in_mem::InMemPubSub::default();

        ledger_db::init(ledger_db_config)
            .await
            .context("initialize ledger db")?;

        // Move the node connection setup *inside* each spawned task so a slow
        // or unreachable URL only blocks its own component, not the whole
        // runtime startup. The previous shape `task::spawn({ ... .await? ... })`
        // ran the .await synchronously in the outer block_on, holding back the
        // indexer-api and wallet-indexer spawns for up to
        // `reconnect_max_attempts × reconnect_max_delay` (≈5 min by default).
        let chain_indexer = {
            let storage = chain_storage::Storage::new(pool.clone());
            let publisher = pub_sub.publisher();
            let application_config = application_config.clone();
            task::spawn(async move {
                let node = SubxtNode::new(node_config)
                    .await
                    .context("create SubxtNode")?;
                let sigterm =
                    signal(SignalKind::terminate()).expect("SIGTERM handler can be registered");
                chain_app::run(application_config.into(), node, storage, publisher, sigterm).await
            })
        };

        let spo_indexer = {
            let storage = spo_storage::Storage::new(pool.clone());
            task::spawn(async move {
                let node = SPOClient::new(spo_node_config.into())
                    .await
                    .context("create SPOClient")?;
                let sigterm =
                    signal(SignalKind::terminate()).expect("SIGTERM handler can be registered");
                spo_app::run(spo_config.into(), node, storage, sigterm).await
            })
        };

        let indexer_api = task::spawn({
            let subscriber = pub_sub.subscriber();
            let storage = api_storage::Storage::new(cipher.clone(), pool.clone());
            let api = AxumApi::new(
                api_config,
                storage,
                subscriber.clone(),
                ledger_query_limiter,
            );

            api_app::run(application_config.clone().into(), api, subscriber)
        });

        let wallet_indexer = task::spawn({
            let storage = wallet_storage::Storage::new(cipher, pool);
            let publisher = pub_sub.publisher();
            let subscriber = pub_sub.subscriber();
            let sigterm =
                signal(SignalKind::terminate()).expect("SIGTERM handler can be registered");

            wallet_app::run(
                application_config.into(),
                storage,
                publisher,
                subscriber,
                sigterm,
            )
        });

        select! {
            result = chain_indexer => handle_exit("chain-indexer", result),
            result = spo_indexer => handle_exit("spo-indexer", result),
            result = wallet_indexer => handle_exit("wallet-indexer", result),
            result = indexer_api => handle_exit("indexer-api", result),
        }

        info!("indexer shutting down");

        std::process::exit(1);
    })
}

#[cfg(feature = "standalone")]
fn handle_exit(task_name: &str, result: Result<anyhow::Result<()>, tokio::task::JoinError>) {
    use log::error;

    match result {
        Ok(Err(error)) => {
            let backtrace = error.backtrace();
            let error = format!("{error:#}");
            error!(error, backtrace:%; "{task_name} exited with ERROR");
        }

        Err(error) => {
            error!(error:% = format!("{error:#}"); "{task_name} panicked");
        }

        _ => {
            error!("{task_name} terminated");
        }
    }
}

#[cfg(not(feature = "standalone"))]
fn main() -> anyhow::Result<()> {
    unimplemented!()
}
