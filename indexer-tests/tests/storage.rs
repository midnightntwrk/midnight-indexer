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
use chacha20poly1305::aead::rand_core::OsRng;
use chain_indexer::domain::{
    Block, BlockHash, BlockInfo, Transaction, TransactionHash, storage::Storage as _,
};
use fake::{Fake, Faker};
use futures::{StreamExt, TryStreamExt, executor::block_on};
use indexer_api::domain::{ContractAction, ContractAttributes, Storage};
use indexer_common::{
    self,
    cipher::make_cipher,
    domain::{
        ApplyStage, BlockAuthor, ContractAddress, Identifier, NetworkId, ProtocolVersion,
        RawTransaction,
    },
    error::BoxError,
    infra::{migrations, pool},
    serialize::SerializableExt,
};
use midnight_ledger::{
    base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode},
    prove::{ExternalResolver, Resolver},
    storage::DefaultDB,
    structure::{Transaction as LedgerTransaction, TransactionHash as LedgerTransactionHash},
    transient_crypto::{hash::HashOutput, proofs::ProofPreimage},
    zswap::{Offer, ZSWAP_EXPECTED_FILES, prove::ZswapResolver},
};
use std::{convert::Into, sync::LazyLock};
use subxt::utils::H256;

#[cfg(feature = "cloud")]
type ChainIndexerStorage = chain_indexer::infra::storage::postgres::PostgresStorage;
#[cfg(feature = "standalone")]
type ChainIndexerStorage = chain_indexer::infra::storage::sqlite::SqliteStorage;

#[cfg(feature = "cloud")]
type IndexerApiStorage = indexer_api::infra::storage::postgres::PostgresStorage;
#[cfg(feature = "standalone")]
type IndexerApiStorage = indexer_api::infra::storage::sqlite::SqliteStorage;

#[tokio::test]
#[cfg(feature = "cloud")]
async fn main() -> anyhow::Result<()> {
    use sqlx::postgres::PgSslMode;
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
        chain_indexer::infra::storage::postgres::PostgresStorage::new(pool.clone()),
        indexer_api::infra::storage::postgres::PostgresStorage::new(cipher, pool),
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
        chain_indexer::infra::storage::sqlite::SqliteStorage::new(pool.clone()),
        indexer_api::infra::storage::sqlite::SqliteStorage::new(cipher, pool),
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
        .get_highest_block()
        .await
        .context("get max block height")?;
    assert!(highest_block_hash.is_none());

    chain_indexer_storage
        .save_block(&BLOCK_0)
        .await
        .context("save block 0 with zswap state 1")?;
    chain_indexer_storage
        .save_block(&BLOCK)
        .await
        .context("save block 1 with zswap state 2")?;
    chain_indexer_storage
        .save_block(&BLOCK_2)
        .await
        .context("save block 2 with zswap state 3")?;
    let highest_block = chain_indexer_storage
        .get_highest_block()
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
    assert_eq!(contract_action_count, (2, 2, 1));

    let chunks = chain_indexer_storage
        .get_transaction_chunks(0, 1)
        .try_collect::<Vec<_>>()
        .await
        .context("collect transactions 0..=1")?;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].len(), 0);
    assert_eq!(chunks[1].len(), 2);
    assert_eq!(chunks[1][0].apply_stage, ApplyStage::Failure);
    assert_eq!(chunks[1][0].raw, RAW_TRANSACTION_1.to_owned());
    assert_eq!(
        chunks[1][0].merkle_tree_root,
        b"merkle_tree_root".as_slice().into()
    );
    assert_eq!(chunks[1][0].start_index, 0);
    assert_eq!(chunks[1][0].end_index, 1);

    // indexer-api =================================================================================

    let block = indexer_api_storage
        .get_block_by_hash([0; 32].into())
        .await
        .context("get block by unknown hash")?;
    assert!(block.is_none());
    let block = indexer_api_storage
        .get_block_by_hash((BLOCK_0_HASH.0).0.into())
        .await
        .context("get block by block 0 hash")?;
    assert!(block.is_some());
    let block = block.unwrap();
    assert_eq!(block.hash, (BLOCK_0_HASH.0).0.into());
    assert_eq!(block.height, 0);
    assert_eq!(block.protocol_version, PROTOCOL_VERSION_0_1);
    assert_eq!(block.parent_hash, (ZERO_HASH.0).0.into());
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
    assert_eq!(block.hash, (BLOCK_1_HASH.0).0.into());
    assert_eq!(block.height, 1);
    assert_eq!(block.protocol_version, PROTOCOL_VERSION_0_1);
    assert_eq!(block.parent_hash, (BLOCK_0_HASH.0).0.into());
    assert_matches!(block.author, Some(author) if author == *BLOCK_1_AUTHOR);
    assert_eq!(block.timestamp, 1);
    let transactions = indexer_api_storage
        .get_transactions_by_block_id(block.id)
        .await
        .context("get_transactions_by_block_id")?;
    assert_eq!(transactions.len(), 2);
    let transaction = &transactions[0];
    assert_eq!(transaction.hash, ((TRANSACTION_1_HASH.0).0).0.into());
    assert_eq!(transaction.block_hash, (BLOCK_1_HASH.0).0.into());
    assert_eq!(transaction.protocol_version, PROTOCOL_VERSION_0_1);
    assert_eq!(transaction.apply_stage, ApplyStage::Failure);
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
    assert_eq!(transaction.hash, ((TRANSACTION_1_HASH.0).0).0.into());
    assert_eq!(transaction.apply_stage, ApplyStage::Success);

    let block = indexer_api_storage
        .get_latest_block()
        .await
        .context("get latest block")?;
    assert!(block.is_some());
    let block = block.unwrap();
    assert_eq!(block.hash, (BLOCK_2_HASH.0).0.into());
    assert_eq!(block.height, 2);
    assert_eq!(block.protocol_version, PROTOCOL_VERSION_0_1);
    assert_eq!(block.parent_hash, (BLOCK_1_HASH.0).0.into());
    assert_eq!(block.timestamp, 2);
    let transactions = indexer_api_storage
        .get_transactions_by_block_id(block.id)
        .await
        .context("get_transactions_by_block_id")?;
    assert_eq!(transactions.len(), 1);
    let transaction = &transactions[0];
    assert_eq!(transaction.hash, ((TRANSACTION_2_HASH.0).0).0.into());
    assert_eq!(transaction.block_hash, (BLOCK_2_HASH.0).0.into());
    assert_eq!(transaction.protocol_version, PROTOCOL_VERSION_0_1);
    assert_eq!(transaction.apply_stage, ApplyStage::Success);
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
        .context("get_transaction_by_id")?;
    assert_eq!(transaction.hash, ((TRANSACTION_2_HASH.0).0).0.into());

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
    let mut transactions = indexer_api_storage
        .get_transactions_by_hash(((TRANSACTION_1_HASH.0).0).0.into())
        .await?;
    assert!(!transactions.is_empty());
    let indexer_api::domain::Transaction { hash, .. } = transactions.pop().unwrap();
    assert_eq!(hash, ((TRANSACTION_1_HASH.0).0).0.into());

    let transactions = indexer_api_storage
        .get_transactions_by_identifier(&b"unknown".as_slice().into())
        .await?;
    assert!(transactions.is_empty());
    let transactions = indexer_api_storage
        .get_transactions_by_identifier(&IDENTIFIER_2)
        .await?;
    assert!(!transactions.is_empty());
    let indexer_api::domain::Transaction { hash, .. } = transactions.first().unwrap();
    assert_eq!(*hash, ((TRANSACTION_2_HASH.0).0).0.into());

    let contract_action = indexer_api_storage
        .get_contract_action_by_address(&b"unknown".as_slice().into())
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address(&ADDRESS)
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
        .get_contract_action_by_address_and_block_hash(
            &b"unknown".as_slice().into(),
            (BLOCK_1_HASH.0).0.into(),
        )
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_block_hash(&ADDRESS, [0; 32].into())
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_block_hash(&ADDRESS, (BLOCK_1_HASH.0).0.into())
        .await?;
    assert_matches!(
        contract_action,
        Some(ContractAction { address, attributes: ContractAttributes::Deploy, .. })
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
            ((TRANSACTION_1_HASH.0).0).0.into(),
        )
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_transaction_hash(&ADDRESS, [0; 32].into())
        .await?;
    assert!(contract_action.is_none());
    let contract_action = indexer_api_storage
        .get_contract_action_by_address_and_transaction_hash(
            &ADDRESS,
            ((TRANSACTION_1_HASH.0).0).0.into(),
        )
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
        0,
        10.try_into().unwrap(),
    );
    let len = contract_actions.count().await;
    assert_eq!(len, 0);

    let contract_actions = indexer_api_storage
        .get_contract_actions_by_address(&ADDRESS, 0, 0, 10.try_into().unwrap())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(contract_actions.len(), 4);

    let contract_actions = indexer_api_storage
        .get_contract_actions_by_address(&ADDRESS, 0, 0, 1.try_into().unwrap())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(contract_actions.len(), 4);

    let contract_actions = indexer_api_storage
        .get_contract_actions_by_address(&ADDRESS, 2, 0, 10.try_into().unwrap())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(contract_actions.len(), 3);

    let contract_actions = indexer_api_storage
        .get_contract_actions_by_address(&ADDRESS, 0, 4, 10.try_into().unwrap())
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(contract_actions.len(), 2);

    let end_indices = indexer_api_storage
        .get_highest_indices([0; 32].into())
        .await?;
    assert_eq!(end_indices, (Some(3), None, None));

    Ok(())
}

static BLOCK_0: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_0_HASH,
    height: 0,
    protocol_version: PROTOCOL_VERSION_0_1,
    parent_hash: ZERO_HASH,
    author: None,
    timestamp: 0,
    zswap_state_root: Faker.fake(),
    transactions: vec![],
});

static BLOCK: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_1_HASH,
    height: 1,
    protocol_version: PROTOCOL_VERSION_0_1,
    parent_hash: BLOCK_0_HASH,
    author: Some(BLOCK_1_AUTHOR.to_owned()),
    timestamp: 1,
    zswap_state_root: Faker.fake(),
    transactions: vec![
        Transaction {
            hash: TRANSACTION_1_HASH,
            protocol_version: PROTOCOL_VERSION_0_1,
            apply_stage: ApplyStage::Failure,
            identifiers: vec![IDENTIFIER_1.to_owned()],
            raw: RAW_TRANSACTION_1.to_owned(),
            contract_actions: vec![chain_indexer::domain::ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: chain_indexer::domain::ContractAttributes::Deploy,
            }],
            merkle_tree_root: b"merkle_tree_root".as_slice().into(),
            start_index: 0,
            end_index: 1,
        },
        Transaction {
            hash: TRANSACTION_1_HASH,
            protocol_version: PROTOCOL_VERSION_0_1,
            apply_stage: ApplyStage::Success,
            identifiers: vec![IDENTIFIER_1.to_owned()],
            raw: RAW_TRANSACTION_1.to_owned(),
            contract_actions: vec![chain_indexer::domain::ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: chain_indexer::domain::ContractAttributes::Deploy,
            }],
            merkle_tree_root: b"merkle_tree_root".as_slice().into(),
            start_index: 0,
            end_index: 1,
        },
    ],
});

static BLOCK_2: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_2_HASH,
    height: 2,
    protocol_version: PROTOCOL_VERSION_0_1,
    parent_hash: BLOCK_1_HASH,
    author: Some(BLOCK_2_AUTHOR.to_owned()),
    timestamp: 2,
    zswap_state_root: Faker.fake(),
    transactions: vec![Transaction {
        hash: TRANSACTION_2_HASH,
        protocol_version: PROTOCOL_VERSION_0_1,
        apply_stage: ApplyStage::Success,
        identifiers: vec![IDENTIFIER_2.to_owned()],
        raw: RAW_TRANSACTION_2.to_owned(),
        contract_actions: vec![
            chain_indexer::domain::ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: chain_indexer::domain::ContractAttributes::Call {
                    entry_point: b"entry_point".as_slice().into(),
                },
            },
            chain_indexer::domain::ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: chain_indexer::domain::ContractAttributes::Update,
            },
            chain_indexer::domain::ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: chain_indexer::domain::ContractAttributes::Call {
                    entry_point: b"entry_point".as_slice().into(),
                },
            },
        ],
        merkle_tree_root: b"merkle_tree_root".as_slice().into(),
        start_index: 2,
        end_index: 3,
    }],
});

const ZERO_HASH: BlockHash = BlockHash(H256::zero());

const BLOCK_0_HASH: BlockHash = BlockHash(H256([
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
]));

const BLOCK_1_HASH: BlockHash = BlockHash(H256([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
]));

const BLOCK_2_HASH: BlockHash = BlockHash(H256([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2,
]));

static BLOCK_1_AUTHOR: LazyLock<BlockAuthor> = LazyLock::new(|| [1; 32].into());
static BLOCK_2_AUTHOR: LazyLock<BlockAuthor> = LazyLock::new(|| [2; 32].into());

const TRANSACTION_1_HASH: TransactionHash = TransactionHash(LedgerTransactionHash(HashOutput([
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
])));

const TRANSACTION_2_HASH: TransactionHash = TransactionHash(LedgerTransactionHash(HashOutput([
    2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
])));

static IDENTIFIER_1: LazyLock<Identifier> = LazyLock::new(|| b"identifier-1".as_slice().into());

static IDENTIFIER_2: LazyLock<Identifier> = LazyLock::new(|| b"identifier-2".as_slice().into());

static RAW_TRANSACTION_1: LazyLock<RawTransaction> = LazyLock::new(|| {
    create_raw_transaction(NetworkId::Undeployed).expect("create raw transaction")
});

static RAW_TRANSACTION_2: LazyLock<RawTransaction> = LazyLock::new(|| {
    create_raw_transaction(NetworkId::Undeployed).expect("create raw transaction")
});

static ADDRESS: LazyLock<ContractAddress> = LazyLock::new(|| b"address".as_slice().into());

static UNKNOWN_ADDRESS: LazyLock<ContractAddress> =
    LazyLock::new(|| b"unknown-address".as_slice().into());

pub const PROTOCOL_VERSION_0_1: ProtocolVersion = ProtocolVersion(1_000);

pub fn create_raw_transaction(network_id: NetworkId) -> Result<RawTransaction, BoxError> {
    let empty_offer = Offer::<ProofPreimage> {
        inputs: vec![],
        outputs: vec![],
        transient: vec![],
        deltas: vec![],
    };
    let pre_transaction = LedgerTransaction::<_, DefaultDB>::new(empty_offer, None, None);

    let zswap_resolver = ZswapResolver(MidnightDataProvider::new(
        FetchMode::OnDemand,
        OutputMode::Log,
        ZSWAP_EXPECTED_FILES.to_owned(),
    ));
    let external_resolver: ExternalResolver = Box::new(|_| Box::pin(std::future::ready(Ok(None))));
    let resolver = Resolver::new(zswap_resolver, external_resolver);

    let transaction = block_on(pre_transaction.prove(OsRng, &resolver, &resolver))?;
    let raw_transaction = transaction.serialize(network_id)?.into();

    Ok(raw_transaction)
}
