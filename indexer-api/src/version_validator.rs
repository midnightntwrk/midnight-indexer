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

//! Version validation module to ensure compatibility between Indexer components and Midnight Node.
//!
//! This module addresses issue #597 by providing validation and warnings when version
//! mismatches are detected, helping users avoid configuration errors.

use log::{error, info, warn};

/// The expected Midnight Node version compatible with this Indexer version.
/// This value should match the content of the NODE_VERSION file in the repository root.
const EXPECTED_NODE_VERSION: &str = "0.18.0";

/// The current Indexer version.
const INDEXER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Validates version compatibility and logs warnings if issues are detected.
///
/// This function should be called during application startup to help users
/// identify version mismatches early.
///
/// # Arguments
///
/// * `node_version` - Optional Midnight Node version string. If None, a warning is logged.
/// * `chain_indexer_version` - Optional chain-indexer version. If None, a warning is logged.
/// * `wallet_indexer_version` - Optional wallet-indexer version. If None, a warning is logged.
///
/// # Returns
///
/// Returns `Ok(())` if all versions are compatible, or an error message if critical
/// mismatches are detected.
pub fn validate_versions(
    node_version: Option<&str>,
    chain_indexer_version: Option<&str>,
    wallet_indexer_version: Option<&str>,
) -> Result<(), String> {
    info!(
        indexer_api_version = INDEXER_VERSION,
        expected_node_version = EXPECTED_NODE_VERSION;
        "validating component versions"
    );

    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Validate Midnight Node version
    match node_version {
        Some(version) if version != EXPECTED_NODE_VERSION => {
            let msg = format!(
                "Midnight Node version mismatch: expected '{}', found '{}'. \
                This may cause API endpoint incompatibilities (see issue #597). \
                Check the NODE_VERSION file in the repository for the correct version.",
                EXPECTED_NODE_VERSION, version
            );
            warnings.push(msg);
        }
        None => {
            warnings.push(
                "Unable to determine Midnight Node version. \
                Please ensure you are using the correct Node version as specified in NODE_VERSION."
                    .to_string(),
            );
        }
        _ => {
            info!(
                node_version = node_version.unwrap();
                "Midnight Node version is compatible"
            );
        }
    }

    // Validate chain-indexer version
    match chain_indexer_version {
        Some(version) if version != INDEXER_VERSION => {
            let msg = format!(
                "chain-indexer version mismatch: expected '{}', found '{}'. \
                All Indexer components (chain-indexer, wallet-indexer, indexer-api) \
                must use the same version.",
                INDEXER_VERSION, version
            );
            errors.push(msg);
        }
        None => {
            warnings.push(
                "Unable to determine chain-indexer version. \
                Ensure all Indexer components use the same version."
                    .to_string(),
            );
        }
        _ => {
            info!(
                chain_indexer_version = chain_indexer_version.unwrap();
                "chain-indexer version is compatible"
            );
        }
    }

    // Validate wallet-indexer version
    match wallet_indexer_version {
        Some(version) if version != INDEXER_VERSION => {
            let msg = format!(
                "wallet-indexer version mismatch: expected '{}', found '{}'. \
                All Indexer components (chain-indexer, wallet-indexer, indexer-api) \
                must use the same version.",
                INDEXER_VERSION, version
            );
            errors.push(msg);
        }
        None => {
            warnings.push(
                "Unable to determine wallet-indexer version. \
                Ensure all Indexer components use the same version."
                    .to_string(),
            );
        }
        _ => {
            info!(
                wallet_indexer_version = wallet_indexer_version.unwrap();
                "wallet-indexer version is compatible"
            );
        }
    }

    // Log all warnings
    for warning in &warnings {
        warn!("{}", warning);
    }

    // Log all errors and return error if any
    if !errors.is_empty() {
        for error_msg in &errors {
            error!("{}", error_msg);
        }
        return Err(errors.join("\n"));
    }

    if warnings.is_empty() {
        info!("all component versions are compatible");
    }

    Ok(())
}

/// Logs a helpful message about version requirements.
///
/// This should be called when the API starts to inform users about proper version usage.
pub fn log_version_info() {
    info!(
        "╔════════════════════════════════════════════════════════════════════════════╗"
    );
    info!(
        "║                    Midnight Indexer API Version Info                      ║"
    );
    info!(
        "╠════════════════════════════════════════════════════════════════════════════╣"
    );
    info!(
        "║  Indexer API Version:        {}                                        ║",
        INDEXER_VERSION
    );
    info!(
        "║  Expected Node Version:      {}                                      ║",
        EXPECTED_NODE_VERSION
    );
    info!(
        "╠════════════════════════════════════════════════════════════════════════════╣"
    );
    info!(
        "║  IMPORTANT: All Indexer components must use the SAME version:             ║"
    );
    info!(
        "║    - chain-indexer:{}                                                 ║",
        INDEXER_VERSION
    );
    info!(
        "║    - wallet-indexer:{}                                                ║",
        INDEXER_VERSION
    );
    info!(
        "║    - indexer-api:{}                                                   ║",
        INDEXER_VERSION
    );
    info!(
        "║                                                                            ║"
    );
    info!(
        "║  NEVER use 'latest' tag in production - always specify exact versions!    ║"
    );
    info!(
        "║                                                                            ║"
    );
    info!(
        "║  For more information, see:                                                ║"
    );
    info!(
        "║  https://github.com/midnightntwrk/midnight-indexer/blob/main/NODE_VERSION ║"
    );
    info!(
        "╚════════════════════════════════════════════════════════════════════════════╝"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_versions_all_compatible() {
        let result = validate_versions(
            Some(EXPECTED_NODE_VERSION),
            Some(INDEXER_VERSION),
            Some(INDEXER_VERSION),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_versions_node_mismatch() {
        let result = validate_versions(Some("0.17.0"), Some(INDEXER_VERSION), Some(INDEXER_VERSION));
        // Should return Ok but log warnings
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_versions_indexer_mismatch() {
        let result = validate_versions(
            Some(EXPECTED_NODE_VERSION),
            Some("2.0.0"),
            Some(INDEXER_VERSION),
        );
        // Should return Err due to indexer version mismatch
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_versions_missing_versions() {
        let result = validate_versions(None, None, None);
        // Should return Ok but log warnings
        assert!(result.is_ok());
    }
}
