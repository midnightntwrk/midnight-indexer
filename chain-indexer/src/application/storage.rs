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

use crate::domain::{
    Block, BlockRef, DParameter, DustRegistrationEvent, SystemParametersChange, TermsAndConditions,
    Transaction, storage::Storage,
};
use indexer_common::domain::{ProtocolVersion, SerializedLedgerStateKey};
use log::warn;
use std::{num::NonZeroUsize, time::Duration};
use tokio::time::sleep;

const INITIAL_BACKOFF: Duration = Duration::from_millis(100);
const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// Storage decorator that endlessly retries `PoolTimedOut` with capped exponential backoff
/// instead of failing. `PoolTimedOut` is only returned when no connection could be acquired from
/// the pool, i.e. before any statement has been executed, hence retrying whole operations is safe.
#[derive(Debug, Clone)]
pub struct RetryingStorage<S>(S);

impl<S> RetryingStorage<S> {
    pub fn new(storage: S) -> Self {
        Self(storage)
    }
}

impl<S> Storage for RetryingStorage<S>
where
    S: Storage,
{
    async fn save_block(
        &mut self,
        block: &Block,
        transactions: &[Transaction],
        dust_registration_events: &[DustRegistrationEvent],
        ledger_state_key: &SerializedLedgerStateKey,
        system_parameters_change: Option<&SystemParametersChange>,
    ) -> Result<Option<u64>, sqlx::Error> {
        let mut backoff = Backoff::new("save_block");

        loop {
            let result = self
                .0
                .save_block(
                    block,
                    transactions,
                    dust_registration_events,
                    ledger_state_key,
                    system_parameters_change,
                )
                .await;

            match result {
                Err(sqlx::Error::PoolTimedOut) => backoff.wait().await,
                result => break result,
            }
        }
    }

    async fn get_highest_block(
        &self,
    ) -> Result<Option<(BlockRef, ProtocolVersion, SerializedLedgerStateKey)>, sqlx::Error> {
        let mut backoff = Backoff::new("get_highest_block");

        loop {
            match self.0.get_highest_block().await {
                Err(sqlx::Error::PoolTimedOut) => backoff.wait().await,
                result => break result,
            }
        }
    }

    async fn get_newest_ledger_state_keys(
        &self,
        limit: NonZeroUsize,
    ) -> Result<Vec<(ProtocolVersion, SerializedLedgerStateKey)>, sqlx::Error> {
        let mut backoff = Backoff::new("get_newest_ledger_state_keys");

        loop {
            match self.0.get_newest_ledger_state_keys(limit).await {
                Err(sqlx::Error::PoolTimedOut) => backoff.wait().await,
                result => break result,
            }
        }
    }

    async fn get_transaction_count(&self) -> Result<u64, sqlx::Error> {
        let mut backoff = Backoff::new("get_transaction_count");

        loop {
            match self.0.get_transaction_count().await {
                Err(sqlx::Error::PoolTimedOut) => backoff.wait().await,
                result => break result,
            }
        }
    }

    async fn get_contract_action_count(&self) -> Result<(u64, u64, u64), sqlx::Error> {
        let mut backoff = Backoff::new("get_contract_action_count");

        loop {
            match self.0.get_contract_action_count().await {
                Err(sqlx::Error::PoolTimedOut) => backoff.wait().await,
                result => break result,
            }
        }
    }

    async fn get_latest_d_parameter(&self) -> Result<Option<DParameter>, sqlx::Error> {
        let mut backoff = Backoff::new("get_latest_d_parameter");

        loop {
            match self.0.get_latest_d_parameter().await {
                Err(sqlx::Error::PoolTimedOut) => backoff.wait().await,
                result => break result,
            }
        }
    }

    async fn get_latest_terms_and_conditions(
        &self,
    ) -> Result<Option<TermsAndConditions>, sqlx::Error> {
        let mut backoff = Backoff::new("get_latest_terms_and_conditions");

        loop {
            match self.0.get_latest_terms_and_conditions().await {
                Err(sqlx::Error::PoolTimedOut) => backoff.wait().await,
                result => break result,
            }
        }
    }
}

/// Capped exponential backoff for retrying a storage operation that failed with `PoolTimedOut`.
struct Backoff {
    operation: &'static str,
    backoff: Duration,
    attempt: u32,
}

impl Backoff {
    fn new(operation: &'static str) -> Self {
        Self {
            operation,
            backoff: INITIAL_BACKOFF,
            attempt: 0,
        }
    }

    async fn wait(&mut self) {
        self.attempt += 1;
        warn!(
            operation = self.operation,
            attempt = self.attempt,
            backoff:? = self.backoff;
            "could not acquire storage connection, retrying"
        );

        sleep(self.backoff).await;
        self.backoff = (self.backoff * 2).min(MAX_BACKOFF);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        application::storage::RetryingStorage,
        domain::{
            Block, BlockRef, DParameter, DustRegistrationEvent, SystemParametersChange,
            TermsAndConditions, Transaction, storage::Storage,
        },
    };
    use indexer_common::domain::{ProtocolVersion, SerializedLedgerStateKey};
    use std::{
        num::NonZeroUsize,
        sync::{
            Arc,
            atomic::{AtomicU32, Ordering},
        },
    };

    #[tokio::test(start_paused = true)]
    async fn retries_pool_timed_out() {
        // The first two calls fail with PoolTimedOut, the third one succeeds.
        let storage = RetryingStorage::new(MockStorage {
            calls: Arc::new(AtomicU32::new(0)),
            failures: 2,
        });

        let result = storage.get_transaction_count().await;

        assert!(matches!(result, Ok(42)));
        assert_eq!(storage.0.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn fails_fast_on_other_errors() {
        let storage = RetryingStorage::new(MockStorage {
            calls: Arc::new(AtomicU32::new(0)),
            failures: 0,
        });

        let result = storage.get_highest_block().await;

        assert!(matches!(result, Err(sqlx::Error::RowNotFound)));
        assert_eq!(storage.0.calls.load(Ordering::SeqCst), 1);
    }

    #[derive(Clone)]
    struct MockStorage {
        calls: Arc<AtomicU32>,
        failures: u32,
    }

    impl MockStorage {
        fn call(&self) -> u32 {
            self.calls.fetch_add(1, Ordering::SeqCst) + 1
        }
    }

    impl Storage for MockStorage {
        async fn save_block(
            &mut self,
            _block: &Block,
            _transactions: &[Transaction],
            _dust_registration_events: &[DustRegistrationEvent],
            _ledger_state_key: &SerializedLedgerStateKey,
            _system_parameters_change: Option<&SystemParametersChange>,
        ) -> Result<Option<u64>, sqlx::Error> {
            unimplemented!()
        }

        async fn get_highest_block(
            &self,
        ) -> Result<Option<(BlockRef, ProtocolVersion, SerializedLedgerStateKey)>, sqlx::Error>
        {
            self.call();
            Err(sqlx::Error::RowNotFound)
        }

        async fn get_newest_ledger_state_keys(
            &self,
            _limit: NonZeroUsize,
        ) -> Result<Vec<(ProtocolVersion, SerializedLedgerStateKey)>, sqlx::Error> {
            unimplemented!()
        }

        async fn get_transaction_count(&self) -> Result<u64, sqlx::Error> {
            if self.call() <= self.failures {
                Err(sqlx::Error::PoolTimedOut)
            } else {
                Ok(42)
            }
        }

        async fn get_contract_action_count(&self) -> Result<(u64, u64, u64), sqlx::Error> {
            unimplemented!()
        }

        async fn get_latest_d_parameter(&self) -> Result<Option<DParameter>, sqlx::Error> {
            unimplemented!()
        }

        async fn get_latest_terms_and_conditions(
            &self,
        ) -> Result<Option<TermsAndConditions>, sqlx::Error> {
            unimplemented!()
        }
    }
}
