// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use indexer_common::domain::NetworkId;
use std::{
    error::Error as StdError,
    sync::{Arc, atomic::AtomicBool},
};

#[trait_variant::make(Send)]
pub trait Api
where
    Self: 'static,
{
    type Error: StdError + Send + Sync + 'static;

    async fn serve(
        self,
        network_id: NetworkId,
        caught_up: Arc<AtomicBool>,
    ) -> Result<(), Self::Error>;
}

// --- SPO domain types (initial draft) ---

#[derive(Debug, Clone)]
pub struct StakePoolOperator {
    pub id: String,                   // canonical operator id (e.g. hash or bech32)
    pub identity_key: Option<String>, // optional identity / metadata key
    pub display_name: Option<String>,
    pub created_at_epoch: Option<i64>,
    pub last_active_epoch: Option<i64>,
    pub performance_score: Option<f64>,
    pub commission_rate: Option<f64>,
    pub total_stake: Option<String>, // string to avoid premature big-int choice
}

#[derive(Debug, Clone)]
pub struct EpochPerformance {
    pub epoch: i64,
    pub operator_id: String,
    pub blocks_produced: Option<i64>,
    pub blocks_expected: Option<i64>,
    pub performance_ratio: Option<f64>,
    pub stake_share: Option<f64>,
}
