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

//! Runtime metadata, fetched from the node once per runtime.

use crate::{
    infra::subxt_node::rpc::{Batch, NodeRpc, Transport},
    pipeline::sourcing::{Error, call_value},
};
use indexer_common::domain::BlockHash;
use parity_scale_codec::{Decode, Encode};
use std::{collections::HashMap, sync::Arc};
use subxt::{ArcMetadata, Metadata, metadata::SUPPORTED_METADATA_VERSIONS};
use tokio::sync::Mutex;

/// Metadata by runtime spec version, fetched from the node once per spec version.
#[derive(Default)]
pub(super) struct MetadataCache(Mutex<HashMap<u32, ArcMetadata>>);

impl MetadataCache {
    /// The metadata of the runtime with the given spec version, which executed the given block.
    /// Unless cached, it is fetched from the parent's state, which runs that runtime after a
    /// `set_code` upgrade, else from the block's own state, which runs it after a switch without
    /// one. Fetched metadata must declare the spec version.
    pub(super) async fn get<T: Transport>(
        &self,
        rpc: &NodeRpc<T>,
        spec_version: u32,
        block: BlockHash,
        parent: Option<BlockHash>,
    ) -> Result<ArcMetadata, Error> {
        let mut cache = self.0.lock().await;
        if let Some(metadata) = cache.get(&spec_version) {
            return Ok(metadata.clone());
        }

        let mut found = vec![];
        for at in parent.into_iter().chain([block]) {
            let metadata = fetch_metadata(rpc, at).await?;
            match metadata_spec_version(&metadata) {
                Some(version) if version == spec_version => {
                    let metadata = Arc::new(metadata);
                    cache.insert(spec_version, metadata.clone());
                    return Ok(metadata);
                }
                version => found.push(version),
            }
        }

        Err(Error::MetadataVersion {
            hash: block,
            spec_version,
            found,
        })
    }
}

/// The spec version a runtime's metadata declares in its `System.Version` constant.
pub fn metadata_spec_version(metadata: &Metadata) -> Option<u32> {
    let version = metadata
        .pallet_by_name("System")?
        .constant_by_name("Version")?;
    // `RuntimeVersion` starts with `spec_name`, `impl_name`, `authoring_version`, `spec_version`.
    let (_, _, _, spec_version) =
        <(String, String, u32, u32)>::decode(&mut version.value()).ok()?;
    Some(spec_version)
}

/// Fetch metadata as subxt does: the highest version `Metadata_metadata_versions` offers that subxt
/// can decode, falling back to `Metadata_metadata`.
async fn fetch_metadata<T: Transport>(rpc: &NodeRpc<T>, at: BlockHash) -> Result<Metadata, Error> {
    let call = |function: &'static str, parameters: Vec<u8>| async move {
        let batch = Batch::default().call(at, function, &parameters);
        let result = rpc.batch(batch).await?.pop().expect("one result per call");
        call_value(result, function, at)
    };
    let decode_error = |error: parity_scale_codec::Error| Error::Decode {
        what: "metadata",
        hash: at,
        source: error.into(),
    };

    let version = call("Metadata_metadata_versions", vec![])
        .await
        .ok()
        .and_then(|versions| Vec::<u32>::decode(&mut &versions[..]).ok())
        .and_then(|versions| {
            SUPPORTED_METADATA_VERSIONS
                .into_iter()
                .find(|supported| versions.contains(supported))
        });

    let metadata = match version {
        Some(version) => {
            let metadata = call("Metadata_metadata_at_version", version.encode()).await?;
            Option::<Vec<u8>>::decode(&mut &metadata[..])
                .map_err(decode_error)?
                .ok_or_else(|| Error::RuntimeCall {
                    function: "Metadata_metadata_at_version",
                    hash: at,
                    error: format!("no metadata of version {version}"),
                })?
        }
        None => {
            let metadata = call("Metadata_metadata", vec![]).await?;
            Vec::<u8>::decode(&mut &metadata[..]).map_err(decode_error)?
        }
    };

    Metadata::decode(&mut &*metadata).map_err(decode_error)
}

#[cfg(test)]
mod tests {
    use crate::{
        infra::subxt_node::rpc::{NodeRpc, ReconnectPolicy, method, testing::FakeNode},
        pipeline::sourcing::metadata::fetch_metadata,
    };
    use indexer_common::domain::ByteArray;
    use parity_scale_codec::{Decode, Encode};
    use serde_json::{Value, json};
    use std::{fs, num::NonZeroUsize, path::Path, sync::Arc, time::Duration};

    #[tokio::test(start_paused = true)]
    async fn test_highest_supported_version() {
        // The node offers V17, which subxt cannot decode, besides V14 to V16 and the unstable one.
        let metadata = node_metadata();
        let node = Arc::new(FakeNode::new(move |call| {
            let function = call.params[1].as_str().expect("function");
            let parameters = const_hex::decode(call.params[2].as_str().expect("hex")).unwrap();
            let value = match function {
                "Metadata_metadata_versions" => vec![14u32, 15, 16, 17, u32::MAX].encode(),
                "Metadata_metadata_at_version" => {
                    match u32::decode(&mut &parameters[..]).expect("version") {
                        16 => Some(metadata.clone()).encode(),
                        _ => None::<Vec<u8>>.encode(),
                    }
                }
                _ => return Ok(Value::Null),
            };
            assert_eq!(call.method, method::ARCHIVE_CALL);
            Ok(json!({ "success": true, "value": const_hex::encode_prefixed(value) }))
        }));
        let rpc = NodeRpc::new(
            node,
            NonZeroUsize::new(64).unwrap(),
            NonZeroUsize::new(4).unwrap(),
            ReconnectPolicy {
                max_delay: Duration::from_millis(10),
                max_attempts: 3,
            },
        );

        fetch_metadata(&rpc, ByteArray([1; 32]))
            .await
            .expect("V16 metadata is fetched and decodes");
    }

    /// The SCALE-encoded `RuntimeMetadataPrefixed` of the 2.1 node.
    fn node_metadata() -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.node/2.1.0-rc.4/metadata.scale");
        fs::read(path).expect("node metadata can be read")
    }
}
