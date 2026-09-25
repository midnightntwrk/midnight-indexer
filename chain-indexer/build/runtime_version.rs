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

// Included by `build.rs` and by `tests/runtime_version.rs`. A build script cannot
// depend on the crate it builds, and `cargo test` never runs a `#[test]` inside
// `build.rs`, so sharing the source is what makes this testable.

/// Splits `spec_version` into its major, minor and patch components.
///
/// Midnight encodes `spec_version` as `major * 1_000_000 + minor * 1_000 + patch`.
/// The split is injective over every `u32`, so two runtimes never reduce to one
/// triple. A `spec_version` breaking the convention still yields a distinct
/// triple, holding numbers that no longer read as a version.
fn runtime_version(spec_version: u32) -> (u32, u32, u32) {
    (
        spec_version / 1_000_000,
        spec_version / 1_000 % 1_000,
        spec_version % 1_000,
    )
}

#[cfg(test)]
mod tests {
    use super::runtime_version;
    use std::collections::HashMap;

    /// Every `spec_version` released on midnight-node `main`, newest first.
    const RELEASED_SPEC_VERSIONS: [(u32, (u32, u32, u32)); 16] = [
        (3_000_000, (3, 0, 0)),
        (2_001_000, (2, 1, 0)),
        (2_000_000, (2, 0, 0)),
        (1_000_000, (1, 0, 0)),
        (22_000, (0, 22, 0)),
        (21_000, (0, 21, 0)),
        (20_000, (0, 20, 0)),
        (19_000, (0, 19, 0)),
        (18_001, (0, 18, 1)),
        (18_000, (0, 18, 0)),
        (17_001, (0, 17, 1)),
        (17_000, (0, 17, 0)),
        (16_004, (0, 16, 4)),
        (100_006_004, (100, 6, 4)),
        (16_003, (0, 16, 3)),
        (16_002, (0, 16, 2)),
    ];

    #[test]
    fn released_spec_versions_split_into_their_components() {
        for (spec_version, expected) in RELEASED_SPEC_VERSIONS {
            assert_eq!(
                runtime_version(spec_version),
                expected,
                "spec_version {spec_version}"
            );
        }
    }

    /// Two runtimes sharing a version would have the second module silently
    /// generated over the first.
    #[test]
    fn distinct_spec_versions_split_into_distinct_components() {
        let boundaries = [
            0,
            1,
            999,
            1_000,
            1_001,
            999_999,
            1_000_000,
            1_000_001,
            1_001_000,
            u32::MAX - 1,
            u32::MAX,
        ];

        let mut versions = HashMap::new();
        for spec_version in boundaries {
            if let Some(other) = versions.insert(runtime_version(spec_version), spec_version) {
                panic!("{spec_version} and {other} share a runtime version");
            }
        }
    }
}
