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

use indexer_common::domain::{
    ByteArray,
    dust::{DustGenerationInfo, DustParameters},
};

/// DUST lifecycle manager that handles value calculations, generation, and decay.
pub struct DustLifecycleManager {
    /// Current block timestamp for calculations
    current_time: u64,
}

impl DustLifecycleManager {
    pub fn new(current_time: u64) -> Self {
        Self { current_time }
    }

    /// Calculate the current value of a DUST UTXO based on generation and decay.
    ///
    /// Assumes ledger provides accurate timestamps and generation info.
    pub fn calculate_dust_value(
        &self,
        initial_value: u128,
        generation_info: &DustGenerationInfo,
        utxo_ctime: u64,
        params: &DustParameters,
    ) -> u128 {
        // If backing Night is still generating (dtime = 0), calculate generation
        if generation_info.dtime == 0 {
            self.calculate_generation_value(initial_value, generation_info, utxo_ctime, params)
        } else {
            // Backing Night was spent, calculate with decay
            self.calculate_value_with_decay(initial_value, generation_info, utxo_ctime, params)
        }
    }

    /// Calculate DUST value during generation phase.
    fn calculate_generation_value(
        &self,
        initial_value: u128,
        generation_info: &DustGenerationInfo,
        utxo_ctime: u64,
        params: &DustParameters,
    ) -> u128 {
        // Generation rate: θ = N × ρ / Δ
        // where N = Night value, ρ = ratio, Δ = time to cap

        let night_value = generation_info.value;
        let max_dust = night_value.saturating_mul(params.night_dust_ratio as u128);

        // Time since DUST UTXO creation
        let elapsed = self.current_time.saturating_sub(utxo_ctime);

        // Linear generation up to cap
        let generation_rate = if params.generation_decay_rate > 0 {
            night_value.saturating_mul(params.night_dust_ratio as u128)
                / params.generation_decay_rate as u128
        } else {
            0
        };

        let generated = generation_rate.saturating_mul(elapsed as u128);
        let total_value = initial_value.saturating_add(generated);

        // Cap at maximum
        total_value.min(max_dust)
    }

    /// Calculate DUST value with decay after backing Night is spent.
    fn calculate_value_with_decay(
        &self,
        initial_value: u128,
        generation_info: &DustGenerationInfo,
        utxo_ctime: u64,
        params: &DustParameters,
    ) -> u128 {
        // First calculate value at time of Night spend (dtime)
        let value_at_dtime = if generation_info.dtime > utxo_ctime {
            // Was still generating when Night was spent
            let generation_time = generation_info.dtime.saturating_sub(utxo_ctime);
            let night_value = generation_info.value;
            let generation_rate = if params.generation_decay_rate > 0 {
                night_value.saturating_mul(params.night_dust_ratio as u128)
                    / params.generation_decay_rate as u128
            } else {
                0
            };
            let generated = generation_rate.saturating_mul(generation_time as u128);
            initial_value.saturating_add(generated)
        } else {
            // Night was spent before this DUST UTXO was created
            initial_value
        };

        // Apply grace period
        let decay_start = generation_info
            .dtime
            .saturating_add(params.dust_grace_period);

        if self.current_time <= decay_start {
            // Still in grace period
            value_at_dtime
        } else {
            // Apply linear decay
            let decay_elapsed = self.current_time.saturating_sub(decay_start);
            let decay_rate = if params.generation_decay_rate > 0 {
                value_at_dtime / params.generation_decay_rate as u128
            } else {
                0
            };
            let decayed = decay_rate.saturating_mul(decay_elapsed as u128);
            value_at_dtime.saturating_sub(decayed)
        }
    }

    /// Process a partial Night spend that creates a new generation entry.
    ///
    /// Assumes ledger provides both old and new generation info in events.
    pub fn process_partial_spend(
        &self,
        old_generation: &DustGenerationInfo,
        spend_amount: u128,
    ) -> DustGenerationInfo {
        // Create new generation info for the change
        DustGenerationInfo {
            value: old_generation.value.saturating_sub(spend_amount),
            owner: old_generation.owner,
            nonce: old_generation.nonce, // TODO(sean): Should be new nonce from ledger.
            ctime: self.current_time,
            dtime: 0, // New generation starts
        }
    }

    /// Check if a DUST UTXO has any remaining value.
    pub fn has_value(
        &self,
        dust_value: u128,
        generation_info: &DustGenerationInfo,
        params: &DustParameters,
    ) -> bool {
        if generation_info.dtime == 0 {
            // Still generating
            true
        } else {
            // Check if fully decayed
            let decay_end = generation_info
                .dtime
                .saturating_add(params.dust_grace_period)
                .saturating_add(params.generation_decay_rate as u64);

            self.current_time < decay_end && dust_value > 0
        }
    }
}

/// Calculate fees that can be paid from "would-be" DUST during registration.
///
/// Assumes node provides Night UTXOs that would generate DUST if registered.
pub fn calculate_registration_fees(
    night_utxos: &[(ByteArray<32>, u128, u64)], // (nonce, value, ctime)
    params: &DustParameters,
    current_time: u64,
) -> u128 {
    night_utxos
        .iter()
        .map(|(_, value, ctime)| {
            let elapsed = current_time.saturating_sub(*ctime);
            let generation_rate = if params.generation_decay_rate > 0 {
                value.saturating_mul(params.night_dust_ratio as u128)
                    / params.generation_decay_rate as u128
            } else {
                0
            };
            generation_rate.saturating_mul(elapsed as u128)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_params() -> DustParameters {
        DustParameters {
            night_dust_ratio: 10,        // 10:1 DUST to Night
            generation_decay_rate: 3600, // 1 hour to full generation/decay
            dust_grace_period: 300,      // 5 minute grace period
        }
    }

    #[test]
    fn test_dust_generation() {
        let params = test_params();
        let manager = DustLifecycleManager::new(1000);

        let generation_info = DustGenerationInfo {
            value: 1000,
            owner: ByteArray([0u8; 32]),
            nonce: ByteArray([1u8; 32]),
            ctime: 0,
            dtime: 0, // Still generating
        };

        // After 1000 seconds, should have generated some DUST
        let value = manager.calculate_dust_value(0, &generation_info, 0, &params);

        // Expected: 1000 * 10 / 3600 * 1000 = ~2777
        assert!(value > 2700 && value < 2800);
    }

    #[test]
    fn test_dust_decay() {
        let params = test_params();
        let manager = DustLifecycleManager::new(5000);

        let generation_info = DustGenerationInfo {
            value: 1000,
            owner: ByteArray([0u8; 32]),
            nonce: ByteArray([1u8; 32]),
            ctime: 0,
            dtime: 3600, // Night spent after 1 hour
        };

        // Initial value was max (10000)
        let value = manager.calculate_dust_value(10000, &generation_info, 0, &params);

        // After grace period + some decay
        assert!(value < 10000);
    }
}
