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

pub mod storage;

use fastrace::trace;
use indexer_common::{
    LedgerTransaction,
    domain::{NetworkId, RawTransaction, ViewingKey},
};
use midnight_coin_structure::coin::Info;
use midnight_ledger::structure::StandardTransaction;
use midnight_serialize::deserialize;
use midnight_storage::{DefaultDB, arena::Sp};
use midnight_transient_crypto::{encryption::SecretKey, proofs::Proof};
use midnight_zswap::Offer;
use sqlx::prelude::FromRow;
use std::io;
use thiserror::Error;

/// Relevant data of a wallet from the perspective of the Wallet Indexer.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Wallet {
    pub viewing_key: ViewingKey,
    pub last_indexed_transaction_id: u64,
}

/// Relevant data of a transaction from the perspective of the Wallet Indexer.
#[derive(Debug, Clone, FromRow)]
pub struct Transaction {
    #[sqlx(try_from = "i64")]
    pub id: u64,
    pub raw: RawTransaction,
}

impl Transaction {
    /// Check the relevance of this transaction for the given wallet and the given network ID.
    #[trace]
    pub fn relevant(
        &self,
        wallet: &Wallet,
        network_id: NetworkId,
    ) -> Result<bool, TransactionIsRelevantError> {
        let ledger_transaction = self
            .deserialize(network_id)
            .map_err(TransactionIsRelevantError::DeserializeTransaction)?;

        let relevant = match ledger_transaction {
            LedgerTransaction::Standard(StandardTransaction {
                guaranteed_coins,
                fallible_coins,
                ..
            }) => {
                let secret_key = &wallet.viewing_key.into();

                let can_decrypt_guaranteed_coins = guaranteed_coins
                    .and_then(Sp::into_inner)
                    .map(|guaranteed_coins| can_decrypt(secret_key, guaranteed_coins))
                    .unwrap_or(true);

                let can_decrypt_fallible_coins = fallible_coins
                    .into_iter()
                    .map(|(_, f)| f)
                    .all(|fallible_coins| can_decrypt(secret_key, fallible_coins));

                can_decrypt_guaranteed_coins && can_decrypt_fallible_coins
            }

            LedgerTransaction::ClaimMint(_) => false,
        };

        Ok(relevant)
    }

    fn deserialize(&self, network_id: NetworkId) -> Result<LedgerTransaction, io::Error> {
        deserialize::<LedgerTransaction, _>(&mut self.raw.as_ref(), network_id.into())
    }
}

#[derive(Debug, Error)]
pub enum TransactionIsRelevantError {
    #[error("cannot deserialize transaction")]
    DeserializeTransaction(#[source] io::Error),

    #[error("cannot deserialize viewing key`")]
    DeserializeViewingKey(#[source] io::Error),
}

fn can_decrypt(key: &SecretKey, offer: Offer<Proof, DefaultDB>) -> bool {
    let outputs = offer
        .outputs
        .iter()
        .filter_map(|o| o.ciphertext.as_ref().cloned().and_then(Sp::into_inner));
    let transient = offer
        .transient
        .iter()
        .filter_map(|o| o.ciphertext.as_ref().cloned().and_then(Sp::into_inner));
    let mut ciphertexts = outputs.chain(transient);
    ciphertexts.any(|ciphertext| key.decrypt::<Info>(&ciphertext.into()).is_some())
}

// TODO Find a way to test this for ledger v5.
// #[cfg(test)]
// mod tests {
//     use crate::domain::{Transaction, Wallet};
//     use assert_matches::assert_matches;
//     use chacha20poly1305::aead::rand_core::OsRng;
//     use futures::executor::block_on;
//     use indexer_common::{
//         domain::{NetworkId, RawTransaction, ViewingKey},
//         error::BoxError,
//         serialize::SerializableExt,
//     };
//     use midnight_base_crypto::{
//         data_provider::{FetchMode, MidnightDataProvider, OutputMode},
//         signatures::Signature,
//     };
//     use midnight_ledger::{
//         prove::{ExternalResolver, Resolver},
//         structure::{ProofMarker, Transaction as LedgerTransaction},
//     };
//     use midnight_storage::{DefaultDB, storage};
//     use midnight_transient_crypto::{commitment::PureGeneratorPedersen, proofs::ProofPreimage};
//     use midnight_zswap::{Offer, ZSWAP_EXPECTED_FILES, prove::ZswapResolver};
//     use std::collections::HashMap;

//     #[tokio::test]
//     async fn test_is_relevant() -> Result<(), BoxError> {
//         let viewing_key = ViewingKey::make_for_testing_yes_i_know_what_i_am_doing();

//         let raw = create_raw_transaction(NetworkId::Undeployed)?;
//         let transaction = Transaction { id: 42, raw };

//         let wallet = Wallet {
//             viewing_key,
//             last_indexed_transaction_id: 0,
//         };

//         let relevant = transaction.relevant(&wallet, NetworkId::Undeployed);
//         assert_matches!(relevant, Ok(false));

//         Ok(())
//     }

//     fn create_raw_transaction(network_id: NetworkId) -> Result<RawTransaction, BoxError> {
//         // let empty_offer = Offer::<ProofPreimage, DefaultDB> {
//         //     inputs: StorageVec::default(),
//         //     outputs: StorageVec::default(),
//         //     transient: StorageVec::default(),
//         //     deltas: StorageVec::default(),
//         // };
//         let pre_transaction = LedgerTransaction::<
//             Signature,
//             ProofMarker,
//             PureGeneratorPedersen,
//             DefaultDB,
//         >::new(storage::HashMap::new(), None, HashMap::new());

//         let zswap_resolver = ZswapResolver(MidnightDataProvider::new(
//             FetchMode::OnDemand,
//             OutputMode::Log,
//             ZSWAP_EXPECTED_FILES.to_owned(),
//         ));
//         let external_resolver: ExternalResolver =
//             Box::new(|_| Box::pin(std::future::ready(Ok(None))));
//         let resolver = Resolver::new(zswap_resolver, external_resolver);

//         let transaction = block_on(pre_transaction.prove(OsRng, &resolver, &resolver))?;
//         let raw_transaction = transaction.serialize(network_id)?.into();

//         Ok(raw_transaction)
//     }
// }
