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

use async_graphql::scalar;
use derive_more::{Debug, Display, derive::From};
use serde::{Deserialize, Serialize};

/// Hex-encoded session ID, exposed as the `HexEncoded` scalar.
#[derive(Debug, Display, Clone, PartialEq, Eq, Serialize, Deserialize, From)]
#[from(&str)]
#[debug("{_0}")]
pub struct SessionId(String);

scalar!(SessionId, "HexEncoded");

#[cfg(test)]
mod tests {
    use crate::infra::api::v4::session_id::SessionId;

    #[test]
    fn test_debug_and_display_redacted() {
        let secret = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let session_id = SessionId::from(secret);

        let debug = format!("{session_id:?}");
        assert!(!debug.contains(secret), "secret in Debug: {debug}");

        let display = session_id.to_string();
        assert!(!display.contains(secret), "secret in Display: {display}");
    }
}
