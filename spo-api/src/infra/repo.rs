// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use indexer_common::infra::pool::postgres::PostgresPool;
use anyhow::Context;

#[derive(Debug, Clone)]
pub struct SpoRepository {
    pool: PostgresPool,
}

impl SpoRepository {
    pub fn new(pool: PostgresPool) -> Self { Self { pool } }

    /// List stake pool operator identifiers (placeholder implementation).
    pub async fn list_stake_pool_operator_ids(&self, limit: i64) -> anyhow::Result<Vec<String>> {
        // TODO: Replace with real schema/table once defined (e.g., spo_operators)
        // For now we query a non-existent placeholder; when integrated this will be updated.
        let rows = sqlx::query_scalar::<_, String>("SELECT id FROM spo_operators ORDER BY id LIMIT $1")
            .bind(limit)
            .fetch_all(&*self.pool)
            .await
            .with_context(|| "query stake pool operator ids")?;
        Ok(rows)
    }
}

// Future: introduce a trait abstraction if multiple backends are needed.
