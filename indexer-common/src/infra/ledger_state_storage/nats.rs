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

use crate::domain::{LedgerStateStorage, SerializedLedgerState};
use async_nats::{
    ConnectError, ConnectOptions,
    jetstream::{
        self, Context as Jetstream,
        context::CreateObjectStoreError,
        object_store::{self, GetError, ObjectStore, PutError},
    },
};
use fastrace::trace;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::io::{self};
use thiserror::Error;
use tokio::io::AsyncReadExt;

const LEDGER_STATE_STORE_NAME: &str = "ledger_state_store";

/// NATS and PostgreSQL based ledger state storage implementation.
#[derive(Clone)]
pub struct NatsLedgerStateStorage {
    ledger_state_store: ObjectStore,
}

impl NatsLedgerStateStorage {
    /// Create a new ledger state storage with the given configuration.
    pub async fn new(config: Config) -> Result<Self, Error> {
        let Config {
            url,
            username,
            password,
            num_replicas,
        } = config;

        let options =
            ConnectOptions::new().user_and_password(username, password.expose_secret().to_owned());
        let client = options.connect(url).await?;
        let jetstream = jetstream::new(client);
        let ledger_state_store = create_ledger_state_store(&jetstream, num_replicas).await?;

        Ok(Self { ledger_state_store })
    }
}

impl LedgerStateStorage for NatsLedgerStateStorage {
    type Error = LedgerStateStorageError;

    #[trace]
    async fn load(&self, key: &str) -> Result<SerializedLedgerState, Self::Error> {
        let mut object = self.ledger_state_store.get(key).await?;

        let mut ledger_state = Vec::with_capacity(object.info.size);
        object.read_to_end(&mut ledger_state).await?;

        Ok(ledger_state.into())
    }

    #[trace]
    async fn save(
        &mut self,
        ledger_state: &SerializedLedgerState,
        key: &str,
    ) -> Result<(), Self::Error> {
        let info = self
            .ledger_state_store
            .put(key, &mut ledger_state.as_slice())
            .await
            .map_err(LedgerStateStorageError::PutLedgerState)?;

        if info.size != ledger_state.len() {
            return Err(LedgerStateStorageError::PutLedgerStateSize(
                info.size,
                ledger_state.len(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot connect to NATS server")]
    Connect(#[from] ConnectError),

    #[error("cannot create ledger state store")]
    CreateLedgerStateStore(#[source] CreateObjectStoreError),
}

#[derive(Debug, Error)]
pub enum LedgerStateStorageError {
    #[error("cannot get ledger state from NATS")]
    GetLedgerState(#[from] GetError),

    #[error("cannot save ledger state to NATS")]
    PutLedgerState(#[source] PutError),

    #[error("cannot save ledger state to NATS: invalid object size {0}, expected {1}")]
    PutLedgerStateSize(usize, usize),

    #[error("cannot read ledger state from NATS")]
    ReadLedgerState(#[from] io::Error),
}

/// Configuration settings for [NatsLedgerStateStorage].
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub url: String,
    pub username: String,
    pub password: SecretString,
    pub num_replicas: usize,
}

async fn create_ledger_state_store(
    jetstream: &Jetstream,
    num_replicas: usize,
) -> Result<ObjectStore, Error> {
    let config = object_store::Config {
        bucket: LEDGER_STATE_STORE_NAME.to_string(),
        num_replicas,
        ..Default::default()
    };

    jetstream
        .create_object_store(config)
        .await
        .map_err(Error::CreateLedgerStateStore)
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{ByteVec, LedgerStateStorage},
        infra::ledger_state_storage::nats::{Config, NatsLedgerStateStorage},
    };
    use anyhow::Context;
    use std::time::{Duration, Instant};
    use testcontainers::{GenericImage, ImageExt, core::WaitFor, runners::AsyncRunner};
    use tokio::time::sleep;

    #[tokio::test]
    async fn test() -> anyhow::Result<()> {
        let nats_container = GenericImage::new("nats", "2.12.3")
            .with_wait_for(WaitFor::message_on_stderr("Server is ready"))
            .with_cmd([
                "--user",
                "indexer",
                "--pass",
                env!("APP__INFRA__LEDGER_STATE_STORAGE__PASSWORD"),
                "-js",
            ])
            .start()
            .await
            .context("start NATS container")?;

        // In spite of the above "WaitFor" NATS stubbornly rejects connections.
        let start = Instant::now();
        while reqwest::get("localhost:8222/healthz")
            .await
            .and_then(|r| r.error_for_status())
            .is_err()
            && Instant::now() - start < Duration::from_millis(1_500)
        {
            sleep(Duration::from_millis(100)).await;
        }

        let nats_port = nats_container
            .get_host_port_ipv4(4222)
            .await
            .context("get NATS port")?;
        let nats_url = format!("localhost:{nats_port}");

        let config = Config {
            url: nats_url,
            username: "indexer".to_string(),
            password: env!("APP__INFRA__LEDGER_STATE_STORAGE__PASSWORD").into(),
            num_replicas: 1,
        };
        let mut ledger_state_storage = NatsLedgerStateStorage::new(config)
            .await
            .context("create NatsZswapStateStorage")?;

        let state = ByteVec::from([0u8; 1024].as_slice());

        ledger_state_storage
            .save(&state, "ledger_state_a")
            .await
            .context("save ledger state")?;

        let ledger_state = ledger_state_storage
            .load("ledger_state_a")
            .await
            .context("load ledger state")?;
        assert_eq!(ledger_state, state);

        let state = ByteVec::from([1u8; 1024].as_slice());

        ledger_state_storage
            .save(&state, "ledger_state_b")
            .await
            .context("save ledger state")?;

        let ledger_state = ledger_state_storage
            .load("ledger_state_b")
            .await
            .context("load ledger state")?;
        assert_eq!(ledger_state, state);

        Ok(())
    }
}
