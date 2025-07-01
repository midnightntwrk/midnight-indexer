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

/// Extension methods for `Serializable` implementations.
pub trait SerializableExt
where
    Self: Serializable,
{
    /// Serialize this `Serializable` implementation.
    #[trace(properties = {
        "network_id": "{network_id}",
        "protocol_version": "{protocol_version}"
    })]
    fn serialize(
        &self,
        network_id: NetworkId,
        protocol_version: ProtocolVersion,
    ) -> Result<Vec<u8>, io::Error> {
        if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
            let mut bytes = Vec::with_capacity(Self::serialized_size(self) + 1);
            serialize(self, &mut bytes, network_id.into())?;

            Ok(bytes)
        } else {
            Err(Error::InvalidProtocolVersion(protocol_version))
        }
    }
}

impl<T> SerializableExt for T where T: Serializable {}
