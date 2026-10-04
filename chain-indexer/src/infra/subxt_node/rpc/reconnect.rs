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

//! [ReconnectPolicy]: how often and how patiently to reconnect.

use crate::infra::subxt_node::rpc::{Error, TransportError};
use log::debug;
use std::time::Duration;
use tokio::time::sleep;

/// How often and how patiently to reconnect after the connection is lost.
#[derive(Debug, Clone, Copy)]
pub struct ReconnectPolicy {
    pub max_delay: Duration,
    pub max_attempts: usize,
}

impl ReconnectPolicy {
    /// Exponential backoff from 10 ms, doubling up to `max_delay`.
    fn delay(&self, attempt: usize) -> Duration {
        let millis = 10u64.saturating_mul(2u64.saturating_pow(attempt as u32));
        Duration::from_millis(millis).min(self.max_delay)
    }

    /// Retry `attempt` after it failed with `error`, waiting before each try, at most
    /// `max_attempts` times; [Error::Unreachable] with the last error if none succeeds.
    pub(crate) async fn retry<T, F: Future<Output = Result<T, TransportError>>>(
        &self,
        error: TransportError,
        mut attempt: impl FnMut() -> F,
    ) -> Result<T, Error> {
        let mut last_error = error;
        for n in 0..self.max_attempts {
            sleep(self.delay(n)).await;
            match attempt().await {
                Ok(value) => {
                    debug!(attempt = n; "connected to node");
                    return Ok(value);
                }
                Err(error) => last_error = error,
            }
        }

        Err(Error::Unreachable {
            attempts: self.max_attempts,
            source: last_error,
        })
    }
}
