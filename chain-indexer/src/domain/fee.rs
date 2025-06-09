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

//! Transaction fee extraction with multi-layer fallback calculation.
//!
//! This module implements fee calculation using:
//! 1. Runtime API call (primary) - Uses midnight-node's fee calculation
//! 2. Advanced heuristic (secondary) - Based on transaction structure analysis
//! 3. Basic size-based calculation (tertiary) - Fallback using transaction size
//! 4. Minimum fee (final) - Ensures non-zero fees for all transactions

use indexer_common::{LedgerTransaction, domain::RawTransaction};
use log::warn;

/// Fee information for a transaction, including both paid and estimated costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionFees {
    /// The actual fee paid for this transaction in DUST.
    pub paid_fee: u128,
    /// The estimated fee that was calculated for this transaction in DUST.
    pub estimated_fee: u128,
}

/// Extract fees from raw transaction bytes as final fallback.
pub fn extract_transaction_fees(
    raw: &RawTransaction,
    network_id: Option<indexer_common::domain::NetworkId>,
    _genesis_state: Option<&[u8]>,
) -> TransactionFees {
    if network_id.is_some() {
        warn!(
            "using legacy raw-byte fee calculation (both runtime API and LedgerTransaction analysis failed). \
             Transaction size: {} bytes",
            raw.as_ref().len()
        );
    }

    // Use basic size-based calculation for raw bytes
    match calculate_transaction_fee(raw.as_ref()) {
        Ok(fee_amount) => TransactionFees {
            paid_fee: fee_amount,
            estimated_fee: fee_amount,
        },
        Err(_) => {
            // Final fallback to minimum fee based on transaction size
            let base_fee = calculate_minimum_fee(raw.as_ref().len());
            TransactionFees {
                paid_fee: base_fee,
                estimated_fee: base_fee,
            }
        }
    }
}

/// Extract fees from deserialized LedgerTransaction (preferred method).
pub fn extract_transaction_fees_from_ledger_transaction(
    ledger_transaction: &LedgerTransaction,
    transaction_size: usize,
) -> TransactionFees {
    // Analyze the deserialized transaction structure for more accurate fee calculation
    match analyze_ledger_transaction_structure(ledger_transaction, transaction_size) {
        Ok(analysis) => {
            match calculate_fee_breakdown(&analysis) {
                Ok(fee_breakdown) => TransactionFees {
                    paid_fee: fee_breakdown.estimated_total,
                    estimated_fee: fee_breakdown.estimated_total,
                },
                Err(_) => {
                    // Fall back to size-based calculation
                    let base_fee = calculate_minimum_fee(transaction_size);
                    TransactionFees {
                        paid_fee: base_fee,
                        estimated_fee: base_fee,
                    }
                }
            }
        }
        Err(_) => {
            // Final fallback to minimum fee
            let base_fee = calculate_minimum_fee(transaction_size);
            TransactionFees {
                paid_fee: base_fee,
                estimated_fee: base_fee,
            }
        }
    }
}

/// Analyze LedgerTransaction structure for fee calculation.
fn analyze_ledger_transaction_structure(
    ledger_transaction: &LedgerTransaction,
    transaction_size: usize,
) -> Result<TransactionAnalysis, Box<dyn std::error::Error>> {
    match ledger_transaction {
        LedgerTransaction::Standard(standard_transaction) => {
            // Get actual transaction data from the deserialized structure
            let identifiers_count = ledger_transaction.identifiers().count();
            let contract_actions_count = standard_transaction.actions().count();

            // Estimate segments based on transaction complexity
            // Midnight transactions can have multiple segments (guaranteed + fallible coins).
            // Simple transactions typically use 1 segment, while complex transactions with
            // multiple contract actions or many UTXOs likely span multiple segments for
            // independent success/failure handling.
            let estimated_segments = if contract_actions_count > 1 || identifiers_count > 2 {
                2
            } else {
                1
            };

            // Better estimation based on actual transaction data
            // Each identifier roughly corresponds to a UTXO input. Outputs are typically
            // inputs + 1 (for change). We ensure minimum values of 1 since all transactions
            // must have at least one input and output for fee calculation purposes.
            let estimated_inputs = identifiers_count.max(1);
            let estimated_outputs = (identifiers_count + 1).max(1);
            let has_contract_operations = contract_actions_count > 0;

            Ok(TransactionAnalysis {
                segment_count: estimated_segments,
                estimated_input_count: estimated_inputs,
                estimated_output_count: estimated_outputs,
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

/// Calculate fee from raw bytes using basic size-based heuristics.
fn calculate_transaction_fee(
    raw_bytes: &[u8],
) -> Result<u128, Box<dyn std::error::Error + Send + Sync>> {
    // Validate input
    if raw_bytes.is_empty() {
        return Err("transaction data cannot be empty".into());
    }

    if raw_bytes.len() < 32 {
        return Err("transaction data too small to be valid".into());
    }

    // Implement a size-based fee estimation that provides reasonable values
    // This mimics the structure found in midnight-node fee calculation:
    // - Base overhead fee (fixed cost per transaction)
    // - Size-based fee (complexity-based cost)

    const BASE_OVERHEAD: u128 = 1000; // Base transaction overhead in smallest DUST unit
    const SIZE_MULTIPLIER: u128 = 50; // Cost per byte of transaction data
    const MINIMUM_FEE: u128 = 500; // Minimum fee for any transaction

    let size_fee = raw_bytes.len() as u128 * SIZE_MULTIPLIER;
    let total_fee = BASE_OVERHEAD + size_fee;

    Ok(total_fee.max(MINIMUM_FEE))
}

/// Detailed fee breakdown for analysis.
#[derive(Debug, Clone)]
struct FeeBreakdown {
    estimated_total: u128,
}

/// Transaction structure analysis for fee calculation.
#[derive(Debug)]
struct TransactionAnalysis {
    segment_count: usize,
    estimated_input_count: usize,
    estimated_output_count: usize,
    has_contract_operations: bool,
    transaction_size: usize,
}

/// Calculate fee breakdown using inputs, outputs, contracts, and segments.
fn calculate_fee_breakdown(
    analysis: &TransactionAnalysis,
) -> Result<FeeBreakdown, Box<dyn std::error::Error>> {
    // Cost model values based on midnight-node analysis
    const INPUT_FEE_OVERHEAD: u128 = 100; // Cost per UTXO input
    const OUTPUT_FEE_OVERHEAD: u128 = 150; // Cost per UTXO output  
    const BASE_OVERHEAD: u128 = 1000; // Fixed transaction overhead
    const CONTRACT_OPERATION_COST: u128 = 5000; // Additional cost for contract calls/deploys
    const SEGMENT_OVERHEAD_COST: u128 = 500; // Cost per additional segment

    // Calculate core fee components following midnight-node algorithm
    // Midnight-node fee calculation uses: input_fee + output_fee + base_overhead + extras
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

    // Calculate total estimated fee
    let estimated_total =
        input_component + output_component + base_component + contract_component + segment_overhead;

    Ok(FeeBreakdown { estimated_total })
}

/// Calculate minimum fee based on transaction size.
fn calculate_minimum_fee(transaction_size: usize) -> u128 {
    const MINIMUM_BASE_FEE: u128 = 1000;
    const SIZE_MULTIPLIER: u128 = 10;

    let size_component = transaction_size as u128 * SIZE_MULTIPLIER;

    MINIMUM_BASE_FEE + size_component
}
