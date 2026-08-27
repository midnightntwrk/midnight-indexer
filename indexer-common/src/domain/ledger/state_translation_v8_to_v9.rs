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

//! State translation from ledger v8 to ledger v9.
//!
//! # Provenance
//!
//! The translation table is the ledger team's
//! [`v8_to_v9_state_translation::StateTranslationTable`], pinned by rev to the
//! exact crate `node-2.1.0-beta.1` migrates with (`midnight-node` PR #2054,
//! backported as #2060, replaced the node's own copy with this crate). This
//! module used to carry a re-ported copy of that table; it now depends on the
//! upstream crate directly, so there is one implementation rather than two to
//! keep in sync.
//!
//! Only the [`translate_ledger_state`] driver below is ours. Upstream ships the
//! table; the node drives it in
//! `ledger/src/host_api/migration_8_to_9.rs::migrate_state_v8_to_v9` over the
//! pallet's serialized arena root. The indexer already holds the typed
//! `LedgerState` in memory, so it skips that decode/encode.
//!
//! # Why the pinning matters
//!
//! At the fork boundary the indexer re-derives `ledger_state.root()` and
//! compares it against the node's value, so the indexer and the node MUST
//! translate with the same table compiled against the same ledger crates. That
//! holds today: both pin this crate at the same rev and patch every v9-side
//! crate to `ledger-9.1.0.0-rc.4`, with `midnight-storage-core`'s `layout-v2` /
//! `gc-v1` features matching. Any drift there breaks the boundary permanently.
//!
//! When the ledger rc moves, re-check in this order: bump the workspace
//! `[patch.crates-io]` tags, bump this crate's rev to whatever the matching node
//! release pins, confirm [`tests::table_tags_match_types`] still passes (it
//! rebuilds every `TranslationId` from the live crate types, so it catches a tag
//! that drifted out from under the table's hardcoded literals), then regenerate
//! the golden-root fixtures `indexer-common/tests/golden_v8_to_v9_*_root.raw`
//! pinned by `LedgerState::translate`'s test and re-run the devnet rehearsal in
//! `docs/hardfork-devnet-rehearsal-8to9.md`.
//!
//! # Note on dust
//!
//! The translation deliberately *wipes* dust rather than carrying it over: the
//! v9 side comes out as the empty state genesis starts from (upstream's
//! `LedgerStateTl::finalize`, from midnight-node #2012, backported as #2057).
//! The node's `pallet_cnight_observation` v2 migration replays cNIGHT dust
//! generation across the blocks after the boundary and is built entirely on that
//! holding. [`tests::dust_state_is_wiped`] guards it from this side.

use midnight_ledger_v8 as ledger_v8;
use midnight_ledger_v9 as ledger_v9;
use midnight_storage_v2 as storage;

use midnight_base_crypto_v1::cost_model::CostDuration;
use std::io;
use storage::{arena::Sp, db::DB, state_translation::*};
use v8_to_v9_state_translation::StateTranslationTable;

// ---------- Driver ----------

/// Drive [`StateTranslationTable`] over an in-memory v8 `LedgerState`, returning
/// the fully-translated v9 `LedgerState`.
///
/// Mirrors the node's single-block, one-shot host call
/// (`ledger/src/host_api/migration_8_to_9.rs::migrate_state_v8_to_v9`): loop
/// `run(budget)` with a fixed per-step budget until `result()` is `Some`, with a
/// runaway backstop. Unlike the node — which operates on the pallet's serialized
/// arena root — the indexer already holds the typed `LedgerState`, so this skips
/// the arena-root decode/encode and returns the value directly. The caller
/// ([`crate::domain::ledger::LedgerState::translate`]) is reached only at the fork
/// boundary and only from a V8 state, so the already-v9 idempotency the node
/// guards for is the caller's `V9 => Ok(s)` arm.
pub fn translate_ledger_state<D: DB>(
    input: ledger_v8::structure::LedgerState<D>,
) -> io::Result<ledger_v9::structure::LedgerState<D>> {
    // Picoseconds granted per `run` step, matching the node's budget. The step
    // cap is a runaway backstop only.
    const RUN_BUDGET_PS: u64 = 10_000_000_000;
    const MAX_STEPS: usize = 100_000;

    let mut tl = TypedTranslationState::<
        ledger_v8::structure::LedgerState<D>,
        ledger_v9::structure::LedgerState<D>,
        StateTranslationTable,
        D,
    >::start(Sp::new(input))?;

    let budget = CostDuration::from_picoseconds(RUN_BUDGET_PS);
    let mut steps = 0usize;
    loop {
        steps += 1;
        if steps > MAX_STEPS {
            return Err(io::Error::other(
                "v8->v9 ledger state translation did not converge",
            ));
        }
        tl = tl.run(budget)?;
        if let Some(result) = tl.result()? {
            return Ok((*result).clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use midnight_onchain_state_v3 as onchain_state_v8;
    use midnight_onchain_state_v4 as onchain_state_v9;
    use midnight_serialize_v1 as serialize;
    use serialize::Tagged;
    use std::{borrow::Cow, ops::Deref};
    use storage::db::InMemoryDB;

    fn translate_to_completion(
        v8: ledger_v8::structure::LedgerState<InMemoryDB>,
    ) -> ledger_v9::structure::LedgerState<InMemoryDB> {
        let tl_state = TypedTranslationState::<
            ledger_v8::structure::LedgerState<InMemoryDB>,
            ledger_v9::structure::LedgerState<InMemoryDB>,
            StateTranslationTable,
            InMemoryDB,
        >::start(Sp::new(v8))
        .expect("Failed to start translation");

        let cost = CostDuration::from_picoseconds(1_000_000_000_000);
        let finished = tl_state.run(cost).expect("Translation failed");

        finished
            .result()
            .expect("Failed to get result")
            .expect("Translation did not complete")
            .deref()
            .clone()
    }

    /// Every `TranslationId` a table entry requires must itself be in the table,
    /// or translation errors at runtime the first time the entry is needed.
    #[test]
    fn table_is_closed() {
        <StateTranslationTable as TranslationTable<InMemoryDB>>::assert_closure();
    }

    /// The `TABLE` hardcodes tag string literals. If a tag on either the v8 or v9
    /// side drifts (e.g. an rc bump changes a `#[tag]`), the literal no longer
    /// matches what `T::tag()` produces and the migration silently mis-dispatches.
    /// Rebuild every expected ID from the node's actual crate types and compare.
    #[test]
    fn table_tags_match_types() {
        use storage::merkle_patricia_trie::{MerklePatriciaTrie, Node};
        use storage::storable::SizeAnn;

        type V8Ann = ledger_v8::annotation::NightAnn;
        type V9Ann = ledger_v9::annotation::NightAnn;
        type V8Contract = onchain_state_v8::state::ContractState<InMemoryDB>;
        type V9Contract = onchain_state_v9::state::ContractState<InMemoryDB>;

        let expected: Vec<(Cow<'static, str>, Cow<'static, str>)> = vec![
            (
                ledger_v8::structure::LedgerState::<InMemoryDB>::tag(),
                ledger_v9::structure::LedgerState::<InMemoryDB>::tag(),
            ),
            (
                ledger_v8::structure::LedgerParameters::tag(),
                ledger_v9::structure::LedgerParameters::tag(),
            ),
            (V8Contract::tag(), V9Contract::tag()),
            (
                MerklePatriciaTrie::<V8Contract, InMemoryDB, V8Ann>::tag(),
                MerklePatriciaTrie::<V9Contract, InMemoryDB, V9Ann>::tag(),
            ),
            (
                Node::<V8Contract, InMemoryDB, V8Ann>::tag(),
                Node::<V9Contract, InMemoryDB, V9Ann>::tag(),
            ),
            (u128::tag(), u128::tag()),
            (
                MerklePatriciaTrie::<u128, InMemoryDB, SizeAnn>::tag(),
                MerklePatriciaTrie::<u128, InMemoryDB, V9Ann>::tag(),
            ),
            (
                Node::<u128, InMemoryDB, SizeAnn>::tag(),
                Node::<u128, InMemoryDB, V9Ann>::tag(),
            ),
        ];

        let actual: Vec<_> = <StateTranslationTable as TranslationTable<InMemoryDB>>::TABLE
            .iter()
            .map(|(id, _)| (id.0.clone(), id.1.clone()))
            .collect();

        assert_eq!(actual, expected);
    }

    /// End-to-end smoke test: a default v8 `LedgerState` translates to v9,
    /// preserving the tag-stable pools and picking up the new v9 default
    /// `min_block_price`, and survives a v9 serialize round-trip.
    #[test]
    fn empty_state_translates_and_round_trips() {
        let v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new("test-network");
        let v9 = translate_to_completion(v8.clone());

        assert_eq!(v9.network_id, v8.network_id);
        assert_eq!(v9.reserve_pool, v8.reserve_pool);
        assert_eq!(v9.locked_pool, v8.locked_pool);
        assert_eq!(v9.block_reward_pool, v8.block_reward_pool);
        assert_eq!(
            v9.parameters.min_block_price,
            ledger_v9::structure::INITIAL_PARAMETERS.min_block_price,
        );

        let mut buf = Vec::new();
        serialize::tagged_serialize(&v9, &mut buf).expect("v9 serialize");
        let v9_rt: ledger_v9::structure::LedgerState<InMemoryDB> =
            serialize::tagged_deserialize(&mut &buf[..]).expect("v9 deserialize");
        assert_eq!(v9_rt.network_id, v9.network_id);
    }

    /// The translation wipes dust: whatever generation/utxo state v8 held, the
    /// v9 side comes out as the empty state genesis starts from.
    ///
    /// Ported from the node's test of the same name (`midnight-node` PR #2012,
    /// backported as #2057). The node's `pallet_cnight_observation` v2 migration
    /// — which replays cNIGHT dust generation across the blocks after the
    /// boundary — is built entirely on this holding, so a silent revert to
    /// `recast(&source.dust)` would desynchronise the indexer from the node for
    /// good.
    #[test]
    fn dust_state_is_wiped() {
        let mut v8 = ledger_v8::structure::LedgerState::<InMemoryDB>::new("test-network");
        let mut dust = (*v8.dust).clone();
        dust.generation.generating_tree_first_free = 7;
        dust.utxo.commitments_first_free = 3;
        v8.dust = Sp::new(dust);

        let v9 = translate_to_completion(v8);

        assert_eq!(*v9.dust, ledger_v9::dust::DustState::default());
    }
}
