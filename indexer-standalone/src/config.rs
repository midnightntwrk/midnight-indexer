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

use indexer_common::{infra::pool, telemetry};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub run_migrations: bool,

    #[serde(rename = "chain_indexer_application")]
    pub chain_indexer_application_config: chain_indexer::application::Config,

    #[serde(alias = "wallet_indexer_application")]
    pub wallet_indexer_application_config: wallet_indexer::application::Config,

    #[serde(rename = "infra")]
    pub infra_config: InfraConfig,

    #[serde(rename = "telemetry")]
    pub telemetry_config: telemetry::Config,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InfraConfig {
    pub secret: secrecy::SecretString,

    #[serde(rename = "node")]
    pub node_config: chain_indexer::infra::node::Config,

    #[serde(rename = "storage")]
    pub storage_config: pool::sqlite::Config,

    #[serde(rename = "api")]
    pub api_config: indexer_api::infra::api::Config,
}
