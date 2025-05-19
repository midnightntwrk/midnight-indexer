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

#![allow(deprecated)]
#![cfg(feature = "cloud")]

mod graphql_ws_client;

use crate::graphql_ws_client::{GraphQLWSClient, GraphQLWSMessage};
use anyhow::{anyhow, bail, Context};
use assert_matches::assert_matches;
use bech32::{encode, Bech32m, Hrp};
use chain_indexer::{
    domain::{Block, Node, Transaction},
    infra::node::{Config as NodeConfig, SubxtNode},
};
use const_hex::ToHexExt;
use contracts_subscription::ContractsSubscriptionContracts::*;
use fs_extra::dir::{copy, CopyOptions};
use futures::{StreamExt, TryStreamExt};
use graphql_client::{GraphQLQuery, Response};
use indexer_api::{
    domain::{AsBytesExt, HexEncoded, ViewingKey},
    infra::api::{
        v1::{ApplyStage, UnshieldedAddress},
        Unit,
    },
};
use indexer_common::{
    domain::{unshielded::to_bech32m, NetworkId, ViewingKey as CommonViewingKey},
    infra::{pub_sub::nats::publisher::NatsPublisher, zswap_state_storage},
};
use indexer_tests::{
    chain_indexer_data::{
        token_type_to_hex, INTENT_HASH, OWNER_ADDR_EMPTY, TOKEN_NIGHT, UT_ADDR_1_HEX,
    },
    graphql_query::{
        transactions_by_address, unshielded_utxos, TransactionsByAddress, UnshieldedUtxos,
    },
};
use midnight_ledger::{
    serialize::Serializable,
    transient_crypto::encryption::SecretKey,
    zswap::keys::{SecretKeys, Seed},
};
use reqwest::{Client, StatusCode};
use std::{
    env,
    net::TcpListener,
    path::Path,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use testcontainers::{
    core::{Mount, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};
use testcontainers_modules::postgres::Postgres;
use tokio::{
    process::{Child, Command},
    time::sleep,
};

const NODE_VERSION: &str = "0.12.0-4337aca9";
const SEED_0001: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const SEED_0002: &str = "0000000000000000000000000000000000000000000000000000000000000002";
const BLOCKS_TO_FETCH: u32 = 30;
const EXPECTED_MIN_LATEST_BLOCK_HEIGHT: i64 = (BLOCKS_TO_FETCH - 1) as i64;
const API_READY_TIMEOUT_SECS: u64 = 60;
const BLOCK_FETCH_TIMEOUT_SECS: u64 = 15;
const NETWORK_ID: NetworkId = NetworkId::Undeployed;

#[tokio::test]
async fn main() -> anyhow::Result<()> {
    let (_postgres_container, postgres_port) = start_postgres().await?;
    let (_nats_container, nats_url) = start_nats().await?;
    let NodeHandle {
        _temp_dir,
        _node_container,
        node_url,
    } = start_node().await?;

    let expected_blocks = collect_blocks_up_to_height(&node_url, BLOCKS_TO_FETCH).await?;

    let api_port = find_free_port()?;
    setup_shared_config(api_port, postgres_port, nats_url.clone(), node_url);

    let mut chain_indexer = spawn_standalone_binary("chain-indexer")?;
    let mut wallet_indexer = spawn_standalone_binary("wallet-indexer")?;
    let mut indexer_api = spawn_standalone_binary("indexer-api")?;

    let client = Client::new();
    wait_for_api_ready(api_port).await?;
    run_scenario(api_port, &client, expected_blocks).await?;

    // It is best practice to kill the processes even when spawned with `kill_on_drop`.
    let _ = indexer_api.kill().await;
    let _ = wallet_indexer.kill().await;
    let _ = chain_indexer.kill().await;

    Ok(())
}

async fn run_scenario(
    api_port: u16,
    client: &Client,
    expected_blocks: Vec<Block>,
) -> anyhow::Result<()> {
    let (block_with_tx, first_tx) = find_first_block_with_transaction(&expected_blocks)
        .context("failed to find any block with transactions in the fetched range")?;

    test_block_queries(&expected_blocks, client, api_port, block_with_tx).await?;
    test_transaction_queries(client, api_port, block_with_tx, first_tx).await?;
    test_unshielded_utxo_queries(api_port, client).await?;
    test_transactions_filtered_by_address(api_port, client).await?;

    let viewing_key_a_str = seed_to_viewing_key(SEED_0001)?;
    let viewing_key_b_str = seed_to_viewing_key(SEED_0002)?;

    let session_a_id = test_connect_mutation(api_port, client, viewing_key_a_str).await?;
    let session_b_id = test_connect_mutation(api_port, client, viewing_key_b_str).await?;

    test_disconnect_mutation(api_port, client, session_a_id).await?;
    test_disconnect_mutation(api_port, client, session_b_id).await?;

    // test_blocks_subscription(&storage, &mut zswap_state_storage, &publisher, &loaded_six_blocks,
    // api_port).await?;
    // test_contracts_subscription(&storage, &mut zswap_state_storage, &publisher,
    // &loaded_six_blocks, api_port).await?;
    // test_wallet_subscription(&viewing_key, &mut wallet_storage, &publisher).await?;

    Ok(())
}

/// Produce a "viewing key" string from a 32‐byte hex seed, for the ‘connect’ mutation.
fn seed_to_viewing_key(seed: &str) -> anyhow::Result<String> {
    let seed_bytes = const_hex::decode(seed).expect("seed can be hex-decoded");
    let seed_bytes = <[u8; 32]>::try_from(seed_bytes).expect("seed has 32 bytes");
    let viewing_key = SecretKeys::from(Seed::from(seed_bytes)).encryption_secret_key;

    let mut bytes = vec![];
    <SecretKey as Serializable>::serialize(&viewing_key, &mut bytes)
        .expect("secret key can be serialized");
    let viewing_key = bytes.encode_hex();

    Ok(viewing_key)
}

async fn collect_blocks_up_to_height(node_url: &str, count: u32) -> anyhow::Result<Vec<Block>> {
    let node_cfg = NodeConfig {
        url: node_url.to_string(),
        ..Default::default()
    };
    let mut subxt_node = SubxtNode::new(node_cfg).await?;

    let blocks = subxt_node
        .finalized_blocks(None, NetworkId::Undeployed)
        .take(count as usize)
        .try_collect::<Vec<Block>>()
        .await?;

    if blocks.len() < count as usize {
        bail!(
            "Expected to fetch {} blocks, but only got {}",
            count,
            blocks.len()
        );
    }

    Ok(blocks)
}

fn find_first_block_with_transaction(blocks: &[Block]) -> Option<(&Block, &Transaction)> {
    blocks
        .iter()
        .filter(|block| !block.transactions.is_empty())
        .nth(1)
        .map(|block| (block, &block.transactions[0]))
}

async fn start_postgres() -> anyhow::Result<(ContainerAsync<Postgres>, u16)> {
    let postgres_container = Postgres::default()
        .with_db_name("indexer")
        .with_user("indexer")
        .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
        .with_tag("17.1-alpine")
        .start()
        .await
        .context("start Postgres container")?;
    let postgres_port = postgres_container
        .get_host_port_ipv4(5432)
        .await
        .context("get Postgres port")?;

    Ok((postgres_container, postgres_port))
}

async fn start_nats() -> anyhow::Result<(ContainerAsync<GenericImage>, String)> {
    let nats_container = GenericImage::new("nats", "2.10.24")
        .with_wait_for(WaitFor::message_on_stderr("Server is ready"))
        .with_cmd([
            "--user",
            "indexer",
            "--pass",
            env!("APP__INFRA__PUB_SUB__PASSWORD"),
            "-js",
        ])
        .start()
        .await
        .context("start NATS container")?;
    // The NATS container seems to take a while before actually accepting connections!
    sleep(Duration::from_millis(500)).await;
    let nats_port = nats_container
        .get_host_port_ipv4(4222)
        .await
        .context("get NATS port")?;
    let nats_url = format!("localhost:{nats_port}");

    Ok((nats_container, nats_url))
}

pub struct NodeHandle {
    _temp_dir: TempDir, // Keep the TempDir around for the entire test
    _node_container: ContainerAsync<GenericImage>,
    pub node_url: String,
}

async fn start_node() -> anyhow::Result<NodeHandle> {
    let node_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.node")
        .join(NODE_VERSION)
        .canonicalize()
        .context("cannot find ../.node/<version> directory")?;

    let temp_dir = tempfile::tempdir().context("cannot create tempdir")?;
    let copy_path = temp_dir.path().join(NODE_VERSION);

    copy(&node_dir, &temp_dir, &CopyOptions::default())
        .context("copying .node directory into tempdir failed")?;

    let host_path = copy_path.display().to_string();

    let node_container = GenericImage::new("ghcr.io/midnight-ntwrk/midnight-node", NODE_VERSION)
        .with_wait_for(WaitFor::message_on_stderr("9944"))
        .with_mount(Mount::bind_mount(host_path, "/node"))
        .with_env_var("SHOW_CONFIG", "false")
        .with_env_var("CFG_PRESET", "dev")
        .start()
        .await
        .context("failed to start node container")?;

    let node_port = node_container
        .get_host_port_ipv4(9944)
        .await
        .context("failed to get node port")?;
    let node_url = format!("ws://localhost:{node_port}");

    Ok(NodeHandle {
        _temp_dir: temp_dir, // underscore to indicate we keep it just for its lifetime
        _node_container: node_container,
        node_url,
    })
}

async fn wait_for_api_ready(api_port: u16) -> anyhow::Result<()> {
    let client = Client::new();
    let timeout = Duration::from_secs(API_READY_TIMEOUT_SECS);
    let start_time = Instant::now();

    while start_time.elapsed() < timeout {
        let url = format!("http://localhost:{}/ready", api_port);
        match client.get(&url).send().await {
            Ok(response) if response.status() == StatusCode::OK => {
                return Ok(());
            }

            _ => {
                sleep(Duration::from_millis(500)).await;
            }
        }
    }

    bail!("server failed to become ready within {timeout:?}")
}

fn setup_shared_config(api_port: u16, postgres_port: u16, nats_url: String, node_url: String) {
    let config_file = format!("{}/config.yaml", env!("CARGO_MANIFEST_DIR"));

    unsafe {
        env::set_var(
        "RUST_LOG",
        "indexer_standalone=info,chain_indexer=info,wallet_indexer=info,indexer_api=info,indexer_common=info,fastrace_opentelemetry=off,warn",
    )
    };

    unsafe {
        env::set_var("CONFIG_FILE", config_file);
        env::set_var("APP__INFRA__API__PORT", api_port.to_string());
        env::set_var("APP__INFRA__NODE__URL", node_url);
        env::set_var("APP__INFRA__ZSWAP_STATE_STORAGE__URL", nats_url.clone());
        env::set_var("APP__INFRA__STORAGE__PORT", postgres_port.to_string());
        env::set_var("APP__INFRA__PUB_SUB__URL", nats_url);
    }
}

fn spawn_standalone_binary(name: &'static str) -> anyhow::Result<Child> {
    Command::new(format!("../target/debug/{name}"))
        .kill_on_drop(true)
        .spawn()
        .context(format!("spawn {name}"))
}

#[allow(unused)]
fn process_blocks_message(
    message: &GraphQLWSMessage<blocks_subscription::ResponseData>,
) -> Option<blocks_subscription::ResponseData> {
    match message {
        GraphQLWSMessage::Next { payload, .. } => payload.data.clone(),
        _ => None,
    }
}

#[allow(unused)]
fn process_contracts_message(
    message: &GraphQLWSMessage<contracts_subscription::ResponseData>,
) -> Option<contracts_subscription::ResponseData> {
    match message {
        GraphQLWSMessage::Next { payload, .. } => payload.data.clone(),
        _ => None,
    }
}

async fn test_block_queries(
    expected_blocks: &[Block],
    client: &Client,
    api_port: u16,
    block_with_tx: &Block,
) -> anyhow::Result<()> {
    let start_time = Instant::now();
    let timeout = Duration::from_secs(BLOCK_FETCH_TIMEOUT_SECS);
    let latest_block_from_api = loop {
        let variables = latest_block::Variables;
        let response = send_graphql_query::<LatestBlock>(api_port, client, variables).await?;

        if let Some(data) = response.data {
            if let Some(block) = data.block {
                if block.height >= EXPECTED_MIN_LATEST_BLOCK_HEIGHT {
                    break block;
                }
            }
        }

        if start_time.elapsed() >= timeout {
            bail!(
                "timeout after 10 seconds waiting for blocks upto {}",
                EXPECTED_MIN_LATEST_BLOCK_HEIGHT
            );
        }
        sleep(Duration::from_millis(50)).await;
    };

    let hash_bytes = latest_block_from_api.hash.hex_decode::<Vec<u8>>()?;
    assert!(!hash_bytes.is_empty());

    let expected_hash = block_with_tx.hash.hex_encode();

    let variables = block_by_hash::Variables {
        hash: expected_hash.clone(),
    };
    let response = send_graphql_query::<BlockByHash>(api_port, client, variables).await?;

    let block = response
        .data
        .ok_or(anyhow!("missing data"))?
        .block
        .ok_or(anyhow!("missing block"))?;

    assert_eq!(block.hash, expected_hash);
    assert_eq!(block.height, block_with_tx.height as i64);
    assert_eq!(
        block.protocol_version,
        block_with_tx.protocol_version.0 as i64,
    );

    assert_eq!(block.author, block_with_tx.author.map(|x| x.hex_encode()));
    assert_eq!(block.timestamp, block_with_tx.timestamp as i64);
    assert_eq!(block.transactions.len(), block_with_tx.transactions.len());

    // let transaction = block.transactions.into_iter().next().unwrap();
    // assert_eq!(
    //     transaction.hash,
    //     expected_block.transactions[0].hash.hex_encode()
    // );
    assert!(block.parent.is_some());

    let parent = block.parent.unwrap();
    assert_eq!(parent.hash, block_with_tx.parent_hash.hex_encode());
    let expected_parent_height = block_with_tx.height as usize - 1;
    assert_eq!(parent.height, expected_parent_height as i64);
    let expected_parent_block = expected_blocks[expected_parent_height].clone();
    assert_eq!(
        parent.author,
        expected_parent_block
            .clone()
            .author
            .map(|x| x.0.hex_encode())
    );
    assert_eq!(
        parent.transactions.len(),
        expected_parent_block.transactions.len()
    );

    // Test block by height
    let variables = block_by_height::Variables { height: 2 };
    let response = send_graphql_query::<BlockByHeight>(api_port, client, variables).await?;
    let block = response
        .data
        .ok_or(anyhow!("missing data"))?
        .block
        .ok_or(anyhow!("missing block with height 2"))?;
    let expected_block = expected_blocks[2].clone();
    assert_eq!(block.hash, expected_block.hash.hex_encode());
    assert_eq!(block.height, expected_block.height as i64);

    Ok(())
}

async fn test_transaction_queries(
    client: &Client,
    api_port: u16,
    expected_block: &Block,
    expected_transaction: &Transaction,
) -> anyhow::Result<()> {
    let transaction_hash = expected_transaction.hash.hex_encode();

    let variables = transaction_by_hash::Variables {
        hash: transaction_hash.clone(),
    };
    let response = send_graphql_query::<TransactionByHash>(api_port, client, variables).await?;
    let mut transactions = response.data.ok_or(anyhow!("missing data"))?.transactions;
    let transaction = transactions.pop();
    assert!(transaction.is_some());
    let transaction = transaction.unwrap();
    assert_eq!(transaction.hash, transaction_hash);
    assert_eq!(transaction.block.hash, expected_block.hash.hex_encode());

    Ok(())
}

async fn test_connect_mutation(
    api_port: u16,
    client: &Client,
    raw_viewing_key: String,
) -> anyhow::Result<HexEncoded> {
    let hex_viewing_key = const_hex::decode(raw_viewing_key)?;
    let hex_common_viewing_key = CommonViewingKey::from(hex_viewing_key); //encode with raw doesn't work
    let hrp = Hrp::parse("mn_shield-esk_undeployed")?;
    let bech32m_viewing_key = encode::<Bech32m>(hrp, hex_common_viewing_key.expose_secret())?;

    let viewing_key = ViewingKey(bech32m_viewing_key.clone());
    let variables = connect::Variables {
        viewing_key: viewing_key.clone(),
    };
    let response = send_graphql_query::<Connect>(api_port, client, variables).await?;

    let session_id = response.data.ok_or(anyhow!("missing data"))?.connect;
    assert_eq!(
        session_id,
        CommonViewingKey::try_from(viewing_key)?
            .as_session_id()
            .hex_encode()
    );

    Ok(session_id)
}

async fn test_disconnect_mutation(
    api_port: u16,
    client: &Client,
    session_id: HexEncoded,
) -> anyhow::Result<()> {
    let variables = disconnect::Variables { session_id };
    let response = send_graphql_query::<Disconnect>(api_port, client, variables).await?;

    assert_eq!(response.data.unwrap().disconnect, Unit);

    Ok(())
}

#[allow(unused)]
async fn test_blocks_subscription(
    storage: &chain_indexer::infra::storage::postgres::PostgresStorage,
    zswap_state_storage: &mut zswap_state_storage::nats::NatsZswapStateStorage,
    publisher: &NatsPublisher,
    expected_blocks: &[Block],
    api_port: u16,
) -> anyhow::Result<()> {
    let mut client = GraphQLWSClient::connect_and_establish::<serde_json::Value>(api_port).await?;

    let blocks_subscription_query =
        BlocksSubscription::build_query(blocks_subscription::Variables {
            offset: Some(blocks_subscription::BlockOffsetInput::Height(0)),
        });

    let variables = serde_json::to_value(&blocks_subscription_query.variables)?;
    client
        .send_subscription(
            blocks_subscription_query.query,
            variables,
            "BlocksSubscription",
            "1",
        )
        .await?;

    sleep(Duration::from_millis(50)).await;

    let process_message =
        |message: &GraphQLWSMessage<blocks_subscription::ResponseData>| match message {
            GraphQLWSMessage::Next { payload, .. } => payload.data.clone(),
            _ => None,
        };

    let received_blocks = client
        .receive_messages::<_, blocks_subscription::ResponseData>(3, process_message)
        .await?;

    assert_eq!(received_blocks.len(), 3);
    let expected_hash = HexEncoded::try_from(
        "d81e35e1addbdb8fe0f88d45c9ee18ec30710fd4d9b9cee2d08f96e5431dd99a".to_string(),
    )?;
    assert_eq!(received_blocks[0].blocks.hash, expected_hash);
    assert_eq!(received_blocks[0].blocks.height, 0);
    let expected_hash = HexEncoded::try_from(
        "22ac5c0448c00de32dacb85bf2c53998f2889b48dca9270e723407ad56a07497".to_string(),
    )?;
    assert_eq!(received_blocks[1].blocks.hash, expected_hash);
    assert_eq!(received_blocks[1].blocks.height, 1);
    let expected_hash = HexEncoded::try_from(
        "caed9de4a8794f92fcecebddc8b33f6f96c375a6b05ecf8b662f3861bb6fdfc3".to_string(),
    )?;
    assert_eq!(received_blocks[2].blocks.hash, expected_hash);
    assert_eq!(received_blocks[2].blocks.height, 2);

    // todo We will use tx-generator to send fund from SEED_0001 to SEED_0002
    // https://shielded.atlassian.net/browse/PM-15324
    // https://github.com/input-output-hk/midnight-substrate-prototype/pull/687

    // let published_blocks = client
    //     .receive_messages::<_, blocks_subscription::ResponseData>(2, process_blocks_message)
    //     .await?;
    //
    // assert_eq!(published_blocks.len(), 2);
    // let expected_hash = HexEncoded::try_from(
    //     "90a655c1dbe04481148fffe2197f2d2ae08935b2645032f430c3ed5b5002cf10".to_string(),
    // )?;
    // assert_eq!(published_blocks[0].blocks.hash, expected_hash);
    // assert_eq!(published_blocks[0].blocks.height, 3);
    // let expected_hash = HexEncoded::try_from(
    //     "eda40018d98a34f6adbf5195e092649e5cdd5076662d91a86ca300de1daef632".to_string(),
    // )?;
    // assert_eq!(published_blocks[1].blocks.hash, expected_hash);
    // assert_eq!(published_blocks[1].blocks.height, 4);

    Ok(())
}

#[allow(unused)]
async fn test_contracts_subscription(
    storage: &chain_indexer::infra::storage::postgres::PostgresStorage,
    zswap_state_storage: &mut zswap_state_storage::nats::NatsZswapStateStorage,
    publisher: &NatsPublisher,
    expected_blocks: &[Block],
    api_port: u16,
) -> anyhow::Result<()> {
    let mut client = GraphQLWSClient::connect_and_establish::<serde_json::Value>(api_port).await?;

    let hex_address = b"address".hex_encode(); //todo to make compiler happy
    let contract_address = hex_address.to_owned();

    let contracts_subscription_query =
        ContractsSubscription::build_query(contracts_subscription::Variables {
            address: contract_address,
            offset: Some(contracts_subscription::BlockOffsetInput::Height(0)),
        });

    let variables = serde_json::to_value(&contracts_subscription_query.variables)?;
    client
        .send_subscription(
            contracts_subscription_query.query,
            variables,
            "ContractsSubscription",
            "1",
        )
        .await?;

    sleep(Duration::from_millis(50)).await;

    let process_message =
        |message: &GraphQLWSMessage<contracts_subscription::ResponseData>| match message {
            GraphQLWSMessage::Next { payload, .. } => payload.data.clone(),
            _ => None,
        };

    let received_contracts = client
        .receive_messages::<_, contracts_subscription::ResponseData>(6, process_message)
        .await?;

    assert_eq!(received_contracts.len(), 6);
    assert_matches!(&received_contracts[0].contracts, ContractDeploy(_));
    assert_matches!(&received_contracts[1].contracts, ContractCall(_));
    assert_matches!(&received_contracts[2].contracts, ContractUpdate(_));
    assert_matches!(&received_contracts[3].contracts, ContractCall(_));
    assert_matches!(&received_contracts[4].contracts, ContractUpdate(_));
    assert_matches!(&received_contracts[5].contracts, ContractCall(_));

    // todo We will use tx-generator to send fund from SEED_0001 to SEED_0002
    // https://shielded.atlassian.net/browse/PM-15324
    // https://github.com/input-output-hk/midnight-substrate-prototype/pull/687

    // let published_contracts = client
    //     .receive_messages::<_, contracts_subscription::ResponseData>(2,
    // process_contracts_message)     .await?;
    //
    // assert_eq!(published_contracts.len(), 2);
    // assert_matches!(&published_contracts[0].contracts, ContractUpdate(_));
    // assert_matches!(&published_contracts[1].contracts, ContractCall(_));

    Ok(())
}

async fn test_unshielded_utxo_queries(api_port: u16, client: &Client) -> anyhow::Result<()> {
    let owner_bech32m = to_bech32m(&const_hex::decode(UT_ADDR_1_HEX)?, NETWORK_ID)?;
    let owner_addr_gql = UnshieldedAddress(owner_bech32m);

    let expected_value_str = "1000";
    let expected_token_type = HexEncoded::try_from(token_type_to_hex(&TOKEN_NIGHT))?;
    let expected_intent_hash = HexEncoded::try_from(const_hex::encode(INTENT_HASH.as_ref()))?;

    let query_json = serde_json::json!({ //oneOf (UnshieldedOffset) is not supported by graphql-client
    "query": "query($address: UnshieldedAddress!, $offset: UnshieldedOffset) {
        unshieldedUtxos(address: $address, offset: $offset) {
            owner value tokenType intentHash outputIndex createdAtTransaction { hash block { height } } spentAtTransaction { hash block { height } }
        }
    }",
    "variables": {
        "address": owner_addr_gql.0,
        "offset": {
            "blockOffsetInput": {
                "height": 0
            }
        }
    }
    });

    let resp = client
        .post(format!("http://localhost:{api_port}/api/v1/graphql"))
        .json(&query_json)
        .send()
        .await
        .context("send request")?
        .json::<Response<unshielded_utxos::ResponseData>>()
        .await
        .context("JSON deserialize response")?;

    let utxos = resp
        .data
        .ok_or_else(|| anyhow!("missing data in UnshieldedUtxos response"))?
        .unshielded_utxos;

    assert!(!utxos.is_empty());

    let utxo = utxos.first().unwrap();
    assert_eq!(utxo.owner, owner_addr_gql);
    assert_eq!(utxo.value, expected_value_str);
    assert_eq!(utxo.token_type, expected_token_type);
    assert_eq!(utxo.intent_hash, expected_intent_hash);
    assert_eq!(utxo.output_index, 0);
    assert!(utxo.created_at_transaction.block.height >= 0);
    assert!(utxo.spent_at_transaction.is_none());

    // address with no UTXOs
    let empty_bech32m = to_bech32m(OWNER_ADDR_EMPTY.as_ref(), NETWORK_ID)?;
    let empty_addr = UnshieldedAddress(empty_bech32m);
    let resp_empty = send_graphql_query::<UnshieldedUtxos>(
        api_port,
        client,
        unshielded_utxos::Variables {
            address: empty_addr,
        },
    )
    .await?;
    let utxos_empty = resp_empty
        .data
        .ok_or_else(|| anyhow!("missing data in empty response"))?
        .unshielded_utxos;
    assert!(utxos_empty.is_empty());

    Ok(())
}

async fn test_transactions_filtered_by_address(
    api_port: u16,
    client: &Client,
) -> anyhow::Result<()> {
    let addr = to_bech32m(&const_hex::decode(UT_ADDR_1_HEX)?, NETWORK_ID)?;
    let unshielded_address = UnshieldedAddress(addr);
    let body = transactions_by_address::Variables {
        address: unshielded_address,
    };
    let resp = send_graphql_query::<TransactionsByAddress>(api_port, client, body).await?;
    let txs = resp
        .data
        .ok_or_else(|| anyhow!("missing data in TransactionsByAddress response"))?
        .transactions;

    assert!(!txs.is_empty());

    assert!(txs.iter().any(|t| {
        !t.unshielded_created_outputs.is_empty() || !t.unshielded_spent_outputs.is_empty()
    }));

    Ok(())
}

// async fn test_wallet_subscription(
//     viewing_key: &CommonViewingKey,
//     storage: &mut wallet_indexer::infra::storage::postgres::PostgresStorage,
//     publisher: &NatsPublisher,
// ) -> anyhow::Result<()> {
//     use wallet_subscription::*;
//     type WalletSyncEvent = WalletSubscriptionWallet;
//     type ViewingUpdate = WalletSubscriptionWalletOnViewingUpdate;
//     type ZswapChainStateUpdate = WalletSubscriptionWalletOnViewingUpdateUpdate;
//     // type MerkleTreeCollapsedUpdate =
//     //     WalletSubscriptionWalletOnViewingUpdateUpdateOnMerkleTreeCollapsedUpdate;
//     type RelevantTransaction =
// WalletSubscriptionWalletOnViewingUpdateUpdateOnRelevantTransaction;     type Transaction =
//     WalletSubscriptionWalletOnViewingUpdateUpdateOnRelevantTransactionTransaction;
//     type ProgressUpdate = WalletSubscriptionWalletOnProgressUpdate;
//
//     // 1) Connect via GraphQL WS
//     let mut client = GraphQLWSClient::connect_and_establish::<serde_json::Value>(8088).await?;
//
//     let wallet_subscription_query = WalletSubscription::build_query(Variables {
//         session_id: viewing_key.as_session_id().hex_encode(),
//         index: None,
//         send_progress_updates: None,
//     });
//
//     let variables = serde_json::to_value(&wallet_subscription_query.variables)?;
//     client
//         .send_subscription(
//             wallet_subscription_query.query,
//             variables,
//             "WalletSubscription",
//             "1",
//         )
//         .await?;
//
//     sleep(Duration::from_millis(50)).await;
//
//     let process_message = |message: &GraphQLWSMessage<ResponseData>| match message {
//         GraphQLWSMessage::Next { payload, .. } => payload.data.clone(),
//         _ => None,
//     };
//
//     let received_updates = client
//         .receive_messages::<_, ResponseData>(3, process_message)
//         .await?;
//
//     assert_eq!(received_updates.len(), 3);
//     let mut wallet_updates = vec![];
//     let mut progress_updates = vec![];
//
//     for update in received_updates {
//         match update {
//             ResponseData {
//                 wallet: WalletSyncEvent::ViewingUpdate(viewing_update),
//             } => wallet_updates.push(viewing_update),
//             ResponseData {
//                 wallet: WalletSyncEvent::ProgressUpdate(progress_update),
//             } => progress_updates.push(progress_update),
//         }
//     }
//
//     assert_eq!(wallet_updates.len(), 2);
//     assert_eq!(progress_updates.len(), 1);
//
//     let ViewingUpdate { index, ref update } = wallet_updates[0];
//     assert_eq!(index, 2);
//     assert_matches!(**update,
//         [
//             ZswapChainStateUpdate::RelevantTransaction(
//                 RelevantTransaction { transaction: Transaction { hash: ref tx_hash_1, .. },
// start: 0, end: 1 }             )
//         ] if *tx_hash_1 == TRANSACTION_1_HASH.hex_encode()
//     );
//     let ViewingUpdate { index, ref update } = wallet_updates[1];
//     assert_eq!(index, 4);
//     assert_matches!(**update,
//         [
//             ZswapChainStateUpdate::RelevantTransaction(
//                 RelevantTransaction { transaction: Transaction { hash: ref tx_hash_2, .. },
// start: 2, end: 3 }             )
//         ] if *tx_hash_2 == TRANSACTION_2_HASH.hex_encode()
//     );
//     let ProgressUpdate { synced, total } = progress_updates[0];
//     assert_eq!(synced, 3);
//     assert_eq!(total, 6);
//
//     let transaction_3 = wallet_indexer::domain::Transaction {
//         id: 3,
//         raw: RAW_TRANSACTION_3.to_owned(),
//     };
//     let transaction_4 = wallet_indexer::domain::Transaction {
//         id: 4,
//         raw: RAW_TRANSACTION_4.to_owned(),
//     };
//
// todo We will use tx-generator to send fund from SEED_0001 to SEED_0002
// https://shielded.atlassian.net/browse/PM-15324
// https://github.com/input-output-hk/midnight-substrate-prototype/pull/687
//
//
//     let received_updates = client
//         .receive_messages::<_, ResponseData>(2, process_message)
//         .await?;
//
//     assert_eq!(received_updates.len(), 2);
//     let ResponseData {
//         wallet: WalletSyncEvent::ViewingUpdate(ViewingUpdate { index, ref update }),
//     } = received_updates[0]
//     else {
//         unreachable!()
//     };
//     assert_eq!(index, 6);
//     assert_matches!(**update,
//         [
//             ZswapChainStateUpdate::RelevantTransaction(
//                 RelevantTransaction { transaction: Transaction { hash: ref tx_hash_3, .. },
// start: 4, end: 5 }             )
//         ] if *tx_hash_3 == TRANSACTION_3_HASH.hex_encode()
//     );
//     let ResponseData {
//         wallet: WalletSyncEvent::ViewingUpdate(ViewingUpdate { index, ref update }),
//     } = received_updates[1]
//     else {
//         unreachable!()
//     };
//     assert_eq!(index, 8);
//     assert_matches!(**update,
//         [
//             ZswapChainStateUpdate::RelevantTransaction(
//                 RelevantTransaction { transaction: Transaction { hash: ref tx_hash_4, .. },
// start: 6, end: 7 }             )
//         ] if *tx_hash_4 == TRANSACTION_4_HASH.hex_encode()
//     );
//
//     Ok(())
// }

/// Generic function for posting GraphQL queries
async fn send_graphql_query<T>(
    api_port: u16,
    client: &Client,
    variables: T::Variables,
) -> anyhow::Result<Response<T::ResponseData>>
where
    T: GraphQLQuery,
{
    let query = T::build_query(variables);
    client
        .post(format!("http://localhost:{api_port}/api/v1/graphql"))
        .json(&query)
        .send()
        .await
        .context("send request")?
        .json::<Response<T::ResponseData>>()
        .await
        .context("JSON deserialize response")
}

fn find_free_port() -> anyhow::Result<u16> {
    // Bind to port 0, which tells the OS to assign a free port.
    let listener = TcpListener::bind("127.0.0.1:0").context("bind to 127.0.0.1:0")?;
    let standalone_address = listener.local_addr().context("get standalone address")?;
    Ok(standalone_address.port())
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
struct BlockByHash;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
struct LatestBlock;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
struct BlockByHeight;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
struct TransactionByHash;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
struct Connect;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
struct Disconnect;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
struct BlocksSubscription;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
struct ContractsSubscription;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "../indexer-api/graphql/schema-v1.graphql",
    query_path = "./queries.graphql",
    response_derives = "Debug,Clone,PartialEq"
)]
struct WalletSubscription;
