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
        ContractActionSubscription, DisconnectMutation, DustGenerationStatusQuery,
        DustLedgerEventsSubscription, ShieldedTransactionsSubscription, TransactionsQuery,
        UnshieldedTransactionsSubscription, ZswapLedgerEventsSubscription, block_query,
        block_subscription::{
            self, BlockSubscriptionBlocks, BlockSubscriptionBlocksTransactions,
            BlockSubscriptionBlocksTransactionsContractActions,
            BlockSubscriptionBlocksTransactionsDustLedgerEvents,
            BlockSubscriptionBlocksTransactionsOnRegularTransaction,
            BlockSubscriptionBlocksTransactionsUnshieldedCreatedOutputs,
            BlockSubscriptionBlocksTransactionsZswapLedgerEvents,
        },
        connect_mutation,
        contract_action_query::{self},
        contract_action_subscription, disconnect_mutation, dust_generation_status_query,
        dust_ledger_events_subscription, shielded_transactions_subscription, transactions_query,
        unshielded_transactions_subscription, zswap_ledger_events_subscription,
    },
    graphql_ws_client,
};
use anyhow::{Context, bail};
use futures::{StreamExt, TryStreamExt, future::ok};
use graphql_client::{GraphQLQuery, Response};
use indexer_api::infra::api::v3::{AsBytesExt, viewing_key::ViewingKey};
use indexer_common::domain::{NetworkId, PROTOCOL_VERSION_000_017_000};
use itertools::Itertools;
use reqwest::Client;
use serde::Serialize;
use shielded_transactions_subscription::ShieldedTransactionsSubscriptionShieldedTransactions as ShieldedTransactions;
use std::{future::ready, time::Duration};
use unshielded_transactions_subscription::UnshieldedTransactionsSubscriptionUnshieldedTransactions as UnshieldedTransactions;

const MAX_HEIGHT: usize = 30;

/// Run comprehensive e2e tests for the Indexer. It is expected that the Indexer is set up with all
/// needed dependencies, e.g. a Node, and its API is exposed securely (https and wss) or insecurely
/// (http and ws) at the given host and port.
///
/// Tests include validation of transaction fee metadata (paid_fee, estimated_fee) and segment
/// results.
pub async fn run(network_id: NetworkId, host: &str, port: u16, secure: bool) -> anyhow::Result<()> {
    println!("Starting e2e testing");

    let (api_url, ws_api_url) = {
        let core = format!("{host}:{port}/api/v3/graphql");

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
    test_dust_generation_status_query(&api_client, &api_url)
        .await
        .context("test dust generation status query")?;

    // Test mutations.
    test_connect_mutation(&api_client, &api_url, &network_id)
        .await
        .context("test connect mutation query")?;
    test_disconnect_mutation(&api_client, &api_url)
        .await
        .context("test disconnect mutation query")?;

    // Test subscriptions (the block subscription has already been tested above).
    test_contract_actions_subscription(&indexer_data, &ws_api_url)
        .await
        .context("test contract action subscription")?;
    test_shielded_transactions_subscription(&ws_api_url, &network_id)
        .await
        .context("test shielded transactions subscription")?;
    test_unshielded_transactions_subscription(&indexer_data, &ws_api_url)
        .await
        .context("test unshielded transactions subscription")?;
    test_zswap_ledger_events_subscription(&indexer_data, &ws_api_url)
        .await
        .context("test zswap ledger events subscription")?;
    test_dust_ledger_events_subscription(&indexer_data, &ws_api_url)
        .await
        .context("test dust ledger events subscription")?;

    println!("Successfully finished e2e testing");

    Ok(())
}

/// All data needed for testing collected from the Indexer via the blocks subscription. To be used
/// as expected data in tests for all other API operations.
struct IndexerData {
    blocks: Vec<BlockSubscriptionBlocks>,
    transactions: Vec<BlockSubscriptionBlocksTransactions>,
    contract_actions: Vec<BlockSubscriptionBlocksTransactionsContractActions>,
    unshielded_utxos: Vec<BlockSubscriptionBlocksTransactionsUnshieldedCreatedOutputs>,
    zswap_ledger_events: Vec<BlockSubscriptionBlocksTransactionsZswapLedgerEvents>,
    dust_ledger_events: Vec<BlockSubscriptionBlocksTransactionsDustLedgerEvents>,
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

        // Verify that each block references its parent and the height is incremented by one.
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

        // Verify that all transactions of a block reference that block and have the same protocol
        // version like the block.
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

        // Verify that there are transactions.
        assert!(!transactions.is_empty());

        // Verify various properties for regular transactions.
        let regular_transactions = transactions
            .iter()
            .filter_map(|transaction| match &transaction.on {
                block_subscription::BlockSubscriptionBlocksTransactionsOn::RegularTransaction(
                    transaction,
                ) => Some(transaction),
                block_subscription::BlockSubscriptionBlocksTransactionsOn::SystemTransaction => {
                    None
                }
            });
        for transaction in regular_transactions {
            // Verify transaction segment results.
            match &transaction.transaction_result.status {
                block_subscription::TransactionResultStatus::SUCCESS => {
                    assert!(transaction.transaction_result.segments.is_none())
                }
                block_subscription::TransactionResultStatus::PARTIAL_SUCCESS => {
                    assert!(transaction.transaction_result.segments.is_some())
                }
                block_subscription::TransactionResultStatus::FAILURE => {
                    assert!(transaction.transaction_result.segments.is_none())
                }
                block_subscription::TransactionResultStatus::Other(other) => {
                    panic!("unexpected variant TransactionResultStatus {other}")
                }
            };

            // Verify fees.
            assert!(
                transaction.fees.paid_fees.parse::<u64>().is_ok()
                    && transaction.fees.estimated_fees.parse::<u64>().is_ok()
            );
        }

        // Verify that contract actions of a transaction reference that transaction.
        assert!(transactions.iter().all(|transaction| {
            transaction
                .contract_actions
                .iter()
                .all(|contract_action| contract_action.transaction.hash == transaction.hash)
        }));

        // Collect contract actions.
        let contract_actions = transactions
            .iter()
            .flat_map(|transaction| transaction.contract_actions.iter().cloned())
            .collect::<Vec<_>>();

        // Verify that there are contract actions.
        assert!(!contract_actions.is_empty());

        // Verify that the contract action zswap state is non-empty.
        assert!(
            contract_actions
                .iter()
                .all(|contract_action| !contract_action.zswap_state.as_ref().is_empty())
        );

        // Verify that contract calls and their deploy have the same address.
        assert!(
            contract_actions
                .iter()
                .filter_map(|contract_action| {
                    let address = &contract_action.address;
                    match &contract_action.on {
                        block_subscription::BlockSubscriptionBlocksTransactionsContractActionsOn::ContractCall(contract_call) => {
                        Some((address, &contract_call.deploy.address))
                    }
                    _ => None,
                }})
                .all(|(call_address, deploy_address)| call_address == deploy_address)
        );

        // Collect unshielded UTXOs.
        let unshielded_utxos = transactions
            .iter()
            .flat_map(|transaction| transaction.unshielded_created_outputs.to_owned())
            .collect::<Vec<_>>();

        // Verify that there are unshielded UTXOs.
        assert!(!unshielded_utxos.is_empty());

        // Collect ledger events.
        let zswap_ledger_events = transactions
            .iter()
            .flat_map(|transaction| transaction.zswap_ledger_events.to_owned())
            .collect::<Vec<_>>();
        let dust_ledger_events = transactions
            .iter()
            .flat_map(|transaction| transaction.dust_ledger_events.to_owned())
            .collect::<Vec<_>>();

        // Verify that there are ledger events.
        assert!(!zswap_ledger_events.is_empty());
        assert!(!dust_ledger_events.is_empty());

        Ok(Self {
            blocks,
            transactions,
            contract_actions,
            unshielded_utxos,
            zswap_ledger_events,
            dust_ledger_events,
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
        assert_eq!(block.to_json_value(), expected_block.to_json_value());

        // Existing height.
        let variables = block_query::Variables {
            block_offset: Some(block_query::BlockOffset::Height(expected_block.height)),
        };
        let block = send_query::<BlockQuery>(api_client, api_url, variables)
            .await?
            .block
            .expect("there is a block");
        assert_eq!(block.to_json_value(), expected_block.to_json_value());
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

/// Test the transactions query, including fee metadata and segment results validation.
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
            .transactions;

        // Verify expected transaction is in results.
        let transaction_values = transactions
            .iter()
            .map(|t| t.to_json_value())
            .collect::<Vec<_>>();
        assert!(transaction_values.contains(&expected_transaction.to_json_value()));

        // Existing identifier for regular transactions.
        if let block_subscription::BlockSubscriptionBlocksTransactionsOn::RegularTransaction(
            BlockSubscriptionBlocksTransactionsOnRegularTransaction { identifiers, .. },
        ) = &expected_transaction.on
        {
            for identifier in identifiers {
                let variables = transactions_query::Variables {
                    transaction_offset: transactions_query::TransactionOffset::Identifier(
                        identifier.to_owned(),
                    ),
                };
                let transactions = send_query::<TransactionsQuery>(api_client, api_url, variables)
                    .await?
                    .transactions;

                // Verify expected transaction is in results.
                let transaction_values = transactions
                    .iter()
                    .map(|t| t.to_json_value())
                    .collect::<Vec<_>>();
                assert!(transaction_values.contains(&expected_transaction.to_json_value()));
            }
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
    for expected_contract_action in &indexer_data.contract_actions {
        // Existing block hash.
        let variables = contract_action_query::Variables {
            address: expected_contract_action.address.to_owned(),
            contract_action_offset: Some(contract_action_query::ContractActionOffset::BlockOffset(
                contract_action_query::BlockOffset::Hash(
                    expected_contract_action.transaction.block.hash.to_owned(),
                ),
            )),
        };
        let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
            .await?
            .contract_action
            .expect("there is a contract action");
        assert_eq!(
            contract_action.to_json_value(),
            expected_contract_action.to_json_value()
        );

        // Existing block height.
        let variables = contract_action_query::Variables {
            address: expected_contract_action.address.to_owned(),
            contract_action_offset: Some(contract_action_query::ContractActionOffset::BlockOffset(
                contract_action_query::BlockOffset::Height(
                    expected_contract_action.transaction.block.height.to_owned(),
                ),
            )),
        };
        let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
            .await?
            .contract_action
            .expect("there is a contract action");
        assert_eq!(
            contract_action.to_json_value(),
            expected_contract_action.to_json_value()
        );

        // Existing transaction hash.
        let variables = contract_action_query::Variables {
            address: expected_contract_action.address.to_owned(),
            contract_action_offset: Some(
                contract_action_query::ContractActionOffset::TransactionOffset(
                    contract_action_query::TransactionOffset::Hash(
                        expected_contract_action.transaction.hash.to_owned(),
                    ),
                ),
            ),
        };
        let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
            .await?
            .contract_action
            .expect("there is a contract action");
        assert_eq!(
            contract_action.to_json_value(),
            expected_contract_action.to_json_value()
        );

        // Existing transaction identifier.
        // The query will not necessarily return the expected contract action, but the most recent
        // one (with the highest ID); hence we can only compare addresses.
        if let block_subscription::BlockSubscriptionBlocksTransactionsContractActionsTransactionOn::RegularTransaction(transaction) = &expected_contract_action.transaction.on {
            for identifier in &transaction.identifiers  {
                let variables = contract_action_query::Variables {
                    address: expected_contract_action.address.to_owned(),
                    contract_action_offset: Some(
                        contract_action_query::ContractActionOffset::TransactionOffset(
                            contract_action_query::TransactionOffset::Identifier(identifier.to_owned()),
                        ),
                    ),
                };
                let contract_action = send_query::<ContractActionQuery>(api_client, api_url, variables)
                    .await?
                    .contract_action
                    .expect("there is a contract action");
                assert_eq!(contract_action.address, expected_contract_action.address);
            }
        }

        // Unknown block hash.
        let variables = contract_action_query::Variables {
            address: expected_contract_action.address.to_owned(),
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
            address: expected_contract_action.address.to_owned(),
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
            address: expected_contract_action.address.to_owned(),
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
            address: expected_contract_action.address.to_owned(),
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

/// Test the dustGenerationStatus query.
async fn test_dust_generation_status_query(
    api_client: &Client,
    api_url: &str,
) -> anyhow::Result<()> {
    // Test with empty stake keys list.
    let variables = dust_generation_status_query::Variables {
        cardano_stake_keys: vec![],
    };
    let response = send_query::<DustGenerationStatusQuery>(api_client, api_url, variables).await?;
    assert!(response.dust_generation_status.is_empty());

    // Test with non-existent stake keys - should return unregistered status.
    let variables = dust_generation_status_query::Variables {
        cardano_stake_keys: vec![
            "0x0000000000000000000000000000000000000000000000000000000000000001"
                .try_into()
                .unwrap(),
            "0x0000000000000000000000000000000000000000000000000000000000000002"
                .try_into()
                .unwrap(),
        ],
    };
    let response = send_query::<DustGenerationStatusQuery>(api_client, api_url, variables).await?;
    assert_eq!(response.dust_generation_status.len(), 2);

    for status in &response.dust_generation_status {
        // All test keys should be unregistered.
        assert!(!status.registered);
        assert!(status.dust_address.is_none());
        // Unregistered addresses have zero rates and balances.
        assert_eq!(status.generation_rate, "0");
        assert_eq!(status.current_capacity, "0");
        assert_eq!(status.night_balance, "0");
    }

    Ok(())
}

/// Test the connect mutation.
async fn test_connect_mutation(
    api_client: &Client,
    api_url: &str,
    network_id: &NetworkId,
) -> anyhow::Result<()> {
    // Valid viewing key.
    let viewing_key = ViewingKey::from(viewing_key(network_id));
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
async fn test_contract_actions_subscription(
    indexer_data: &IndexerData,
    ws_api_url: &str,
) -> anyhow::Result<()> {
    // Map expected contract actions by address.
    let contract_actions_by_address = indexer_data
        .contract_actions
        .iter()
        .map(|c| (c.address.to_owned(), c.to_json_value()))
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
                .map_ok(|data| data.contract_actions.to_json_value())
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
                .map_ok(|data| data.contract_actions.to_json_value())
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
                .map_ok(|data| data.contract_actions.to_json_value())
                .try_collect::<Vec<_>>()
                .await
                .context("collect blocks from contract action subscription")?;
        assert_eq!(contract_actions, expected_contract_actions);
    }

    Ok(())
}

/// Test the shielded transactions subscription.
async fn test_shielded_transactions_subscription(
    ws_api_url: &str,
    network_id: &NetworkId,
) -> anyhow::Result<()> {
    let session_id = ViewingKey::from(viewing_key(network_id))
        .try_into_domain(network_id, PROTOCOL_VERSION_000_017_000)?
        .to_session_id()
        .hex_encode();

    // Collect shielded transactions events until there are no more relevant transactions.
    let variables = shielded_transactions_subscription::Variables { session_id };
    let relevant_transactions =
        graphql_ws_client::subscribe::<ShieldedTransactionsSubscription>(ws_api_url, variables)
            .await
            .context("subscribe to shielded transactions")?
            .map_ok(|data| data.shielded_transactions)
            .try_filter_map(|event| match event {
                ShieldedTransactions::RelevantTransaction(t) => ok(Some(t)),
                ShieldedTransactions::ShieldedTransactionsProgress(_) => ok(None),
            });
    let relevant_transactions =
        tokio_stream::StreamExt::timeout(relevant_transactions, Duration::from_secs(3))
            .take_while(|timeout_result| ready(timeout_result.is_ok()))
            .filter_map(|timeout_result| ready(timeout_result.map(Some).unwrap_or(None)))
            .try_collect::<Vec<_>>()
            .await
            .context("collect relevant transactions from shielded transactions events")?;

    // Verify that there are no index gaps.
    let mut expected_start_index = 0;
    for relevant_transaction in relevant_transactions {
        if let Some(collapsed_merkle_tree) = relevant_transaction.collapsed_merkle_tree {
            assert_eq!(collapsed_merkle_tree.start_index, expected_start_index);
            assert!(collapsed_merkle_tree.end_index >= collapsed_merkle_tree.start_index);

            expected_start_index = collapsed_merkle_tree.end_index + 1;
        }

        assert!(relevant_transaction.transaction.start_index == expected_start_index);
        assert!(
            relevant_transaction.transaction.end_index
                >= relevant_transaction.transaction.start_index
        );

        expected_start_index = relevant_transaction.transaction.end_index;
    }

    Ok(())
}

async fn test_unshielded_transactions_subscription(
    indexer_data: &IndexerData,
    ws_api_url: &str,
) -> anyhow::Result<()> {
    if let Some(unshielded_address) = indexer_data
        .unshielded_utxos
        .first()
        .cloned()
        .map(|a| a.owner)
    {
        let variables = unshielded_transactions_subscription::Variables {
            address: unshielded_address.clone(),
        };
        let unshielded_utxos_updates = graphql_ws_client::subscribe::<
            UnshieldedTransactionsSubscription,
        >(ws_api_url, variables)
        .await
        .context("subscribe to unshielded UTXOs")?
        .take(3)
        .map_ok(|data| data.unshielded_transactions)
        .try_filter_map(|event| match event {
            UnshieldedTransactions::UnshieldedTransaction(t) => ok(Some(t)),
            _ => ok(None),
        })
        .try_collect::<Vec<_>>()
        .await
        .context("collect unshielded UTXO events")?;

        assert!(unshielded_utxos_updates.iter().any(move |update| {
            update
                .created_utxos
                .iter()
                .any(|u| u.owner == unshielded_address)
        }));
    }

    Ok(())
}

async fn test_zswap_ledger_events_subscription(
    indexer_data: &IndexerData,
    ws_api_url: &str,
) -> anyhow::Result<()> {
    let expected_zswap_ledger_events = indexer_data
        .zswap_ledger_events
        .iter()
        .map(|event| event.to_json_value())
        .collect::<Vec<_>>();

    let variables = zswap_ledger_events_subscription::Variables { id: None };
    let zswap_ledger_events =
        graphql_ws_client::subscribe::<ZswapLedgerEventsSubscription>(ws_api_url, variables)
            .await
            .context("subscribe to zswap ledger events")?
            .take(expected_zswap_ledger_events.len())
            .map_ok(|data| data.zswap_ledger_events.to_json_value())
            .try_collect::<Vec<_>>()
            .await
            .context("collect zswap ledger events from subscription")?;

    assert_eq!(zswap_ledger_events, expected_zswap_ledger_events);

    Ok(())
}

async fn test_dust_ledger_events_subscription(
    indexer_data: &IndexerData,
    ws_api_url: &str,
) -> anyhow::Result<()> {
    let expected_dust_ledger_events = indexer_data
        .dust_ledger_events
        .iter()
        .map(|event| event.to_json_value())
        .collect::<Vec<_>>();

    let variables = dust_ledger_events_subscription::Variables { id: None };
    let dust_ledger_events =
        graphql_ws_client::subscribe::<DustLedgerEventsSubscription>(ws_api_url, variables)
            .await
            .context("subscribe to dust ledger events")?
            .map_ok(|data| data.dust_ledger_events.to_json_value());
    let dust_ledger_events =
        tokio_stream::StreamExt::timeout(dust_ledger_events, Duration::from_secs(3))
            .take_while(|timeout_result| ready(timeout_result.is_ok()))
            .filter_map(|timeout_result| ready(timeout_result.map(Some).unwrap_or(None)))
            .try_collect::<Vec<_>>()
            .await
            .context("collect dust ledger events from subscription")?;

    assert_eq!(dust_ledger_events, expected_dust_ledger_events);

    Ok(())
}

trait SerializeExt
where
    Self: Serialize,
{
    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("can be JSON-serialized")
    }
}

impl<T> SerializeExt for T where T: Serialize {}

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

fn viewing_key(network_id: &NetworkId) -> &'static str {
    match network_id.as_ref() {
        "undeployed" => {
            "mn_shield-esk_undeployed1dlyj7u8juj68fd4psnkqhjxh32sec0q480vzswg8kd485e2kljcs9ete5h"
        }
        "dev" => "mn_shield-esk_dev1dlyj7u8juj68fd4psnkqhjxh32sec0q480vzswg8kd485e2kljcsp7rsx2",
        "test" => "mn_shield-esk_test1dlyj7u8juj68fd4psnkqhjxh32sec0q480vzswg8kd485e2kljcsuv0u5j",
        "mainnet" => "mn_shield-esk1dlyj7u8juj68fd4psnkqhjxh32sec0q480vzswg8kd485e2kljcsucf6ww",
        other => panic!("unexpected network ID {other}"),
    }
}

mod graphql {
    use graphql_client::GraphQLQuery;
    use indexer_api::infra::api::v3::{
        HexEncoded, mutation::Unit, unshielded::UnshieldedAddress, viewing_key::ViewingKey,
    };

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct BlockQuery;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct TransactionsQuery;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct ContractActionQuery;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct UnshieldedTransactionsSubscription;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct ConnectMutation;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct DisconnectMutation;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct BlockSubscription;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct ContractActionSubscription;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct ShieldedTransactionsSubscription;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct ZswapLedgerEventsSubscription;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct DustLedgerEventsSubscription;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "../indexer-api/graphql/schema-v3.graphql",
        query_path = "./e2e.graphql",
        response_derives = "Debug, Clone, Serialize"
    )]
    pub struct DustGenerationStatusQuery;
}
