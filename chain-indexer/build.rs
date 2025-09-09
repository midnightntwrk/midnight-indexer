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

use std::{env, fs, path::Path};

// Configure the path to the node version file
const NODE_VERSION_FILE: &str = "../NODE_VERSION";

fn main() {
    let node_version = read_node_version();
    let metadata_path = format!("../.node/{}/metadata.scale", node_version);

    if !Path::new(&metadata_path).exists() {
        panic!(
            "Metadata file not found at: {}\nMake sure the node version '{}' exists in ../.node/",
            metadata_path, node_version
        );
    }

    // Extract version for module name (replace dots and hyphens with underscores)
    // e.g. "0.16.0-da0b6c69" becomes "0_16"
    let module_suffix = node_version
        .split('.')
        .take(2)
        .collect::<Vec<&str>>()
        .join("_");

    // Generate the subxt macro call
    let generated_code = format!(
        r#"#[subxt::subxt(
    runtime_metadata_path = "{}",
    derive_for_type(
        path = "sp_consensus_slots::Slot",
        derive = "parity_scale_codec::Encode, parity_scale_codec::Decode",
        recursive
    )
)]
pub mod runtime_{} {{}}
"#,
        metadata_path, module_suffix
    );

    // Write to output file in the OUT_DIR
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("generated_runtime.rs");

    fs::write(&dest_path, generated_code).expect("Failed to write generated runtime file");

    // Tell cargo to rerun build script if:
    // 1. The NODE_VERSION file changes
    println!("cargo:rerun-if-changed={}", NODE_VERSION_FILE);

    // 2. The metadata file itself changes
    println!("cargo:rerun-if-changed={}", metadata_path);

    // 3. The .node directory structure changes
    println!("cargo:rerun-if-changed=../.node");

    // Output information for debugging
    println!("cargo:rustc-env=USED_NODE_VERSION={}", node_version);
    println!("cargo:warning=Using node version: {}", node_version);
}

fn read_node_version() -> String {
    // Check if NODE_VERSION file exists
    if !Path::new(NODE_VERSION_FILE).exists() {
        panic!(
            "{} file not found. Please create a {} file containing the node version (e.g., '0.16.0-da0b6c69')",
            NODE_VERSION_FILE, NODE_VERSION_FILE
        );
    }

    // Read and clean the version string
    match fs::read_to_string(NODE_VERSION_FILE) {
        Ok(content) => {
            let version = content.trim().to_string();

            if version.is_empty() {
                panic!(
                    "{} file is empty. Please specify a node version (e.g., '0.16.0-da0b6c69')",
                    NODE_VERSION_FILE
                );
            }

            println!(
                "cargo:warning=Read node version from {}: {}",
                NODE_VERSION_FILE, version
            );
            version
        }
        Err(e) => {
            panic!("Failed to read {} file: {}", NODE_VERSION_FILE, e);
        }
    }
}
