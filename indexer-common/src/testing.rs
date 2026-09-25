// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
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

//! Test support for building ledger transactions in code, classifying why the ledger rejects them, and initializing the ledger DB.

#[cfg(any(feature = "cloud", feature = "standalone"))]
use crate::infra::ledger_db;
use crate::{
    domain::{LedgerVersion, SerializedTransaction, ledger::TaggedSerializableExt},
    error::BoxError,
    infra::ledger_db::v1_1::LedgerDb,
};
use midnight_base_crypto_v1::{
    signatures::{Signature as SignatureV8, SigningKey as SchnorrSigningKey},
    time::Timestamp,
};
use midnight_ledger_v8::{
    dust::{
        DustActions as DustActionsV8, DustPublicKey as DustPublicKeyV8,
        DustRegistration as DustRegistrationV8, DustSecretKey as DustSecretKeyV8,
    },
    error::{
        MalformedTransaction as MalformedTransactionV8,
        TransactionApplicationError as TransactionApplicationErrorV8,
    },
    structure::{
        Intent as IntentV8, ProofPreimageMarker as ProofPreimageMarkerV8,
        Transaction as TransactionV8,
    },
};
use midnight_ledger_v9::{
    dust::{
        DustActions as DustActionsV9, DustPublicKey as DustPublicKeyV9,
        DustRegistration as DustRegistrationV9, DustSecretKey as DustSecretKeyV9,
    },
    error::{
        MalformedTransaction as MalformedTransactionV9,
        TransactionApplicationError as TransactionApplicationErrorV9,
    },
    structure::{
        Intent as IntentV9, ProofPreimageMarker as ProofPreimageMarkerV9, Signature as SignatureV9,
        SigningKey as SigningKeyV9, Transaction as TransactionV9,
    },
};
use midnight_onchain_runtime_v3::cost_model::INITIAL_COST_MODEL as INITIAL_COST_MODEL_V8;
use midnight_onchain_runtime_v4::cost_model::INITIAL_COST_MODEL as INITIAL_COST_MODEL_V9;
use midnight_storage_core_v1::arena::Sp;
use midnight_transient_crypto_v2::{
    commitment::PedersenRandomness as PedersenRandomnessV8,
    curve::Fr as FrV8,
    proofs::{
        Proof as ProofV8, ProofPreimage as ProofPreimageV8, ProvingProvider as ProvingProviderV8,
    },
};
use midnight_transient_crypto_v3::{
    commitment::PedersenRandomness as PedersenRandomnessV9,
    curve::Fr as FrV9,
    proofs::{
        KeyLocation as KeyLocationV9, Proof as ProofV9, ProofPreimage as ProofPreimageV9,
        ProvingKeyMaterial as ProvingKeyMaterialV9, ProvingProvider as ProvingProviderV9,
        Resolver as ResolverV9,
    },
};
use rand::{SeedableRng, rngs::StdRng};
use std::{error::Error as StdError, io, iter};
#[cfg(feature = "standalone")]
use tempfile::tempdir;
#[cfg(feature = "cloud")]
use {
    crate::infra::{
        migrations,
        pool::postgres::{self, PostgresPool},
    },
    sqlx::postgres::PgSslMode,
    std::time::Duration,
    testcontainers::{ImageExt, runners::AsyncRunner},
    testcontainers_modules::postgres::Postgres,
};

/// Network ID of every transaction this module builds.
pub const NETWORK_ID: &str = "undeployed";

/// A reason the ledger rejects a transaction as malformed.
#[derive(Debug, PartialEq, Eq)]
pub enum Malformed {
    IntentTtlExpired,
    IntentTtlTooFarInFuture,
    OutOfDustValidityWindow,
    /// Any other malformed-transaction error, as displayed.
    Other(String),
}

/// Returns why the ledger rejected a transaction as malformed, if `error` or any of its sources reports it.
pub fn malformed(error: &(dyn StdError + 'static)) -> Option<Malformed> {
    iter::successors(Some(error), |&error| error.source()).find_map(|error| {
        if let Some(malformed) = error.downcast_ref::<MalformedTransactionV8<LedgerDb>>() {
            use MalformedTransactionV8::*;
            match malformed {
                TransactionApplicationError(TransactionApplicationErrorV8::IntentTtlExpired(
                    ..,
                )) => Some(Malformed::IntentTtlExpired),
                TransactionApplicationError(
                    TransactionApplicationErrorV8::IntentTtlTooFarInFuture(..),
                ) => Some(Malformed::IntentTtlTooFarInFuture),
                OutOfDustValidityWindow { .. } => Some(Malformed::OutOfDustValidityWindow),
                other => Some(Malformed::Other(other.to_string())),
            }
        } else if let Some(malformed) = error.downcast_ref::<MalformedTransactionV9<LedgerDb>>() {
            use MalformedTransactionV9::*;
            match malformed {
                TransactionApplicationError(TransactionApplicationErrorV9::IntentTtlExpired(
                    ..,
                )) => Some(Malformed::IntentTtlExpired),
                TransactionApplicationError(
                    TransactionApplicationErrorV9::IntentTtlTooFarInFuture(..),
                ) => Some(Malformed::IntentTtlTooFarInFuture),
                OutOfDustValidityWindow { .. } => Some(Malformed::OutOfDustValidityWindow),
                other => Some(Malformed::Other(other.to_string())),
            }
        } else {
            None
        }
    })
}

/// Builds a serialized transaction whose single intent carries only a signed dust registration.
///
/// The transaction needs no proofs, so it applies to a fresh ledger state for [NETWORK_ID]. `ttl` is the intent TTL and `dust_ctime` the dust actions' creation time, both in seconds. Transactions built with the same arguments are identical.
///
/// # Errors
///
/// Returns an error if signing, proving or serializing the transaction fails.
pub async fn dust_registration(
    ledger_version: LedgerVersion,
    ttl: u64,
    dust_ctime: u64,
) -> Result<SerializedTransaction, BoxError> {
    let mut rng = StdRng::seed_from_u64(0);
    let ttl = Timestamp::from_secs(ttl);
    let dust_ctime = Timestamp::from_secs(dust_ctime);

    let transaction = match ledger_version {
        LedgerVersion::V8 => {
            let night_key = SchnorrSigningKey::sample(&mut rng);
            let registration = DustRegistrationV8 {
                night_key: night_key.verifying_key(),
                dust_address: Some(Sp::new(DustPublicKeyV8::from(DustSecretKeyV8::sample(
                    &mut rng,
                )))),
                allow_fee_payment: 0,
                signature: None,
            };
            let dust_actions = DustActionsV8::<SignatureV8, ProofPreimageMarkerV8, LedgerDb> {
                spends: vec![].into(),
                registrations: vec![registration].into(),
                ctime: dust_ctime,
            };
            let intent = IntentV8::<_, _, PedersenRandomnessV8, _>::new(
                &mut rng,
                None,
                None,
                vec![],
                vec![],
                vec![],
                Some(dust_actions),
                ttl,
            )
            .sign(&mut rng, 1, &[], &[], &[night_key])
            .map_err(|error| format!("sign intent: {error}"))?;

            TransactionV8::from_intents(NETWORK_ID, [(1, intent)].into_iter().collect())
                .prove(NoProofs, &INITIAL_COST_MODEL_V8)
                .await
                .map_err(|error| format!("prove: {error}"))?
                .seal(rng)
                .tagged_serialize()?
        }

        LedgerVersion::V9 => {
            let night_key = SigningKeyV9::Schnorr(SchnorrSigningKey::sample(&mut rng));
            let registration = DustRegistrationV9 {
                night_key: night_key.verifying_key(),
                dust_address: Some(Sp::new(DustPublicKeyV9::from(DustSecretKeyV9::sample(
                    &mut rng,
                )))),
                allow_fee_payment: 0,
                signature: None,
            };
            let dust_actions = DustActionsV9::<SignatureV9, ProofPreimageMarkerV9, LedgerDb> {
                spends: vec![].into(),
                registrations: vec![registration].into(),
                ctime: dust_ctime,
            };
            let intent = IntentV9::<_, _, PedersenRandomnessV9, _>::new(
                &mut rng,
                None,
                None,
                vec![],
                vec![],
                vec![],
                Some(dust_actions),
                ttl,
            )
            .sign(&mut rng, 1, &[], &[], &[night_key])
            .map_err(|error| format!("sign intent: {error}"))?;

            TransactionV9::from_intents(NETWORK_ID, [(1, intent)].into_iter().collect())
                .prove(NoProofs, &INITIAL_COST_MODEL_V9)
                .await
                .map_err(|error| format!("prove: {error}"))?
                .seal(rng)
                .tagged_serialize()?
        }
    };

    Ok(transaction)
}

/// Initializes the ledger DB on a fresh Postgres container and returns the container, which must outlive the ledger DB's use.
///
/// # Errors
///
/// Returns an error if the container cannot be started or the migrations cannot be run.
#[cfg(feature = "cloud")]
pub async fn init_ledger_db() -> Result<impl Sized, BoxError> {
    let postgres_container = Postgres::default()
        .with_db_name("indexer")
        .with_user("indexer")
        .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
        .with_tag("17.1-alpine")
        .start()
        .await?;
    let postgres_port = postgres_container.get_host_port_ipv4(5432).await?;

    let config = postgres::Config {
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
    let pool = PostgresPool::new(config).await?;
    migrations::postgres::run(&pool).await?;
    ledger_db::init(
        ledger_db::Config {
            cache_max_nodes: 1_024,
        },
        pool,
    );

    Ok(postgres_container)
}

/// Initializes the ledger DB in a fresh temporary directory and returns the directory, which must outlive the ledger DB's use.
///
/// # Errors
///
/// Returns an error if the directory or the ledger DB cannot be created.
#[cfg(feature = "standalone")]
pub async fn init_ledger_db() -> Result<impl Sized, BoxError> {
    let temp_dir = tempdir()?;
    ledger_db::init(ledger_db::Config {
        cache_max_nodes: 1_024,
        cnn_url: temp_dir
            .path()
            .join("ledger-db.sqlite")
            .display()
            .to_string(),
    })
    .await?;

    Ok(temp_dir)
}

// A proving provider for transactions without proofs; `prove` only ever calls `split` on it.
struct NoProofs;

impl ProvingProviderV8 for NoProofs {
    async fn check(&self, _preimage: &ProofPreimageV8) -> anyhow::Result<Vec<Option<usize>>> {
        unreachable!("test transactions carry no proofs")
    }

    async fn prove(
        self,
        _preimage: &ProofPreimageV8,
        _overwrite_binding_input: Option<FrV8>,
    ) -> anyhow::Result<ProofV8> {
        unreachable!("test transactions carry no proofs")
    }

    fn split(&mut self) -> Self {
        NoProofs
    }
}

impl ResolverV9 for NoProofs {
    async fn resolve_key(&self, _key: KeyLocationV9) -> io::Result<Option<ProvingKeyMaterialV9>> {
        unreachable!("test transactions carry no proofs")
    }
}

impl ProvingProviderV9 for NoProofs {
    async fn check(&self, _preimage: &ProofPreimageV9) -> anyhow::Result<Vec<Option<usize>>> {
        unreachable!("test transactions carry no proofs")
    }

    async fn prove(
        self,
        _preimage: &ProofPreimageV9,
        _overwrite_binding_input: Option<FrV9>,
    ) -> anyhow::Result<ProofV9> {
        unreachable!("test transactions carry no proofs")
    }

    fn split(&mut self) -> Self {
        NoProofs
    }

    fn resolver(&self) -> &impl ResolverV9 {
        self
    }
}
