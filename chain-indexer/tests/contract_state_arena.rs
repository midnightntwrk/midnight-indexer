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

//! SPIKE — REMOVE BEFORE MERGE.
//!
//! Feasibility gate for storing `contract_actions.state` as a ledger-arena key instead of a
//! serialized blob. This is a measurement harness, not a regression test: it exists to justify
//! the design to a reviewer and is deleted in the final commit, together with the `midnight-*`
//! dev-dependencies it adds to chain-indexer.
//!
//! It needs no node RPC and no chain replay. The `state` blobs already stored in an existing
//! `contract_actions` table *are* the node's answers, so for any action whose block is still
//! inside the ledger-state retention window we can load that block's ledger state out of the
//! arena and compare, entirely offline and deterministically.
//!
//! Point it at a **copy** of a real (e.g. preprod) standalone database pair — some of these tests
//! persist and flush arena nodes, so they write to the ledger DB:
//!
//! ```text
//! SPIKE_INDEXER_DB=/path/indexer.sqlite \
//! SPIKE_LEDGER_DB=/path/ledger-db.sqlite \
//! cargo nextest run -p chain-indexer --features standalone --run-ignored all \
//!     --no-capture -E 'binary(contract_state_arena)'
//! ```
//!
//! `--no-capture` matters: most of the output is measurements, not assertions. The tests run in
//! separate processes, which is what lets each install its own process-global arena.
//!
//! Knobs, all optional: `SPIKE_CACHE_MAX_NODES` (default 100000), `SPIKE_ADDRESSES` (default 25
//! distinct addresses to compare), `SPIKE_MIN_PAIRS` (default 20, the floor below which the
//! fidelity gate fails rather than passing vacuously), `SPIKE_SCAN_ACTIONS` (default 5000, how far
//! back the covering `(id, address)` index is scanned for candidates) and `SPIKE_DELTA_ACTIONS`
//! (default 25 consecutive actions for the dedup, delta and root-growth measurements).

#![cfg(feature = "standalone")]

use anyhow::{Context, bail};
use indexer_common::{
    domain::{
        ByteVec, LedgerVersion, ProtocolVersion, SerializedContractAddress,
        SerializedLedgerStateKey, ledger::LedgerState,
    },
    infra::{
        ledger_db::{self, v1_1},
        pool::sqlite::{Config as SqliteConfig, SqlitePool},
    },
};
use midnight_coin_structure_v2::contract::ContractAddress as ContractAddressV8;
use midnight_coin_structure_v3::contract::ContractAddress as ContractAddressV9;
use midnight_onchain_runtime_v3::state::ContractState as ContractStateV3;
use midnight_onchain_runtime_v4::state::ContractState as ContractStateV4;
use midnight_serialize_v1::{Deserializable, Serializable, Tagged, tagged_serialize};
use midnight_storage_core_v1::{
    DefaultHasher, Storage,
    arena::{ArenaHash, TypedArenaKey},
    db::DB,
    storage::default_storage,
};
use midnight_zswap_v8::ledger::State as ZswapStateV8;
use midnight_zswap_v9::ledger::State as ZswapStateV9;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

/// One candidate action: everything needed to reproduce its state from the arena and compare.
#[derive(Debug, Clone)]
struct Candidate {
    id: i64,
    block_height: i64,
    address: SerializedContractAddress,
    state: ByteVec,
    zswap_state: ByteVec,
    ledger_state_key: SerializedLedgerStateKey,
    ledger_version: LedgerVersion,
}

// --------------------------------------------------------------------------------------------
// 1. Fidelity — the gate.
// --------------------------------------------------------------------------------------------

/// Re-serializing the contract state held in the arena must be byte-identical to the blob the
/// node's `get_contract_state` RPC produced and that we stored. If this fails, the design cannot
/// serve a byte-identical `state` field and the fallback — a content-hash dedup table — applies.
///
/// The known ways this could break: `MerkleTreeNode.hash` is part of the storable payload and is
/// `None` until `rehash()`, and `ChargedState.charged_keys` is replay-derived. If the indexer's
/// replayed state and the node's differ in either, the bytes differ for the same logical state.
///
/// This serializes the resident pointer rather than one obtained by `get_lazy`-ing a persisted key,
/// which keeps the gate read-only. The two are equivalent:
/// `serialize_to_node_list_bounded` reads an `ArenaKey::Direct` root straight out of its inlined
/// `data`/`children` and an `ArenaKey::Ref` root out of the backend, and a node's payload and
/// children are the same either way — so the emitted node list is identical. The full
/// persist-flush-`get_lazy`-serialize path is exercised by
/// [measure_node_count_depth_and_cold_load_latency], which asserts cold and warm reads agree.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "SPIKE: requires an existing indexer + ledger DB pair via SPIKE_* env vars"]
async fn contract_state_reserializes_byte_identically() -> anyhow::Result<()> {
    let indexer = init().await?;

    let candidates = candidates(&indexer, env_usize("SPIKE_ADDRESSES", 25)).await?;
    let min_pairs = env_usize("SPIKE_MIN_PAIRS", 20);

    let mut compared = 0usize;
    let mut aged_out = 0usize;
    let mut empty = 0usize;

    for candidate in &candidates {
        if candidate.state.is_empty() {
            // Today's "empty state means failed action" workaround; nothing to compare.
            empty += 1;
            continue;
        }

        if !LedgerState::root_loadable(&candidate.ledger_state_key, candidate.ledger_version)
            .context("check ledger state root loadable")?
        {
            aged_out += 1;
            continue;
        }

        let ledger_state = LedgerState::load(&candidate.ledger_state_key, candidate.ledger_version)
            .context("load ledger state")?;

        match contract_state_bytes(&ledger_state, &candidate.address)? {
            Some(bytes) => {
                assert_eq!(
                    bytes.as_ref(),
                    candidate.state.as_ref(),
                    "re-serialized contract state differs from the stored blob for action {} \
                     (address {}, block {})",
                    candidate.id,
                    candidate.address,
                    candidate.block_height,
                );
                compared += 1;
            }

            // A non-empty stored state means the contract was present at that block, so an
            // absent arena entry is a real mismatch rather than something to skip.
            None => bail!(
                "contract {} absent from the ledger state of block {} although action {} stored a \
                 non-empty state",
                candidate.address,
                candidate.block_height,
                candidate.id
            ),
        }
    }

    println!(
        "fidelity: candidates={} compared={compared} aged_out={aged_out} empty_state={empty}",
        candidates.len()
    );

    assert!(
        compared >= min_pairs,
        "only {compared} comparable action(s), below the floor of {min_pairs}; an all-skipped run \
         must not pass — raise SPIKE_SCAN_ACTIONS or use a database whose recent blocks are still \
         within the retention window"
    );

    Ok(())
}

/// Today's blob comes from a runtime API called *at the block*, so every action on the same address
/// within one block reports identical bytes. That is what makes end-of-block capture the
/// behaviour-preserving choice, so assert it directly on real data.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "SPIKE: requires an existing indexer + ledger DB pair via SPIKE_* env vars"]
async fn actions_on_one_address_in_one_block_store_the_same_state() -> anyhow::Result<()> {
    let indexer = init().await?;
    let scan = env_usize("SPIKE_SCAN_ACTIONS", 5_000) as i64;

    let groups = sqlx::query_as::<_, (i64, Vec<u8>, i64)>(
        "WITH recent AS ( \
           SELECT contract_actions.id AS id, contract_actions.address AS address, \
                  transactions.block_id AS block_id \
           FROM contract_actions \
           INNER JOIN transactions ON transactions.id = contract_actions.transaction_id \
           ORDER BY contract_actions.id DESC \
           LIMIT $1 \
         ) \
         SELECT block_id, address, COUNT(*) AS n \
         FROM recent \
         GROUP BY block_id, address \
         HAVING COUNT(*) > 1 \
         ORDER BY block_id DESC \
         LIMIT 5",
    )
    .bind(scan)
    .fetch_all(&*indexer)
    .await
    .context("find blocks with several actions on one address")?;

    if groups.is_empty() {
        println!(
            "end-of-block invariant: no block within the last {scan} actions has two actions on \
             one address; nothing to assert"
        );
        return Ok(());
    }

    for (block_id, address, count) in &groups {
        let states = sqlx::query_as::<_, (i64, Vec<u8>)>(
            "SELECT contract_actions.id, contract_actions.state \
             FROM contract_actions \
             INNER JOIN transactions ON transactions.id = contract_actions.transaction_id \
             WHERE transactions.block_id = $1 AND contract_actions.address = $2 \
             ORDER BY contract_actions.id",
        )
        .bind(block_id)
        .bind(address.as_slice())
        .fetch_all(&*indexer)
        .await
        .context("fetch the states of the actions in one block")?;

        let (first_id, first) = states.first().expect("group is non-empty").to_owned();
        for (id, state) in &states[1..] {
            assert_eq!(
                state, &first,
                "actions {first_id} and {id} act on the same address in block {block_id} but \
                 stored different states; capture is not end-of-block"
            );
        }

        println!(
            "end-of-block invariant: block {block_id} has {count} actions on one address, all \
             storing the same {} bytes",
            first.len()
        );
    }

    Ok(())
}

// --------------------------------------------------------------------------------------------
// 2. Node count, DAG depth, cold re-serialize latency.
// --------------------------------------------------------------------------------------------

/// Per contract state: the exact reachable node set, its depth and its on-disk size; then cold
/// load-and-serialize latency across cache size x prefetch, plus a warm repeat. These numbers size
/// the API cache, decide the prefetch, bound read concurrency, and are what confirms 100000 is the
/// right `cache_max_nodes` rather than an inherited guess.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "SPIKE: requires an existing indexer + ledger DB pair via SPIKE_* env vars"]
async fn measure_node_count_depth_and_cold_load_latency() -> anyhow::Result<()> {
    let indexer = init().await?;
    let ledger_db_url = env_var("SPIKE_LEDGER_DB")?;

    let candidates = candidates(&indexer, env_usize("SPIKE_ADDRESSES", 25)).await?;
    let keys = contract_state_keys(&candidates)?;
    if keys.is_empty() {
        bail!("no contract state key could be captured; run the fidelity gate first");
    }
    flush();

    // A second, plain `LedgerDb` over the same file, purely for measurement: the walk below is a
    // `DB` trait method, so it bypasses the arena's caches entirely.
    let measure_db = plain_ledger_db(&ledger_db_url).await?;

    for (candidate, key) in &keys {
        let (nodes, depth, bytes) = reachable(&measure_db, key.hash());
        println!(
            "state action={} address={} nodes={} depth={depth} arena_bytes={bytes} blob_bytes={}",
            candidate.id,
            hex_prefix(&candidate.address),
            nodes.len(),
            candidate.state.len(),
        );
    }

    // Latency: a fresh `Storage` per cell, so each read starts from a genuinely cold cache, and a
    // fresh pool with it so nothing is warm at the sqlx layer either.
    println!("cache_max_nodes prefetch cold_ms warm_ms bytes");
    for cache_max_nodes in [1_024usize, 100_000] {
        for prefetch in [false, true] {
            let mut cold = Duration::ZERO;
            let mut warm = Duration::ZERO;
            let mut bytes_total = 0usize;

            for (candidate, key) in &keys {
                let storage = local_storage(&ledger_db_url, cache_max_nodes).await?;

                let started = Instant::now();
                if prefetch {
                    storage.with_backend(|b| b.pre_fetch(key.hash(), None, true));
                }
                let bytes = load_and_serialize(&storage, candidate, key)?;
                cold += started.elapsed();

                let started = Instant::now();
                let again = load_and_serialize(&storage, candidate, key)?;
                warm += started.elapsed();

                assert_eq!(bytes, again, "warm read differs from cold read");
                bytes_total += bytes.len();
            }

            let n = keys.len() as u32;
            println!(
                "{cache_max_nodes:>15} {prefetch:>8} {:>7.1} {:>7.1} {}",
                (cold / n).as_secs_f64() * 1_000.0,
                (warm / n).as_secs_f64() * 1_000.0,
                bytes_total / keys.len(),
            );
        }
    }

    Ok(())
}

// --------------------------------------------------------------------------------------------
// 3. Dedup factor and per-action delta.
// --------------------------------------------------------------------------------------------

/// `sum |S_i|` versus `|union S_i|` over consecutive actions of one contract, plus
/// `|S_i \ S_{i-1}|`. The delta is what validates the projected retained size *and* what drives gc
/// live-set growth — they are the same quantity, so the win and the cost are measured here
/// together.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "SPIKE: requires an existing indexer + ledger DB pair via SPIKE_* env vars"]
async fn measure_dedup_factor_and_per_action_delta() -> anyhow::Result<()> {
    let indexer = init().await?;
    let ledger_db_url = env_var("SPIKE_LEDGER_DB")?;
    let measure_db = plain_ledger_db(&ledger_db_url).await?;

    let address = busiest_address(&indexer).await?;
    let actions =
        consecutive_actions(&indexer, &address, env_usize("SPIKE_DELTA_ACTIONS", 25)).await?;
    let keys = contract_state_keys(&actions)?;
    if keys.len() < 2 {
        bail!(
            "need at least two loadable actions on address {} to measure a delta, got {}",
            hex_prefix(&address),
            keys.len()
        );
    }
    flush();

    let mut union = HashSet::new();
    let mut sum_nodes = 0usize;
    let mut sum_bytes = 0usize;
    let mut previous = None::<HashSet<ArenaHash<DefaultHasher>>>;

    for (candidate, key) in &keys {
        let (nodes, _, bytes) = reachable(&measure_db, key.hash());
        sum_nodes += nodes.len();
        sum_bytes += bytes;

        let new = previous
            .as_ref()
            .map(|previous| nodes.difference(previous).count().to_string())
            .unwrap_or_else(|| "-".to_owned());
        println!(
            "delta action={} nodes={} arena_bytes={bytes} new_vs_previous={new}",
            candidate.id,
            nodes.len(),
        );

        union.extend(nodes.iter().cloned());
        previous = Some(nodes);
    }

    let union_bytes = union
        .iter()
        .filter_map(|hash| measure_db.get_node(hash))
        .map(|object| object.serialized_size())
        .sum::<usize>();

    println!(
        "dedup address={} actions={} sum_nodes={sum_nodes} union_nodes={} sum_bytes={sum_bytes} \
         union_bytes={union_bytes} factor={:.2} blob_bytes={}",
        hex_prefix(&address),
        keys.len(),
        union.len(),
        sum_bytes as f64 / union_bytes.max(1) as f64,
        keys.iter().map(|(c, _)| c.state.len()).sum::<usize>(),
    );

    Ok(())
}

// --------------------------------------------------------------------------------------------
// 4. zswap_state sizing.
// --------------------------------------------------------------------------------------------

/// `zswap_state` is measured 100% identical to the previous row, so content addressing should
/// collapse it onto one stored value. Unlike the contract state there is no `lookup_sp` shortcut:
/// `filter()` constructs a fresh value whose interior nodes are new to the arena and referenced by
/// nothing else, so this also quantifies the per-node overhead against a single blob.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "SPIKE: requires an existing indexer + ledger DB pair via SPIKE_* env vars"]
async fn measure_zswap_state_arena_size() -> anyhow::Result<()> {
    let indexer = init().await?;
    let ledger_db_url = env_var("SPIKE_LEDGER_DB")?;
    let measure_db = plain_ledger_db(&ledger_db_url).await?;

    let address = busiest_address(&indexer).await?;
    let actions =
        consecutive_actions(&indexer, &address, env_usize("SPIKE_DELTA_ACTIONS", 25)).await?;

    let mut distinct_blobs = HashSet::new();
    let mut hashes = vec![];
    let mut identical_to_previous = 0usize;
    let mut previous = None::<ByteVec>;
    let mut blob_bytes = 0usize;

    for candidate in &actions {
        if !LedgerState::root_loadable(&candidate.ledger_state_key, candidate.ledger_version)? {
            continue;
        }
        let ledger_state =
            LedgerState::load(&candidate.ledger_state_key, candidate.ledger_version)?;

        // The same construction the writer performs today, so the bytes are comparable with the
        // stored column — and asserting that keeps this measurement honest.
        let zswap_state = ledger_state
            .extract_contract_zswap_state(&candidate.address)
            .context("extract contract zswap state")?;
        assert_eq!(
            zswap_state.as_ref(),
            candidate.zswap_state.as_ref(),
            "reconstructed zswap state differs from the stored blob for action {}",
            candidate.id
        );

        if previous
            .as_ref()
            .is_some_and(|previous| previous == &zswap_state)
        {
            identical_to_previous += 1;
        }
        blob_bytes += zswap_state.len();
        distinct_blobs.insert(zswap_state.as_ref().to_vec());
        previous = Some(zswap_state);

        // `alloc` is content-addressed and idempotent, so an identical value yields an identical
        // key however often it is allocated, and the SQL write is an upsert.
        hashes.push(alloc_zswap_state(&ledger_state, &candidate.address)?);
    }

    flush();

    let distinct_keys = hashes.iter().collect::<HashSet<_>>();
    let mut union = HashSet::new();
    let mut sum_bytes = 0usize;
    for hash in &distinct_keys {
        let (nodes, depth, bytes) = reachable(&measure_db, hash);
        sum_bytes += bytes;
        println!(
            "zswap key={} nodes={} depth={depth} arena_bytes={bytes}",
            hex_hash(hash),
            nodes.len(),
        );
        union.extend(nodes);
    }

    println!(
        "zswap address={} actions={} identical_to_previous={identical_to_previous} \
         distinct_blobs={} distinct_keys={} union_nodes={} arena_bytes={sum_bytes} \
         blob_bytes={blob_bytes}",
        hex_prefix(&address),
        hashes.len(),
        distinct_blobs.len(),
        distinct_keys.len(),
        union.len(),
    );
    assert_eq!(
        distinct_blobs.len(),
        distinct_keys.len(),
        "content addressing must map identical zswap states onto identical keys"
    );

    Ok(())
}

// --------------------------------------------------------------------------------------------
// 5. Root-row growth.
// --------------------------------------------------------------------------------------------

/// Root rows grow with *distinct* states, not with actions: K actions sharing one state share one
/// root row whose count is K. Measure both the distinct-state ratio and the actual
/// `ledger_db_roots` growth caused by capturing them.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "SPIKE: requires an existing indexer + ledger DB pair via SPIKE_* env vars"]
async fn measure_root_row_growth() -> anyhow::Result<()> {
    let indexer = init().await?;

    let address = busiest_address(&indexer).await?;
    let actions =
        consecutive_actions(&indexer, &address, env_usize("SPIKE_DELTA_ACTIONS", 25)).await?;

    let roots_before = LedgerState::persisted_root_hashes().len();
    let keys = contract_state_keys(&actions)?;
    flush();
    let roots_after = LedgerState::persisted_root_hashes().len();

    let mut per_state = HashMap::<Vec<u8>, usize>::new();
    for (_, key) in &keys {
        *per_state.entry(key.hash().0.to_vec()).or_default() += 1;
    }

    println!(
        "roots address={} actions={} distinct_states={} max_refcount={} \
         root_rows_before={roots_before} root_rows_after={roots_after}",
        hex_prefix(&address),
        keys.len(),
        per_state.len(),
        per_state.values().copied().max().unwrap_or(0),
    );

    Ok(())
}

// --------------------------------------------------------------------------------------------
// Helpers.
// --------------------------------------------------------------------------------------------

/// Install the process-global arena over the ledger DB and return a pool on the indexer DB.
async fn init() -> anyhow::Result<SqlitePool> {
    ledger_db::init(ledger_db::Config {
        cache_max_nodes: env_usize("SPIKE_CACHE_MAX_NODES", 100_000),
        cnn_url: env_var("SPIKE_LEDGER_DB")?,
    })
    .await
    .context("init ledger db")?;

    SqlitePool::new(SqliteConfig {
        cnn_url: env_var("SPIKE_INDEXER_DB")?,
    })
    .await
    .context("create indexer pool")
}

/// Push the global arena's pending writes to SQL, so the measurement `LedgerDb` can see them.
fn flush() {
    default_storage::<v1_1::LedgerDb>().with_backend(|b| b.flush_all_changes_to_db());
}

/// A `Storage` of its own, so each latency cell starts from a genuinely cold read cache.
async fn local_storage(
    cnn_url: &str,
    cache_max_nodes: usize,
) -> anyhow::Result<Storage<v1_1::LedgerDb>> {
    Ok(Storage::new(
        cache_max_nodes,
        plain_ledger_db(cnn_url).await?,
    ))
}

async fn plain_ledger_db(cnn_url: &str) -> anyhow::Result<v1_1::LedgerDb> {
    let pool = SqlitePool::new(SqliteConfig {
        cnn_url: cnn_url.to_owned(),
    })
    .await
    .context("create ledger db pool")?;

    Ok(v1_1::LedgerDb::new(pool))
}

/// The most recent action of each of the `limit` most recently active distinct addresses.
///
/// Two phases on purpose: the first scans only the covering `(id, address)` index, so the ~1 MB
/// `state` blobs are fetched for the chosen rows alone.
async fn candidates(indexer: &SqlitePool, limit: usize) -> anyhow::Result<Vec<Candidate>> {
    let scan = env_usize("SPIKE_SCAN_ACTIONS", 5_000) as i64;

    let recent = sqlx::query_as::<_, (i64, Vec<u8>)>(
        "SELECT id, address FROM contract_actions ORDER BY id DESC LIMIT $1",
    )
    .bind(scan)
    .fetch_all(&**indexer)
    .await
    .context("scan recent contract actions")?;

    let mut seen = HashSet::new();
    let mut ids = vec![];
    for (id, address) in recent {
        if seen.insert(address) {
            ids.push(id);
            if ids.len() == limit {
                break;
            }
        }
    }

    let mut candidates = vec![];
    for id in ids {
        candidates.push(candidate(indexer, id).await?);
    }

    Ok(candidates)
}

/// The address with the most actions among the recently scanned ones: the interesting one for dedup
/// and delta, since growth is quadratic in actions per contract.
async fn busiest_address(indexer: &SqlitePool) -> anyhow::Result<SerializedContractAddress> {
    let scan = env_usize("SPIKE_SCAN_ACTIONS", 5_000) as i64;

    let (address, count) = sqlx::query_as::<_, (Vec<u8>, i64)>(
        "WITH recent AS (SELECT address FROM contract_actions ORDER BY id DESC LIMIT $1) \
         SELECT address, COUNT(*) AS n FROM recent GROUP BY address ORDER BY n DESC LIMIT 1",
    )
    .bind(scan)
    .fetch_optional(&**indexer)
    .await
    .context("find busiest contract address")?
    .context("no contract actions in the indexer database")?;

    println!(
        "busiest address {} with {count} of the last {scan} actions",
        hex_prefix(&address.clone().into())
    );

    Ok(address.into())
}

/// The newest `limit` actions on one address, oldest first.
async fn consecutive_actions(
    indexer: &SqlitePool,
    address: &SerializedContractAddress,
    limit: usize,
) -> anyhow::Result<Vec<Candidate>> {
    let ids = sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM contract_actions WHERE address = $1 ORDER BY id DESC LIMIT $2",
    )
    .bind(address.as_ref())
    .bind(limit as i64)
    .fetch_all(&**indexer)
    .await
    .context("get the recent action ids for an address")?;

    let mut candidates = vec![];
    for (id,) in ids.into_iter().rev() {
        candidates.push(candidate(indexer, id).await?);
    }

    Ok(candidates)
}

async fn candidate(indexer: &SqlitePool, id: i64) -> anyhow::Result<Candidate> {
    let (address, state, zswap_state, protocol_version, ledger_state_key, block_height) =
        sqlx::query_as::<_, (Vec<u8>, Vec<u8>, Vec<u8>, i64, Vec<u8>, i64)>(
            "SELECT contract_actions.address, contract_actions.state, \
                    contract_actions.zswap_state, transactions.protocol_version, \
                    blocks.ledger_state_key, blocks.height \
             FROM contract_actions \
             INNER JOIN transactions ON transactions.id = contract_actions.transaction_id \
             INNER JOIN blocks ON blocks.id = transactions.block_id \
             WHERE contract_actions.id = $1",
        )
        .bind(id)
        .fetch_one(&**indexer)
        .await
        .with_context(|| format!("get contract action {id}"))?;

    let ledger_version = ProtocolVersion::try_from(protocol_version)
        .context("convert protocol version")?
        .ledger_version();

    Ok(Candidate {
        id,
        block_height,
        address: address.into(),
        state: state.into(),
        zswap_state: zswap_state.into(),
        ledger_state_key: ledger_state_key.into(),
        ledger_version,
    })
}

/// The arena keys of the candidates' contract states, skipping the ones whose ledger state has aged
/// out of the retention window and the ones whose contract is absent.
fn contract_state_keys(candidates: &[Candidate]) -> anyhow::Result<Vec<(Candidate, StateKey)>> {
    let mut keys = vec![];

    for candidate in candidates {
        if !LedgerState::root_loadable(&candidate.ledger_state_key, candidate.ledger_version)? {
            continue;
        }
        let ledger_state =
            LedgerState::load(&candidate.ledger_state_key, candidate.ledger_version)?;
        if let Some(key) = contract_state_key(&ledger_state, &candidate.address)? {
            keys.push((candidate.to_owned(), key));
        }
    }

    Ok(keys)
}

/// Version-dispatched arena key of a contract state. The two variants are separate types with
/// layouts that are not compatible, and arena node payloads carry no version tag, so the key has to
/// record which one it is.
enum StateKey {
    V3(TypedArenaKey<ContractStateV3<v1_1::LedgerDb>, DefaultHasher>),
    V4(TypedArenaKey<ContractStateV4<v1_1::LedgerDb>, DefaultHasher>),
}

impl StateKey {
    fn hash(&self) -> &ArenaHash<DefaultHasher> {
        match self {
            Self::V3(key) => key.key.hash(),
            Self::V4(key) => key.key.hash(),
        }
    }
}

/// ORDER IS LOAD-BEARING: `persist()` before `as_typed_key()`. `ContractState`'s own payload is
/// under storage-core's small-object limit, so before persisting its key is an `ArenaKey::Direct`
/// that inlines the payload rather than referencing a row; `persist()` promotes it to a `Ref`.
/// `lookup_sp` returns the already-allocated `Sp`, so nothing is hashed or copied here.
fn contract_state_key(
    ledger_state: &LedgerState,
    address: &SerializedContractAddress,
) -> anyhow::Result<Option<StateKey>> {
    match ledger_state {
        LedgerState::V8 { ledger_state, .. } => {
            let address = ContractAddressV8::deserialize(&mut address.as_ref(), 0)
                .context("deserialize contract address")?;

            Ok(ledger_state.contract.lookup_sp(&address).map(|mut sp| {
                sp.persist();
                StateKey::V3(sp.as_typed_key())
            }))
        }

        LedgerState::V9 { ledger_state, .. } => {
            let address = ContractAddressV9::deserialize(&mut address.as_ref(), 0)
                .context("deserialize contract address")?;

            Ok(ledger_state.contract.lookup_sp(&address).map(|mut sp| {
                sp.persist();
                StateKey::V4(sp.as_typed_key())
            }))
        }
    }
}

/// Tagged-serialize the contract state held in the arena, straight off the `Sp`: the derived
/// `Serializable` of a `Storable` goes through `Sp::new(self.clone())`, so serializing the `Sp` we
/// already have is byte-identical and skips a clone plus a re-allocation.
fn contract_state_bytes(
    ledger_state: &LedgerState,
    address: &SerializedContractAddress,
) -> anyhow::Result<Option<ByteVec>> {
    match ledger_state {
        LedgerState::V8 { ledger_state, .. } => {
            let address = ContractAddressV8::deserialize(&mut address.as_ref(), 0)
                .context("deserialize contract address")?;

            ledger_state
                .contract
                .lookup_sp(&address)
                .map(|sp| tagged_bytes(&sp))
                .transpose()
        }

        LedgerState::V9 { ledger_state, .. } => {
            let address = ContractAddressV9::deserialize(&mut address.as_ref(), 0)
                .context("deserialize contract address")?;

            ledger_state
                .contract
                .lookup_sp(&address)
                .map(|sp| tagged_bytes(&sp))
                .transpose()
        }
    }
}

/// Allocate a contract's filtered zswap state into the arena, root it, and return its arena hash.
fn alloc_zswap_state(
    ledger_state: &LedgerState,
    address: &SerializedContractAddress,
) -> anyhow::Result<ArenaHash<DefaultHasher>> {
    let storage = default_storage::<v1_1::LedgerDb>();

    let hash = match ledger_state {
        LedgerState::V8 { ledger_state, .. } => {
            let address = ContractAddressV8::deserialize(&mut address.as_ref(), 0)
                .context("deserialize contract address")?;

            let mut zswap_state = ZswapStateV8::new();
            zswap_state.coin_coms = ledger_state.zswap.filter(&[address]);

            let mut sp = storage.alloc(zswap_state);
            sp.persist();
            sp.hash()
        }

        LedgerState::V9 { ledger_state, .. } => {
            let address = ContractAddressV9::deserialize(&mut address.as_ref(), 0)
                .context("deserialize contract address")?;

            let mut zswap_state = ZswapStateV9::new();
            zswap_state.coin_coms = ledger_state.zswap.filter(&[address]);

            let mut sp = storage.alloc(zswap_state);
            sp.persist();
            sp.hash()
        }
    };

    Ok(hash)
}

fn load_and_serialize(
    storage: &Storage<v1_1::LedgerDb>,
    candidate: &Candidate,
    key: &StateKey,
) -> anyhow::Result<ByteVec> {
    match key {
        StateKey::V3(key) => tagged_bytes(
            &storage
                .get_lazy(key)
                .with_context(|| format!("get contract state of action {}", candidate.id))?,
        ),

        StateKey::V4(key) => tagged_bytes(
            &storage
                .get_lazy(key)
                .with_context(|| format!("get contract state of action {}", candidate.id))?,
        ),
    }
}

fn tagged_bytes<T>(value: &T) -> anyhow::Result<ByteVec>
where
    T: Serializable + Tagged,
{
    let mut bytes = Vec::with_capacity(value.serialized_size() + 32);
    tagged_serialize(value, &mut bytes).context("tagged-serialize")?;

    Ok(bytes.into())
}

/// The reachable node set, DAG depth and total on-disk size below `root`, straight off the `DB`
/// trait. This is `bfs_get_nodes`' own traversal, spelled out level by level so that the depth
/// falls out of it too, and using the same batched `batch_get_nodes` it uses.
fn reachable(
    db: &v1_1::LedgerDb,
    root: &ArenaHash<DefaultHasher>,
) -> (HashSet<ArenaHash<DefaultHasher>>, usize, usize) {
    let mut visited = HashSet::new();
    let mut bytes = 0;
    let mut depth = 0;
    let mut level = vec![root.to_owned()];

    while !level.is_empty() {
        let unknown = level
            .into_iter()
            .filter(|hash| visited.insert(hash.to_owned()))
            .collect::<Vec<_>>();

        let mut next = vec![];
        for (_, object) in db.batch_get_nodes(unknown.into_iter()) {
            let Some(object) = object else { continue };
            bytes += object.serialized_size();
            next.extend(
                object
                    .children
                    .iter()
                    .flat_map(|child| child.refs())
                    .cloned(),
            );
        }

        if next.is_empty() {
            break;
        }
        depth += 1;
        level = next;
    }

    (visited, depth, bytes)
}

fn env_var(name: &str) -> anyhow::Result<String> {
    std::env::var(name).with_context(|| format!("{name} must be set"))
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn hex_prefix(bytes: &SerializedContractAddress) -> String {
    const_hex::encode(&bytes.as_ref()[..bytes.len().min(8)])
}

fn hex_hash(hash: &ArenaHash<DefaultHasher>) -> String {
    const_hex::encode(&hash.0.as_slice()[..8])
}
