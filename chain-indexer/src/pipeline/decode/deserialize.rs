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

//! Deserialization of regular ledger transactions on the decode pool, each pool thread in a
//! storage of its own, so that threads don't contend on one storage's locks.

use crate::domain::ContractAction;
use indexer_common::domain::{LedgerVersion, SerializedTransactionIdentifier, ledger};
use midnight_storage_core_v1::{
    Storage,
    db::{DB, InMemoryDB},
};

/// Nodes each thread's storage caches; a transaction is dropped once read.
const STORAGE_CACHE_NODES: usize = 1_024;

thread_local! {
    /// The calling thread's storage.
    static STORAGE: Storage<InMemoryDB> =
        Storage::new(STORAGE_CACHE_NODES, InMemoryDB::default());
}

/// What decode takes from a regular transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deserialized {
    pub identifiers: Vec<SerializedTransactionIdentifier>,
    pub contract_actions: Vec<ContractAction>,
}

/// Deserialize a regular transaction into the calling thread's storage and read its identifiers and
/// contract actions; the deserialized transaction is dropped there.
///
/// A contract deploy's address is the hash of the deploy's serialization, and serializing
/// allocates in the process-wide default storage, so a transaction deploying contracts is read from
/// the default in-memory storage instead.
pub fn deserialize(
    transaction: &[u8],
    ledger_version: LedgerVersion,
) -> Result<Deserialized, ledger::Error> {
    STORAGE.with(|storage| {
        let in_thread =
            ledger::Transaction::deserialize_into(storage, transaction, ledger_version)?;
        if in_thread.deploys_contracts() {
            read(&ledger::Transaction::<InMemoryDB>::deserialize_in(
                transaction,
                ledger_version,
            )?)
        } else {
            read(&in_thread)
        }
    })
}

fn read<D: DB>(transaction: &ledger::Transaction<D>) -> Result<Deserialized, ledger::Error> {
    Ok(Deserialized {
        identifiers: transaction.identifiers()?,
        contract_actions: transaction
            .contract_actions()?
            .into_iter()
            .map(Into::into)
            .collect(),
    })
}
