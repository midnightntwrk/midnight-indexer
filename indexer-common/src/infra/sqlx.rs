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

use crate::domain::{ByteArray, TryFromForByteArrayError};
use sqlx::{Database, Decode, Type, error::BoxDynError};

/// A helper to use `Option<T>` where T does not implement `sqlx::Type` but a `TryFrom` into a
/// supported type with `sqlx::FromRow` like this:
/// ```
/// #[sqlx(try_from = "SqlxOption<&'a [u8]>")]
/// pub author: Option<ByteArray>,
/// ```
pub struct SqlxOption<T>(Option<T>);

impl<T> From<SqlxOption<T>> for Option<T> {
    fn from(value: SqlxOption<T>) -> Self {
        value.0
    }
}

impl<T, D> Type<D> for SqlxOption<T>
where
    T: Type<D>,
    D: Database,
{
    fn type_info() -> D::TypeInfo {
        Option::<T>::type_info()
    }
}

impl<'r, T, D> Decode<'r, D> for SqlxOption<T>
where
    T: Decode<'r, D>,
    D: Database,
{
    fn decode(value: D::ValueRef<'r>) -> Result<Self, BoxDynError> {
        let option = Option::<T>::decode(value)?;
        Ok(Self(option))
    }
}

impl TryFrom<SqlxOption<i64>> for Option<u64> {
    type Error = BoxDynError;

    fn try_from(value: SqlxOption<i64>) -> Result<Self, Self::Error> {
        let value = value.0.map(TryInto::try_into).transpose()?;
        Ok(value)
    }
}

impl<const N: usize> TryFrom<SqlxOption<&[u8]>> for Option<ByteArray<N>> {
    type Error = TryFromForByteArrayError;

    fn try_from(value: SqlxOption<&[u8]>) -> Result<Self, Self::Error> {
        let value = value.0.map(TryInto::try_into).transpose()?;
        Ok(value)
    }
}
