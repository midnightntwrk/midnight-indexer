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

//! Shared test data for the backfill integration tests.

use indexer_common::domain::ledger::TaggedSerializableExt;
use midnight_base_crypto_v1::hash::HashOutput;
use midnight_coin_structure_v2::coin::{TokenType as MidnightTokenType, UnshieldedTokenType};
use midnight_onchain_runtime_v4::state::ContractState as ContractStateV4;
use midnight_storage_core_v1::DefaultDB;

/// Real contract state captured from preview-green (2026-06-10): holds 439,000,000 of
/// one unshielded token.
pub const REAL_STATE_HEX: &str = include_str!("../fixtures/state-45692-contract-3ba7cb40.hex");
pub const REAL_TOKEN_TYPE: &str =
    "578f00a20340d71020d9003bea6a5377c277c47fe7f23024d6c395acba5c6017";
pub const REAL_AMOUNT: u128 = 439_000_000;

pub const SYNTHETIC_TOKEN_TYPE: [u8; 32] = [7; 32];
pub const SYNTHETIC_AMOUNT: u128 = 1_000_000;

pub fn synthetic_v4_state() -> Vec<u8> {
    let mut contract_state = ContractStateV4::<DefaultDB>::default();
    contract_state.balance = contract_state.balance.insert(
        MidnightTokenType::Unshielded(UnshieldedTokenType(HashOutput(SYNTHETIC_TOKEN_TYPE))),
        SYNTHETIC_AMOUNT,
    );
    contract_state
        .tagged_serialize()
        .expect("synthetic state serializes")
        .to_vec()
}

pub fn from_hex(s: &str) -> Vec<u8> {
    let s = s.trim();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
        .collect()
}
