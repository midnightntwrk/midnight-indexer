// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

pub mod api;

#[cfg_attr(docsrs, doc(cfg(feature = "cloud")))]
#[cfg(feature = "cloud")]
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    #[serde(rename = "api")]
    pub api_config: api::Config,

    #[serde(rename = "storage")]
    pub storage_config: indexer_common::infra::pool::postgres::Config,
}
