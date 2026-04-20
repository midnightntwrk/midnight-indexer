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
        infra::{api::AxumApi, storage as api_storage},
    };
    use indexer_common::{
        cipher::make_cipher,
        config::ConfigExt,
        infra::{ledger_db, migrations, pool, pub_sub},
        telemetry,
    };
    use log::{info, warn};
    use spo_indexer::{
        application as spo_app,
        infra::{spo_client::SPOClient, storage as spo_storage},
    };
    use std::panic;
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

    let InfraConfig {
        run_migrations,
        storage_config,
        ledger_db_config,
        node_config,
        spo_node_config,
        api_config,
        secret,
    } = infra_config;

    let runtime = Builder::new_multi_thread()
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

        let chain_node = SubxtNode::new(node_config.clone())
            .await
            .context("create SubxtNode")?;

        // Share the chain node's websocket RPC client with the SPO indexer when
        // they target the same node URL; otherwise fall back to a dedicated
        // client. This halves socket usage in the common single-node setup.
        let spo_client = if spo_node_config.url == node_config.url {
            let spo_cfg: spo_indexer::infra::spo_client::Config =
                spo_node_config.clone().into();
            SPOClient::new_with_rpc_client(
                chain_node.rpc_client(),
                spo_cfg.blockfrost_id,
                spo_cfg.http_pool,
            )
            .await
            .context("create SPOClient")?
        } else {
            warn!(
                chain_url = node_config.url.as_str(),
                spo_url = spo_node_config.url.as_str();
                "chain and spo node URLs differ; keeping separate websocket RPC clients"
            );
            SPOClient::new(spo_node_config.clone().into())
                .await
                .context("create SPOClient")?
        };

        let chain_indexer = task::spawn({
            let storage = chain_storage::Storage::new(pool.clone());

            let sigterm =
                signal(SignalKind::terminate()).expect("SIGTERM handler can be registered");

            chain_app::run(
                application_config.clone().into(),
                chain_node,
                storage,
                pub_sub.publisher(),
                sigterm,
            )
        });

        let spo_indexer = task::spawn({
            let storage = spo_storage::Storage::new(pool.clone());

            let sigterm =
                signal(SignalKind::terminate()).expect("SIGTERM handler can be registered");

            spo_app::run(spo_config.clone().into(), spo_client, storage, sigterm)
        });

        let indexer_api = task::spawn({
            let subscriber = pub_sub.subscriber();
            let storage = api_storage::Storage::new(cipher.clone(), pool.clone());
            let api = AxumApi::new(api_config, storage, subscriber.clone());

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
