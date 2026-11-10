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

/// SPO identity information.
#[derive(Debug, Clone)]
pub struct SpoIdentity {
    pub pool_id_hex: String,
    pub mainchain_pubkey_hex: String,
    pub sidechain_pubkey_hex: String,
    pub aura_pubkey_hex: Option<String>,
    pub validator_class: String,
}

/// Pool metadata from Cardano.
#[derive(Debug, Clone)]
pub struct PoolMetadata {
    pub pool_id_hex: String,
    pub hex_id: Option<String>,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub homepage_url: Option<String>,
    pub logo_url: Option<String>,
}

/// SPO with optional metadata.
#[derive(Debug, Clone)]
pub struct Spo {
    pub pool_id_hex: String,
    pub validator_class: String,
    pub sidechain_pubkey_hex: String,
    pub aura_pubkey_hex: Option<String>,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub homepage_url: Option<String>,
    pub logo_url: Option<String>,
}

/// Composite SPO data (identity + metadata + performance).
#[derive(Debug, Clone)]
pub struct SpoComposite {
    pub identity: Option<SpoIdentity>,
    pub metadata: Option<PoolMetadata>,
    pub performance: Vec<EpochPerf>,
}

/// SPO performance for an epoch.
#[derive(Debug, Clone)]
pub struct EpochPerf {
    pub epoch_no: i64,
    pub spo_sk_hex: String,
    pub produced: i64,
    pub expected: i64,
    pub identity_label: Option<String>,
    pub stake_snapshot: Option<String>,
    pub pool_id_hex: Option<String>,
    pub validator_class: Option<String>,
}

/// Current epoch information.
#[derive(Debug, Clone)]
pub struct EpochInfo {
    pub epoch_no: i64,
    pub duration_seconds: i64,
    pub elapsed_seconds: i64,
}

/// Committee member for an epoch.
#[derive(Debug, Clone)]
pub struct CommitteeMember {
    pub epoch_no: i64,
    pub position: i32,
    pub sidechain_pubkey_hex: String,
    pub expected_slots: i32,
    pub aura_pubkey_hex: Option<String>,
    pub pool_id_hex: Option<String>,
    pub spo_sk_hex: Option<String>,
}

/// Registration statistics for an epoch.
#[derive(Debug, Clone)]
pub struct RegisteredStat {
    pub epoch_no: i64,
    pub federated_valid_count: i64,
    pub federated_invalid_count: i64,
    pub registered_valid_count: i64,
    pub registered_invalid_count: i64,
    pub dparam: Option<f64>,
}

/// Cumulative registration totals for an epoch.
#[derive(Debug, Clone)]
pub struct RegisteredTotals {
    pub epoch_no: i64,
    pub total_registered: i64,
    pub newly_registered: i64,
}

/// Presence event for an SPO in an epoch.
#[derive(Debug, Clone)]
pub struct PresenceEvent {
    pub epoch_no: i64,
    pub id_key: String,
    pub source: String,
    pub status: Option<String>,
}

/// First valid epoch for an SPO identity.
#[derive(Debug, Clone)]
pub struct FirstValidEpoch {
    pub id_key: String,
    pub first_valid_epoch: i64,
}

/// Stake share information for an SPO.
#[derive(Debug, Clone)]
pub struct StakeShare {
    pub pool_id_hex: String,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub homepage_url: Option<String>,
    pub logo_url: Option<String>,
    pub live_stake: Option<String>,
    pub active_stake: Option<String>,
    pub live_delegators: Option<i64>,
    pub live_saturation: Option<f64>,
    pub declared_pledge: Option<String>,
    pub live_pledge: Option<String>,
    pub stake_share: Option<f64>,
}

/// Strip an optional `0x` or `0X` prefix from a hex string.
pub fn strip_hex_prefix(input: &str) -> &str {
    match input.as_bytes() {
        // The pattern matched two ASCII bytes, so index 2 is a char boundary and cannot panic.
        [b'0', b'x' | b'X', ..] => &input[2..],
        _ => input,
    }
}

/// Normalize a hex string by stripping an optional `0x` or `0X` prefix and lowercasing, or `None`
/// if it is not a non-empty, even-length hex string.
pub fn normalize_hex(input: &str) -> Option<String> {
    let s = strip_hex_prefix(input);
    // `check` strips the prefix itself, so it must see the original input to strip it only once.
    (!s.is_empty() && const_hex::check(input).is_ok()).then(|| s.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::{normalize_hex, strip_hex_prefix};

    #[test]
    fn strips_hex_prefix() {
        for (input, expected) in [
            ("abc", "abc"),
            ("0xabc", "abc"),
            ("0XABC", "ABC"),
            ("0x", ""),
            ("0X", ""),
            ("", ""),
            // Only a single leading prefix is stripped.
            ("0x0xab", "0xab"),
            ("ab0x", "ab0x"),
            ("x0ab", "x0ab"),
            ("0\u{0445}ab", "0\u{0445}ab"), // Cyrillic "х"
            // Non-hex input is passed through.
            ("0xpool", "pool"),
        ] {
            assert_eq!(strip_hex_prefix(input), expected, "input: {input:?}");
        }
    }

    #[test]
    fn accepts_hex() {
        for (input, expected) in [
            ("abcd", "abcd"),
            ("ABCD", "abcd"),
            ("AbCd", "abcd"),
            ("0xabcd", "abcd"),
            ("0xABCD", "abcd"),
            ("0Xabcd", "abcd"),
            ("0XAbCd", "abcd"),
            ("0123456789abcdefABCDEF", "0123456789abcdefabcdef"),
            // Realistic pool ID (28 bytes) and sidechain key (32 bytes).
            (
                "0x8F63E953A61DA4860DE78F58C44BA7AF7121726B5C21CC2D157E9F89",
                "8f63e953a61da4860de78f58c44ba7af7121726b5c21cc2d157e9f89",
            ),
            (
                "0X03B827F4DA9711BAB7292E5695576A841A4D20AF9A07B1BA7A230168D2A78E9D",
                "03b827f4da9711bab7292e5695576a841a4d20af9a07b1ba7a230168d2a78e9d",
            ),
        ] {
            assert_eq!(
                normalize_hex(input).as_deref(),
                Some(expected),
                "input: {input:?}"
            );
        }
    }

    #[test]
    fn rejects_non_hex() {
        for input in [
            "",
            "0x",
            "0X",
            "abc",
            "0xabc",
            "zz",
            "0xzz",
            "0x0xab",
            "0X0Xab",
            "ab0x",
            "x0ab",
            " abcd",
            "abcd ",
            "pool name",
            // Non-ASCII digits, letters and lookalikes are not hex.
            "\u{ff10}\u{ff11}", // fullwidth "01"
            "\u{0660}\u{0661}", // Arabic-Indic "01"
            "0\u{0445}ab",      // Cyrillic "х" in place of the prefix "x"
            "\u{0430}b",        // Cyrillic "а"
            "\u{00e9}",         // two-byte "é", even byte length
            "ab\u{00e9}",
            "\u{fffd}",
            "ab\0",
            "\0\0",
        ] {
            assert_eq!(normalize_hex(input), None, "input: {input:?}");
        }
    }
}
