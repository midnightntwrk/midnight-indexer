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

use crate::{
    domain::{UnshieldedUtxo, storage::unshielded::UnshieldedUtxoStorage},
    infra::storage::Storage,
};
use fastrace::trace;
use indexer_common::domain::{ProtocolVersion, UnshieldedAddress};
use indoc::indoc;
use sqlx::FromRow;

/// A `unshielded_utxos` row plus the protocol versions needed for dust-epoch
/// scoping. Both versions are selected in the UTXO query so scoping does not add
/// another database round trip.
#[derive(Debug, FromRow)]
struct UnshieldedUtxoRow {
    #[sqlx(flatten)]
    utxo: UnshieldedUtxo,
    creating_protocol_version: i64,
    current_protocol_version: i64,
}

/// Report `registered_for_dust_generation` only to readers in the dust epoch the
/// row was written in.
///
/// The column is a snapshot: `registered_for_dust_generation_v8` / `_v9`
/// (`indexer-common`'s `ledger_state`) ask the ledger whether the new UTXO's
/// initial nonce is in `dust.generation.night_indices` at the moment the UTXO is
/// created, and the chain-indexer writes that answer once, on INSERT. For the
/// whole normal life of a chain the snapshot stays true, because the ledger's
/// `night_indices` is append-only (`DustGenerationState::night_indices`: "a
/// nonce, once inserted, is never removed") and a later registration only
/// attaches generations to NIGHT outputs of the registering intent itself
/// (`DustGenerationState::apply_registration`) -- never to a UTXO that already
/// exists.
///
/// A dust-wiping fork breaks exactly that. The 8 -> 9 translation replaces dust
/// state with `DustState::default()`, so every stranded nonce leaves
/// `night_indices` at once and those UTXOs stop generating DUST -- which is why
/// their holders must re-register before they can pay a fee. The stored `true`
/// then contradicts the ledger, and nothing retires it: the wipe happens inside
/// the state translation rather than via a transaction that could emit an event.
///
/// This is the same failure `dust_epoch` already fixes for `dust_generation_info`
/// (migration `008_dust_generation_epoch`), handled the same way and for the same
/// reason -- at read time, so the answer is right for data indexed by an older
/// build and stays right across a re-index.
fn scope_to_dust_epoch(rows: Vec<UnshieldedUtxoRow>) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
    rows.into_iter()
        .map(
            |UnshieldedUtxoRow {
                 mut utxo,
                 creating_protocol_version,
                 current_protocol_version,
             }| {
                let created_in = ProtocolVersion::try_from(creating_protocol_version)
                    .map_err(|error| sqlx::Error::Decode(error.into()))?
                    .ledger_version()
                    .dust_epoch();
                let current = ProtocolVersion::try_from(current_protocol_version)
                    .map_err(|error| sqlx::Error::Decode(error.into()))?
                    .ledger_version()
                    .dust_epoch();

                utxo.registered_for_dust_generation &= created_in == current;

                Ok(utxo)
            },
        )
        .collect()
}

impl UnshieldedUtxoStorage for Storage {
    #[trace(properties = { "address": "{address}" })]
    async fn get_unshielded_utxos_by_address(
        &self,
        address: UnshieldedAddress,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                unshielded_utxos.creating_transaction_id,
                unshielded_utxos.spending_transaction_id,
                unshielded_utxos.owner,
                unshielded_utxos.token_type,
                unshielded_utxos.value,
                unshielded_utxos.intent_hash,
                unshielded_utxos.output_index,
                unshielded_utxos.ctime,
                unshielded_utxos.initial_nonce,
                unshielded_utxos.registered_for_dust_generation,
                transactions.protocol_version AS creating_protocol_version,
                (SELECT protocol_version FROM blocks ORDER BY height DESC LIMIT 1)
                    AS current_protocol_version
            FROM unshielded_utxos
            JOIN transactions ON transactions.id = unshielded_utxos.creating_transaction_id
            WHERE unshielded_utxos.owner = $1
            ORDER BY unshielded_utxos.id
        "};

        let rows = sqlx::query_as(query)
            .bind(address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        scope_to_dust_epoch(rows)
    }

    #[trace(properties = { "transaction_id": "{transaction_id}" })]
    async fn get_unshielded_utxos_created_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                unshielded_utxos.creating_transaction_id,
                unshielded_utxos.spending_transaction_id,
                unshielded_utxos.owner,
                unshielded_utxos.token_type,
                unshielded_utxos.value,
                unshielded_utxos.intent_hash,
                unshielded_utxos.output_index,
                unshielded_utxos.ctime,
                unshielded_utxos.initial_nonce,
                unshielded_utxos.registered_for_dust_generation,
                transactions.protocol_version AS creating_protocol_version,
                (SELECT protocol_version FROM blocks ORDER BY height DESC LIMIT 1)
                    AS current_protocol_version
            FROM unshielded_utxos
            JOIN transactions ON transactions.id = unshielded_utxos.creating_transaction_id
            WHERE unshielded_utxos.creating_transaction_id = $1
            ORDER BY unshielded_utxos.output_index
        "};

        let rows = sqlx::query_as(query)
            .bind(transaction_id as i64)
            .fetch_all(&*self.pool)
            .await?;

        scope_to_dust_epoch(rows)
    }

    #[trace(properties = { "transaction_id": "{transaction_id}" })]
    async fn get_unshielded_utxos_spent_by_transaction(
        &self,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                unshielded_utxos.creating_transaction_id,
                unshielded_utxos.spending_transaction_id,
                unshielded_utxos.owner,
                unshielded_utxos.token_type,
                unshielded_utxos.value,
                unshielded_utxos.intent_hash,
                unshielded_utxos.output_index,
                unshielded_utxos.ctime,
                unshielded_utxos.initial_nonce,
                unshielded_utxos.registered_for_dust_generation,
                transactions.protocol_version AS creating_protocol_version,
                (SELECT protocol_version FROM blocks ORDER BY height DESC LIMIT 1)
                    AS current_protocol_version
            FROM unshielded_utxos
            JOIN transactions ON transactions.id = unshielded_utxos.creating_transaction_id
            WHERE unshielded_utxos.spending_transaction_id = $1
            ORDER BY unshielded_utxos.output_index
        "};

        let rows = sqlx::query_as(query)
            .bind(transaction_id as i64)
            .fetch_all(&*self.pool)
            .await?;

        scope_to_dust_epoch(rows)
    }

    #[trace(properties = { "address": "{address}", "transaction_id": "{transaction_id}" })]
    async fn get_unshielded_utxos_by_address_created_by_transaction(
        &self,
        address: UnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                unshielded_utxos.creating_transaction_id,
                unshielded_utxos.spending_transaction_id,
                unshielded_utxos.owner,
                unshielded_utxos.token_type,
                unshielded_utxos.value,
                unshielded_utxos.intent_hash,
                unshielded_utxos.output_index,
                unshielded_utxos.ctime,
                unshielded_utxos.initial_nonce,
                unshielded_utxos.registered_for_dust_generation,
                transactions.protocol_version AS creating_protocol_version,
                (SELECT protocol_version FROM blocks ORDER BY height DESC LIMIT 1)
                    AS current_protocol_version
            FROM unshielded_utxos
            JOIN transactions ON transactions.id = unshielded_utxos.creating_transaction_id
            WHERE unshielded_utxos.creating_transaction_id = $1
            AND unshielded_utxos.owner = $2
            ORDER BY unshielded_utxos.output_index
        "};

        let rows = sqlx::query_as(query)
            .bind(transaction_id as i64)
            .bind(address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        scope_to_dust_epoch(rows)
    }

    #[trace(properties = { "address": "{address}", "transaction_id": "{transaction_id}" })]
    async fn get_unshielded_utxos_by_address_spent_by_transaction(
        &self,
        address: UnshieldedAddress,
        transaction_id: u64,
    ) -> Result<Vec<UnshieldedUtxo>, sqlx::Error> {
        let query = indoc! {"
            SELECT
                unshielded_utxos.creating_transaction_id,
                unshielded_utxos.spending_transaction_id,
                unshielded_utxos.owner,
                unshielded_utxos.token_type,
                unshielded_utxos.value,
                unshielded_utxos.intent_hash,
                unshielded_utxos.output_index,
                unshielded_utxos.ctime,
                unshielded_utxos.initial_nonce,
                unshielded_utxos.registered_for_dust_generation,
                transactions.protocol_version AS creating_protocol_version,
                (SELECT protocol_version FROM blocks ORDER BY height DESC LIMIT 1)
                    AS current_protocol_version
            FROM unshielded_utxos
            JOIN transactions ON transactions.id = unshielded_utxos.creating_transaction_id
            WHERE unshielded_utxos.spending_transaction_id = $1
            AND unshielded_utxos.owner = $2
            ORDER BY unshielded_utxos.output_index
        "};

        let rows = sqlx::query_as(query)
            .bind(transaction_id as i64)
            .bind(address.as_ref())
            .fetch_all(&*self.pool)
            .await?;

        scope_to_dust_epoch(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexer_common::domain::ByteArray;

    /// Protocol versions on either side of the 8 -> 9 boundary.
    const LEDGER_8: i64 = 1_000_000;
    const LEDGER_9: i64 = 2_001_000;

    fn row(
        creating_protocol_version: i64,
        current_protocol_version: i64,
        registered: bool,
    ) -> UnshieldedUtxoRow {
        UnshieldedUtxoRow {
            utxo: UnshieldedUtxo {
                creating_transaction_id: 1,
                spending_transaction_id: None,
                owner: ByteArray::default(),
                token_type: ByteArray::default(),
                value: 42,
                intent_hash: ByteArray::default(),
                output_index: 0,
                ctime: Some(1),
                initial_nonce: ByteArray::default(),
                registered_for_dust_generation: registered,
            },
            creating_protocol_version,
            current_protocol_version,
        }
    }

    fn flags(rows: Vec<UnshieldedUtxoRow>) -> Vec<bool> {
        scope_to_dust_epoch(rows)
            .expect("scoping should succeed for supported protocol versions")
            .into_iter()
            .map(|utxo| utxo.registered_for_dust_generation)
            .collect()
    }

    /// Before the fork the stored value is the ledger's value, so it must survive
    /// untouched. A regression here would tell every NIGHT holder on a
    /// pre-fork chain that they generate no DUST.
    #[test]
    fn same_epoch_keeps_the_stored_value() {
        assert_eq!(
            flags(vec![
                row(LEDGER_8, LEDGER_8, true),
                row(LEDGER_8, LEDGER_8, false),
            ]),
            vec![true, false]
        );
    }

    /// The bug this scoping fixes: the 8 -> 9 wipe strands every pre-fork
    /// generation entry, so a `true` recorded in the old epoch is no longer the
    /// ledger's answer and must read `false`.
    #[test]
    fn earlier_epoch_reads_as_unregistered() {
        assert_eq!(
            flags(vec![
                row(LEDGER_8, LEDGER_9, true),
                row(LEDGER_8, LEDGER_9, false),
            ]),
            vec![false, false]
        );
    }

    /// Rows written after the fork are in the reader's own epoch, so the replayed
    /// registrations they record still count.
    #[test]
    fn post_fork_rows_keep_their_value_after_the_fork() {
        assert_eq!(
            flags(vec![
                row(LEDGER_9, LEDGER_9, true),
                row(LEDGER_9, LEDGER_9, false),
            ]),
            vec![true, false]
        );
    }

    /// Mixed cohorts are the realistic post-fork shape: the scoping has to
    /// separate them row by row rather than per query.
    #[test]
    fn mixed_epochs_are_scoped_per_row() {
        assert_eq!(
            flags(vec![
                row(LEDGER_8, LEDGER_9, true),
                row(LEDGER_9, LEDGER_9, true),
            ]),
            vec![false, true]
        );
    }
}
