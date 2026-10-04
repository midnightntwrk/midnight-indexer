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

//! `archive_v1_storage` queries, and the storage items a block is sourced with.

use crate::{
    infra::subxt_node::rpc::{NodeRpc, Subscription, Transport, hex, method},
    pipeline::sourcing::{
        AUTHORITY_SET_ITEMS, CNIGHT_MAPPINGS_ITEMS, Error, SYSTEM_EVENTS_ITEM,
        SYSTEM_PARAMETERS_ITEMS, storage_key,
    },
};
use futures::StreamExt;
use indexer_common::domain::{BlockHash, ByteVec};
use parity_scale_codec::Encode;
use serde::Deserialize;
use serde_json::{Value, json};

/// One result of an `archive_v1_storage` query.
#[derive(Debug)]
pub(super) struct StorageItem {
    pub(super) key: ByteVec,
    pub(super) value: Option<ByteVec>,
    pub(super) hash: Option<ByteVec>,
}

/// An `archive_v1_storage` event.
#[derive(Debug, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
enum StorageEvent {
    Storage {
        key: String,
        value: Option<String>,
        hash: Option<String>,
    },
    StorageError {
        error: String,
    },
    StorageDone,
}

/// Run an `archive_v1_storage` query at the given block and collect its results.
pub(super) async fn query_storage<T: Transport>(
    rpc: &NodeRpc<T>,
    hash: BlockHash,
    items: Vec<Value>,
) -> Result<Vec<StorageItem>, Error> {
    let Subscription {
        mut notifications, ..
    } = rpc
        .subscribe(
            method::ARCHIVE_STORAGE,
            vec![hex(hash.0), items.into(), Value::Null],
            method::ARCHIVE_STOP_STORAGE,
        )
        .await?;

    let mut items = vec![];
    loop {
        let event = notifications
            .next()
            .await
            .ok_or_else(|| Error::Storage {
                hash,
                error: "subscription ended before storageDone".to_owned(),
            })?
            .map_err(|error| Error::Storage {
                hash,
                error: error.to_string(),
            })?;

        let event = serde_json::from_value(event).map_err(|error| Error::Decode {
            what: "storage event",
            hash,
            source: error.into(),
        })?;
        use StorageEvent::*;
        match event {
            Storage {
                key,
                value,
                hash: value_hash,
            } => {
                let decode = |bytes: String| {
                    const_hex::decode(bytes)
                        .map(ByteVec::from)
                        .map_err(|error| Error::Decode {
                            what: "storage item",
                            hash,
                            source: error.into(),
                        })
                };
                let key = decode(key)?;
                let value = value.map(decode).transpose()?;
                let value_hash = value_hash.map(decode).transpose()?;

                let bytes = value
                    .as_ref()
                    .or(value_hash.as_ref())
                    .map(|v| v.len())
                    .unwrap_or(0);
                rpc.counters().record(
                    &format!("{} {}", method::ARCHIVE_STORAGE, storage_label(&key)),
                    1,
                    0,
                    bytes as u64,
                );

                items.push(StorageItem {
                    key,
                    value,
                    hash: value_hash,
                });
            }
            StorageError { error } => return Err(Error::Storage { hash, error }),
            StorageDone => return Ok(items),
        }
    }
}

/// The item a storage key belongs to, as `Pallet.Entry`.
fn storage_label(key: &[u8]) -> String {
    std::iter::once(SYSTEM_EVENTS_ITEM)
        .chain(AUTHORITY_SET_ITEMS)
        .chain(SYSTEM_PARAMETERS_ITEMS)
        .chain(CNIGHT_MAPPINGS_ITEMS)
        .find(|&item| key.starts_with(&storage_key(item)))
        .map(|(pallet, entry)| format!("{pallet}.{entry}"))
        .unwrap_or_else(|| "other".to_owned())
}

pub(super) fn storage_query(item: (&str, &str), query_type: &str) -> Value {
    json!({ "key": hex(storage_key(item)), "type": query_type })
}

/// A block's storage query: its events, and the hashes of its authority set and system parameters.
pub(super) fn block_storage_items() -> Vec<Value> {
    std::iter::once(storage_query(SYSTEM_EVENTS_ITEM, "value"))
        .chain(authority_set_items("hash"))
        .chain(system_parameter_items())
        .collect()
}

/// A parent's storage query: its authority set, and the hashes of its authority set and system
/// parameters.
pub(super) fn parent_storage_items() -> Vec<Value> {
    authority_set_items("value")
        .chain(authority_set_items("hash"))
        .chain(system_parameter_items())
        .collect()
}

pub(super) fn authority_set_items(query_type: &str) -> impl Iterator<Item = Value> {
    AUTHORITY_SET_ITEMS
        .into_iter()
        .map(move |item| storage_query(item, query_type))
}

fn system_parameter_items() -> impl Iterator<Item = Value> {
    SYSTEM_PARAMETERS_ITEMS
        .into_iter()
        .map(|item| storage_query(item, "hash"))
}

fn find_item<'a>(items: &'a [StorageItem], item: (&str, &str)) -> Option<&'a StorageItem> {
    let key = storage_key(item);
    items.iter().find(|stored| *stored.key == key)
}

/// The authority-set items present, as key and value, in [AUTHORITY_SET_ITEMS] order.
pub(super) fn authority_set_of(items: &[StorageItem]) -> Vec<(ByteVec, ByteVec)> {
    authority_set_entries(items, |item| item.value.as_ref())
}

/// The authority-set items present, as key and value hash, in [AUTHORITY_SET_ITEMS] order.
pub(super) fn authority_set_hashes(items: &[StorageItem]) -> Vec<(ByteVec, ByteVec)> {
    authority_set_entries(items, |item| item.hash.as_ref())
}

fn authority_set_entries(
    items: &[StorageItem],
    field: impl Fn(&StorageItem) -> Option<&ByteVec>,
) -> Vec<(ByteVec, ByteVec)> {
    AUTHORITY_SET_ITEMS
        .into_iter()
        .filter_map(|item| {
            let key = storage_key(item);
            items
                .iter()
                .filter(|stored| *stored.key == key)
                .find_map(|stored| {
                    field(stored).map(|bytes| (stored.key.to_owned(), bytes.to_owned()))
                })
        })
        .collect()
}

pub(super) fn system_parameter_hashes(items: &[StorageItem]) -> [Option<ByteVec>; 2] {
    SYSTEM_PARAMETERS_ITEMS.map(|item| find_item(items, item).and_then(|item| item.hash.to_owned()))
}

/// The serialized `System.Events` value; absent storage means no events.
pub(super) fn events_of(items: &[StorageItem]) -> ByteVec {
    find_item(items, SYSTEM_EVENTS_ITEM)
        .and_then(|item| item.value.to_owned())
        .unwrap_or_else(|| Vec::<()>::new().encode().into())
}

#[cfg(test)]
mod tests {
    use crate::pipeline::sourcing::{
        AUTHORITY_SET_ITEMS, CNIGHT_MAPPINGS_ITEMS, SYSTEM_EVENTS_ITEM, SYSTEM_PARAMETERS_ITEMS,
        storage_key,
    };
    use std::{fs, path::Path};
    use subxt::Metadata;

    #[test]
    fn test_storage_keys() {
        // Published key of `System.Events`.
        assert_eq!(
            const_hex::encode(storage_key(SYSTEM_EVENTS_ITEM)),
            "26aa394eea5630e07c48ae0c9558cef780d41e5e16056765bc8461851072c9d7"
        );

        let node_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.node");
        let node_versions = fs::read_to_string(node_dir.join("../NODE_VERSIONS"))
            .expect("NODE_VERSIONS can be read");

        for node_version in node_versions
            .lines()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            let metadata = fs::read(node_dir.join(node_version).join("metadata.scale"))
                .expect("metadata can be read");
            let metadata = <Metadata as parity_scale_codec::Decode>::decode(&mut &*metadata)
                .expect("metadata can be decoded");

            let has = |(pallet, entry): (&str, &str)| {
                metadata
                    .pallet_by_name(pallet)
                    .and_then(|pallet| pallet.storage())
                    .and_then(|storage| storage.entry_by_name(entry))
                    .is_some()
            };

            assert!(has(SYSTEM_EVENTS_ITEM), "{node_version}: System.Events");
            assert!(
                has(AUTHORITY_SET_ITEMS[0]),
                "{node_version}: Aura.Authorities"
            );
            for item in SYSTEM_PARAMETERS_ITEMS {
                assert!(has(item), "{node_version}: {item:?}");
            }
            assert!(
                CNIGHT_MAPPINGS_ITEMS.into_iter().any(has),
                "{node_version}: cNight mappings"
            );
        }
    }
}
