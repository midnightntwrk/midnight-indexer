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

#[cfg_attr(docsrs, doc(cfg(feature = "cloud")))]
#[cfg(feature = "cloud")]
pub mod postgres;
#[cfg_attr(docsrs, doc(cfg(feature = "standalone")))]
#[cfg(feature = "standalone")]
pub mod sqlite;

use crate::domain;
use chacha20poly1305::ChaCha20Poly1305;
use indexer_common::domain::{DecryptViewingKeyError, ViewingKey};
use sqlx::{
    prelude::FromRow,
    types::{Uuid, time::OffsetDateTime},
};

/// Persistent wallet data.
#[derive(Debug, Clone, FromRow)]
pub struct Wallet {
    pub id: Uuid,

    pub viewing_key: Vec<u8>,

    #[sqlx(try_from = "i64")]
    pub last_indexed_transaction_id: u64,

    pub last_active: OffsetDateTime,
}

impl TryFrom<(Wallet, &ChaCha20Poly1305)> for domain::Wallet {
    type Error = DecryptViewingKeyError;

    fn try_from((wallet, cipher): (Wallet, &ChaCha20Poly1305)) -> Result<Self, Self::Error> {
        let Wallet {
            id,
            viewing_key,
            last_indexed_transaction_id,
            ..
        } = wallet;

        let viewing_key = ViewingKey::decrypt(&viewing_key, id, cipher)?;

        Ok(domain::Wallet {
            viewing_key,
            last_indexed_transaction_id,
        })
    }
}
