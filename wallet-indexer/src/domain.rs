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
use indexer_common::domain::{NetworkId, RawTransaction, ViewingKey};
use midnight_ledger::{
    coin_structure::coin::Info,
    serialize::deserialize,
    storage::DefaultDB,
    structure::{Proof, StandardTransaction, Transaction as LedgerTransaction},
    transient_crypto::encryption::SecretKey,
    zswap::Offer,
};
use sqlx::prelude::FromRow;
use std::io;
use thiserror::Error;

/// Relevant data of a wallet from the perspective of the Wallet Indexer.
#[derive(Debug, Clone, FromRow)]
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
                can_decrypt(secret_key, guaranteed_coins)
                    || fallible_coins
                        .map(|fallible_coins| can_decrypt(secret_key, fallible_coins))
                        .unwrap_or_default()
            }

            LedgerTransaction::ClaimMint(_) => false,
        };

        Ok(relevant)
    }

    fn deserialize(
        &self,
        network_id: NetworkId,
    ) -> Result<LedgerTransaction<Proof, DefaultDB>, io::Error> {
        deserialize::<LedgerTransaction<Proof, DefaultDB>, _>(
            &mut self.raw.as_ref(),
            network_id.into(),
        )
    }
}

#[derive(Debug, Error)]
pub enum TransactionIsRelevantError {
    #[error("cannot deserialize transaction")]
    DeserializeTransaction(#[source] std::io::Error),

    #[error("cannot deserialize viewing key`")]
    DeserializeViewingKey(#[source] std::io::Error),
}

fn can_decrypt<P>(key: &SecretKey, offer: Offer<P>) -> bool {
    let outputs = offer.outputs.into_iter().filter_map(|o| o.ciphertext);
    let transient = offer.transient.into_iter().filter_map(|t| t.ciphertext);
    let mut ciphertexts = outputs.chain(transient);
    ciphertexts.any(|ciphertext| key.decrypt::<Info>(&ciphertext.into()).is_some())
}

#[cfg(test)]
mod tests {
    use crate::domain::{Transaction, Wallet};
    use assert_matches::assert_matches;
    use chacha20poly1305::aead::rand_core::OsRng;
    use futures::executor::block_on;
    use indexer_common::{
        domain::{NetworkId, RawTransaction, ViewingKey},
        error::BoxError,
        serialize::SerializableExt,
    };
    use midnight_ledger::{
        base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode},
        prove::{ExternalResolver, Resolver},
        storage::DefaultDB,
        structure::Transaction as LedgerTransaction,
        transient_crypto::proofs::ProofPreimage,
        zswap::{Offer, ZSWAP_EXPECTED_FILES, prove::ZswapResolver},
    };

    #[tokio::test]
    async fn test_is_relevant() -> Result<(), BoxError> {
        let viewing_key = ViewingKey::make_for_testing_yes_i_know_what_i_am_doing();

        let raw = create_raw_transaction(NetworkId::Undeployed)?;
        let transaction = Transaction { id: 42, raw };

        let wallet = Wallet {
            viewing_key,
            last_indexed_transaction_id: 0,
        };

        let relevant = transaction.relevant(&wallet, NetworkId::Undeployed);
        assert_matches!(relevant, Ok(false));

        Ok(())
    }

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
        let external_resolver: ExternalResolver =
            Box::new(|_| Box::pin(std::future::ready(Ok(None))));
        let resolver = Resolver::new(zswap_resolver, external_resolver);

        let transaction = block_on(pre_transaction.prove(OsRng, &resolver, &resolver))?;
        let raw_transaction = transaction.serialize(network_id)?.into();

        Ok(raw_transaction)
    }
}
