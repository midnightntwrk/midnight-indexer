// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use super::{IndexerData, graphql::block_subscription};
use anyhow::{Context, ensure};
use futures::TryStreamExt;
use graphql_client::GraphQLQuery;
use reqwest::Client;
use serde_json::{Value, json};
use std::time::Duration;

const DOCUMENT: &str = include_str!("../../contract_events.graphql");
const EVENT_TYPES: [&str; 11] = [
    "SHIELDED_SPEND",
    "SHIELDED_RECEIVE",
    "SHIELDED_MINT",
    "SHIELDED_BURN",
    "UNSHIELDED_SPEND",
    "UNSHIELDED_RECEIVE",
    "UNSHIELDED_MINT",
    "UNSHIELDED_BURN",
    "PAUSED",
    "UNPAUSED",
    "MISC",
];

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v4.graphql",
    query_path = "./contract_events.graphql",
    response_derives = "Debug, Clone, Serialize"
)]
struct ContractEventSubscription;

use indexer_api::infra::api::v4::{HexEncodable, HexEncoded};

fn padded_hex(text: &str, len: usize) -> String {
    let mut bytes = text.as_bytes().to_vec();
    bytes.resize(len, 0);
    bytes.hex_encode().as_ref().to_owned()
}

fn payloads() -> Vec<(&'static str, Value)> {
    let a = "61".repeat(32);
    let b = "62".repeat(32);
    let c = "63".repeat(32);
    let d = "64".repeat(32);
    vec![
        ("ShieldedSpendEvent", json!({"nullifier": a})),
        (
            "ShieldedReceiveEvent",
            json!({"commitment": b, "ciphertext": padded_hex("ciphertext", 512), "receivingContractAddress": c}),
        ),
        (
            "ShieldedMintEvent",
            json!({"commitment": b, "domainSep": d, "amount": "123"}),
        ),
        (
            "ShieldedBurnEvent",
            json!({"nullifier": a, "amount": "456"}),
        ),
        (
            "UnshieldedSpendEvent",
            json!({"sender": {"kind": "USER", "userAddress": a, "contractAddress": null}, "domainSep": d, "tokenType": b, "amount": "789"}),
        ),
        (
            "UnshieldedReceiveEvent",
            json!({"recipient": {"kind": "CONTRACT", "userAddress": null, "contractAddress": c}, "domainSep": d, "tokenType": b, "amount": "987"}),
        ),
        (
            "UnshieldedMintEvent",
            json!({"domainSep": d, "tokenType": b, "amount": "654"}),
        ),
        (
            "UnshieldedBurnEvent",
            json!({"sender": {"kind": "CONTRACT", "userAddress": null, "contractAddress": c}, "tokenType": b, "amount": "321"}),
        ),
        ("PausedEvent", json!({})),
        ("UnpausedEvent", json!({})),
        (
            "MiscContractEvent",
            json!({"name": a, "payload": padded_hex("indexer event fixture", 256)}),
        ),
    ]
}

// Expected indexed values come from the fixture contract, not from API output.
fn indexed_fields(payload: &Value) -> Vec<(String, String)> {
    payload
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(name, value)| match name.as_str() {
            "nullifier" | "commitment" | "domainSep" | "tokenType" | "ciphertext" => {
                Some((name.clone(), value.as_str().unwrap().to_owned()))
            }
            "sender" | "recipient" => Some((
                name.clone(),
                value["userAddress"]
                    .as_str()
                    .or_else(|| value["contractAddress"].as_str())
                    .unwrap()
                    .to_owned(),
            )),
            _ => None,
        })
        .collect()
}

async fn query(
    client: &Client,
    url: &str,
    operation: &str,
    variables: Value,
) -> anyhow::Result<Value> {
    let response: Value = client
        .post(url)
        .json(&json!({
            "query": DOCUMENT, "operationName": operation, "variables": variables,
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(response.get("errors").is_none(), "{operation}: {response}");
    response
        .get("data")
        .cloned()
        .context("missing GraphQL data")
}

async fn events(client: &Client, url: &str, filter: Value) -> anyhow::Result<Vec<Value>> {
    let data = query(
        client,
        url,
        "ContractEventQuery",
        json!({"filter": filter, "limit": 500}),
    )
    .await?;
    data["contractEvents"]
        .as_array()
        .cloned()
        .context("missing contractEvents")
}

async fn subscribe(
    url: &str,
    address: &str,
    height: i64,
    id: i64,
    field: Option<(&str, &str)>,
) -> anyhow::Result<Vec<Value>> {
    use contract_event_subscription::{ContractEventFilter, FieldPrefixFilter, Variables};
    let field_prefixes = field.map(|(name, prefix)| {
        vec![FieldPrefixFilter {
            field_name: name.to_owned(),
            prefix: prefix.to_owned().try_into().unwrap(),
        }]
    });
    let variables = Variables {
        filter: ContractEventFilter {
            contract_address: address.to_owned().try_into()?,
            types: None,
            field_prefixes,
            from_block: Some(0),
            to_block: Some(height),
            transaction_hash: None,
        },
        id: Some(id),
    };
    // Collect until the server completes: take(expected.len()) would mask a broken
    // toBlock terminator and extra events. A missing terminator must fail the test.
    let stream =
        crate::graphql_ws_client::subscribe::<ContractEventSubscription>(url, variables).await?;
    let results = tokio::time::timeout(Duration::from_secs(20), stream.try_collect::<Vec<_>>())
        .await
        .context("contractEvents did not complete at toBlock")??;
    results
        .into_iter()
        .map(|r| serde_json::to_value(r.contract_events).map_err(Into::into))
        .collect()
}

pub(super) async fn run(
    data: &IndexerData,
    client: &Client,
    url: &str,
    ws: &str,
) -> anyhow::Result<()> {
    let call = data.contract_actions.iter().find(|action| matches!(
        &action.on, block_subscription::BlockSubscriptionBlocksTransactionsContractActionsOn::ContractCall(c)
            if c.entry_point == "emit_all"
    )).context("emit_all fixture call is missing; regenerate the current node fixture")?;
    let address = call.address.as_ref();
    let height = call.transaction.block.height;
    let all = events(client, url, json!({"contractAddress": address})).await?;
    let expected = payloads();
    ensure!(
        all.len() == expected.len(),
        "expected all eleven events, got {}",
        all.len()
    );
    for (event, (name, payload)) in all.iter().zip(&expected) {
        assert_eq!(event["__typename"], *name);
        assert_eq!(event["contractAddress"], address);
        assert_eq!(event["transaction"]["block"]["height"], height);
        ensure!(
            !event["raw"].as_str().unwrap_or_default().is_empty(),
            "missing raw event"
        );
        for (field, value) in payload.as_object().unwrap() {
            assert_eq!(&event[field], value, "{name}.{field}");
        }
    }
    assert!(
        all.windows(2)
            .all(|pair| pair[0]["id"].as_i64() < pair[1]["id"].as_i64())
    );

    // Every variant must be selectable without leaking any other variant.
    for (index, kind) in EVENT_TYPES.iter().enumerate() {
        assert_eq!(
            events(
                client,
                url,
                json!({"contractAddress": address, "types": [kind]})
            )
            .await?,
            vec![all[index].clone()]
        );
    }
    // Exercise each persisted indexed field with an independent expected subset.
    for (_, payload) in &expected {
        for (field, value) in indexed_fields(payload) {
            let matching: Vec<_> = all
                .iter()
                .zip(&expected)
                .filter(|(_, (_, p))| {
                    indexed_fields(p)
                        .iter()
                        .any(|(n, v)| n == &field && v.starts_with(&value[..2]))
                })
                .map(|(e, _)| e.clone())
                .collect();
            assert_eq!(
                events(
                    client,
                    url,
                    json!({"contractAddress": address,
                "fieldPrefixes": [{"fieldName": field, "prefix": &value[..2]}]})
                )
                .await?,
                matching
            );
            assert!(
                events(
                    client,
                    url,
                    json!({"contractAddress": address,
                "fieldPrefixes": [{"fieldName": field, "prefix": "ff"}]})
                )
                .await?
                .is_empty()
            );
        }
    }
    let and_filter = json!({"contractAddress": address, "fieldPrefixes": [
        {"fieldName": "sender", "prefix": "61"}, {"fieldName": "domainSep", "prefix": "64"}
    ]});
    assert_eq!(events(client, url, and_filter).await?, vec![all[4].clone()]);
    assert!(
        events(
            client,
            url,
            json!({"contractAddress": address, "fieldPrefixes": [
                {"fieldName": "sender", "prefix": "61"}, {"fieldName": "domainSep", "prefix": "ff"}
            ]})
        )
        .await?
        .is_empty()
    );
    assert!(
        events(client, url, json!({"contractAddress": "ff".repeat(32)}))
            .await?
            .is_empty()
    );
    assert!(
        events(
            client,
            url,
            json!({"contractAddress": address, "toBlock": height - 1})
        )
        .await?
        .is_empty()
    );
    assert_eq!(
        events(
            client,
            url,
            json!({"contractAddress": address,
        "fromBlock": height, "toBlock": height, "transactionHash": call.transaction.hash})
        )
        .await?,
        all
    );

    // Data-level nested assertion against the real emitting ContractCall.
    let nested = query(
        client,
        url,
        "ContractEventBlockQuery",
        json!({"height": height}),
    )
    .await?;
    let calls: Vec<_> = nested["block"]["transactions"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tx| tx["contractActions"].as_array().unwrap())
        .filter(|a| a["address"] == address && a["entryPoint"] == "emit_all")
        .collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["contractEvents"], json!(all));

    assert_eq!(subscribe(ws, address, height, 0, None).await?, all);
    assert!(
        subscribe(ws, address, height - 1, 0, None)
            .await?
            .is_empty()
    );
    assert_eq!(
        subscribe(
            ws,
            address,
            height,
            all[5]["id"].as_i64().unwrap() + 1,
            None
        )
        .await?,
        all[6..]
    );
    assert_eq!(
        subscribe(ws, address, height, 0, Some(("nullifier", "61"))).await?,
        vec![all[0].clone(), all[3].clone()]
    );
    assert!(
        subscribe(ws, address, height, 0, Some(("nullifier", "ff")))
            .await?
            .is_empty()
    );
    Ok(())
}
