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

//! Transaction fees extraction with multi-layer fallback calculation.
//!
//! This module implements fees calculation using:
//! 1. Runtime API call (primary) - Uses midnight-node's fees calculation
//! 2. Advanced heuristic (secondary) - Based on transaction structure analysis
//! 3. Basic size-based calculation (tertiary) - Fallback using transaction size
//! 4. Minimum fees (final) - Ensures non-zero fees for all transactions

use indexer_common::{LedgerTransaction, domain::RawTransaction};
use log::warn;
use thiserror::Error;

// Fee calculation constants
const BASE_OVERHEAD: u128 = 1000; // Base transaction overhead in smallest DUST unit
const SIZE_MULTIPLIER: u128 = 50; // Cost per byte of transaction data
const MINIMUM_FEE: u128 = 500; // Minimum fees for any transaction
const INPUT_FEE_OVERHEAD: u128 = 100; // Cost per UTXO input
const OUTPUT_FEE_OVERHEAD: u128 = 150; // Cost per UTXO output  
const CONTRACT_OPERATION_COST: u128 = 5000; // Additional cost for contract calls/deploys
const SEGMENT_OVERHEAD_COST: u128 = 500; // Cost per additional segment
const MINIMUM_BASE_FEE: u128 = 1000; // Minimum base fees
const SIZE_MULTIPLIER_MIN: u128 = 10; // Size multiplier for minimum fees

/// Errors that can occur during fees calculation.
#[derive(Error, Debug)]
pub enum FeesError {
    #[error("transaction data cannot be empty")]
    EmptyTransactionData,
    #[error("transaction data too small to be valid (minimum 32 bytes required)")]
    TransactionDataTooSmall,
    #[error("fees calculation failed: {0}")]
    CalculationFailed(String),
}

/// Fees information for a transaction, including both paid and estimated fees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionFees {
    /// The actual fees paid for this transaction in DUST.
    pub paid_fees: u128,
    /// The estimated fees that was calculated for this transaction in DUST.
    pub estimated_fees: u128,
}

impl TransactionFees {
    /// Extract fees from raw transaction bytes as final fallback.
    pub fn extract_from_raw(
        raw: &RawTransaction,
        network_id: Option<indexer_common::domain::NetworkId>,
        _genesis_state: Option<&[u8]>,
    ) -> TransactionFees {
        // Network ID is provided when we have context about which network we're processing,
        // indicating this is a live transaction processing scenario rather than a unit test.
        // In such cases, falling back to raw-byte calculation suggests both the runtime API
        // and LedgerTransaction analysis have failed, which warrants a warning.
        if network_id.is_some() {
            warn!(
                "using legacy raw-byte fees calculation (both runtime API and LedgerTransaction analysis failed), transaction_size = {}",
                raw.as_ref().len()
            );
        }

        // Use basic size-based calculation for raw bytes.
        match calculate_transaction_fees(raw.as_ref()) {
            Ok(fees) => TransactionFees {
                paid_fees: fees,
                estimated_fees: fees,
            },

            _ => {
                // Final fallback to minimum fees based on transaction size.
                let fees = calculate_minimum_fees(raw.as_ref().len());
                TransactionFees {
                    paid_fees: fees,
                    estimated_fees: fees,
                }
            }
        }
    }

    /// Extract fees from deserialized LedgerTransaction (preferred method).
    pub fn extract_from_ledger_transaction(
        ledger_transaction: &LedgerTransaction,
        transaction_size: usize,
    ) -> TransactionFees {
        // Analyze the deserialized transaction structure for more accurate fees calculation.
        match analyze_ledger_transaction_structure(ledger_transaction, transaction_size) {
            Ok(analysis) => {
                match calculate_fees_breakdown(&analysis) {
                    Ok(fees) => TransactionFees {
                        paid_fees: fees.estimated_total,
                        estimated_fees: fees.estimated_total,
                    },

                    _ => {
                        // Fall back to size-based calculation.
                        let fees = calculate_minimum_fees(transaction_size);
                        TransactionFees {
                            paid_fees: fees,
                            estimated_fees: fees,
                        }
                    }
                }
            }

            _ => {
                // Final fallback to minimum fees.
                let fees = calculate_minimum_fees(transaction_size);
                TransactionFees {
                    paid_fees: fees,
                    estimated_fees: fees,
                }
            }
        }
    }
}

/// Analyze LedgerTransaction structure for fees calculation.
fn analyze_ledger_transaction_structure(
    ledger_transaction: &LedgerTransaction,
    transaction_size: usize,
) -> Result<TransactionAnalysis, FeesError> {
    match ledger_transaction {
        LedgerTransaction::Standard(standard_transaction) => {
            // Get actual transaction data from the deserialized structure
            let identifiers_count = ledger_transaction.identifiers().count();
            let contract_actions_count = standard_transaction.actions().count();

            // Estimate segments based on transaction complexity.
            // Midnight transactions can have multiple segments (guaranteed + fallible coins).
            // Simple transactions typically use 1 segment, while complex transactions with
            // multiple contract actions or many UTXOs likely span multiple segments for
            // independent success/failure handling.
            let segment_count = if contract_actions_count > 1 || identifiers_count > 2 {
                2
            } else {
                1
            };

            // Better estimation based on actual transaction data.
            // Each identifier roughly corresponds to a UTXO input. Outputs are typically
            // inputs + 1 (for change). We ensure minimum values of 1 since all transactions
            // must have at least one input and output for fees calculation purposes.
            let estimated_input_count = identifiers_count.max(1);
            let estimated_output_count = (identifiers_count + 1).max(1);
            let has_contract_operations = contract_actions_count > 0;

            Ok(TransactionAnalysis {
                segment_count,
                estimated_input_count,
                estimated_output_count,
                has_contract_operations,
                transaction_size,
            })
        }

        LedgerTransaction::ClaimMint(_) => {
            // ClaimMint transactions are simpler atomic operations that convert unclaimed
            // tokens to spendable tokens. They have a fixed structure: single atomic operation
            // (1 segment), consume one claim input (1 input), produce one spendable output
            // (1 output), and never involve contract operations.
            Ok(TransactionAnalysis {
                segment_count: 1,
                estimated_input_count: 1,
                estimated_output_count: 1,
                has_contract_operations: false,
                transaction_size,
            })
        }
    }
}

/// Calculate fees from raw bytes using basic size-based heuristics.
fn calculate_transaction_fees(raw_bytes: &[u8]) -> Result<u128, FeesError> {
    // Validate input.
    if raw_bytes.is_empty() {
        return Err(FeesError::EmptyTransactionData);
    }

    if raw_bytes.len() < 32 {
        return Err(FeesError::TransactionDataTooSmall);
    }

    // Implement a size-based fees estimation that provides reasonable values.
    // This mimics the structure found in midnight-node fees calculation:
    // - Base overhead fees (fixed cost per transaction)
    // - Size-based fees (complexity-based cost)
    let size_fees = raw_bytes.len() as u128 * SIZE_MULTIPLIER;
    let total_fees = BASE_OVERHEAD + size_fees;

    Ok(total_fees.max(MINIMUM_FEE))
}

/// Detailed fees breakdown for analysis.
#[derive(Debug, Clone)]
struct FeesBreakdown {
    estimated_total: u128,
}

/// Transaction structure analysis for fees calculation.
#[derive(Debug)]
struct TransactionAnalysis {
    segment_count: usize,
    estimated_input_count: usize,
    estimated_output_count: usize,
    has_contract_operations: bool,
    transaction_size: usize,
}

/// Calculate fees breakdown using inputs, outputs, contracts, and segments.
fn calculate_fees_breakdown(analysis: &TransactionAnalysis) -> Result<FeesBreakdown, FeesError> {
    // Calculate core fees components following midnight-node algorithm.
    // Midnight-node fees calculation uses: input_fees + output_fees + base_overhead + extras
    // - Input fees: charged per UTXO consumed (storage and validation costs)
    // - Output fees: charged per UTXO created (storage and commitment costs)
    // - Base overhead: fixed per-transaction processing cost
    let input_component = analysis.estimated_input_count as u128 * INPUT_FEE_OVERHEAD;
    let output_component = analysis.estimated_output_count as u128 * OUTPUT_FEE_OVERHEAD;
    let base_component = BASE_OVERHEAD;

    let contract_component = if analysis.has_contract_operations {
        let complexity_multiplier = if analysis.transaction_size > 2000 {
            2
        } else {
            1
        };
        CONTRACT_OPERATION_COST * complexity_multiplier
    } else {
        0
    };

    let segment_overhead = if analysis.segment_count > 1 {
        (analysis.segment_count as u128 - 1) * SEGMENT_OVERHEAD_COST
    } else {
        0
    };

    // Calculate total estimated fees.
    let estimated_total =
        input_component + output_component + base_component + contract_component + segment_overhead;

    Ok(FeesBreakdown { estimated_total })
}

/// Calculate minimum fees based on transaction size.
fn calculate_minimum_fees(transaction_size: usize) -> u128 {
    let size_component = transaction_size as u128 * SIZE_MULTIPLIER_MIN;

    MINIMUM_BASE_FEE + size_component
}
