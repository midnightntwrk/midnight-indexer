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

use crate::{application, infra};
use std::num::NonZeroUsize;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    #[serde(with = "byte_unit_serde")]
    pub thread_stack_size: u64,

    /// Cap for the Tokio blocking pool. `None` uses
    /// [`DEFAULT_MAX_BLOCKING_THREADS`](crate::infra::api::ledger_query_limit::DEFAULT_MAX_BLOCKING_THREADS)
    /// rather than tokio's default of 512, which at `thread_stack_size` would be gigabytes of
    /// thread stacks. A ledger walk occupies one of these threads for its whole duration.
    #[serde(default)]
    pub max_blocking_threads: Option<NonZeroUsize>,

    /// Maximum concurrent ledger-DB-backed GraphQL queries (issue #595). `None` defaults to half
    /// of the storage pool's `max_connections`, the pool the ledger DB shares with every other
    /// resolver. Must stay below `max_blocking_threads`, or ledger queries can still exhaust the
    /// blocking pool and wedge the runtime.
    #[serde(default)]
    pub ledger_query_concurrency: Option<NonZeroUsize>,

    #[serde(rename = "application")]
    pub application_config: application::Config,

    #[serde(rename = "infra")]
    pub infra_config: infra::Config,

    #[serde(rename = "telemetry")]
    pub telemetry_config: indexer_common::telemetry::Config,
}
