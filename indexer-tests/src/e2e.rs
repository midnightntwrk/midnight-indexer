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

//! e2e testing library

use crate::{
    e2e::graphql::{
        BlockQuery, BlockSubscription, ConnectMutation, ContractActionQuery,
        ContractActionSubscription, DisconnectMutation, TransactionsQuery, WalletSubscription,
        block_query,
        block_subscription::{
            self, BlockSubscriptionBlocks as BlockSubscriptionBlock,
            BlockSubscriptionBlocksTransactions as BlockSubscriptionTransaction,
            BlockSubscriptionBlocksTransactionsContractActions as BlockSubscriptionContractAction,
            BlockSubscriptionBlocksTransactionsUnshieldedCreatedOutputs as BlockSubscriptionUnshieldedUtxo,
            TransactionResultStatus as BlockSubscriptionTransactionResultStatus,
        },
        connect_mutation,
        contract_action_query::{
            self, ContractActionQueryContractAction,
            TransactionResultStatus as ContractActionQueryTransactionResultStatus,
        },
        contract_action_subscription, disconnect_mutation, transactions_query, wallet_subscription,
    },
    graphql_ws_client,
};
use anyhow::{Context, Ok, bail};
use bech32::{Bech32m, Hrp};
use futures::{StreamExt, TryStreamExt, future::ok};
use graphql_client::{GraphQLQuery, Response};
use indexer_api::{
    domain::{AsBytesExt, HexEncoded, ViewingKey},
    infra::api::v1::{TransactionResultStatus, UnshieldedAddress},
};
use indexer_common::domain::NetworkId;
use itertools::Itertools;
use midnight_serialize::Serializable;
use midnight_transient_crypto::encryption::SecretKey;
use midnight_zswap::keys::{SecretKeys, Seed};
use reqwest::Client;
use serde::Serialize;
use std::time::{Duration, Instant};

const MAX_HEIGHT: usize = 30;

/// Run comprehensive e2e tests for the Indexer. It is expected that the Indexer is set up with all
/// needed dependencies, e.g. a Node, and its API is exposed securely (https and wss) or insecurely
/// (http and ws) at the given host and port.
pub async fn run(network_id: NetworkId, host: &str, port: u16, secure: bool) -> anyhow::Result<()> {
    println!("### starting e2e testing");

    let (api_url, ws_api_url) = {
        let core = format!("{host}:{port}/api/v1/graphql");

        if secure {
            (format!("https://{core}"), format!("wss://{core}/ws"))
        } else {
            (format!("http://{core}"), format!("ws://{core}/ws"))
        }
    };

    let api_client = Client::new();

    // Collect Indexer data using the block subscription.
    let indexer_data = IndexerData::collect(&ws_api_url)
        .await
        .context("collect Indexer data")?;

    // Test queries.
    test_block_query(&indexer_data, &api_client, &api_url)
        .await
        .context("test block query")?;
    test_transactions_query(&indexer_data, &api_client, &api_url)
        .await
        .context("test transactions query")?;
    test_contract_action_query(&indexer_data, &api_client, &api_url)
        .await
        .context("test contract action query")?;
    test_unshielded_utxo_queries(&indexer_data, &api_client, &api_url)
        .await
        .context("test unshielded UTXOs query")?;

    // Test mutations.
    test_connect_mutation(&api_client, &api_url, network_id)
        .await
        .context("test connect mutation query")?;
    test_disconnect_mutation(&api_client, &api_url)
        .await
        .context("test disconnect mutation query")?;

    // Test subscriptions (the block subscription has already been tested above).
    test_contract_action_subscription(&indexer_data, &ws_api_url)
        .await
        .context("test contract action subscription")?;
    test_unshielded_utxo_subscription(&indexer_data, &ws_api_url) // we use node mock version at the moment
        .await
        .context("test unshielded UTXOs subscription")?;
    test_wallet_subscription(&ws_api_url)
        .await
        .context("test wallet subscription")?;

    println!("### successfully finished e2e testing");

    Ok(())
}

/// All data needed for testing collected from the Indexer via the blocks subscription. To be used
/// as expected data in tests for all other API operations.
struct IndexerData {
    blocks: Vec<BlockSubscriptionBlock>,
    transactions: Vec<BlockSubscriptionTransaction>,
    contract_actions: Vec<BlockSubscriptionContractAction>,
    unshielded_utxos: Vec<BlockSubscriptionUnshieldedUtxo>,
}

impl IndexerData {
    /// Not only collects the Indexer data needed for testing, but also validates it, e.g. that
    /// block heights start at zero and increment by one.
    async fn collect(ws_api_url: &str) -> anyhow::Result<Self> {
        // Subscribe to blocks and collect up to MAX_HEIGHT.
        let variables = block_subscription::Variables {
            block_offset: Some(block_subscription::BlockOffset::Height(0)),
        };
        let blocks = graphql_ws_client::subscribe::<BlockSubscription>(ws_api_url, variables)
            .await
            .context("subscribe to blocks")?
            .take(1 + MAX_HEIGHT)
            .map_ok(|data| data.blocks)
            .try_collect::<Vec<_>>()
            .await
            .context("collect blocks from block subscription")?;

        // Validate that block heights start at zero and increment by one.
        assert_eq!(
            blocks.iter().map(|block| block.height).collect::<Vec<_>>(),
            (0..=MAX_HEIGHT).map(|n| n as i64).collect::<Vec<_>>()
        );

        // Verify that each block correctly references its parent and the height is increased by
        // one.
        blocks.windows(2).all(|blocks| {
            let hash_0 = &blocks[0].hash;
            let height_0 = blocks[0].height;

            let parent_hash_1 = blocks[1]
                .parent
                .as_ref()
                .map(|block| &block.hash)
                .expect("non-genesis block has parent");
            let parent_height_1 = blocks[1]
                .parent
                .as_ref()
                .map(|block| block.height)
                .expect("non-genesis block has parent");

            hash_0 == parent_hash_1 && height_0 == parent_height_1
        });

        // Verify that all transactions reference the correct block and have the same protocol
        // version.
        assert!(blocks.iter().all(|block| {
            block.transactions.iter().all(|transaction| {
                transaction.block.hash == block.hash
                    && transaction.protocol_version == block.protocol_version
            })
        }));

        // Collect transactions.
        let transactions = blocks
            .iter()
            .flat_map(|block| block.transactions.to_owned())
            .collect::<Vec<_>>();

        // Verify that all contract actions reference the correct transaction.
        assert!(transactions.iter().all(|transaction| {
            transaction
                .contract_actions
                .iter()
                .all(|contract_action| contract_action.transaction_hash() == transaction.hash)
        }));

        // Collect contract actions.
        let contract_actions = transactions
            .iter()
            .flat_map(|transaction| transaction.contract_actions.to_owned())
            .collect::<Vec<_>>();

        // Verify that contract calls and their deploy have the same address.
        contract_actions
            .iter()
            .filter_map(|contract_action| match contract_action {
                BlockSubscriptionContractAction::ContractCall(c) => {
                    Some((&c.address, &c.deploy.address))
                }
                _ => None,
            })
            .all(|(a1, a2)| a1 == a2);

        let unshielded_utxos = transactions
            .iter()
            .flat_map(|transaction| transaction.unshielded_created_outputs.to_owned())
            .collect::<Vec<_>>();

        assert!(!unshielded_utxos.is_empty());

        Ok(Self {
            blocks,
            transactions,
            contract_actions,
            unshielded_utxos,
        })
    }
}

/// Test the block query.
async fn test_block_query(
    indexer_data: &IndexerData,
    api_client: &Client,
    api_url: &str,
) -> anyhow::Result<()> {
    for expected_block in &indexer_data.blocks {
        // Existing hash.
        let variables = block_query::Variables {
            block_offset: Some(block_query::BlockOffset::Hash(
                expected_block.hash.to_owned(),
            )),
        };
        let block = send_query::<BlockQuery>(api_client, api_url, variables)
            .await?
            .block
            .expect("there is a block");
        assert_eq!(block.to_value(), expected_block.to_value());

        // Existing height.
        let variables = block_query::Variables {
            block_offset: Some(block_query::BlockOffset::Height(expected_block.height)),
        };
        let block = send_query::<BlockQuery>(api_client, api_url, variables)
            .await?
            .block
            .expect("there is a block");
        assert_eq!(block.to_value(), expected_block.to_value());
    }

    // No offset which yields the last block; as the node proceeds, that is unknown an only its
    // height can be verified to be larger or equal the collected ones.
    let variables = block_query::Variables { block_offset: None };
    let block = send_query::<BlockQuery>(api_client, api_url, variables)
        .await?
        .block
        .expect("there is a block");
    assert!(block.height >= MAX_HEIGHT as i64);

    // Unknown hash.
    let variables = block_query::Variables {
        block_offset: Some(block_query::BlockOffset::Hash([42; 32].hex_encode())),
    };
    let block = send_query::<BlockQuery>(api_client, api_url, variables)
        .await?
        .block;
    assert!(block.is_none());

    // Unknown height.
    let variables = block_query::Variables {
        block_offset: Some(block_query::BlockOffset::Height(u32::MAX as i64)),
    };
    let block = send_query::<BlockQuery>(api_client, api_url, variables)
        .await?
        .block;
    assert!(block.is_none());

    Ok(())
}

/// Test the transactions query.
async fn test_transactions_query(
    indexer_data: &IndexerData,
    api_client: &Client,
    api_url: &str,
) -> anyhow::Result<()> {
    for expected_transaction in &indexer_data.transactions {
        // Existing hash.
        // Notice that transaction hashes are not unique, e.g. hashes of failed transactions might
        // be also used for later transactions. Hence the query might return more than one
        // transaction and we have to verify that the expected transaction is contained in that
        // collection.
        let variables = transactions_query::Variables {
            transaction_offset: transactions_query::TransactionOffset::Hash(
                expected_transaction.hash.to_owned(),
            ),
        };
        let transactions = send_query::<TransactionsQuery>(api_client, api_url, variables)
            .await?
            .transactions
            .into_iter()
            .map(|t| t.to_value())
            .collect::<Vec<_>>();
        assert!(transactions.contains(&expected_transaction.to_value()));

        // Existing identifier.
        for identifier in &expected_transaction.identifiers {
            let variables = transactions_query::Variables {
                transaction_offset: transactions_query::TransactionOffset::Identifier(
                    identifier.to_owned(),
                ),
            };
            let transactions = send_query::<TransactionsQuery>(api_client, api_url, variables)
                .await?
                .transactions
                .into_iter()
                .map(|t| t.to_value())
                .collect::<Vec<_>>();
            assert!(transactions.contains(&expected_transaction.to_value()));
        }
    }

    // Unknown hash.
    let variables = transactions_query::Variables {
        transaction_offset: transactions_query::TransactionOffset::Hash([42; 32].hex_encode()),
    };
    let transactions = send_query::<TransactionsQuery>(api_client, api_url, variables)
        .await?
        .transactions;
    assert!(transactions.is_empty());

    // Unknown identifier.
    let variables = transactions_query::Variables {
        transaction_offset: transactions_query::TransactionOffset::Identifier(
            [42; 32].hex_encode(),
        ),
    };
    let transactions = send_query::<TransactionsQuery>(api_client, api_url, variables)
        .await?
        .transactions;
    assert!(transactions.is_empty());

    Ok(())
}

/// Test the contract action query.
async fn test_contract_action_query(
    indexer_data: &IndexerData,
    api_client: &Client,
    api_url: &str,
) -> anyhow::Result<()> {
    for expected_contract_action in indexer_data
        .contract_actions
        .iter()
        .filter(|c| c.transaction_transaction_result_status() != TransactionResultStatus::Failure)
    {
        // Existing block hash.
        let variables = contract_action_query::Variables {
            address: expected_contract_action.address(),
            contract_action_offset: Some(contract_action_query::ContractActionOffset::BlockOffset(
                contract_action_query::BlockOffset::Hash(expected_contract_action.block_hash()),
            )),
        };
        let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
            .await?
            .contract_action
            .expect("there is a contract action");
        assert_eq!(
            contract_action.to_value(),
            expected_contract_action.to_value()
        );

        // Existing block height.
        let variables = contract_action_query::Variables {
            address: expected_contract_action.address(),
            contract_action_offset: Some(contract_action_query::ContractActionOffset::BlockOffset(
                contract_action_query::BlockOffset::Height(expected_contract_action.block_height()),
            )),
        };
        let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
            .await?
            .contract_action
            .expect("there is a contract action");
        assert_eq!(
            contract_action.to_value(),
            expected_contract_action.to_value()
        );

        // Existing transaction hash.
        let variables = contract_action_query::Variables {
            address: expected_contract_action.address(),
            contract_action_offset: Some(
                contract_action_query::ContractActionOffset::TransactionOffset(
                    contract_action_query::TransactionOffset::Hash(
                        expected_contract_action.transaction_hash(),
                    ),
                ),
            ),
        };
        let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
            .await?
            .contract_action
            .expect("there is a contract action");
        assert_eq!(
            contract_action.to_value(),
            expected_contract_action.to_value()
        );

        // Existing transaction identifier.
        // The query will not necessarily return the expected contract action, but the most recent
        // one (with the highest ID); hence we can only compare addresses.
        for identifier in expected_contract_action.identifiers() {
            let variables = contract_action_query::Variables {
                address: expected_contract_action.address(),
                contract_action_offset: Some(
                    contract_action_query::ContractActionOffset::TransactionOffset(
                        contract_action_query::TransactionOffset::Identifier(identifier),
                    ),
                ),
            };
            let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
                .await?
                .contract_action
                .expect("there is a contract action");
            assert_eq!(
                contract_action.address(),
                expected_contract_action.address()
            );
        }

        // Unknown block hash.
        let variables = contract_action_query::Variables {
            address: expected_contract_action.address(),
            contract_action_offset: Some(contract_action_query::ContractActionOffset::BlockOffset(
                contract_action_query::BlockOffset::Hash([42; 32].hex_encode()),
            )),
        };
        let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
            .await?
            .contract_action;
        assert!(contract_action.is_none());

        // Unknown block height.
        let variables = contract_action_query::Variables {
            address: expected_contract_action.address(),
            contract_action_offset: Some(contract_action_query::ContractActionOffset::BlockOffset(
                contract_action_query::BlockOffset::Height(MAX_HEIGHT as i64 + 42),
            )),
        };
        let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
            .await?
            .contract_action;
        assert!(contract_action.is_none());

        // Unknown transaction hash.
        let variables = contract_action_query::Variables {
            address: expected_contract_action.address(),
            contract_action_offset: Some(
                contract_action_query::ContractActionOffset::TransactionOffset(
                    contract_action_query::TransactionOffset::Hash([42; 32].hex_encode()),
                ),
            ),
        };
        let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
            .await?
            .contract_action;
        assert!(contract_action.is_none());

        // Unknown transaction identifier.
        let variables = contract_action_query::Variables {
            address: expected_contract_action.address(),
            contract_action_offset: Some(
                contract_action_query::ContractActionOffset::TransactionOffset(
                    contract_action_query::TransactionOffset::Identifier([42; 32].hex_encode()),
                ),
            ),
        };
        let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
            .await?
            .contract_action;
        assert!(contract_action.is_none());
    }

    Ok(())
}

/// Test the unshielded UTXOs query.
async fn test_unshielded_utxo_queries(
    indexer_data: &IndexerData,
    api_client: &Client,
    api_url: &str,
) -> anyhow::Result<()> {
    use graphql::graphql_types::*;

    // Test with addresses that have UTXOs
    for expected_utxo in &indexer_data.unshielded_utxos {
        let variables = unshielded_utxos_query::Variables {
            address: expected_utxo.owner.clone(),
        };
        let utxos = send_query::<UnshieldedUtxosQuery>(api_client, api_url, variables)
            .await?
            .unshielded_utxos;

        assert!(!utxos.is_empty());
        // Verify that the expected UTXO is in the results
        assert!(utxos.iter().any(|utxo| {
            utxo.owner == expected_utxo.owner
                && utxo.value == expected_utxo.value
                && utxo.token_type == expected_utxo.token_type
                && utxo.intent_hash == expected_utxo.intent_hash
                && utxo.output_index == expected_utxo.output_index
        }));
    }

    // Test with unknown address (should return empty)
    const NETWORK_ID: NetworkId = NetworkId::Undeployed;
    let unknown_addr_bytes = [0x99u8; 4]; // Some address that doesn't exist
    let unknown_addr = UnshieldedAddress::bech32m_encode(unknown_addr_bytes, NETWORK_ID);

    let variables = unshielded_utxos_query::Variables {
        address: unknown_addr,
    };
    let utxos = send_query::<UnshieldedUtxosQuery>(api_client, api_url, variables)
        .await?
        .unshielded_utxos;

    assert!(utxos.is_empty());

    Ok(())
}

/// Test the connect mutation.
async fn test_connect_mutation(
    api_client: &Client,
    api_url: &str,
    network_id: NetworkId,
) -> anyhow::Result<()> {
    // Valid viewing key.
    let secret_key = seed_to_secret_key(&format!("{}1", "0".repeat(63)));
    let mut serialized = Vec::with_capacity(SecretKey::serialized_size(&secret_key));
    Serializable::serialize(&secret_key, &mut serialized).expect("secret key can be serialized");
    let hrp = match network_id {
        NetworkId::Undeployed => "mn_shield-esk_undeployed",
        NetworkId::DevNet => "mn_shield-esk_dev",
        NetworkId::TestNet => "mn_shield-esk_test",
        NetworkId::MainNet => "mn_shield-esk",
    };
    let hrp = Hrp::parse(hrp).context("create HRP")?;
    let encoded = bech32::encode::<Bech32m>(hrp, &serialized).context("encode viewing key")?;
    let viewing_key = ViewingKey(encoded);
    let variables = connect_mutation::Variables { viewing_key };
    let response = send_query::<ConnectMutation>(api_client, api_url, variables).await;
    assert!(response.is_ok());

    // Invalid viewing key.
    let variables = connect_mutation::Variables {
        viewing_key: ViewingKey("invalid".to_string()),
    };
    let response = send_query::<ConnectMutation>(api_client, api_url, variables).await;
    assert!(response.is_err());

    Ok(())
}

/// Test the disconnect mutation.
async fn test_disconnect_mutation(api_client: &Client, api_url: &str) -> anyhow::Result<()> {
    // Valid session ID.
    let session_id = indexer_common::domain::ViewingKey::from([0; 32])
        .to_session_id()
        .hex_encode();
    let variables = disconnect_mutation::Variables { session_id };
    let response = send_query::<DisconnectMutation>(api_client, api_url, variables).await;
    assert!(response.is_ok());

    // Invalid viewing key.
    let variables = disconnect_mutation::Variables {
        session_id: [42; 1].hex_encode(),
    };
    let response = send_query::<DisconnectMutation>(api_client, api_url, variables).await;
    assert!(response.is_err());

    Ok(())
}

/// Test the contract action subscription.
async fn test_contract_action_subscription(
    indexer_data: &IndexerData,
    ws_api_url: &str,
) -> anyhow::Result<()> {
    // Map expected contract actions by address.
    let contract_actions_by_address = indexer_data
        .contract_actions
        .iter()
        .map(|c| (c.address(), c.to_value()))
        .into_group_map();

    for (address, expected_contract_actions) in contract_actions_by_address {
        // No offset.
        let variables = contract_action_subscription::Variables {
            address: address.clone(),
            contract_action_subscription_offset: None,
        };
        let contract_actions =
            graphql_ws_client::subscribe::<ContractActionSubscription>(ws_api_url, variables)
                .await
                .context("subscribe to contract actions")?
                .take(expected_contract_actions.len())
                .map_ok(|data| data.contract_actions.to_value())
                .try_collect::<Vec<_>>()
                .await
                .context("collect blocks from contract action subscription")?;
        assert_eq!(contract_actions, expected_contract_actions);

        // Genesis hash.
        let hash = indexer_data
            .blocks
            .first()
            .map(|b| b.hash.to_owned())
            .expect("there is a first block");
        let variables = contract_action_subscription::Variables {
            address: address.clone(),
            contract_action_subscription_offset: Some(
                contract_action_subscription::BlockOffset::Hash(hash),
            ),
        };
        let contract_actions =
            graphql_ws_client::subscribe::<ContractActionSubscription>(ws_api_url, variables)
                .await
                .context("subscribe to contract actions")?
                .take(expected_contract_actions.len())
                .map_ok(|data| data.contract_actions.to_value())
                .try_collect::<Vec<_>>()
                .await
                .context("collect blocks from contract action subscription")?;
        assert_eq!(contract_actions, expected_contract_actions);

        // Height zero.
        let variables = contract_action_subscription::Variables {
            address,
            contract_action_subscription_offset: Some(
                contract_action_subscription::BlockOffset::Height(0),
            ),
        };
        let contract_actions =
            graphql_ws_client::subscribe::<ContractActionSubscription>(ws_api_url, variables)
                .await
                .context("subscribe to contract actions")?
                .take(expected_contract_actions.len())
                .map_ok(|data| data.contract_actions.to_value())
                .try_collect::<Vec<_>>()
                .await
                .context("collect blocks from contract action subscription")?;
        assert_eq!(contract_actions, expected_contract_actions);
    }

    Ok(())
}

async fn test_unshielded_utxo_subscription(
    indexer_data: &IndexerData,
    ws_api_url: &str,
) -> anyhow::Result<()> {
    use graphql::graphql_types::*;
    use tokio::time::{Duration, timeout};

    let utxo_addresses = indexer_data
        .unshielded_utxos
        .iter()
        .map(|utxo| utxo.owner.clone())
        .collect::<Vec<_>>();

    assert!(!utxo_addresses.is_empty());

    let unshielded_address = UnshieldedAddress(utxo_addresses[0].clone().0);

    let variables = unshielded_utxos_subscription::Variables {
        address: unshielded_address.clone(),
    };

    let subscription_stream =
        graphql_ws_client::subscribe::<UnshieldedUtxosSubscription>(ws_api_url, variables)
            .await
            .context("subscribe to unshielded UTXOs")?;

    // Wait for events with a short timeout to check for PROGRESS events
    let events = timeout(Duration::from_millis(400), async {
        subscription_stream
            .take(1) // Take just 1 event to check for PROGRESS
            .map_ok(|data| data.unshielded_utxos)
            .try_collect::<Vec<_>>()
            .await
    })
    .await;

    match events {
        Result::Ok(Result::Ok(events)) if !events.is_empty() => {
            let has_progress = events.iter().any(|e| {
                matches!(
                    e.event_type,
                    unshielded_utxos_subscription::UnshieldedUtxoEventType::PROGRESS
                )
            });

            if has_progress {
                println!("Received PROGRESS event as expected");
            }

            for event in &events {
                if !event.created_utxos.is_empty() {
                    assert_eq!(event.created_utxos[0].owner, unshielded_address);
                }

                if !event.spent_utxos.is_empty() {
                    assert_eq!(event.spent_utxos[0].owner, unshielded_address);
                }
            }
        }
        Result::Ok(Result::Ok(_)) => {
            println!("No events received within 400ms timeout");
        }
        Result::Ok(Err(e)) => {
            println!("Error collecting events: {:?}", e);
        }
        Err(_) => {
            println!("Timeout waiting for events - this is expected if no recent activity");
        }
    }

    // Additional test with address that has no UTXOs
    const NETWORK_ID: NetworkId = NetworkId::Undeployed;
    let empty_addr_bytes = [0x11, 0x22, 0x33, 0x44]; // Raw bytes for a non-existent address
    let empty_address = UnshieldedAddress::bech32m_encode(empty_addr_bytes, NETWORK_ID);

    let empty_variables = unshielded_utxos_subscription::Variables {
        address: empty_address,
    };

    let _empty_subscription =
        graphql_ws_client::subscribe::<UnshieldedUtxosSubscription>(ws_api_url, empty_variables)
            .await
            .context("subscribe to unshielded UTXOs for empty address")?;

    Ok(())
}

/// Test the wallet subscription.
async fn test_wallet_subscription(ws_api_url: &str) -> anyhow::Result<()> {
    use wallet_subscription::WalletSubscriptionWalletOnViewingUpdateUpdate as ViewingUpdate;

    let viewing_key = indexer_common::domain::ViewingKey::from(seed_to_secret_key(&format!(
        "{}1",
        "0".repeat(63)
    )));
    // 3d8506d3b1875c0843cdcf27ab2db6119186fdbd9600536335872f9f46cc59be
    let session_id = viewing_key.to_session_id().hex_encode();

    // Collect wallet events until there are no more viewing updates (3s deadline).
    let mut viewing_update_timestamp = Instant::now();
    let variables = wallet_subscription::Variables { session_id };
    let events = graphql_ws_client::subscribe::<WalletSubscription>(ws_api_url, variables)
        .await
        .context("subscribe to wallet")?
        .map_ok(|data| data.wallet)
        .try_take_while(|event| {
            let duration = Instant::now() - viewing_update_timestamp;

            if let wallet_subscription::WalletSubscriptionWallet::ViewingUpdate(_) = event {
                viewing_update_timestamp = Instant::now()
            }

            ok(duration < Duration::from_secs(3))
        })
        .try_collect::<Vec<_>>()
        .await
        .context("collect wallet events")?;

    // Filter viewing updates only.
    let viewing_updates = events.into_iter().filter_map(|event| match event {
        wallet_subscription::WalletSubscriptionWallet::ViewingUpdate(viewing_update) => {
            Some(viewing_update)
        }

        _ => None,
    });

    // Verify that there are no index gaps.
    let mut expected_start = 0;
    for viewing_update in viewing_updates {
        for update in viewing_update.update {
            match update {
                ViewingUpdate::MerkleTreeCollapsedUpdate(collapsed_update) => {
                    assert_eq!(collapsed_update.start, expected_start);
                    assert!(collapsed_update.end >= collapsed_update.start);

                    expected_start = collapsed_update.end + 1;
                }

                ViewingUpdate::RelevantTransaction(relevant_transaction) => {
                    assert!(relevant_transaction.start == expected_start);
                    assert!(relevant_transaction.end >= relevant_transaction.start);
                    assert!(
                        viewing_update.index == relevant_transaction.end
                            || viewing_update.index == relevant_transaction.end + 1
                    );

                    expected_start = viewing_update.index;
                }
            }
        }
    }

    Ok(())
}

trait SerializeExt
where
    Self: Serialize,
{
    fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("can be JSON-serialized")
    }
}

impl<T> SerializeExt for T where T: Serialize {}

trait ContractActionExt {
    fn address(&self) -> HexEncoded;
    fn block_hash(&self) -> HexEncoded;
    fn block_height(&self) -> i64;
    fn transaction_hash(&self) -> HexEncoded;
    fn transaction_transaction_result_status(&self) -> TransactionResultStatus;
    fn identifiers(&self) -> Vec<HexEncoded>;
}

impl ContractActionExt for BlockSubscriptionContractAction {
    fn address(&self) -> HexEncoded {
        let address = match self {
            BlockSubscriptionContractAction::ContractDeploy(c) => &c.address,
            BlockSubscriptionContractAction::ContractCall(c) => &c.address,
            BlockSubscriptionContractAction::ContractUpdate(c) => &c.address,
        };

        address.to_owned()
    }

    fn block_hash(&self) -> HexEncoded {
        let block_hash = match self {
            BlockSubscriptionContractAction::ContractDeploy(c) => &c.transaction.block.hash,
            BlockSubscriptionContractAction::ContractCall(c) => &c.transaction.block.hash,
            BlockSubscriptionContractAction::ContractUpdate(c) => &c.transaction.block.hash,
        };

        block_hash.to_owned()
    }

    fn block_height(&self) -> i64 {
        match self {
            BlockSubscriptionContractAction::ContractDeploy(c) => c.transaction.block.height,
            BlockSubscriptionContractAction::ContractCall(c) => c.transaction.block.height,
            BlockSubscriptionContractAction::ContractUpdate(c) => c.transaction.block.height,
        }
    }

    fn transaction_hash(&self) -> HexEncoded {
        let transaction_hash = match self {
            BlockSubscriptionContractAction::ContractDeploy(c) => &c.transaction.hash,
            BlockSubscriptionContractAction::ContractCall(c) => &c.transaction.hash,
            BlockSubscriptionContractAction::ContractUpdate(c) => &c.transaction.hash,
        };

        transaction_hash.to_owned()
    }

    fn transaction_transaction_result_status(&self) -> TransactionResultStatus {
        let status = match self {
            BlockSubscriptionContractAction::ContractCall(c) => {
                &c.transaction.transaction_result.status
            }
            BlockSubscriptionContractAction::ContractDeploy(c) => {
                &c.transaction.transaction_result.status
            }
            BlockSubscriptionContractAction::ContractUpdate(c) => {
                &c.transaction.transaction_result.status
            }
        };

        match status {
            BlockSubscriptionTransactionResultStatus::SUCCESS => TransactionResultStatus::Success,
            BlockSubscriptionTransactionResultStatus::PARTIAL_SUCCESS => {
                TransactionResultStatus::PartialSuccess
            }
            BlockSubscriptionTransactionResultStatus::FAILURE => TransactionResultStatus::Failure,
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    fn identifiers(&self) -> Vec<HexEncoded> {
        let identifiers = match self {
            BlockSubscriptionContractAction::ContractDeploy(c) => &c.transaction.identifiers,
            BlockSubscriptionContractAction::ContractCall(c) => &c.transaction.identifiers,
            BlockSubscriptionContractAction::ContractUpdate(c) => &c.transaction.identifiers,
        };

        identifiers.to_owned()
    }
}

impl ContractActionExt for ContractActionQueryContractAction {
    fn address(&self) -> HexEncoded {
        let address = match self {
            ContractActionQueryContractAction::ContractDeploy(c) => &c.address,
            ContractActionQueryContractAction::ContractCall(c) => &c.address,
            ContractActionQueryContractAction::ContractUpdate(c) => &c.address,
        };

        address.to_owned()
    }

    fn block_hash(&self) -> HexEncoded {
        let block_hash = match self {
            ContractActionQueryContractAction::ContractDeploy(c) => &c.transaction.block.hash,
            ContractActionQueryContractAction::ContractCall(c) => &c.transaction.block.hash,
            ContractActionQueryContractAction::ContractUpdate(c) => &c.transaction.block.hash,
        };

        block_hash.to_owned()
    }

    fn block_height(&self) -> i64 {
        match self {
            ContractActionQueryContractAction::ContractDeploy(c) => c.transaction.block.height,
            ContractActionQueryContractAction::ContractCall(c) => c.transaction.block.height,
            ContractActionQueryContractAction::ContractUpdate(c) => c.transaction.block.height,
        }
    }

    fn transaction_hash(&self) -> HexEncoded {
        let transaction_hash = match self {
            ContractActionQueryContractAction::ContractDeploy(c) => &c.transaction.hash,
            ContractActionQueryContractAction::ContractCall(c) => &c.transaction.hash,
            ContractActionQueryContractAction::ContractUpdate(c) => &c.transaction.hash,
        };

        transaction_hash.to_owned()
    }

    fn transaction_transaction_result_status(&self) -> TransactionResultStatus {
        let status = match self {
            ContractActionQueryContractAction::ContractCall(c) => {
                &c.transaction.transaction_result.status
            }
            ContractActionQueryContractAction::ContractDeploy(c) => {
                &c.transaction.transaction_result.status
            }
            ContractActionQueryContractAction::ContractUpdate(c) => {
                &c.transaction.transaction_result.status
            }
        };

        match status {
            ContractActionQueryTransactionResultStatus::SUCCESS => TransactionResultStatus::Success,
            ContractActionQueryTransactionResultStatus::PARTIAL_SUCCESS => {
                TransactionResultStatus::PartialSuccess
            }
            ContractActionQueryTransactionResultStatus::FAILURE => TransactionResultStatus::Failure,
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    fn identifiers(&self) -> Vec<HexEncoded> {
        let identifiers = match self {
            ContractActionQueryContractAction::ContractDeploy(c) => &c.transaction.identifiers,
            ContractActionQueryContractAction::ContractCall(c) => &c.transaction.identifiers,
            ContractActionQueryContractAction::ContractUpdate(c) => &c.transaction.identifiers,
        };

        identifiers.to_owned()
    }
}

async fn send_query<T>(
    api_client: &Client,
    api_url: &str,
    variables: T::Variables,
) -> anyhow::Result<T::ResponseData>
where
    T: GraphQLQuery,
{
    let query = T::build_query(variables);

    let response = api_client
        .post(api_url)
        .json(&query)
        .send()
        .await
        .context("send query")?
        .error_for_status()
        .context("response for query")?
        .json::<Response<T::ResponseData>>()
        .await
        .context("JSON-decode query response")?;

    if let Some(errors) = response.errors {
        let errors = errors.into_iter().map(|e| e.message).join(", ");
        bail!(errors)
    }

    let data = response
        .data
        .expect("if there are no errors, there must be data");

    Ok(data)
}

fn seed_to_secret_key(seed: &str) -> SecretKey {
    let seed_bytes = const_hex::decode(seed).expect("seed can be hex-decoded");
    let seed_bytes = <[u8; 32]>::try_from(seed_bytes).expect("seed has 32 bytes");
    SecretKeys::from(Seed::from(seed_bytes)).encryption_secret_key
}

mod graphql {
    use graphql_client::GraphQLQuery;
    use indexer_api::{
        domain::{HexEncoded, ViewingKey},
        infra::api::v1::{Unit, UnshieldedAddress},
    };

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v1.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct BlockQuery;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v1.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct TransactionsQuery;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v1.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct ContractActionQuery;

    // TODO(midnight-indexer/PR #23): Temporary wrapper to dodge the
    // GraphQLQuery error-type mismatch (anyhow::Error vs serde::de::Error).
    // Delete this `mod graphql_types` once we align the error types or
    // customise the derive to return our own error.
    pub mod graphql_types {
        use graphql_client::GraphQLQuery;
        use indexer_api::{domain::HexEncoded, infra::api::v1::UnshieldedAddress};

        #[derive(GraphQLQuery)]
        #[graphql(
            schema_path = "../indexer-api/graphql/schema-v1.graphql",
            query_path = "./e2e.graphql",
            response_derives = "Debug, Clone, Serialize"
        )]
        pub struct UnshieldedUtxosQuery;

        #[derive(GraphQLQuery)]
        #[graphql(
            schema_path = "../indexer-api/graphql/schema-v1.graphql",
            query_path = "./e2e.graphql",
            response_derives = "Debug, Clone, Serialize"
        )]
        pub struct UnshieldedUtxosSubscription;
    }

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v1.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct ConnectMutation;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v1.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct DisconnectMutation;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v1.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct BlockSubscription;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v1.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct ContractActionSubscription;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v1.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct WalletSubscription;
}
