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

//! Tests for DUST GraphQL endpoints

use indexer_api::infra::api::v1::export_schema;
use indexer_common::domain::AddressType;

#[test]
fn test_dust_schema_exports() {
    let schema = export_schema();

    // Verify that DUST types are included in the schema
    assert!(schema.contains("DustSystemState"));
    assert!(schema.contains("DustGenerationStatus"));
    assert!(schema.contains("DustMerkleTreeType"));
    assert!(schema.contains("DustGenerationEvent"));
    assert!(schema.contains("DustNullifierTransactionEvent"));
    assert!(schema.contains("DustCommitmentEvent"));
    assert!(schema.contains("RegistrationUpdateEvent"));

    // Verify that DUST queries are included
    assert!(schema.contains("currentDustState"));
    assert!(schema.contains("dustGenerationStatus"));
    assert!(schema.contains("dustMerkleRoot"));

    // Verify that DUST subscriptions are included
    assert!(schema.contains("dustGenerations"));
    assert!(schema.contains("dustNullifierTransactions"));
    assert!(schema.contains("dustCommitments"));
    assert!(schema.contains("registrationUpdates"));
}

#[test]
fn test_address_type_conversion() {
    let address_type = AddressType::Dust;
    assert_eq!(format!("{address_type:?}"), "Dust");

    let address_type = AddressType::Night;
    assert_eq!(format!("{address_type:?}"), "Night");

    let address_type = AddressType::CardanoStake;
    assert_eq!(format!("{address_type:?}"), "CardanoStake");
}

#[cfg(test)]
mod integration_tests {
    use indexer_api::infra::api::v1::dust::{
        AddressTypeGraphQL, DustMerkleTreeTypeGraphQL, RegistrationAddress,
    };
    use indexer_common::domain::{AddressType, DustMerkleTreeType};

    #[test]
    fn test_dust_merkle_tree_type_conversion() {
        let commitment_type = DustMerkleTreeTypeGraphQL::Commitment;
        let domain_type: DustMerkleTreeType = commitment_type.into();
        assert_eq!(domain_type, DustMerkleTreeType::Commitment);

        let generation_type = DustMerkleTreeTypeGraphQL::Generation;
        let domain_type: DustMerkleTreeType = generation_type.into();
        assert_eq!(domain_type, DustMerkleTreeType::Generation);
    }

    #[test]
    fn test_address_type_graphql_conversion() {
        let dust_type = AddressTypeGraphQL::Dust;
        let domain_type: AddressType = dust_type.into();
        assert_eq!(domain_type, AddressType::Dust);

        let night_type = AddressTypeGraphQL::Night;
        let domain_type: AddressType = night_type.into();
        assert_eq!(domain_type, AddressType::Night);

        let cardano_type = AddressTypeGraphQL::CardanoStake;
        let domain_type: AddressType = cardano_type.into();
        assert_eq!(domain_type, AddressType::CardanoStake);
    }

    #[test]
    fn test_registration_address_structure() {
        let reg_address = RegistrationAddress {
            address_type: AddressTypeGraphQL::Dust,
            value: "test_address".to_owned(),
        };

        assert_eq!(reg_address.value, "test_address");
        assert_eq!(reg_address.address_type, AddressTypeGraphQL::Dust);
    }
}

#[cfg(test)]
mod mock_tests {
    use indexer_api::domain::{DustGenerationStatus, DustSystemState, storage::dust::DustStorage};
    use indexer_common::domain::{AddressType, DustMerkleTreeType};
    use std::num::NonZeroU32;

    #[derive(Debug, Clone)]
    struct MockStorage;

    impl indexer_api::domain::storage::block::BlockStorage for MockStorage {
        async fn get_latest_block(
            &self,
        ) -> Result<Option<indexer_api::domain::Block>, sqlx::Error> {
            Ok(None)
        }

        async fn get_block_by_hash(
            &self,
            _hash: indexer_common::domain::BlockHash,
        ) -> Result<Option<indexer_api::domain::Block>, sqlx::Error> {
            Ok(None)
        }

        async fn get_block_by_height(
            &self,
            _height: u32,
        ) -> Result<Option<indexer_api::domain::Block>, sqlx::Error> {
            Ok(None)
        }

        fn get_blocks(
            &self,
            _height: u32,
            _batch_size: NonZeroU32,
        ) -> impl futures::Stream<Item = Result<indexer_api::domain::Block, sqlx::Error>> {
            futures::stream::empty()
        }
    }

    impl indexer_api::domain::storage::transaction::TransactionStorage for MockStorage {
        async fn get_transactions_by_hash(
            &self,
            _hash: indexer_common::domain::TransactionHash,
        ) -> Result<Vec<indexer_api::domain::Transaction>, sqlx::Error> {
            Ok(vec![])
        }

        async fn get_transactions_by_identifier(
            &self,
            _identifier: &indexer_common::domain::RawTransactionIdentifier,
        ) -> Result<Vec<indexer_api::domain::Transaction>, sqlx::Error> {
            Ok(vec![])
        }

        fn get_transactions_involving_unshielded(
            &self,
            _address: indexer_common::domain::RawUnshieldedAddress,
            _transaction_id: u64,
            _batch_size: NonZeroU32,
        ) -> impl futures::Stream<Item = Result<indexer_api::domain::Transaction, sqlx::Error>>
        {
            futures::stream::empty()
        }

        async fn get_highest_transaction_id_for_unshielded_address(
            &self,
            _address: indexer_common::domain::RawUnshieldedAddress,
        ) -> Result<Option<u64>, sqlx::Error> {
            Ok(None)
        }

        async fn get_transaction_by_id(
            &self,
            _id: u64,
        ) -> Result<Option<indexer_api::domain::Transaction>, sqlx::Error> {
            Ok(None)
        }

        async fn get_transactions_by_block_id(
            &self,
            _block_id: u64,
        ) -> Result<Vec<indexer_api::domain::Transaction>, sqlx::Error> {
            Ok(vec![])
        }

        fn get_relevant_transactions(
            &self,
            _session_id: indexer_common::domain::SessionId,
            _index: u64,
            _batch_size: NonZeroU32,
        ) -> impl futures::Stream<Item = Result<indexer_api::domain::Transaction, sqlx::Error>>
        {
            futures::stream::empty()
        }

        async fn get_highest_end_indices(
            &self,
            _session_id: indexer_common::domain::SessionId,
        ) -> Result<(Option<u64>, Option<u64>, Option<u64>), sqlx::Error> {
            Ok((None, None, None))
        }
    }

    impl DustStorage for MockStorage {
        async fn get_current_dust_state(&self) -> Result<DustSystemState, sqlx::Error> {
            Ok(DustSystemState {
                commitment_tree_root: "test_commitment_root".to_owned(),
                generation_tree_root: "test_generation_root".to_owned(),
                block_height: 100,
                timestamp: 1234567890,
                total_registrations: 5,
            })
        }

        async fn get_dust_generation_status_batch(
            &self,
            stake_keys: &[String],
        ) -> Result<Vec<DustGenerationStatus>, sqlx::Error> {
            Ok(stake_keys
                .iter()
                .map(|key| DustGenerationStatus {
                    cardano_stake_key: key.clone(),
                    dust_address: Some("test_dust_address".to_owned()),
                    is_registered: true,
                    generation_rate: "100".to_owned(),
                    current_capacity: "1000".to_owned(),
                    night_balance: "5000".to_owned(),
                })
                .collect())
        }

        async fn get_dust_merkle_root_at_timestamp(
            &self,
            _tree_type: DustMerkleTreeType,
            _timestamp: i64,
        ) -> Result<Option<Vec<u8>>, sqlx::Error> {
            Ok(Some(vec![0u8; 32]))
        }

        fn get_dust_generations(
            &self,
            _dust_address: &str,
            _from_generation_index: i64,
            _from_merkle_index: i64,
            _only_active: bool,
            _batch_size: NonZeroU32,
        ) -> impl futures::Stream<Item = Result<indexer_api::domain::DustGenerationEvent, sqlx::Error>>
        {
            futures::stream::empty()
        }

        fn get_dust_nullifier_transactions(
            &self,
            _prefixes: &[String],
            _min_prefix_length: usize,
            _from_block: i64,
            _batch_size: NonZeroU32,
        ) -> impl futures::Stream<
            Item = Result<indexer_api::domain::DustNullifierTransactionEvent, sqlx::Error>,
        > {
            futures::stream::empty()
        }

        fn get_dust_commitments(
            &self,
            _commitment_prefixes: &[String],
            _min_prefix_length: usize,
            _start_index: i64,
            _batch_size: NonZeroU32,
        ) -> impl futures::Stream<Item = Result<indexer_api::domain::DustCommitmentEvent, sqlx::Error>>
        {
            futures::stream::empty()
        }

        fn get_registration_updates(
            &self,
            _addresses: &[(AddressType, String)],
            _from_timestamp: i64,
            _batch_size: NonZeroU32,
        ) -> impl futures::Stream<Item = Result<indexer_api::domain::RegistrationUpdateEvent, sqlx::Error>>
        {
            futures::stream::empty()
        }
    }

    #[tokio::test]
    async fn test_mock_dust_storage() {
        let storage = MockStorage;

        // Test get_current_dust_state
        let state = storage.get_current_dust_state().await.unwrap();
        assert_eq!(state.commitment_tree_root, "test_commitment_root");
        assert_eq!(state.generation_tree_root, "test_generation_root");
        assert_eq!(state.block_height, 100);
        assert_eq!(state.timestamp, 1234567890);
        assert_eq!(state.total_registrations, 5);

        // Test get_dust_generation_status_batch
        let stake_keys = vec!["stake_key_1".to_owned(), "stake_key_2".to_owned()];
        let statuses = storage
            .get_dust_generation_status_batch(&stake_keys)
            .await
            .unwrap();
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].cardano_stake_key, "stake_key_1");
        assert_eq!(statuses[1].cardano_stake_key, "stake_key_2");
        assert!(statuses[0].is_registered);
        assert!(statuses[1].is_registered);

        // Test get_dust_merkle_root_at_timestamp
        let root = storage
            .get_dust_merkle_root_at_timestamp(DustMerkleTreeType::Commitment, 1234567890)
            .await
            .unwrap();
        assert!(root.is_some());
        assert_eq!(root.unwrap().len(), 32);
    }
}
