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

//! One-off, idempotent backfill for `contract_balances` (see issue #1245).
//!
//! Every release from 3.0.0 to 4.3.3 silently extracted empty contract balances, so
//! `contract_balances` is empty for history indexed by those versions. This tool
//! recomputes the missing rows from the per-action contract state already stored in
//! `contract_actions.state` and inserts them with `ON CONFLICT DO NOTHING`: re-runs are
//! no-ops, only existing action ids are touched, and rows are byte-identical to what the
//! fixed chain-indexer writes at index time.
//!
//! Configuration via environment variables, matching the deployed components:
//! `APP__INFRA__STORAGE__*` (cloud: HOST, PORT, DBNAME, USER, PASSWORD, SSLMODE;
//! standalone: CNN_URL), plus `APPLY` (default 0 = dry-run) and `BATCH` (default 500).

use anyhow::Context;
use indexer_common::domain::{ContractBalance, ProtocolVersion, ledger::ContractState};
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let apply = env::var("APPLY").map(|apply| apply == "1").unwrap_or(false);
    let batch = env::var("BATCH")
        .unwrap_or_else(|_| "500".into())
        .parse::<i64>()
        .context("BATCH must be an integer")?;

    let pool = backend::connect().await?;

    println!(
        "mode: {}, batch: {batch}, target: {}",
        if apply { "APPLY" } else { "DRY-RUN" },
        backend::target_description()
    );
    println!(
        "contract_balances rows before: {}",
        backend::count_balances(&pool).await?
    );

    let mut last_id = 0_i64;
    let mut scanned = 0_u64;
    let mut actions_with_balances = 0_u64;
    let mut rows_written = 0_u64;
    let mut decode_errors = 0_u64;

    loop {
        let rows = backend::fetch_batch(&pool, last_id, batch).await?;
        if rows.is_empty() {
            break;
        }

        let mut inserts = Vec::<(i64, ContractBalance)>::new();

        for (id, state, protocol_version) in &rows {
            last_id = *id;
            scanned += 1;

            match decode_balances(*protocol_version, state) {
                Ok(balances) => {
                    if !balances.is_empty() {
                        actions_with_balances += 1;
                        inserts.extend(balances.into_iter().map(|balance| (*id, balance)));
                    }
                }
                Err(error) => {
                    eprintln!("action {id}: decode/extract error: {error}");
                    decode_errors += 1;
                }
            }
        }

        if !inserts.is_empty() {
            if apply {
                backend::insert_balances(&pool, &inserts).await?;
            } else {
                for (action_id, balance) in &inserts {
                    println!(
                        "would insert: action {action_id} token {} amount {}",
                        hex_lower(balance.token_type.as_ref()),
                        balance.amount
                    );
                }
            }
            rows_written += inserts.len() as u64;
        }

        println!("progress: scanned {scanned} actions (last id {last_id})");
    }

    println!(
        "done: scanned {scanned}, actions with balances {actions_with_balances}, rows {} {rows_written}, decode errors {decode_errors}",
        if apply { "inserted" } else { "would insert" }
    );
    println!(
        "contract_balances rows after: {}",
        backend::count_balances(&pool).await?
    );

    if decode_errors > 0 {
        anyhow::bail!("{decode_errors} decode errors, see stderr");
    }
    Ok(())
}

/// The core projection: contract_actions.state bytes plus the transaction's
/// protocol_version (as stored in the DB) to the extracted contract balances, exactly as
/// the fixed chain-indexer computes them at index time.
fn decode_balances(protocol_version: i64, state: &[u8]) -> anyhow::Result<Vec<ContractBalance>> {
    let ledger_version = ProtocolVersion::try_from(protocol_version)
        .with_context(|| format!("unsupported protocol_version {protocol_version}"))?
        .ledger_version();
    let balances = ContractState::deserialize(state, ledger_version)
        .and_then(|contract_state| contract_state.balances())?;
    Ok(balances)
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(feature = "cloud")]
mod backend {
    use anyhow::Context;
    use indexer_common::{domain::ContractBalance, infra::sqlx::U128BeBytes};
    use sqlx::{
        PgPool, QueryBuilder, Row,
        postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
    };
    use std::env;

    pub fn target_description() -> String {
        format!(
            "postgres {}:{}/{}",
            env::var("APP__INFRA__STORAGE__HOST").unwrap_or_else(|_| "localhost".into()),
            env::var("APP__INFRA__STORAGE__PORT").unwrap_or_else(|_| "5432".into()),
            env::var("APP__INFRA__STORAGE__DBNAME").unwrap_or_else(|_| "indexer".into()),
        )
    }

    pub async fn connect() -> anyhow::Result<PgPool> {
        let host = env::var("APP__INFRA__STORAGE__HOST").unwrap_or_else(|_| "localhost".into());
        let port = env::var("APP__INFRA__STORAGE__PORT")
            .unwrap_or_else(|_| "5432".into())
            .parse::<u16>()
            .context("APP__INFRA__STORAGE__PORT must be a port number")?;
        let dbname = env::var("APP__INFRA__STORAGE__DBNAME").unwrap_or_else(|_| "indexer".into());
        let user = env::var("APP__INFRA__STORAGE__USER").unwrap_or_else(|_| "indexer".into());
        let password = env::var("APP__INFRA__STORAGE__PASSWORD")
            .context("APP__INFRA__STORAGE__PASSWORD must be set")?;
        let sslmode = match env::var("APP__INFRA__STORAGE__SSLMODE")
            .unwrap_or_else(|_| "prefer".into())
            .as_str()
        {
            "disable" => PgSslMode::Disable,
            "require" => PgSslMode::Require,
            _ => PgSslMode::Prefer,
        };

        let options = PgConnectOptions::new()
            .host(&host)
            .port(port)
            .database(&dbname)
            .username(&user)
            .password(&password)
            .ssl_mode(sslmode);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .context("cannot connect to Postgres")?;
        Ok(pool)
    }

    pub async fn fetch_batch(
        pool: &PgPool,
        last_id: i64,
        batch: i64,
    ) -> anyhow::Result<Vec<(i64, Vec<u8>, i64)>> {
        let rows = sqlx::query(
            "SELECT ca.id::BIGINT AS id, ca.state, t.protocol_version::BIGINT AS protocol_version
             FROM contract_actions ca
             JOIN transactions t ON t.id = ca.transaction_id
             WHERE ca.id > $1 AND octet_length(ca.state) > 0
             ORDER BY ca.id
             LIMIT $2",
        )
        .bind(last_id)
        .bind(batch)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| {
            (
                row.get::<i64, _>("id"),
                row.get::<Vec<u8>, _>("state"),
                row.get::<i64, _>("protocol_version"),
            )
        })
        .collect();
        Ok(rows)
    }

    pub async fn insert_balances(
        pool: &PgPool,
        inserts: &[(i64, ContractBalance)],
    ) -> anyhow::Result<()> {
        // Mirrors chain-indexer save_contract_balances (token_type.as_ref(), U128BeBytes)
        // so backfilled rows are byte-identical to fixed-code rows. Chunked to stay under
        // Postgres's 65,535 bind-parameter limit (3 per row).
        for chunk in inserts.chunks(5_000) {
            QueryBuilder::new(
                "INSERT INTO contract_balances (contract_action_id, token_type, amount) ",
            )
            .push_values(chunk.iter(), |mut q, (action_id, balance)| {
                q.push_bind(*action_id)
                    .push_bind(balance.token_type.as_ref())
                    .push_bind(U128BeBytes::from(balance.amount));
            })
            .push(" ON CONFLICT DO NOTHING")
            .build()
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    pub async fn count_balances(pool: &PgPool) -> anyhow::Result<i64> {
        let count = sqlx::query("SELECT count(*)::BIGINT AS n FROM contract_balances")
            .fetch_one(pool)
            .await?
            .get::<i64, _>("n");
        Ok(count)
    }
}

#[cfg(feature = "standalone")]
mod backend {
    use anyhow::Context;
    use indexer_common::{domain::ContractBalance, infra::sqlx::U128BeBytes};
    use sqlx::{QueryBuilder, Row, SqlitePool, sqlite::SqliteConnectOptions};
    use std::env;

    fn cnn_url() -> String {
        env::var("APP__INFRA__STORAGE__CNN_URL")
            .unwrap_or_else(|_| "target/data/indexer.sqlite".into())
    }

    pub fn target_description() -> String {
        format!("sqlite {}", cnn_url())
    }

    pub async fn connect() -> anyhow::Result<SqlitePool> {
        let options = cnn_url()
            .parse::<SqliteConnectOptions>()
            .context("APP__INFRA__STORAGE__CNN_URL must be a valid sqlite URL or path")?;
        let pool = SqlitePool::connect_with(options)
            .await
            .context("cannot connect to SQLite")?;
        Ok(pool)
    }

    pub async fn fetch_batch(
        pool: &SqlitePool,
        last_id: i64,
        batch: i64,
    ) -> anyhow::Result<Vec<(i64, Vec<u8>, i64)>> {
        let rows = sqlx::query(
            "SELECT ca.id AS id, ca.state, t.protocol_version AS protocol_version
             FROM contract_actions ca
             JOIN transactions t ON t.id = ca.transaction_id
             WHERE ca.id > ?1 AND length(ca.state) > 0
             ORDER BY ca.id
             LIMIT ?2",
        )
        .bind(last_id)
        .bind(batch)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| {
            (
                row.get::<i64, _>("id"),
                row.get::<Vec<u8>, _>("state"),
                row.get::<i64, _>("protocol_version"),
            )
        })
        .collect();
        Ok(rows)
    }

    pub async fn insert_balances(
        pool: &SqlitePool,
        inserts: &[(i64, ContractBalance)],
    ) -> anyhow::Result<()> {
        // Mirrors chain-indexer save_contract_balances; chunked to stay under SQLite's
        // default bind-parameter limit.
        for chunk in inserts.chunks(5_000) {
            QueryBuilder::new(
                "INSERT INTO contract_balances (contract_action_id, token_type, amount) ",
            )
            .push_values(chunk.iter(), |mut q, (action_id, balance)| {
                q.push_bind(*action_id)
                    .push_bind(balance.token_type.as_ref())
                    .push_bind(U128BeBytes::from(balance.amount));
            })
            .push(" ON CONFLICT DO NOTHING")
            .build()
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    pub async fn count_balances(pool: &SqlitePool) -> anyhow::Result<i64> {
        let count = sqlx::query("SELECT count(*) AS n FROM contract_balances")
            .fetch_one(pool)
            .await?
            .get::<i64, _>("n");
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_balances, hex_lower};
    use indexer_common::{domain::ledger::TaggedSerializableExt, infra::sqlx::U128BeBytes};
    use midnight_base_crypto_v1::hash::HashOutput;
    use midnight_coin_structure_v2::coin::{TokenType as MidnightTokenType, UnshieldedTokenType};
    use midnight_onchain_runtime_v3::state::ContractState as ContractStateV3;
    use midnight_onchain_runtime_v4::state::ContractState as ContractStateV4;
    use midnight_storage_core_v1::DefaultDB;

    #[test]
    fn test_decode_balances_v8() {
        let state = serialized_v3_state(AMOUNT);
        let balances = decode_balances(PV_V8, &state).expect("balances can be decoded");
        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].token_type.0, TOKEN_TYPE);
        assert_eq!(balances[0].amount, AMOUNT);
    }

    #[test]
    fn test_decode_balances_v9() {
        let mut contract_state = ContractStateV4::<DefaultDB>::default();
        contract_state.balance = contract_state.balance.insert(
            MidnightTokenType::Unshielded(UnshieldedTokenType(HashOutput(TOKEN_TYPE))),
            AMOUNT,
        );
        let state = contract_state
            .tagged_serialize()
            .expect("state can be serialized");

        let balances = decode_balances(PV_V9, &state).expect("balances can be decoded");
        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].token_type.0, TOKEN_TYPE);
        assert_eq!(balances[0].amount, AMOUNT);
    }

    #[test]
    fn test_decode_balances_zero_amount_filtered() {
        let state = serialized_v3_state(0);
        let balances = decode_balances(PV_V8, &state).expect("balances can be decoded");
        assert!(balances.is_empty());
    }

    #[test]
    fn test_decode_balances_empty_balance_map() {
        let contract_state = ContractStateV3::<DefaultDB>::default();
        let state = contract_state
            .tagged_serialize()
            .expect("state can be serialized");
        let balances = decode_balances(PV_V8, &state).expect("balances can be decoded");
        assert!(balances.is_empty());
    }

    #[test]
    fn test_decode_balances_unsupported_protocol_version() {
        let state = serialized_v3_state(AMOUNT);
        assert!(decode_balances(999, &state).is_err());
    }

    #[test]
    fn test_decode_balances_garbage_state() {
        assert!(decode_balances(PV_V8, &[0xde, 0xad, 0xbe, 0xef]).is_err());
    }

    /// Real contract state captured from preview-green (2026-06-10): the contract holds
    /// 439,000,000 of one unshielded token. Guards the extraction against real
    /// arena-encoded data, not just synthetic states.
    #[test]
    fn test_decode_balances_real_preview_state() {
        let hex = include_str!("../tests/fixtures/state-45692-contract-3ba7cb40.hex");
        let state = (0..hex.trim().len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex"))
            .collect::<Vec<_>>();

        let balances = decode_balances(PV_V8, &state).expect("balances can be decoded");
        assert_eq!(balances.len(), 1);
        assert_eq!(
            hex_lower(balances[0].token_type.as_ref()),
            "578f00a20340d71020d9003bea6a5377c277c47fe7f23024d6c395acba5c6017"
        );
        assert_eq!(balances[0].amount, 439_000_000);
    }

    #[test]
    fn test_amount_encoding_matches_chain_indexer() {
        assert_eq!(U128BeBytes::from(AMOUNT).0, AMOUNT.to_be_bytes());
    }

    #[test]
    fn test_hex_lower() {
        assert_eq!(hex_lower(&[0x00, 0xab, 0xff]), "00abff");
    }

    const TOKEN_TYPE: [u8; 32] = [7; 32];
    const AMOUNT: u128 = 1_000_000;
    const PV_V8: i64 = 22_000;
    const PV_V9: i64 = 2_000_000;

    fn serialized_v3_state(amount: u128) -> Vec<u8> {
        let mut contract_state = ContractStateV3::<DefaultDB>::default();
        contract_state.balance = contract_state.balance.insert(
            MidnightTokenType::Unshielded(UnshieldedTokenType(HashOutput(TOKEN_TYPE))),
            amount,
        );
        contract_state
            .tagged_serialize()
            .expect("state can be serialized")
            .to_vec()
    }
}
