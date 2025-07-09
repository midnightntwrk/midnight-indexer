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

use anyhow::Context;
use assert_matches::assert_matches;
use chain_indexer::domain::{
    Block, BlockInfo, BlockTransactions, Transaction, storage::Storage as _,
};
use fake::{Fake, Faker};
use futures::{StreamExt, TryStreamExt};
use indexer_api::domain::{
    ContractAction, ContractAttributes,
    storage::{
        block::BlockStorage, contract_action::ContractActionStorage,
        transaction::TransactionStorage,
    },
};
use indexer_common::{
    self,
    cipher::make_cipher,
    domain::{
        BlockAuthor, BlockHash, ByteArray, ByteVec, IntentHash, NetworkId,
        PROTOCOL_VERSION_000_013_000, RawContractAddress, RawTokenType, RawTransaction,
        RawTransactionIdentifier, RawUnshieldedAddress, TransactionHash, TransactionResult,
        UnshieldedUtxo, ledger::ZswapStateRoot,
    },
    error::BoxError,
    infra::{migrations, pool},
};
use std::{convert::Into, sync::LazyLock};

/// Network ID used throughout storage tests.
/// Required for contract balance extraction during block saving. The NetworkId must match
/// the network context used when the contract state was originally serialized. Since test
/// data uses undeployed network format, we use NetworkId::Undeployed for consistency.
const TEST_NETWORK_ID: NetworkId = NetworkId::Undeployed;

type ChainIndexerStorage = chain_indexer::infra::storage::Storage;
type IndexerApiStorage = indexer_api::infra::storage::Storage;

#[tokio::test]
#[cfg(feature = "cloud")]
async fn main() -> anyhow::Result<()> {
    use sqlx::postgres::PgSslMode;
    use std::time::Duration;
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres;

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

    let config = pool::postgres::Config {
        host: "localhost".to_string(),
        port: postgres_port,
        dbname: "indexer".to_string(),
        user: "indexer".to_string(),
        password: env!("APP__INFRA__STORAGE__PASSWORD").into(),
        sslmode: PgSslMode::Prefer,
        max_connections: 10,
        idle_timeout: Duration::from_secs(60),
        max_lifetime: Duration::from_secs(5 * 60),
    };
    let pool = pool::postgres::PostgresPool::new(config)
        .await
        .context("create PostgresPool")?;

    migrations::postgres::run(&pool)
        .await
        .context("run Postgres migrations")?;

    let cipher =
        make_cipher(env!("APP__INFRA__SECRET").to_string().into()).context("make cipher")?;

    run_tests(
        chain_indexer::infra::storage::Storage::new(pool.clone()),
        indexer_api::infra::storage::Storage::new(cipher, pool),
    )
    .await?;

    Ok(())
}

#[tokio::test]
#[cfg(feature = "standalone")]
async fn main() -> anyhow::Result<()> {
    let pool = pool::sqlite::SqlitePool::new(pool::sqlite::Config::default())
        .await
        .context("create SqlitePool")?;

    migrations::sqlite::run(&pool)
        .await
        .context("run SqlitePool migrations")?;

    let cipher =
        make_cipher(env!("APP__INFRA__SECRET").to_string().into()).context("make cipher")?;

    run_tests(
        chain_indexer::infra::storage::Storage::new(pool.clone()),
        indexer_api::infra::storage::Storage::new(cipher, pool),
    )
    .await?;

    Ok(())
}

async fn run_tests(
    chain_indexer_storage: ChainIndexerStorage,
    indexer_api_storage: IndexerApiStorage,
) -> anyhow::Result<()> {
    // chain-indexer ===============================================================================

    let highest_block_hash = chain_indexer_storage
        .get_highest_block_info()
        .await
        .context("get max block height")?;
    assert!(highest_block_hash.is_none());

    let mut block_0 = BLOCK_0.clone();
    let mut block_1 = BLOCK_1.clone();
    let mut block_2 = BLOCK_2.clone();

    chain_indexer_storage
        .save_block(&mut block_0)
        .await
        .context("save block 0")?;
    chain_indexer_storage
        .save_block(&mut block_1)
        .await
        .context("save block 1")?;
    chain_indexer_storage
        .save_block(&mut block_2)
        .await
        .context("save block 2")?;

    let highest_block = chain_indexer_storage
        .get_highest_block_info()
        .await
        .context("get highest block hash")?;
    assert_matches!(
        highest_block,
        Some(BlockInfo { hash, height }) if hash == BLOCK_2_HASH && height == 2
    );

    let transaction_count = chain_indexer_storage
        .get_transaction_count()
        .await
        .context("get transaction count")?;
    assert_eq!(transaction_count, 3);

    let contract_action_count = chain_indexer_storage
        .get_contract_action_count()
        .await
        .context("get contract action count")?;
    assert_eq!(contract_action_count, (1, 3, 1));

    let block_transactions = chain_indexer_storage
        .get_block_transactions(0)
        .await
        .context("get block transactions for 0")?;
    assert_eq!(
        block_transactions,
        BlockTransactions {
            transactions: vec![],
            protocol_version: PROTOCOL_VERSION_000_013_000,
            block_parent_hash: BLOCK_0.parent_hash,
            block_timestamp: BLOCK_0.timestamp
        }
    );

    let block_transactions = chain_indexer_storage
        .get_block_transactions(1)
        .await
        .context("get block transactions for 1")?;
    assert_eq!(
        block_transactions,
        BlockTransactions {
            transactions: BLOCK_1
                .transactions
                .iter()
                .map(|t| t.raw.to_owned())
                .collect::<Vec<_>>(),
            protocol_version: PROTOCOL_VERSION_000_013_000,
            block_parent_hash: BLOCK_1.parent_hash,
            block_timestamp: BLOCK_1.timestamp
        }
    );

    // indexer-api =================================================================================

    let block = indexer_api_storage
        .get_block_by_hash([0; 32].into())
        .await
        .context("get block by unknown hash")?;
    assert!(block.is_none());
    let block = indexer_api_storage
        .get_block_by_hash(BLOCK_0_HASH)
        .await
        .context("get block by block 0 hash")?;
    assert!(block.is_some());
    let block = block.unwrap();
    assert_eq!(block.hash, BLOCK_0_HASH);
    assert_eq!(block.height, 0);
    assert_eq!(block.protocol_version, PROTOCOL_VERSION_000_013_000);
    assert_eq!(block.parent_hash, ZERO_HASH);
    assert!(block.author.is_none());
    assert_eq!(block.timestamp, 0);

    let block = indexer_api_storage
        .get_block_by_height(666)
        .await
        .context("get block by unknown height")?;
    assert!(block.is_none());
    let block = indexer_api_storage
        .get_block_by_height(1)
        .await
        .context("get block by height 1")?;
    assert!(block.is_some());
    let block = block.unwrap();
    assert_eq!(block.hash, BLOCK_1_HASH);
    assert_eq!(block.height, 1);
    assert_eq!(block.protocol_version, PROTOCOL_VERSION_000_013_000);
    assert_eq!(block.parent_hash, BLOCK_0_HASH);
    assert_matches!(block.author, Some(author) if author == *BLOCK_1_AUTHOR);
    assert_eq!(block.timestamp, 1);
    let transactions = indexer_api_storage
        .get_transactions_by_block_id(block.id)
        .await
        .context("get_transactions_by_block_id")?;
    assert_eq!(transactions.len(), 2);
    assert_eq!(transactions[1].id - transactions[0].id, 1);
    let transaction = &transactions[0];
    assert_eq!(transaction.hash, TRANSACTION_1_HASH);
    assert_eq!(transaction.block_hash, BLOCK_1_HASH);
    assert_eq!(transaction.protocol_version, PROTOCOL_VERSION_000_013_000);
    assert_eq!(transaction.transaction_result, TransactionResult::Success);
    assert_eq!(transaction.identifiers, vec![IDENTIFIER_1.to_owned()]);
    assert_eq!(&transaction.raw, &*RAW_TRANSACTION_1);
    let contract_actions = indexer_api_storage
        .get_contract_actions_by_transaction_id(transaction.id)
        .await
        .context("get_contract_actions_by_transaction_id")?;
    assert_matches!(
        contract_actions.as_slice(),
        [
            ContractAction {
                address, state, attributes: ContractAttributes::Deploy, ..
            }
        ] if *address == ADDRESS.to_owned() &&
             *state == b"state".as_slice().into()
    );
    let transaction = &transactions[1];
    assert_eq!(transaction.hash, TRANSACTION_1_HASH);
    assert_eq!(transaction.transaction_result, TransactionResult::Failure);

    let block = indexer_api_storage
        .get_latest_block()
        .await
        .context("get latest block")?;
    assert!(block.is_some());
    let block = block.unwrap();
    assert_eq!(block.hash, BLOCK_2_HASH);
    assert_eq!(block.height, 2);
    assert_eq!(block.protocol_version, PROTOCOL_VERSION_000_013_000);
    assert_eq!(block.parent_hash, BLOCK_1_HASH);
    assert_eq!(block.timestamp, 2);
    let transactions = indexer_api_storage
        .get_transactions_by_block_id(block.id)
        .await
        .context("get_transactions_by_block_id")?;
    assert_eq!(transactions.len(), 1);
    let transaction = &transactions[0];
    assert_eq!(transaction.hash, TRANSACTION_2_HASH);
    assert_eq!(transaction.block_hash, BLOCK_2_HASH);
    assert_eq!(transaction.protocol_version, PROTOCOL_VERSION_000_013_000);
    assert_eq!(transaction.transaction_result, TransactionResult::Success);
    assert_eq!(transaction.identifiers, vec![IDENTIFIER_2.to_owned()]);
    assert_eq!(&transaction.raw, &*RAW_TRANSACTION_2);
    let contract_actions = indexer_api_storage
        .get_contract_actions_by_transaction_id(transaction.id)
        .await
        .context("get_contract_actions_by_transaction_id")?;
    assert_eq!(contract_actions.len(), 3);
    assert_matches!(
        contract_actions.as_slice(),
        [
            ContractAction {
                address, state, attributes: ContractAttributes::Call { .. }, ..
            },
            ContractAction {
                attributes: ContractAttributes::Update, ..
            },
            ContractAction {
                attributes: ContractAttributes::Call { .. }, ..
            },
        ] if *address == ADDRESS.to_owned() &&
             *state == b"state".as_slice().into()
    );
    let transaction = indexer_api_storage
        .get_transaction_by_id(transaction.id)
        .await
        .context("get_transaction_by_id")?
        .expect("transaction with ID exists");
    assert_eq!(transaction.hash, TRANSACTION_2_HASH);

    let blocks = indexer_api_storage.get_blocks(10, 10.try_into()?);
    let len = blocks.count().await;
    assert_eq!(len, 0);

    let blocks = indexer_api_storage
        .get_blocks(0, 10.try_into().unwrap())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(blocks.len(), 3);

    let blocks = indexer_api_storage
        .get_blocks(1, 10.try_into().unwrap())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(blocks.len(), 2);

    let blocks = indexer_api_storage
        .get_blocks(0, 1.try_into().unwrap())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(blocks.len(), 3);

    let transactions = indexer_api_storage
        .get_transactions_by_hash([0; 32].into())
        .await?;
    assert!(transactions.is_empty());
    let transactions = indexer_api_storage
        .get_transactions_by_hash(TRANSACTION_1_HASH)
        .await?;
    assert_eq!(transactions.len(), 2);
    assert_eq!(transactions[0].id - transactions[1].id, 1);
    assert_eq!(transactions[0].hash, TRANSACTION_1_HASH);
    assert_eq!(transactions[1].hash, TRANSACTION_1_HASH);

    let transactions = indexer_api_storage
        .get_transactions_by_identifier(&b"unknown".as_slice().into())
        .await?;
    assert!(transactions.is_empty());
    let transactions = indexer_api_storage
        .get_transactions_by_identifier(&IDENTIFIER_1)
        .await?;
    assert_eq!(transactions.len(), 2);
    assert_eq!(transactions[1].id - transactions[0].id, 1);
    assert_eq!(transactions[0].hash, TRANSACTION_1_HASH);
    assert_eq!(transactions[1].hash, TRANSACTION_1_HASH);

    let contract_action = indexer_api_storage
        .get_latest_contract_action_by_address(&b"unknown".as_slice().into())
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_latest_contract_action_by_address(&ADDRESS)
        .await?;
    assert_matches!(
        contract_action,
        Some(
            ContractAction {
                address,
                attributes: ContractAttributes::Call { .. },
                ..
            }
        ) if address == ADDRESS.to_owned()
    );

    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_block_hash(&b"unknown".as_slice().into(), BLOCK_1_HASH)
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_block_hash(&ADDRESS, [0; 32].into())
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_block_hash(&ADDRESS, BLOCK_1_HASH)
        .await?;
    assert_matches!(
        contract_action,
        Some(ContractAction { address, attributes: ContractAttributes::Call { .. }, .. })
            if address == ADDRESS.to_owned()
    );

    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_block_height(&b"unknown".as_slice().into(), 2)
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_block_height(&ADDRESS, 666)
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_block_height(&ADDRESS, 2)
        .await?;
    assert_matches!(
        contract_action,
        Some(ContractAction { address, attributes: ContractAttributes::Call { .. }, .. })
            if address == ADDRESS.to_owned()
    );

    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_transaction_hash(
            &b"unknown".as_slice().into(),
            TRANSACTION_1_HASH,
        )
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_transaction_hash(&ADDRESS, [0; 32].into())
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_transaction_hash(&ADDRESS, TRANSACTION_1_HASH)
        .await?;
    assert_matches!(
        contract_action,
        Some(ContractAction { address, attributes: ContractAttributes::Deploy, .. })
            if address == ADDRESS.to_owned()
    );

    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_transaction_identifier(
            &b"unknown".as_slice().into(),
            &IDENTIFIER_2,
        )
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_transaction_identifier(
            &ADDRESS,
            &b"unknown".as_slice().into(),
        )
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_transaction_identifier(&ADDRESS, &IDENTIFIER_2)
        .await?;
    assert_matches!(
        contract_action,
        Some(ContractAction { address, attributes: ContractAttributes::Call { .. }, .. })
            if address == ADDRESS.to_owned()
    );

    let contract_actions = indexer_api_storage.get_contract_actions_by_address(
        &UNKNOWN_ADDRESS,
        0,
        10.try_into().unwrap(),
    );
    let len = contract_actions.count().await;
    assert_eq!(len, 0);

    let contract_actions = indexer_api_storage
        .get_contract_actions_by_address(&ADDRESS, 0, 10.try_into().unwrap())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(contract_actions.len(), 5);

    let contract_actions = indexer_api_storage
        .get_contract_actions_by_address(&ADDRESS, 0, 1.try_into().unwrap())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(contract_actions.len(), 5);

    let contract_actions = indexer_api_storage
        .get_contract_actions_by_address(&ADDRESS, 3, 10.try_into().unwrap())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(contract_actions.len(), 3);

    let contract_actions = indexer_api_storage
        .get_contract_actions_by_address(&ADDRESS, 4, 10.try_into().unwrap())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(contract_actions.len(), 2);

    let end_indices = indexer_api_storage
        .get_highest_end_indices([0; 32].into())
        .await?;
    assert_eq!(end_indices, (Some(3), None, None));

    let id = indexer_api_storage
        .get_highest_transaction_id_for_unshielded_address(OWNER_0)
        .await?;
    assert!(id.is_none());

    let id = indexer_api_storage
        .get_highest_transaction_id_for_unshielded_address(OWNER_1)
        .await?;
    assert_eq!(id, Some(3));

    Ok(())
}

static BLOCK_0: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_0_HASH,
    height: 0,
    protocol_version: PROTOCOL_VERSION_000_013_000,
    parent_hash: ZERO_HASH,
    author: None,
    timestamp: 0,
    zswap_state_root: ZswapStateRoot::V5(Faker.fake()),
    transactions: vec![],
});

static BLOCK_1: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_1_HASH,
    height: 1,
    protocol_version: PROTOCOL_VERSION_000_013_000,
    parent_hash: BLOCK_0_HASH,
    author: Some(BLOCK_1_AUTHOR.to_owned()),
    timestamp: 1,
    zswap_state_root: ZswapStateRoot::V5(Faker.fake()),
    transactions: vec![
        Transaction {
            id: 0, //they are not saved in the db yet
            hash: TRANSACTION_1_HASH,
            protocol_version: PROTOCOL_VERSION_000_013_000,
            transaction_result: TransactionResult::Success,
            identifiers: vec![IDENTIFIER_1.to_owned()],
            raw: RAW_TRANSACTION_1.to_owned(),
            contract_actions: vec![chain_indexer::domain::ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: indexer_common::domain::ContractAttributes::Deploy,
                // Deploy actions typically start with zero balances
                extracted_balances: Vec::new(),
            }],
            merkle_tree_root: b"merkle_tree_root".as_slice().into(),
            created_unshielded_utxos: vec![UnshieldedUtxo {
                value: 100,
                owner: OWNER_1,
                token_type: *TOKEN_NIGHT,
                intent_hash: *INTENT_HASH,
                output_index: 0,
            }],
            spent_unshielded_utxos: vec![],
            start_index: 0,
            end_index: 1,
            paid_fees: 0,
            estimated_fees: 0,
        },
        Transaction {
            id: 0,
            hash: TRANSACTION_1_HASH,
            protocol_version: PROTOCOL_VERSION_000_013_000,
            transaction_result: TransactionResult::Failure,
            identifiers: vec![IDENTIFIER_1.to_owned()],
            raw: RAW_TRANSACTION_1.to_owned(),
            contract_actions: vec![chain_indexer::domain::ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: indexer_common::domain::ContractAttributes::Call {
                    entry_point: b"entry_point".as_slice().into(),
                },
                // Call actions may receive tokens and have balances
                extracted_balances: vec![indexer_common::domain::ContractBalance {
                    token_type: *TOKEN_NIGHT,
                    amount: 1000,
                }],
            }],
            merkle_tree_root: b"merkle_tree_root".as_slice().into(),
            created_unshielded_utxos: vec![UnshieldedUtxo {
                value: 100,
                owner: OWNER_1,
                token_type: *TOKEN_NIGHT,
                intent_hash: *INTENT_HASH_2,
                output_index: 0,
            }],
            spent_unshielded_utxos: vec![],
            start_index: 0,
            end_index: 1,
            paid_fees: 0,
            estimated_fees: 0,
        },
    ],
});

static BLOCK_2: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_2_HASH,
    height: 2,
    protocol_version: PROTOCOL_VERSION_000_013_000,
    parent_hash: BLOCK_1_HASH,
    author: Some(BLOCK_2_AUTHOR.to_owned()),
    timestamp: 2,
    zswap_state_root: ZswapStateRoot::V5(Faker.fake()),
    transactions: vec![Transaction {
        id: 0,
        hash: TRANSACTION_2_HASH,
        protocol_version: PROTOCOL_VERSION_000_013_000,
        transaction_result: TransactionResult::Success,
        identifiers: vec![IDENTIFIER_2.to_owned()],
        raw: RAW_TRANSACTION_2.to_owned(),
        contract_actions: vec![
            chain_indexer::domain::ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: indexer_common::domain::ContractAttributes::Call {
                    entry_point: b"entry_point".as_slice().into(),
                },
                // First call action with Night tokens
                extracted_balances: vec![indexer_common::domain::ContractBalance {
                    token_type: *TOKEN_NIGHT,
                    amount: 500,
                }],
            },
            chain_indexer::domain::ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: indexer_common::domain::ContractAttributes::Update,
                // Update action with multiple token types
                extracted_balances: vec![
                    indexer_common::domain::ContractBalance {
                        token_type: *TOKEN_NIGHT,
                        amount: 750,
                    },
                    indexer_common::domain::ContractBalance {
                        token_type: RawTokenType::from([1u8; 32]), // Custom token
                        amount: 100,
                    },
                ],
            },
            chain_indexer::domain::ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: indexer_common::domain::ContractAttributes::Call {
                    entry_point: b"entry_point".as_slice().into(),
                },
                // Second call action with different amounts
                extracted_balances: vec![indexer_common::domain::ContractBalance {
                    token_type: *TOKEN_NIGHT,
                    amount: 250,
                }],
            },
        ],
        merkle_tree_root: b"merkle_tree_root".as_slice().into(),
        created_unshielded_utxos: vec![UnshieldedUtxo {
            value: 50,
            owner: OWNER_2,
            token_type: *TOKEN_NIGHT,
            intent_hash: *INTENT_HASH_3,
            output_index: 0,
        }],
        spent_unshielded_utxos: vec![UnshieldedUtxo {
            value: 0,
            owner: OWNER_1,
            token_type: *TOKEN_NIGHT,
            intent_hash: *INTENT_HASH,
            output_index: 0,
        }],
        start_index: 2,
        end_index: 3,
        paid_fees: 0,
        estimated_fees: 0,
    }],
});

const ZERO_HASH: BlockHash = ByteArray::<32>([0; 32]);

const BLOCK_0_HASH: BlockHash = ByteArray::<32>({
    let mut hash = [0; 32];
    hash[0] = 1;
    hash
});
const BLOCK_1_HASH: BlockHash = ByteArray::<32>([1; 32]);
const BLOCK_2_HASH: BlockHash = ByteArray::<32>([2; 32]);

static BLOCK_1_AUTHOR: LazyLock<BlockAuthor> = LazyLock::new(|| [1; 32].into());
static BLOCK_2_AUTHOR: LazyLock<BlockAuthor> = LazyLock::new(|| [2; 32].into());

const TRANSACTION_1_HASH: TransactionHash = ByteArray::<32>([1; 32]);
const TRANSACTION_2_HASH: TransactionHash = ByteArray::<32>([2; 32]);

static IDENTIFIER_1: LazyLock<RawTransactionIdentifier> =
    LazyLock::new(|| b"identifier-1".as_slice().into());

static IDENTIFIER_2: LazyLock<RawTransactionIdentifier> =
    LazyLock::new(|| b"identifier-2".as_slice().into());

static RAW_TRANSACTION_1: LazyLock<RawTransaction> =
    LazyLock::new(|| create_raw_transaction(TEST_NETWORK_ID).expect("create raw transaction"));

static RAW_TRANSACTION_2: LazyLock<RawTransaction> =
    LazyLock::new(|| create_raw_transaction(TEST_NETWORK_ID).expect("create raw transaction"));

static ADDRESS: LazyLock<RawContractAddress> = LazyLock::new(|| b"address".as_slice().into());

static UNKNOWN_ADDRESS: LazyLock<RawContractAddress> =
    LazyLock::new(|| b"unknown-address".as_slice().into());

pub const UT_ADDR_EMPTY_HEX: &str = "11223344"; // Address with no UTXOs for testing

pub static OWNER_0: RawUnshieldedAddress = ByteArray([0; 32]);
pub static OWNER_1: RawUnshieldedAddress = ByteArray([1; 32]);
pub static OWNER_2: RawUnshieldedAddress = ByteArray([2; 32]);

pub static INTENT_HASH: LazyLock<IntentHash> = LazyLock::new(|| [0x11u8; 32].into());
pub static INTENT_HASH_2: LazyLock<IntentHash> = LazyLock::new(|| [0x22u8; 32].into());
pub static INTENT_HASH_3: LazyLock<IntentHash> = LazyLock::new(|| [0x33u8; 32].into());

pub static TOKEN_NIGHT: LazyLock<RawTokenType> = LazyLock::new(|| [0u8; 32].into());
pub fn create_raw_transaction(_network_id: NetworkId) -> Result<RawTransaction, BoxError> {
    // let empty_offer = Offer::<ProofPreimage> {
    //     inputs: vec![],
    //     outputs: vec![],
    //     transient: vec![],
    //     deltas: vec![],
    // };
    // let pre_transaction = LedgerTransaction::new(empty_offer, None, None);

    // let zswap_resolver = ZswapResolver(MidnightDataProvider::new(
    //     FetchMode::OnDemand,
    //     OutputMode::Log,
    //     ZSWAP_EXPECTED_FILES.to_owned(),
    // ));
    // let external_resolver: ExternalResolver = Box::new(|_|
    // Box::pin(std::future::ready(Ok(None)))); let resolver = Resolver::new(zswap_resolver,
    // external_resolver);

    // let transaction = block_on(pre_transaction.prove(OsRng, &resolver, &resolver))?;
    // let raw_transaction = transaction.serialize(network_id)?.into();

    Ok(ByteVec::default())
}
