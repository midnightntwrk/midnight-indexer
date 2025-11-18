// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{application, infra};
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
