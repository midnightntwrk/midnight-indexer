// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::domain::Api;
use anyhow::Context as AnyhowContext;
use indexer_common::domain::{NetworkId, Subscriber};
use log::warn;
use serde::Deserialize;
use serde_with::{DisplayFromStr, serde_as};
use std::sync::{Arc, atomic::AtomicBool};
use tokio::{select, signal::unix::Signal, task};

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde_as(as = "DisplayFromStr")]
    pub network_id: NetworkId,
}

pub async fn run(
    config: Config,
    api: impl Api,
    _subscriber: impl Subscriber,
    mut sigterm: Signal,
) -> anyhow::Result<()> {
    let Config { network_id } = config;

    // For now we don't track catch-up; expose ready immediately. We'll wire NATS later.
    let caught_up = Arc::new(AtomicBool::new(true));

    let serve_api_task = {
        task::spawn(async move {
            api.serve(network_id, caught_up)
                .await
                .context("serving SPO API")
        })
    };

    select! {
        result = serve_api_task => result
            .context("serve_api_task panicked")
            .and_then(|r| r.context("serve_api_task failed")),
        _ = sigterm.recv() => {
            warn!("SIGTERM received");
            Ok(())
        }
    }
}
