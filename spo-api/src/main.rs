// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "cloud")]
#[tokio::main]
async fn main() {
    use indexer_common::telemetry;
    use log::error;
    use std::panic;

    telemetry::init_logging();
    panic::set_hook(Box::new(|panic| error!(panic:%; "process panicked")));

    if let Err(error) = run().await {
        let backtrace = error.backtrace();
        let error = format!("{error:#}");
        error!(error, backtrace:%; "process exited with ERROR");
        std::process::exit(1);
    }
}

#[cfg(feature = "cloud")]
async fn run() -> anyhow::Result<()> {
    use anyhow::Context;
    use indexer_common::{config::ConfigExt, domain::NoopSubscriber, infra::pool, telemetry};
    use log::info;
    use spo_api::{
        application,
        config::Config,
        infra,
        infra::api::{AxumApi, Db},
    };
    use tokio::signal::unix::{SignalKind, signal};

    let sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler can be registered");
    let config = Config::load().context("load configuration")?;
    info!(config:?; "starting");
    let Config {
        run_migrations: _,
        application_config,
        infra_config,
        telemetry_config:
            telemetry::Config {
                tracing_config,
                metrics_config,
            },
    } = config;

    telemetry::init_tracing(tracing_config);
    telemetry::init_metrics(metrics_config);

    let infra::Config {
        api_config,
        storage_config,
    } = infra_config;

    // Create Postgres pool (for read-only access initially) and run migrations if/when added later.
    let pool = pool::postgres::PostgresPool::new(storage_config)
        .await
        .context("create DB pool for Postgres")?;

    // Build API without NATS for now.
    let api = AxumApi::new(api_config).with_db(Db(pool));

    // Until we have a catch-up signal, application::run will just serve the API and listen for
    // SIGTERM. Pass a no-op subscriber for now.
    let subscriber = NoopSubscriber::default();
    application::run(application_config, api, subscriber, sigterm)
        .await
        .context("run SPO API application")
}

#[cfg(not(feature = "cloud"))]
fn main() {
    unimplemented!()
}
