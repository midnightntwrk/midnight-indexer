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

//! A serialized `OnDiskObject` stored as parts rather than one blob: its payload, the hashes of its
//! by-reference children, and the rest of its children section. gc's mark reads only the hashes;
//! a load joins the parts back into the original bytes, which storage-core then deserializes.
//!
//! With layout-v2 an object is its payload (`Vec<u8>`), then its children (`Vec<ArenaKey>`). An
//! `ArenaKey` is a `u8` variant, then a 32-byte hash for `Ref`, or an inlined child's payload and
//! children for `Direct`. Every primitive is read with midnight-serialize itself and copied as
//! read.

use midnight_serialize_v1::Deserializable;
use midnight_storage_core_v1::{DefaultHasher, arena::ArenaHash};
use std::io;

/// `ArenaKey::Ref`'s variant.
const REF: u8 = 0;
/// `ArenaKey::Direct`'s variant.
const DIRECT: u8 = 1;

/// A serialized object as parts.
#[derive(Debug, Default, PartialEq)]
pub struct Parts {
    /// The serialized payload.
    pub data: Vec<u8>,
    /// The serialized hashes of the by-reference children, in walk order.
    pub child_keys: Vec<u8>,
    /// The children section without those hashes: counts, variants and inlined children.
    pub child_values: Vec<u8>,
}

/// Splits a serialized object into its parts.
pub fn split(object: &[u8]) -> io::Result<Parts> {
    let mut reader = object;
    let mut parts = Parts {
        data: take::<Vec<u8>>(&mut reader, 0)?.to_vec(),
        ..Default::default()
    };
    split_children(&mut reader, &mut parts, 0)?;
    ensure_empty(reader)?;
    Ok(parts)
}

/// Joins parts back into the serialized object they were split from.
pub fn join(parts: &Parts) -> io::Result<Vec<u8>> {
    let mut object = parts.data.clone();
    let mut values = parts.child_values.as_slice();
    let mut keys = parts.child_keys.as_slice();
    join_children(&mut values, &mut keys, &mut object, 0)?;
    ensure_empty(values)?;
    ensure_empty(keys)?;
    Ok(object)
}

fn split_children(reader: &mut &[u8], parts: &mut Parts, depth: u32) -> io::Result<()> {
    let count = copy::<u32>(reader, &mut parts.child_values, depth)?;
    for _ in 0..count {
        match copy::<u8>(reader, &mut parts.child_values, depth)? {
            REF => parts
                .child_keys
                .extend_from_slice(take::<ArenaHash<DefaultHasher>>(reader, depth)?),
            DIRECT => {
                copy::<Vec<u8>>(reader, &mut parts.child_values, depth)?;
                split_children(reader, parts, depth + 1)?;
            }
            variant => return Err(invalid(format!("unknown child variant {variant}"))),
        }
    }
    Ok(())
}

fn join_children(
    values: &mut &[u8],
    keys: &mut &[u8],
    object: &mut Vec<u8>,
    depth: u32,
) -> io::Result<()> {
    let count = copy::<u32>(values, object, depth)?;
    for _ in 0..count {
        match copy::<u8>(values, object, depth)? {
            REF => {
                copy::<ArenaHash<DefaultHasher>>(keys, object, depth)?;
            }
            DIRECT => {
                copy::<Vec<u8>>(values, object, depth)?;
                join_children(values, keys, object, depth + 1)?;
            }
            variant => return Err(invalid(format!("unknown child variant {variant}"))),
        }
    }
    Ok(())
}

/// Reads a `T` and returns the bytes it was read from.
fn take<'a, T: Deserializable>(reader: &mut &'a [u8], depth: u32) -> io::Result<&'a [u8]> {
    let start = *reader;
    T::deserialize(reader, depth)?;
    Ok(&start[..start.len() - reader.len()])
}

/// Reads a `T`, appends the bytes it was read from to `out`, and returns it.
fn copy<T: Deserializable>(reader: &mut &[u8], out: &mut Vec<u8>, depth: u32) -> io::Result<T> {
    let start = *reader;
    let value = T::deserialize(reader, depth)?;
    out.extend_from_slice(&start[..start.len() - reader.len()]);
    Ok(value)
}

fn ensure_empty(rest: &[u8]) -> io::Result<()> {
    if rest.is_empty() {
        Ok(())
    } else {
        Err(invalid(format!("{} bytes left over", rest.len())))
    }
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(all(test, feature = "standalone"))]
mod tests {
    use super::{Parts, join, split};
    use crate::infra::ledger_db::v1_1::{LedgerDb, sqlite_tests::setup_pool};
    use midnight_serialize_v1::{Deserializable, Serializable};
    use midnight_storage_core_v1::{
        DefaultHasher, Storage, backend::OnDiskObject, db::DB, storable::SMALL_OBJECT_LIMIT,
    };
    use std::{collections::HashSet, error::Error as StdError};

    // Every stored node splits and joins back into the same bytes, which deserialize, and its child
    // keys are exactly its by-reference children, also those nested in inlined children.
    #[tokio::test(flavor = "multi_thread")]
    async fn split_then_join_restores_every_object() -> Result<(), Box<dyn StdError>> {
        let pool = setup_pool().await?;
        let storage = Storage::new(16, LedgerDb::new(pool.clone()));
        let db = LedgerDb::new(pool);

        // Small `Option`s are inlined; an array over the limit is stored by reference, so a small
        // `Option` around one is an inlined child with a by-reference child of its own.
        let mut nested = (0..4u32)
            .map(|value| {
                let big = storage.alloc([value; SMALL_OBJECT_LIMIT / 4]);
                storage.alloc(Some(storage.alloc(Some(big))))
            })
            .collect::<Vec<_>>();
        let mut inlined = (0..4u32)
            .map(|value| storage.alloc(Some(storage.alloc(Some(storage.alloc(value))))))
            .collect::<Vec<_>>();
        nested.iter_mut().for_each(|root| root.persist());
        inlined.iter_mut().for_each(|root| root.persist());
        storage.with_backend(|backend| backend.flush_all_changes_to_db());

        let (nodes, _) = db.scan(None, 100);
        let mut refs_seen = 0;
        for (key, object) in nodes {
            let mut bytes = vec![];
            object.serialize(&mut bytes)?;

            let parts = split(&bytes)?;
            let joined = join(&parts)?;
            assert_eq!(joined, bytes, "{key:?} joins back");
            OnDiskObject::<DefaultHasher>::deserialize(&mut joined.as_slice(), 0)?;

            let (keys, rest) = parts.child_keys.as_chunks::<32>();
            assert!(rest.is_empty());
            let refs = object
                .children
                .iter()
                .flat_map(|child| child.refs())
                .map(|hash| hash.0.to_vec())
                .collect::<HashSet<_>>();
            assert_eq!(keys.iter().map(|key| key.to_vec()).collect::<HashSet<_>>(), refs);
            refs_seen += refs.len();

            // A truncated object, or parts with a key too many, are rejected.
            assert!(split(&bytes[..bytes.len() - 1]).is_err());
            let child_keys = [parts.child_keys.as_slice(), &[0; 32]].concat();
            assert!(join(&Parts { child_keys, ..parts }).is_err());
        }
        assert!(refs_seen > 0, "some objects have by-reference children");

        Ok(())
    }
}
