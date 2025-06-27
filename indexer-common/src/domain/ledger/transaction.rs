use crate::domain::{
    ContractAction, ContractAttributes, NetworkId, PROTOCOL_VERSION_000_013_000, ProtocolVersion,
    RawContractAddress, RawContractState, RawTransactionIdentifier, TransactionHash,
    TransactionStructure, ViewingKey,
    ledger::{ContractState, Error, LedgerTransactionV5, NetworkIdExt, SerializableV5Ext},
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
use midnight_storage::{DefaultDB as DefaultDBV5, arena::Sp as SpV5};
use midnight_transient_crypto::{encryption::SecretKey as SecretKeyV5, proofs::Proof as ProofV5};
use midnight_zswap::Offer as OfferV5;
use std::{error::Error as StdError, future::Future};

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
    ) -> Result<Vec<RawTransactionIdentifier>, Error> {
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
        get_contract_state: impl Fn(RawContractAddress) -> F,
        network_id: NetworkId,
        protocol_version: ProtocolVersion,
    ) -> Result<Vec<ContractAction>, Error>
    where
        E: StdError + 'static + Send + Sync,
        F: Future<Output = Result<RawContractState, E>>,
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

                                    // Extract balances from contract state
                                    let extracted_balances = ContractState::deserialize(
                                        &state,
                                        network_id,
                                        protocol_version,
                                    )?
                                    .balances(network_id)?;

                                    Ok::<_, Error>(ContractAction {
                                        address,
                                        state,
                                        attributes: ContractAttributes::Deploy,
                                        extracted_balances,
                                    })
                                }

                                ContractActionV5::Call(call) => {
                                    let address =
                                        serialize_contract_address(call.address, network_id)?;
                                    let state = get_contract_state(address.clone())
                                        .await
                                        .map_err(|error| Error::GetContractState(error.into()))?;
                                    let entry_point = call.entry_point.as_ref().into();

                                    // Extract balances from contract state
                                    let extracted_balances = ContractState::deserialize(
                                        &state,
                                        network_id,
                                        protocol_version,
                                    )?
                                    .balances(network_id)?;

                                    Ok(ContractAction {
                                        address,
                                        state,
                                        attributes: ContractAttributes::Call { entry_point },
                                        extracted_balances,
                                    })
                                }

                                ContractActionV5::Maintain(update) => {
                                    let address =
                                        serialize_contract_address(update.address, network_id)?;
                                    let state = get_contract_state(address.clone())
                                        .await
                                        .map_err(|error| Error::GetContractState(error.into()))?;

                                    // Extract balances from contract state
                                    let extracted_balances = ContractState::deserialize(
                                        &state,
                                        network_id,
                                        protocol_version,
                                    )?
                                    .balances(network_id)?;

                                    Ok(ContractAction {
                                        address,
                                        state,
                                        attributes: ContractAttributes::Update,
                                        extracted_balances,
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
                        .expect("SecretKey can be created from repr");

                    let can_decrypt_guaranteed_coins = guaranteed_coins
                        .as_ref()
                        .cloned()
                        .and_then(SpV5::into_inner)
                        .map(|guaranteed_coins| can_decrypt_v5(&secret_key, guaranteed_coins))
                        .unwrap_or(true);

                    let can_decrypt_fallible_coins = fallible_coins
                        .values()
                        .all(|fallible_coins| can_decrypt_v5(&secret_key, fallible_coins));

                    can_decrypt_guaranteed_coins && can_decrypt_fallible_coins
                }

                TransactionV5::ClaimMint(_) => false,
            },
        }
    }
}

fn serialize_contract_address(
    address: ContractAddressV5,
    network_id: NetworkId,
) -> Result<RawContractAddress, Error> {
    let address = address
        .serialize(network_id)
        .map_err(|error| Error::Io("cannot serialize ContractAddressV5", error))?;
    Ok(address.into())
}

fn can_decrypt_v5(key: &SecretKeyV5, offer: OfferV5<ProofV5, DefaultDBV5>) -> bool {
    let outputs = offer
        .outputs
        .iter()
        .filter_map(|o| o.ciphertext.as_ref().cloned().and_then(SpV5::into_inner));

    let transient = offer
        .transient
        .iter()
        .filter_map(|o| o.ciphertext.as_ref().cloned().and_then(SpV5::into_inner));

    let mut ciphertexts = outputs.chain(transient);

    ciphertexts.any(|ciphertext| key.decrypt::<InfoV5>(&ciphertext.into()).is_some())
}
