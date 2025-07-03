// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// TEMPORARY: This entire file will be deleted once we have a node image with ledger-5.0.0-alpha.3+.
// Mock code is only for development/testing until proper ledger integration is available.
// Heiko: Please note this is temporary mock code that violates our no-mocks policy.

use indexer_common::domain::{
    ByteArray, ByteVec, TransactionHash,
    dust::{
        DustEvent, DustEventDetails, DustGenerationInfo, DustParameters, DustRegistration,
        QualifiedDustOutput,
    },
};

/// Mock DUST event generator for testing while ledger integration is pending.
/// TEMPORARY: Will be removed once node provides real DUST events.
pub struct DustMockEventGenerator {
    current_time: u64,
    next_generation_index: u64,
    next_commitment_index: u64,
}

impl DustMockEventGenerator {
    pub fn new(current_time: u64) -> Self {
        Self {
            current_time,
            next_generation_index: 0,
            next_commitment_index: 0,
        }
    }

    /// Generate a mock DustInitialUtxo event.
    pub fn generate_initial_utxo(
        &mut self,
        transaction_hash: TransactionHash,
        night_value: u128,
        owner: ByteArray<32>,
    ) -> DustEvent {
        let generation_index = self.next_generation_index;
        self.next_generation_index += 1;

        let output = QualifiedDustOutput {
            initial_value: 0, // Starts at zero.
            owner,
            nonce: ByteArray([1u8; 32]),
            seq: 0,
            ctime: self.current_time,
            backing_night: ByteArray([2u8; 32]),
            mt_index: self.next_commitment_index,
        };
        self.next_commitment_index += 1;

        let generation = DustGenerationInfo {
            value: night_value,
            owner,
            nonce: ByteArray([3u8; 32]),
            ctime: self.current_time,
            dtime: 0, // Not spent yet.
        };

        DustEvent {
            transaction_hash,
            logical_segment: 0,
            physical_segment: 0,
            event_details: DustEventDetails::DustInitialUtxo {
                output,
                generation,
                generation_index,
            },
        }
    }

    /// Generate a mock DustGenerationDtimeUpdate event.
    pub fn generate_dtime_update(
        &mut self,
        transaction_hash: TransactionHash,
        generation_index: u64,
        owner: ByteArray<32>,
        night_value: u128,
    ) -> DustEvent {
        let generation = DustGenerationInfo {
            value: night_value,
            owner,
            nonce: ByteArray([3u8; 32]),
            ctime: self.current_time.saturating_sub(3600),
            dtime: self.current_time, // Night spent now.
        };

        DustEvent {
            transaction_hash,
            logical_segment: 0,
            physical_segment: 0,
            event_details: DustEventDetails::DustGenerationDtimeUpdate {
                generation,
                generation_index,
            },
        }
    }

    /// Generate a mock DustSpendProcessed event.
    pub fn generate_dust_spend(
        &mut self,
        transaction_hash: TransactionHash,
        commitment: ByteArray<32>,
        v_fee: u128,
    ) -> DustEvent {
        let commitment_index = self.next_commitment_index;
        self.next_commitment_index += 1;

        DustEvent {
            transaction_hash,
            logical_segment: 0,
            physical_segment: 0,
            event_details: DustEventDetails::DustSpendProcessed {
                commitment,
                commitment_index,
                nullifier: ByteArray([4u8; 32]),
                v_fee,
                time: self.current_time,
                params: DustParameters {
                    night_dust_ratio: 10,
                    generation_decay_rate: 3600,
                    dust_grace_period: 300,
                },
            },
        }
    }

    /// Generate a mock registration.
    pub fn generate_registration(
        &self,
        cardano_address: Vec<u8>,
        dust_address: ByteArray<32>,
        is_valid: bool,
    ) -> DustRegistration {
        DustRegistration {
            cardano_address: ByteVec::from(cardano_address),
            dust_address,
            is_valid,
            registered_at: self.current_time,
            removed_at: if is_valid {
                None
            } else {
                Some(self.current_time)
            },
        }
    }

    /// Generate a complete scenario for testing.
    pub fn generate_test_scenario(&mut self, transaction_hash: TransactionHash) -> Vec<DustEvent> {
        let owner1 = ByteArray([10u8; 32]);
        let owner2 = ByteArray([20u8; 32]);

        vec![
            // User 1 creates DUST from 1000 Night.
            self.generate_initial_utxo(transaction_hash, 1000, owner1),
            // User 2 creates DUST from 2000 Night.
            self.generate_initial_utxo(transaction_hash, 2000, owner2),
            // User 1 spends 50 DUST for fees.
            self.generate_dust_spend(transaction_hash, owner1, 50),
            // User 2's Night is spent (decay starts).
            self.generate_dtime_update(transaction_hash, 1, owner2, 2000),
            // User 2 spends 100 DUST during decay.
            self.generate_dust_spend(transaction_hash, owner2, 100),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_event_generation() {
        let mut generator = DustMockEventGenerator::new(1000);
        let tx_hash = ByteArray([5u8; 32]);

        let events = generator.generate_test_scenario(tx_hash);
        assert_eq!(events.len(), 5);

        // Check first event is initial UTXO.
        matches!(
            &events[0].event_details,
            DustEventDetails::DustInitialUtxo { .. }
        );
    }
}
