// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

// Verify the real chain-indexer writes, not hand-inserted test rows. The unique
// Misc event identifies our emitter independently of its generated address.
const QUERY: &str = "
    SELECT CAST(e.variant AS TEXT), f.field_name, f.field_value
    FROM ledger_events e
    LEFT JOIN contract_event_indexed_fields f ON f.ledger_event_id = e.id
    WHERE e.contract_address = (
        SELECT contract_address FROM ledger_events WHERE variant = 'Misc' LIMIT 1
    )";

#[cfg(feature = "cloud")]
type Pool = sqlx::PgPool;
#[cfg(feature = "standalone")]
type Pool = sqlx::SqlitePool;

#[cfg(feature = "cloud")]
pub async fn cloud(port: u16) -> anyhow::Result<()> {
    let options = sqlx::postgres::PgConnectOptions::new()
        .host("localhost")
        .port(port)
        .database("indexer")
        .username("indexer")
        .password(env!("APP__INFRA__STORAGE__PASSWORD"));
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    verify(&pool).await
}

#[cfg(feature = "standalone")]
pub async fn standalone(path: &std::path::Path) -> anyhow::Result<()> {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .read_only(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    verify(&pool).await
}

async fn verify(pool: &Pool) -> anyhow::Result<()> {
    type Row = (String, Option<String>, Option<Vec<u8>>);
    let actual: Vec<Row> = sqlx::query_as(QUERY).fetch_all(pool).await?;
    let mut expected = BTreeSet::new();
    for (variant, fields) in [
        ("ShieldedSpend", vec![("nullifier", vec![b'a'; 32])]),
        (
            "ShieldedReceive",
            vec![
                ("commitment", vec![b'b'; 32]),
                ("ciphertext", {
                    let mut value = b"ciphertext".to_vec();
                    value.resize(512, 0);
                    value
                }),
            ],
        ),
        (
            "ShieldedMint",
            vec![
                ("commitment", vec![b'b'; 32]),
                ("domainSep", vec![b'd'; 32]),
            ],
        ),
        ("ShieldedBurn", vec![("nullifier", vec![b'a'; 32])]),
        (
            "UnshieldedSpend",
            vec![
                ("sender", vec![b'a'; 32]),
                ("domainSep", vec![b'd'; 32]),
                ("tokenType", vec![b'b'; 32]),
            ],
        ),
        (
            "UnshieldedReceive",
            vec![
                ("recipient", vec![b'c'; 32]),
                ("domainSep", vec![b'd'; 32]),
                ("tokenType", vec![b'b'; 32]),
            ],
        ),
        (
            "UnshieldedMint",
            vec![("domainSep", vec![b'd'; 32]), ("tokenType", vec![b'b'; 32])],
        ),
        (
            "UnshieldedBurn",
            vec![("sender", vec![b'c'; 32]), ("tokenType", vec![b'b'; 32])],
        ),
        ("Paused", vec![]),
        ("Unpaused", vec![]),
        ("Misc", vec![]),
    ] {
        if fields.is_empty() {
            expected.insert((variant.to_owned(), None, None));
        }
        for (name, value) in fields {
            expected.insert((variant.to_owned(), Some(name.to_owned()), Some(value)));
        }
    }
    assert_eq!(
        actual.len(),
        expected.len(),
        "missing or duplicate indexed field rows"
    );
    assert_eq!(actual.into_iter().collect::<BTreeSet<_>>(), expected);
    Ok(())
}
