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

use crate::{PROTOCOL_VERSION_0_1, create_raw_transaction};
use chain_indexer::domain::{
    Block, BlockHash, ContractAction, ContractAttributes, Transaction, TransactionHash,
    UnshieldedUtxo,
};
use const_hex;
use fake::{Fake, Faker};
use indexer_api::domain::{AsBytesExt, HexEncoded};
use indexer_common::{
    self,
    domain::{
        ApplyStage, BlockAuthor, ContractAddress, Identifier, IntentHash, NetworkId, RawTokenType,
        RawTransaction, RawZswapState, UnshieldedAddress,
    },
    serialize::SerializableExt,
};
use midnight_ledger::{
    storage::DefaultDB, structure::TransactionHash as LedgerTransactionHash,
    transient_crypto::hash::HashOutput,
};
use std::{convert::Into, sync::LazyLock};
use subxt::utils::H256;

pub const UT_ADDR_1_HEX: &str = "01020304";
pub const UT_ADDR_2_HEX: &str = "05060708";

pub static OWNER_ADDR_1: LazyLock<UnshieldedAddress> =
    LazyLock::new(|| const_hex::decode(UT_ADDR_1_HEX).unwrap().into());
pub static OWNER_ADDR_2: LazyLock<UnshieldedAddress> =
    LazyLock::new(|| const_hex::decode(UT_ADDR_2_HEX).unwrap().into());

pub static INTENT_HASH: LazyLock<IntentHash> = LazyLock::new(|| [0x11u8; 32].into());

pub static TOKEN_NIGHT: LazyLock<RawTokenType> = LazyLock::new(|| [0u8; 32].into());

pub fn token_type_to_hex(token: &RawTokenType) -> String {
    let bytes = token.as_ref();
    const_hex::encode(bytes)
}

pub fn sample_created_utxo(
    tx_id: u64,
    index: u32,
    owner: UnshieldedAddress,
    value_u128: u128,
) -> UnshieldedUtxo {
    UnshieldedUtxo {
        creating_transaction_id: tx_id,
        output_index: index,
        owner_address: owner,
        token_type: *TOKEN_NIGHT,
        intent_hash: *INTENT_HASH,
        value: value_u128,
    }
}

pub fn sample_spent_utxo() -> UnshieldedUtxo {
    UnshieldedUtxo {
        creating_transaction_id: 0,
        output_index: 0,
        owner_address: OWNER_ADDR_1.clone(),
        token_type: *TOKEN_NIGHT,
        intent_hash: *INTENT_HASH,
        value: 0,
    }
}

pub static BLOCK_0: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_0_HASH,
    height: 0,
    protocol_version: PROTOCOL_VERSION_0_1,
    parent_hash: ZERO_HASH,
    author: None,
    timestamp: 0,
    zswap_state_root: Faker.fake(),
    transactions: vec![],
});

pub static BLOCK_1: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_1_HASH,
    height: 1,
    protocol_version: PROTOCOL_VERSION_0_1,
    parent_hash: BLOCK_0_HASH,
    author: Some(BLOCK_1_AUTHOR.to_owned()),
    timestamp: 1,
    zswap_state_root: Faker.fake(),
    transactions: vec![Transaction {
        hash: TRANSACTION_1_HASH,
        protocol_version: PROTOCOL_VERSION_0_1,
        apply_stage: ApplyStage::Success,
        identifiers: vec![IDENTIFIER_1.to_owned()],
        raw: RAW_TRANSACTION_1.to_owned(),
        contract_actions: vec![ContractAction {
            address: ADDRESS.to_owned(),
            state: b"state".as_slice().into(),
            zswap_state: b"zswap_state".as_slice().into(),
            attributes: ContractAttributes::Deploy,
        }],
        merkle_tree_root: b"merkle_tree_root".as_slice().into(),
        created_unshielded_utxos: vec![sample_created_utxo(1, 0, OWNER_ADDR_1.clone(), 100)],
        spent_unshielded_utxos: vec![],
        start_index: 0,
        end_index: 1,
    }],
});

pub static BLOCK_1_B: LazyLock<Block> = LazyLock::new(|| Block {
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
            contract_actions: vec![ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: ContractAttributes::Deploy,
            }],
            merkle_tree_root: b"merkle_tree_root".as_slice().into(),
            created_unshielded_utxos: vec![], // Failed tx creates no UTXOs
            spent_unshielded_utxos: vec![],
            start_index: 0,
            end_index: 1,
        },
        Transaction {
            hash: TRANSACTION_1_HASH,
            protocol_version: PROTOCOL_VERSION_0_1,
            apply_stage: ApplyStage::Success,
            identifiers: vec![IDENTIFIER_1.to_owned()],
            raw: RAW_TRANSACTION_1.to_owned(),
            contract_actions: vec![ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: ContractAttributes::Deploy,
            }],
            merkle_tree_root: b"merkle_tree_root".as_slice().into(),
            created_unshielded_utxos: vec![sample_created_utxo(1, 0, OWNER_ADDR_1.clone(), 100)], /* Success part creates UTXO */
            spent_unshielded_utxos: vec![],
            start_index: 0,
            end_index: 1,
        },
    ],
});

pub static BLOCK_2: LazyLock<Block> = LazyLock::new(|| Block {
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
            ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: ContractAttributes::Call {
                    entry_point: b"entry_point".as_slice().into(),
                },
            },
            ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: ContractAttributes::Update,
            },
            ContractAction {
                address: ADDRESS.to_owned(),
                state: b"state".as_slice().into(),
                zswap_state: b"zswap_state".as_slice().into(),
                attributes: ContractAttributes::Call {
                    entry_point: b"entry_point".as_slice().into(),
                },
            },
        ],
        merkle_tree_root: b"merkle_tree_root".as_slice().into(),
        created_unshielded_utxos: vec![sample_created_utxo(2, 0, OWNER_ADDR_2.clone(), 50)],
        spent_unshielded_utxos: vec![sample_spent_utxo()],

        start_index: 2,
        end_index: 3,
    }],
});

pub static BLOCK_3: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_3_HASH,
    height: 3,
    protocol_version: PROTOCOL_VERSION_0_1,
    parent_hash: BLOCK_2_HASH,
    author: Some(BLOCK_3_AUTHOR.to_owned()),
    timestamp: 3,
    zswap_state_root: Faker.fake(),
    transactions: vec![Transaction {
        hash: TRANSACTION_3_HASH,
        protocol_version: PROTOCOL_VERSION_0_1,
        apply_stage: ApplyStage::Success,
        identifiers: vec![IDENTIFIER_3.to_owned()],
        raw: RAW_TRANSACTION_3.to_owned(),
        contract_actions: vec![ContractAction {
            address: ADDRESS.to_owned(),
            state: b"state".as_slice().into(),
            zswap_state: b"zswap_state".as_slice().into(),
            attributes: ContractAttributes::Update,
        }],
        merkle_tree_root: b"merkle_tree_root".as_slice().into(),
        created_unshielded_utxos: vec![],
        spent_unshielded_utxos: vec![],
        start_index: 4,
        end_index: 5,
    }],
});

pub static BLOCK_4: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_4_HASH,
    height: 4,
    protocol_version: PROTOCOL_VERSION_0_1,
    parent_hash: BLOCK_3_HASH,
    author: Some(BLOCK_4_AUTHOR.to_owned()),
    timestamp: 4,
    zswap_state_root: Faker.fake(),
    transactions: vec![Transaction {
        hash: TRANSACTION_4_HASH,
        protocol_version: PROTOCOL_VERSION_0_1,
        apply_stage: ApplyStage::Success,
        identifiers: vec![IDENTIFIER_4.to_owned()],
        raw: RAW_TRANSACTION_4.to_owned(),
        contract_actions: vec![ContractAction {
            address: ADDRESS.to_owned(),
            state: b"state".as_slice().into(),
            zswap_state: b"zswap_state".as_slice().into(),
            attributes: ContractAttributes::Call {
                entry_point: b"entry_point".as_slice().into(),
            },
        }],
        merkle_tree_root: b"merkle_tree_root".as_slice().into(),
        created_unshielded_utxos: vec![],
        spent_unshielded_utxos: vec![],
        start_index: 6,
        end_index: 7,
    }],
});

pub static BLOCK_5: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_5_HASH,
    height: 5,
    protocol_version: PROTOCOL_VERSION_0_1,
    parent_hash: BLOCK_4_HASH, // Ensure this matches the correct parent
    author: Some(BLOCK_5_AUTHOR.to_owned()),
    timestamp: 5,
    zswap_state_root: Faker.fake(),
    transactions: vec![Transaction {
        hash: TRANSACTION_5_HASH,
        protocol_version: PROTOCOL_VERSION_0_1,
        apply_stage: ApplyStage::Success,
        identifiers: vec![IDENTIFIER_5.to_owned()],
        raw: RAW_TRANSACTION_5.to_owned(),
        contract_actions: vec![ContractAction {
            address: ADDRESS.to_owned(),
            state: b"state_5".as_slice().into(),
            zswap_state: b"zswap_state_5".as_slice().into(),
            attributes: ContractAttributes::Update,
        }],
        merkle_tree_root: b"merkle_tree_root_5".as_slice().into(),
        created_unshielded_utxos: vec![],
        spent_unshielded_utxos: vec![],
        start_index: 8,
        end_index: 9,
    }],
});

pub static BLOCK_6: LazyLock<Block> = LazyLock::new(|| Block {
    hash: BLOCK_6_HASH,
    height: 6,
    protocol_version: PROTOCOL_VERSION_0_1,
    parent_hash: BLOCK_5_HASH, // Ensure this matches the correct parent
    author: Some(BLOCK_6_AUTHOR.to_owned()),
    timestamp: 6,
    zswap_state_root: Faker.fake(),
    transactions: vec![Transaction {
        hash: TRANSACTION_6_HASH,
        protocol_version: PROTOCOL_VERSION_0_1,
        apply_stage: ApplyStage::Success,
        identifiers: vec![IDENTIFIER_6.to_owned()],
        raw: RAW_TRANSACTION_6.to_owned(),
        contract_actions: vec![ContractAction {
            address: ADDRESS.to_owned(),
            state: b"state_6".as_slice().into(),
            zswap_state: b"zswap_state_6".as_slice().into(),
            attributes: ContractAttributes::Call {
                entry_point: b"entry_point_6".as_slice().into(),
            },
        }],
        merkle_tree_root: b"merkle_tree_root_6".as_slice().into(),
        created_unshielded_utxos: vec![],
        spent_unshielded_utxos: vec![],
        start_index: 10,
        end_index: 11,
    }],
});

pub const ZERO_HASH: BlockHash = BlockHash(H256::zero());

pub const BLOCK_0_HASH: BlockHash = BlockHash(H256([
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
]));

pub const BLOCK_1_HASH: BlockHash = BlockHash(H256([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
]));

pub const BLOCK_2_HASH: BlockHash = BlockHash(H256([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2,
]));

pub const BLOCK_3_HASH: BlockHash = BlockHash(H256([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3,
]));

pub const BLOCK_4_HASH: BlockHash = BlockHash(H256([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4,
]));

pub const BLOCK_5_HASH: BlockHash = BlockHash(H256([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5,
]));

pub const BLOCK_6_HASH: BlockHash = BlockHash(H256([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6,
]));

pub static BLOCK_1_AUTHOR: LazyLock<BlockAuthor> = LazyLock::new(|| [1; 32].into());
pub static BLOCK_2_AUTHOR: LazyLock<BlockAuthor> = LazyLock::new(|| [2; 32].into());
pub static BLOCK_3_AUTHOR: LazyLock<BlockAuthor> = LazyLock::new(|| [3; 32].into());
pub static BLOCK_4_AUTHOR: LazyLock<BlockAuthor> = LazyLock::new(|| [4; 32].into());
pub static BLOCK_5_AUTHOR: LazyLock<BlockAuthor> = LazyLock::new(|| [5; 32].into());
pub static BLOCK_6_AUTHOR: LazyLock<BlockAuthor> = LazyLock::new(|| [6; 32].into());

pub const TRANSACTION_1_HASH: TransactionHash =
    TransactionHash(LedgerTransactionHash(HashOutput([
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ])));

pub const TRANSACTION_2_HASH: TransactionHash =
    TransactionHash(LedgerTransactionHash(HashOutput([
        2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ])));

pub const TRANSACTION_3_HASH: TransactionHash =
    TransactionHash(LedgerTransactionHash(HashOutput([
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ])));

pub const TRANSACTION_4_HASH: TransactionHash =
    TransactionHash(LedgerTransactionHash(HashOutput([
        4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ])));

pub const TRANSACTION_5_HASH: TransactionHash =
    TransactionHash(LedgerTransactionHash(HashOutput([
        5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ])));

pub const TRANSACTION_6_HASH: TransactionHash =
    TransactionHash(LedgerTransactionHash(HashOutput([
        6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ])));

pub static IDENTIFIER_1: LazyLock<Identifier> = LazyLock::new(|| b"identifier-1".as_slice().into());

pub static IDENTIFIER_2: LazyLock<Identifier> = LazyLock::new(|| b"identifier-2".as_slice().into());

pub static IDENTIFIER_3: LazyLock<Identifier> = LazyLock::new(|| b"identifier-3".as_slice().into());

pub static IDENTIFIER_4: LazyLock<Identifier> = LazyLock::new(|| b"identifier-4".as_slice().into());

pub static IDENTIFIER_5: LazyLock<Identifier> = LazyLock::new(|| b"identifier-5".as_slice().into());
pub static IDENTIFIER_6: LazyLock<Identifier> = LazyLock::new(|| b"identifier-6".as_slice().into());

pub static RAW_TRANSACTION_1: LazyLock<RawTransaction> = LazyLock::new(|| {
    create_raw_transaction(NetworkId::Undeployed).expect("create raw transaction")
});

pub static RAW_TRANSACTION_2: LazyLock<RawTransaction> = LazyLock::new(|| {
    create_raw_transaction(NetworkId::Undeployed).expect("create raw transaction")
});

pub static RAW_TRANSACTION_3: LazyLock<RawTransaction> = LazyLock::new(|| {
    create_raw_transaction(NetworkId::Undeployed).expect("create raw transaction")
});

pub static RAW_TRANSACTION_4: LazyLock<RawTransaction> = LazyLock::new(|| {
    create_raw_transaction(NetworkId::Undeployed).expect("create raw transaction")
});

pub static RAW_TRANSACTION_5: LazyLock<RawTransaction> = LazyLock::new(|| {
    create_raw_transaction(NetworkId::Undeployed).expect("create raw transaction")
});

pub static RAW_TRANSACTION_6: LazyLock<RawTransaction> = LazyLock::new(|| {
    create_raw_transaction(NetworkId::Undeployed).expect("create raw transaction")
});

pub static HEX_ADDRESS: LazyLock<HexEncoded> = LazyLock::new(|| b"address".hex_encode());

pub static ADDRESS: LazyLock<ContractAddress> = LazyLock::new(|| b"address".as_slice().into());

pub static UNKNOWN_ADDRESS: LazyLock<ContractAddress> =
    LazyLock::new(|| b"unknown-address".as_slice().into());

pub static ZSWAP_STATE_1: LazyLock<RawZswapState> = LazyLock::new(|| {
    let bytes = midnight_ledger::zswap::ledger::State::<DefaultDB>::default()
        .serialize(NetworkId::Undeployed)
        .expect("zswap state");
    bytes.as_slice().into()
});

pub static ZSWAP_STATE_2: LazyLock<RawZswapState> = LazyLock::new(|| {
    let bytes = midnight_ledger::zswap::ledger::State::<DefaultDB>::default()
        .serialize(NetworkId::Undeployed)
        .expect("zswap state");
    bytes.as_slice().into()
});

pub static ZSWAP_STATE_3: LazyLock<RawZswapState> = LazyLock::new(|| {
    let bytes = midnight_ledger::zswap::ledger::State::<DefaultDB>::default()
        .serialize(NetworkId::Undeployed)
        .expect("zswap state");
    bytes.as_slice().into()
});
