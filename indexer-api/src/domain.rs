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

mod api;
mod block;
mod contract_action;
mod grpc;
mod storage;
mod transaction;
mod viewing_key;
mod zswap;

pub use api::*;
pub use block::*;
pub use contract_action::*;
pub use grpc::*;
pub use storage::*;
pub use transaction::*;
pub use viewing_key::*;
pub use zswap::*;

use async_graphql::scalar;
use const_hex::FromHexError;
use derive_more::{Debug, Display};
use serde::{Deserialize, Serialize};
use std::any::type_name;
use thiserror::Error;

/// Wrapper around hex-encoded bytes.
#[derive(Debug, Display, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[debug("{_0}")]
pub struct HexEncoded(String);

scalar!(HexEncoded);

impl HexEncoded {
    /// Hex-decode this [HexEncoded] into some type that can be made from bytes.
    pub fn hex_decode<T>(&self) -> Result<T, HexDecodeError>
    where
        T: TryFrom<Vec<u8>>,
    {
        let bytes = const_hex::decode(&self.0)?;
        let decoded = bytes
            .try_into()
            .map_err(|_| HexDecodeError::Convert(type_name::<T>()))?;
        Ok(decoded)
    }
}

#[derive(Debug, Error)]
pub enum HexDecodeError {
    #[error("cannot hex-decode")]
    Decode(#[from] FromHexError),

    #[error("cannot convert to {0}")]
    Convert(&'static str),
}

// Needed to derive `Interface` for `ContractAction`. Weird!
impl From<&HexEncoded> for HexEncoded {
    #[cfg_attr(coverage, coverage(off))]
    fn from(value: &HexEncoded) -> Self {
        value.to_owned()
    }
}

impl TryFrom<String> for HexEncoded {
    type Error = FromHexError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        const_hex::decode(&s)?;
        Ok(Self(s))
    }
}

impl TryFrom<&str> for HexEncoded {
    type Error = FromHexError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        const_hex::decode(s)?;
        Ok(Self(s.to_owned()))
    }
}

pub trait AsBytesExt
where
    Self: AsRef<[u8]>,
{
    fn hex_encode(&self) -> HexEncoded {
        HexEncoded(const_hex::encode(self.as_ref()))
    }
}

impl<T> AsBytesExt for T where T: AsRef<[u8]> {}
