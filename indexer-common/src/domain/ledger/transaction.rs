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

use crate::domain::{
    ByteArray, ByteVec, NetworkId, PROTOCOL_VERSION_000_013_000, ProtocolVersion, ViewingKey,
    ledger::{Error, LedgerTransactionV5, NetworkIdExt, SerializableV5Ext},
};
use fastrace::trace;
use futures::{StreamExt, TryStreamExt};
use midnight_coin_structure::{
    coin::Info as InfoV5, contract::ContractAddress as ContractAddressV5,
};
use midnight_ledger::structure::{
    ContractAction as ContractActionV5, StandardTransaction as StandardTransactionV5,
    Transaction as TransactionV5,
};
use midnight_serialize::deserialize as deserialize_v5;
use midnight_storage::DefaultDB as DefaultDBV5;
use midnight_transient_crypto::{encryption::SecretKey as SecretKeyV5, proofs::Proof as ProofV5};
use midnight_zswap::Offer as OfferV5;
use std::error::Error as StdError;

pub type SerializedContractAddress = ByteVec;
pub type SerializedContractEntryPoint = ByteVec;
pub type SerializedContractState = ByteVec;
pub type SerializedTransactionIdentifier = ByteVec;
pub type TransactionHash = ByteArray<32>;

/// Facade for `Transaction` from `midnight_ledger` across supported (protocol) versions.
#[derive(Debug, Clone)]
pub enum Transaction {
    V5(LedgerTransactionV5),
}

impl Transaction {
    /// Deserialize the given raw transaction using the given protocol version and network ID.
    #[trace(properties = {
        "network_id": "{network_id}",
        "protocol_version": "{protocol_version}"
    })]
    pub fn deserialize(
        raw_transaction: impl AsRef<[u8]>,
        network_id: NetworkId,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error> {
        if protocol_version.is_compatible(PROTOCOL_VERSION_000_013_000) {
            let transaction =
                deserialize_v5(&mut raw_transaction.as_ref(), network_id.into_ledger_v5())
                    .map_err(|error| Error::Io("cannot deserialize LedgerTransactionV5", error))?;
            Ok(Self::V5(transaction))
        } else {
            Err(Error::InvalidProtocolVersion(protocol_version))
        }
    }

    /// Get the hash.
    pub fn hash(&self) -> TransactionHash {
        match self {
            Transaction::V5(transaction) => transaction.transaction_hash().0.0.into(),
        }
    }

    /// Get the identifiers.
    pub fn identifiers(
        &self,
        network_id: NetworkId,
    ) -> Result<Vec<SerializedTransactionIdentifier>, Error> {
        match self {
            Transaction::V5(transaction) => transaction
                .identifiers()
                .map(|identifier| {
                    let identifier = identifier.serialize(network_id).map_err(|error| {
                        Error::Io("cannot serialize TransactionIdentifierV5", error)
                    })?;
                    Ok(identifier.into())
                })
                .collect(),
        }
    }

    /// Get the contract actions; this involves node calls.
    #[trace(properties = { "network_id": "{network_id}" })]
    pub async fn contract_actions<E, F>(
        &self,
        get_contract_state: impl Fn(SerializedContractAddress) -> F,
        network_id: NetworkId,
    ) -> Result<Vec<ContractAction>, Error>
    where
        E: StdError + 'static + Send + Sync,
        F: Future<Output = Result<SerializedContractState, E>>,
    {
        match self {
            Transaction::V5(transaction) => match transaction {
                TransactionV5::Standard(standard_transaction) => {
                    let contract_actions = futures::stream::iter(standard_transaction.actions())
                        .then(|(_, contract_action)| async {
                            match contract_action {
                                ContractActionV5::Deploy(deploy) => {
                                    let address =
                                        serialize_contract_address(deploy.address(), network_id)?;
                                    let state = get_contract_state(address.clone())
                                        .await
                                        .map_err(|error| Error::GetContractState(error.into()))?;

                                    Ok::<_, Error>(ContractAction {
                                        address,
                                        state,
                                        attributes: ContractAttributes::Deploy,
                                    })
                                }

                                ContractActionV5::Call(call) => {
                                    let address =
                                        serialize_contract_address(call.address, network_id)?;
                                    let state = get_contract_state(address.clone())
                                        .await
                                        .map_err(|error| Error::GetContractState(error.into()))?;
                                    let entry_point = call
                                        .entry_point
                                        .serialize(network_id)
                                        .map_err(|error| {
                                            Error::Io("cannot serialize EntryPointBufV5", error)
                                        })?
                                        .into();

                                    Ok(ContractAction {
                                        address,
                                        state,
                                        attributes: ContractAttributes::Call { entry_point },
                                    })
                                }

                                ContractActionV5::Maintain(update) => {
                                    let address =
                                        serialize_contract_address(update.address, network_id)?;
                                    let state = get_contract_state(address.clone())
                                        .await
                                        .map_err(|error| Error::GetContractState(error.into()))?;

                                    Ok(ContractAction {
                                        address,
                                        state,
                                        attributes: ContractAttributes::Update,
                                    })
                                }
                            }
                        })
                        .try_collect::<Vec<_>>()
                        .await?;

                    Ok(contract_actions)
                }

                TransactionV5::ClaimMint(_) => Ok(vec![]),
            },
        }
    }

    /// Get the structure of this transaction for fees calculation.
    pub fn structure(&self, size: usize) -> TransactionStructure {
        match self {
            Transaction::V5(transaction) => match transaction {
                LedgerTransactionV5::Standard(standard_transaction) => {
                    let contract_action_count = standard_transaction.actions().count();
                    let identifier_count = transaction.identifiers().count();

                    let segment_count = if contract_action_count > 1 || identifier_count > 2 {
                        2
                    } else {
                        1
                    };
                    let estimated_input_count = identifier_count.max(1);
                    let estimated_output_count = (identifier_count + 1).max(1);
                    let has_contract_operations = contract_action_count > 0;

                    TransactionStructure {
                        segment_count,
                        estimated_input_count,
                        estimated_output_count,
                        has_contract_operations,
                        size,
                    }
                }

                LedgerTransactionV5::ClaimMint(_) => TransactionStructure {
                    segment_count: 1,
                    estimated_input_count: 1,
                    estimated_output_count: 1,
                    has_contract_operations: false,
                    size,
                },
            },
        }
    }

    // Check if this transaction belongs to the given viewing key.
    pub fn relevant(&self, viewing_key: ViewingKey) -> bool {
        match self {
            Transaction::V5(transaction) => match transaction {
                TransactionV5::Standard(StandardTransactionV5 {
                    guaranteed_coins,
                    fallible_coins,
                    ..
                }) => {
                    let secret_key = SecretKeyV5::from_repr(&viewing_key.expose_secret().0)
                        .expect("SecretKeyV5 can be created from repr");

                    let can_decrypt_guaranteed_coins = guaranteed_coins
                        .as_ref()
                        .map(|guaranteed_coins| can_decrypt_v5(&secret_key, guaranteed_coins))
                        .unwrap_or(true);

                    let can_decrypt_fallible_coins = fallible_coins
                        .values()
                        .all(|fallible_coins| can_decrypt_v5(&secret_key, &fallible_coins));

                    can_decrypt_guaranteed_coins && can_decrypt_fallible_coins
                }

                TransactionV5::ClaimMint(_) => false,
            },
        }
    }
}

/// A contract action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractAction {
    pub address: SerializedContractAddress,
    pub state: SerializedContractState,
    pub attributes: ContractAttributes,
}

/// Attributes for a specific contract action.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum ContractAttributes {
    Deploy,
    Call {
        entry_point: SerializedContractEntryPoint,
    },
    Update,
}

/// Transaction structure for fees calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionStructure {
    pub segment_count: usize,
    pub estimated_input_count: usize,
    pub estimated_output_count: usize,
    pub has_contract_operations: bool,
    pub size: usize,
}

fn serialize_contract_address(
    address: ContractAddressV5,
    network_id: NetworkId,
) -> Result<SerializedContractAddress, Error> {
    let address = address
        .serialize(network_id)
        .map_err(|error| Error::Io("cannot serialize ContractAddressV5", error))?;
    Ok(address.into())
}

fn can_decrypt_v5(key: &SecretKeyV5, offer: &OfferV5<ProofV5, DefaultDBV5>) -> bool {
    let outputs = offer.outputs.iter().filter_map(|o| o.ciphertext.clone());
    let transient = offer.transient.iter().filter_map(|o| o.ciphertext.clone());
    let mut ciphertexts = outputs.chain(transient);

    ciphertexts.any(|ciphertext| {
        key.decrypt::<InfoV5>(&(*ciphertext).to_owned().into())
            .is_some()
    })
}

#[cfg(test)]
mod tests {
    use crate::domain::{NetworkId, PROTOCOL_VERSION_000_013_000, ViewingKey, ledger::Transaction};
    use bip32::{DerivationPath, XPrv};
    use midnight_zswap::keys::{SecretKeys, Seed};
    use std::{fs, str::FromStr};

    /// Notice: The raw test data is created with `generate_txs.sh`.
    #[test]
    fn test_deserialize_relevant() {
        let raw_transaction =
            fs::read(format!("{}/tests/tx_1_2_2.raw", env!("CARGO_MANIFEST_DIR")))
                .expect("transaction file can be read");
        let transaction = Transaction::deserialize(
            raw_transaction,
            NetworkId::Undeployed,
            PROTOCOL_VERSION_000_013_000,
        )
        .expect("transaction can be deserialized");

        assert!(transaction.relevant(viewing_key(1)));
        assert!(transaction.relevant(viewing_key(2)));
        assert!(!transaction.relevant(viewing_key(3)));

        let raw_transaction =
            fs::read(format!("{}/tests/tx_1_2_3.raw", env!("CARGO_MANIFEST_DIR")))
                .expect("transaction file can be read");
        let transaction = Transaction::deserialize(
            raw_transaction,
            NetworkId::Undeployed,
            PROTOCOL_VERSION_000_013_000,
        )
        .expect("transaction can be deserialized");

        assert!(transaction.relevant(viewing_key(1)));
        assert!(transaction.relevant(viewing_key(2)));
        assert!(!transaction.relevant(viewing_key(3)));
    }

    fn viewing_key(n: u8) -> ViewingKey {
        let mut seed = [0; 32];
        seed[31] = n;

        let derivation_path =
            DerivationPath::from_str("m/44'/2400'/0'/3/0").expect("derivation path can be created");
        let derived_seed: [u8; 32] = XPrv::derive_from_path(seed, &derivation_path)
            .expect("key can be derived")
            .private_key()
            .to_bytes()
            .into();

        SecretKeys::from(Seed::from(derived_seed))
            .encryption_secret_key
            .repr()
            .into()
    }
}
