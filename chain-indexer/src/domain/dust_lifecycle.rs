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

/// Clamp a value between min and max bounds.
/// From midnight-architecture/specification/dust.md.
fn clamp(value: u128, min: u128, max: u128) -> u128 {
    value.max(min).min(max)
}

/// Validate that a DUST spend transaction time is within the grace period.
/// From midnight-architecture/specification/dust.md lines 113-120.
pub fn validate_dust_spend_time(
    transaction_time: u64,
    current_time: u64,
    grace_period: u64,
) -> bool {
    // Transaction can be backdated up to grace_period
    // but cannot be future-dated
    transaction_time >= current_time.saturating_sub(grace_period) && transaction_time <= current_time
}

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
    /// Based on midnight-architecture/specification/dust.md lines 408-443.
    fn calculate_generation_value(
        &self,
        initial_value: u128,
        generation_info: &DustGenerationInfo,
        utxo_ctime: u64,
        params: &DustParameters,
    ) -> u128 {
        // From spec: The maximum capacity is gen.value * night_dust_ratio.
        let vfull = generation_info.value.saturating_mul(params.night_dust_ratio as u128);
        
        // From spec: The slope of generation and decay for a specific dust UTXO
        // is proportional to the value of its backing night.
        // rate = gen.value * params.generation_decay_rate
        let rate = generation_info.value.saturating_mul(params.generation_decay_rate as u128);
        
        // Time since DUST UTXO creation
        let elapsed = self.current_time.saturating_sub(utxo_ctime);
        
        // Calculate generated value
        let generated = rate.saturating_mul(elapsed as u128);
        let total_value = initial_value.saturating_add(generated);
        
        // Clamp to reasonable bounds as per spec
        clamp(total_value, initial_value, vfull)
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
            // From spec: rate = gen.value * params.generation_decay_rate
            let rate = night_value.saturating_mul(params.generation_decay_rate as u128);
            let generated = rate.saturating_mul(generation_time as u128);
            let value_with_generation = initial_value.saturating_add(generated);
            // Clamp to reasonable bounds as per spec
            let vfull = night_value.saturating_mul(params.night_dust_ratio as u128);
            clamp(value_with_generation, initial_value, vfull)
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
            // From spec: same rate as generation
            let decay_elapsed = self.current_time.saturating_sub(decay_start);
            let rate = generation_info.value.saturating_mul(params.generation_decay_rate as u128);
            let decayed = rate.saturating_mul(decay_elapsed as u128);
            clamp(value_at_dtime.saturating_sub(decayed), 0, value_at_dtime)
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
        } else if dust_value == 0 {
            // Already fully decayed
            false
        } else {
            // Check if within grace period or still has value
            // Grace period starts after backing Night is spent
            let grace_end = generation_info.dtime.saturating_add(params.dust_grace_period);
            self.current_time <= grace_end || dust_value > 0
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
            // From spec: rate = gen.value * params.generation_decay_rate
            let rate = value.saturating_mul(params.generation_decay_rate as u128);
            let generated = rate.saturating_mul(elapsed as u128);
            // Cap at maximum
            let vfull = value.saturating_mul(params.night_dust_ratio as u128);
            generated.min(vfull)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_params() -> DustParameters {
        // Note: Using simplified test values.
        // In production, generation_decay_rate would be ~8267 for 1 week to cap.
        DustParameters {
            night_dust_ratio: 10,        // 10:1 DUST to Night
            generation_decay_rate: 3,    // 3 atomic units per Night per second (for testing)
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

        // Expected: rate = 1000 * 3 = 3000 per second
        // After 1000 seconds: 3000 * 1000 = 3,000,000
        // But capped at: 1000 * 10 = 10,000
        assert_eq!(value, 10_000); // Should be at cap
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

    #[test]
    fn test_grace_period_validation() {
        let current_time = 1000;
        let grace_period = 300; // 5 minutes

        // Valid: within grace period (backdated)
        assert!(validate_dust_spend_time(800, current_time, grace_period));
        assert!(validate_dust_spend_time(700, current_time, grace_period));
        assert!(validate_dust_spend_time(current_time, current_time, grace_period));

        // Invalid: too far in the past
        assert!(!validate_dust_spend_time(600, current_time, grace_period));
        assert!(!validate_dust_spend_time(0, current_time, grace_period));

        // Invalid: future dated
        assert!(!validate_dust_spend_time(1001, current_time, grace_period));
        assert!(!validate_dust_spend_time(2000, current_time, grace_period));
    }
}
