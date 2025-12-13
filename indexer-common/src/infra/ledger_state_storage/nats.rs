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

use crate::domain::{LedgerStateStorage, ProtocolVersion, SerializedLedgerState};
use async_nats::{
    ConnectError, ConnectOptions,
    jetstream::{
        self, Context as Jetstream,
        context::{CreateKeyValueError, CreateObjectStoreError},
        kv::{self, EntryError, Store},
        object_store::{self, GetError, ObjectStore, PutError},
    },
};
use fastrace::trace;
use futures::{StreamExt, stream};
use log::info;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::io::{self, Cursor};
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio_util::{bytes::Bytes, io::StreamReader};

const AB_SELECTOR_STORE_NAME: &str = "ab_selector_store";
const AB_SELECTOR_KEY: &str = "ab_selector";
const LEDGER_STATE_STORE_NAME: &str = "ledger_state_store";
const LEDGER_STATE_KEY_A: &str = "ledger_state_a";
const LEDGER_STATE_KEY_B: &str = "ledger_state_b";

/// NATS based ledger state storage implementation.
pub struct NatsLedgerStateStorage {
    ledger_state_store: ObjectStore,
    ab_selector_store: Store,
}

impl NatsLedgerStateStorage {
    /// Create a new ledger state storage with the given configuration.
    pub async fn new(config: Config) -> Result<Self, Error> {
        let Config {
            url,
            username,
            password,
        } = config;

        let options =
            ConnectOptions::new().user_and_password(username, password.expose_secret().to_owned());
        let client = options.connect(url).await?;
        let jetstream = jetstream::new(client);
        let ledger_state_store = create_ledger_state_store(&jetstream).await?;
        let ab_selector_store = create_ab_selector_store(&jetstream).await?;

        Ok(Self {
            ledger_state_store,
            ab_selector_store,
        })
    }

    async fn ab_selector(&self) -> Result<ABSelector, LedgerStateStorageError> {
        let ab_selector = self
            .ab_selector_store
            .get(AB_SELECTOR_KEY)
            .await?
            .map(Into::into)
            .unwrap_or_default();

        Ok(ab_selector)
    }
}

impl LedgerStateStorage for NatsLedgerStateStorage {
    type Error = LedgerStateStorageError;

    #[trace]
    async fn load_highest_zswap_state_index(&self) -> Result<Option<u64>, Self::Error> {
        let ab_selector = self.ab_selector().await?;
        let object = self.ledger_state_store.get(ab_selector.object_name()).await;

        match object {
            Ok(mut object) => {
                let highest_zswap_state_index = object.read_u64_le().await?;

                // We (ab)use `u64::MAX` as None!
                Ok((highest_zswap_state_index != u64::MAX).then_some(highest_zswap_state_index))
            }

            Err(error) if error.kind() == object_store::GetErrorKind::NotFound => Ok(None),

            Err(other) => Err(other)?,
        }
    }

    #[trace]
    async fn load_ledger_state(
        &self,
    ) -> Result<Option<(SerializedLedgerState, u32, ProtocolVersion)>, Self::Error> {
        let ab_selector = self.ab_selector().await?;
        let object = self.ledger_state_store.get(ab_selector.object_name()).await;

        match object {
            Ok(mut object) => {
                let _ = object.read_u64_le().await?;
                let block_height = object.read_u32_le().await?;
                let protocol_version = object.read_u32_le().await?.into();
                let mut bytes = Vec::with_capacity(object.info.size - 16);
                object.read_to_end(&mut bytes).await?;

                Ok(Some((bytes.into(), block_height, protocol_version)))
            }

            Err(error) if error.kind() == object_store::GetErrorKind::NotFound => Ok(None),

            Err(other) => Err(other)?,
        }
    }

    #[trace]
    async fn save(
        &mut self,
        ledger_state: &SerializedLedgerState,
        block_height: u32,
        highest_zswap_state_index: Option<u64>,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error> {
        info!(block_height, highest_zswap_state_index:?; "saving ledger state");

        let mut size = 0;

        // We (ab)use `u64::MAX` as None!
        let highest_zswap_state_index = highest_zswap_state_index.unwrap_or(u64::MAX).to_le_bytes();
        size += highest_zswap_state_index.len();
        let highest_zswap_state_index = Cursor::new(highest_zswap_state_index.as_slice());

        let block_height = block_height.to_le_bytes();
        size += block_height.len();
        let block_height = Cursor::new(block_height.as_slice());

        let protocol_version = protocol_version.0.to_le_bytes();
        size += protocol_version.len();
        let protocol_version = Cursor::new(protocol_version.as_slice());

        size += ledger_state.len();
        let ledger_state = Cursor::new(ledger_state.as_ref());

        let object = stream::iter([
            highest_zswap_state_index,
            block_height,
            protocol_version,
            ledger_state,
        ]);
        let mut object = StreamReader::new(object.map(Ok::<_, io::Error>));

        let ab_selector = self.ab_selector().await?.toggle();

        let info = self
            .ledger_state_store
            .put(ab_selector.object_name(), &mut object)
            .await?;

        if info.size != size {
            return Err(LedgerStateStorageError::SaveLedgerStateSize(
                info.size, size,
            ));
        }

        self.ab_selector_store
            .put(AB_SELECTOR_KEY, ab_selector.into())
            .await?;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
enum ABSelector {
    #[default]
    A,
    B,
}

impl ABSelector {
    fn toggle(self) -> Self {
        match self {
            ABSelector::A => ABSelector::B,
            ABSelector::B => ABSelector::A,
        }
    }

    fn object_name(self) -> &'static str {
        match self {
            ABSelector::A => LEDGER_STATE_KEY_A,
            ABSelector::B => LEDGER_STATE_KEY_B,
        }
    }
}

impl From<Bytes> for ABSelector {
    fn from(bytes: Bytes) -> Self {
        match bytes.as_ref() {
            [] | [0] => ABSelector::A,
            _ => ABSelector::B,
        }
    }
}

impl From<ABSelector> for Bytes {
    fn from(ab_selector: ABSelector) -> Self {
        match ab_selector {
            ABSelector::A => vec![0].into(),
            ABSelector::B => vec![1].into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot connect to NATS server")]
    Connect(#[from] ConnectError),

    #[error("cannot create ledger state store")]
    CreateLedgerStateStore(#[source] CreateObjectStoreError),

    #[error("cannot create A/B selector store")]
    CreateABSelectorStore(#[source] CreateKeyValueError),
}

#[derive(Debug, Error)]
pub enum LedgerStateStorageError {
    #[error("cannot get AB selector")]
    GetABSelector(#[from] EntryError),

    #[error("cannot put AB selector")]
    PutABSelector(#[from] kv::PutError),

    #[error("cannot load ledger state")]
    GetLedgerState(#[from] GetError),

    #[error("cannot read ledger state")]
    ReadLedgerState(#[from] io::Error),

    #[error("cannot save ledger state")]
    SaveLedgerState(#[from] PutError),

    #[error("cannot save ledger state: invalid object size ${0}, expected ${1}")]
    SaveLedgerStateSize(usize, usize),
}

/// Configuration settings for [NatsLedgerStateStorage].
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub url: String,
    pub username: String,
    pub password: SecretString,
}

async fn create_ledger_state_store(jetstream: &Jetstream) -> Result<ObjectStore, Error> {
    let config = object_store::Config {
        bucket: LEDGER_STATE_STORE_NAME.to_string(),
        ..Default::default()
    };

    jetstream
        .create_object_store(config)
        .await
        .map_err(Error::CreateLedgerStateStore)
}

async fn create_ab_selector_store(jetstream: &Jetstream) -> Result<Store, Error> {
    let config = kv::Config {
        bucket: AB_SELECTOR_STORE_NAME.to_string(),
        ..Default::default()
    };

    jetstream
        .create_key_value(config)
        .await
        .map_err(Error::CreateABSelectorStore)
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{ByteVec, LedgerStateStorage, PROTOCOL_VERSION_000_018_000},
        infra::ledger_state_storage::nats::{Config, NatsLedgerStateStorage},
    };
    use anyhow::Context;
    use assert_matches::assert_matches;
    use std::time::{Duration, Instant};
    use testcontainers::{GenericImage, ImageExt, core::WaitFor, runners::AsyncRunner};
    use tokio::time::sleep;

    #[tokio::test]
    async fn test() -> anyhow::Result<()> {
        let nats_container = GenericImage::new("nats", "2.11.1")
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
        };
        let mut ledger_state_storage = NatsLedgerStateStorage::new(config)
            .await
            .context("create NatsZswapStateStorage")?;

        let last_index = ledger_state_storage
            .load_highest_zswap_state_index()
            .await
            .context("load last index")?;
        assert!(last_index.is_none());

        let ledger_state = ledger_state_storage
            .load_ledger_state()
            .await
            .context("load ledger state")?;
        assert!(ledger_state.is_none());

        let state = ByteVec::from([0u8; 1024].as_slice());

        ledger_state_storage
            .save(&state, 0, None, PROTOCOL_VERSION_000_018_000)
            .await
            .context("save ledger state")?;

        let last_index = ledger_state_storage
            .load_highest_zswap_state_index()
            .await
            .context("load last index")?;
        assert!(last_index.is_none());

        let ledger_state = ledger_state_storage
            .load_ledger_state()
            .await
            .context("load ledger state")?;
        assert_matches!(
            ledger_state,
            Some((s, 0, PROTOCOL_VERSION_000_018_000)) if s == state
        );

        let state = ByteVec::from([1u8; 1024].as_slice());

        ledger_state_storage
            .save(&state, 42, Some(42), PROTOCOL_VERSION_000_018_000)
            .await
            .context("save ledger state")?;

        let last_index = ledger_state_storage
            .load_highest_zswap_state_index()
            .await
            .context("load last index")?;
        assert_matches!(last_index, Some(42));

        let ledger_state = ledger_state_storage
            .load_ledger_state()
            .await
            .context("load ledger state")?;
        assert_matches!(
            ledger_state,
            Some((s, 42, PROTOCOL_VERSION_000_018_000)) if s == state
        );

        Ok(())
    }
}
