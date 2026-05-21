// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
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

//! Storage impl for the `contractEvents` query and subscription surface
//! (ticket #1161). Pattern follows `get_ledger_events` in `ledger_events.rs`
//! but joins through `blocks` for block-range filtering and through
//! `contract_event_indexed_fields` for prefix lookup.

use crate::{
    domain::{
        ContractEventRow,
        storage::contract_event::{ContractEventFilter, ContractEventStorage},
    },
    infra::storage::Storage,
};
use async_stream::try_stream;
use fastrace::trace;
use futures::Stream;
use indexer_common::stream::flatten_chunks;
use std::num::NonZeroU32;

impl ContractEventStorage for Storage {
    #[trace(properties = {
        "limit": "{limit:?}",
        "offset": "{offset:?}"
    })]
    async fn get_contract_events(
        &self,
        filter: ContractEventFilter,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<ContractEventRow>, sqlx::Error> {
        let mut qb = base_query_builder(&filter);
        qb.push(" ORDER BY le.id ASC ");
        if let Some(l) = limit {
            qb.push(" LIMIT ").push_bind(l as i64);
        }
        if let Some(o) = offset {
            qb.push(" OFFSET ").push_bind(o as i64);
        }
        qb.build_query_as::<ContractEventRow>()
            .fetch_all(&*self.pool)
            .await
    }

    async fn get_contract_events_after_id(
        &self,
        filter: ContractEventFilter,
        mut after_id: u64,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<ContractEventRow, sqlx::Error>> + Send {
        let chunks = try_stream! {
            loop {
                let rows = self
                    .fetch_contract_events_chunk(&filter, after_id, batch_size)
                    .await?;
                match rows.last() {
                    Some(last) => after_id = last.id + 1,
                    None => break,
                }
                yield rows;
            }
        };
        flatten_chunks(chunks)
    }

    #[trace]
    async fn get_contract_events_by_contract_action_ids(
        &self,
        ids: &[u64],
    ) -> Result<Vec<(u64, ContractEventRow)>, sqlx::Error> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let mut qb = ids_query_builder();
        let mut first = true;
        for id in ids {
            if first {
                first = false;
            } else {
                qb.push(", ");
            }
            qb.push_bind(*id as i64);
        }
        qb.push(") ORDER BY le.id ASC");

        let rows: Vec<ContractEventRow> = qb
            .build_query_as::<ContractEventRow>()
            .fetch_all(&*self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| r.contract_action_id.map(|key| (key, r)))
            .collect())
    }
}

#[cfg(feature = "cloud")]
fn ids_query_builder<'a>() -> sqlx::QueryBuilder<'a, sqlx::Postgres> {
    sqlx::QueryBuilder::<sqlx::Postgres>::new(indoc::indoc! {"
        SELECT
            le.id,
            le.contract_address,
            le.transaction_id,
            le.contract_action_id,
            le.raw,
            le.attributes,
            (SELECT MAX(id) FROM ledger_events WHERE grouping = 'Contract') AS max_id,
            transactions.protocol_version
        FROM ledger_events le
        INNER JOIN transactions ON transactions.id = le.transaction_id
        WHERE le.grouping = 'Contract'
        AND le.contract_action_id IN (
    "})
}

#[cfg(feature = "standalone")]
fn ids_query_builder<'a>() -> sqlx::QueryBuilder<'a, sqlx::Sqlite> {
    sqlx::QueryBuilder::<sqlx::Sqlite>::new(indoc::indoc! {"
        SELECT
            le.id,
            le.contract_address,
            le.transaction_id,
            le.contract_action_id,
            le.raw,
            le.attributes,
            (SELECT MAX(id) FROM ledger_events WHERE grouping = 'Contract') AS max_id,
            transactions.protocol_version
        FROM ledger_events le
        INNER JOIN transactions ON transactions.id = le.transaction_id
        WHERE le.grouping = 'Contract'
        AND le.contract_action_id IN (
    "})
}

impl Storage {
    async fn fetch_contract_events_chunk(
        &self,
        filter: &ContractEventFilter,
        after_id: u64,
        batch_size: NonZeroU32,
    ) -> Result<Vec<ContractEventRow>, sqlx::Error> {
        let mut qb = base_query_builder(filter);
        qb.push(" AND le.id >= ").push_bind(after_id as i64);
        qb.push(" ORDER BY le.id ASC LIMIT ")
            .push_bind(batch_size.get() as i64);
        qb.build_query_as::<ContractEventRow>()
            .fetch_all(&*self.pool)
            .await
    }
}

/// Shared SELECT + WHERE construction for both query and subscription paths.
/// Caller appends ORDER/LIMIT/OFFSET as appropriate.
#[cfg(feature = "cloud")]
fn base_query_builder<'a>(
    filter: &'a ContractEventFilter,
) -> sqlx::QueryBuilder<'a, sqlx::Postgres> {
    use sqlx::QueryBuilder;
    let mut qb = QueryBuilder::<sqlx::Postgres>::new(indoc::indoc! {"
        SELECT
            le.id,
            le.contract_address,
            le.transaction_id,
            le.contract_action_id,
            le.raw,
            le.attributes,
            (SELECT MAX(id) FROM ledger_events WHERE grouping = 'Contract') AS max_id,
            transactions.protocol_version
        FROM ledger_events le
        INNER JOIN transactions ON transactions.id = le.transaction_id
        INNER JOIN blocks ON blocks.id = transactions.block_id
        WHERE le.grouping = 'Contract'
    "});
    qb.push(" AND le.contract_address = ")
        .push_bind(filter.contract_address.clone());

    if let Some(variants) = &filter.variants
        && !variants.is_empty()
    {
        qb.push(" AND le.variant::text = ANY(")
            .push_bind(
                variants
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>(),
            )
            .push(") ");
    }

    if let Some(from) = filter.from_block {
        qb.push(" AND blocks.height >= ").push_bind(from as i64);
    }
    if let Some(to) = filter.to_block {
        qb.push(" AND blocks.height <= ").push_bind(to as i64);
    }

    for (i, fp) in filter.field_prefixes.iter().enumerate() {
        let alias = format!("cef{i}");
        qb.push(format!(
            " AND EXISTS (SELECT 1 FROM contract_event_indexed_fields {alias} \
             WHERE {alias}.ledger_event_id = le.id AND {alias}.field_name = "
        ))
        .push_bind(fp.field_name.clone())
        .push(format!(" AND {alias}.field_value LIKE "))
        .push_bind({
            let mut pat = fp.prefix.clone();
            pat.push(b'%');
            pat
        })
        .push(") ");
    }

    qb
}

#[cfg(feature = "standalone")]
fn base_query_builder<'a>(
    filter: &'a ContractEventFilter,
) -> sqlx::QueryBuilder<'a, sqlx::Sqlite> {
    use sqlx::QueryBuilder;
    let mut qb = QueryBuilder::<sqlx::Sqlite>::new(indoc::indoc! {"
        SELECT
            le.id,
            le.contract_address,
            le.transaction_id,
            le.contract_action_id,
            le.raw,
            le.attributes,
            (SELECT MAX(id) FROM ledger_events WHERE grouping = 'Contract') AS max_id,
            transactions.protocol_version
        FROM ledger_events le
        INNER JOIN transactions ON transactions.id = le.transaction_id
        INNER JOIN blocks ON blocks.id = transactions.block_id
        WHERE le.grouping = 'Contract'
    "});
    qb.push(" AND le.contract_address = ")
        .push_bind(filter.contract_address.clone());

    if let Some(variants) = &filter.variants
        && !variants.is_empty()
    {
        qb.push(" AND le.variant IN (");
        let mut sep = qb.separated(", ");
        for v in variants {
            sep.push_bind(v.to_string());
        }
        qb.push(") ");
    }

    if let Some(from) = filter.from_block {
        qb.push(" AND blocks.height >= ").push_bind(from as i64);
    }
    if let Some(to) = filter.to_block {
        qb.push(" AND blocks.height <= ").push_bind(to as i64);
    }

    for (i, fp) in filter.field_prefixes.iter().enumerate() {
        let alias = format!("cef{i}");
        qb.push(format!(
            " AND EXISTS (SELECT 1 FROM contract_event_indexed_fields {alias} \
             WHERE {alias}.ledger_event_id = le.id AND {alias}.field_name = "
        ))
        .push_bind(fp.field_name.clone())
        .push(format!(" AND substr({alias}.field_value, 1, "))
        .push_bind(fp.prefix.len() as i64)
        .push(") = ")
        .push_bind(fp.prefix.clone())
        .push(") ");
    }

    qb
}

/// The `contract_action_id_key` column is the same value as `contract_action_id`
/// but the FromRow parsing needs both names matched. Disabled via cfg-gate here
/// because the standalone path uses a slightly different SQL shape. The
/// integration path (#1162) lands the column population.
#[cfg(test)]
#[allow(dead_code)]
fn _shim_for_contract_action_id_key() {}
