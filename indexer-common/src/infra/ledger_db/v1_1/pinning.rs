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

//! Pinning, the only writes the indexer makes to the ledger DB beside storage-core's.
//!
//! It writes nothing but the `pinned` flags of `ledger_db_nodes` and `ledger_db_roots`, which
//! storage-core neither reads nor writes: the stored objects and root counts, the ledger state,
//! stay as they are. The flags only change which nodes gc considers: its sweep skips pinned nodes,
//! and its mark starts from the unpinned roots.
//!
//! Pins move through three phases:
//! - queued: roots to pin, see [queue];
//! - pinning: storage-core's flush writes a queued root's new nodes pinned, see [Pinning];
//! - pinned: [PINNED], the nodes this process knows are pinned.
//!
//! A root a flush cannot pin is pinned after it by walking the ledger DB, see [pin_queued].

#[cfg(feature = "standalone")]
use super::BATCH_DELETE_SIZE;
#[cfg(feature = "cloud")]
use super::BATCH_ROWS;
use super::{LedgerDb, ResultExt};
use crate::domain::ledger;
use indoc::indoc;
#[cfg(feature = "cloud")]
use itertools::{EitherOrBoth, Itertools};
use midnight_serialize_v1::Deserializable;
use midnight_storage_core_v1::{DefaultHasher, arena::ArenaHash, backend::OnDiskObject};
#[cfg(feature = "standalone")]
use sqlx::QueryBuilder;
use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, Mutex, OnceLock},
};
use tokio::{runtime::Handle, task::block_in_place};

/// Nodes this process pinned or found pinned, so [Pinning::nodes] decides without reading. A node
/// is never unpinned, nor a pinned node deleted, so a node here stays pinned; dropping one only
/// means the walk reads it again.
static PINNED: LazyLock<Mutex<PinnedNodes>> = LazyLock::new(Default::default);
/// The handle pinning writes through, set by [init]. Nothing else writes the flags.
static PINNER: OnceLock<LedgerDb> = OnceLock::new();
/// The roots to pin, the input; see [queue].
static QUEUE: LazyLock<Mutex<Queue>> = LazyLock::new(Default::default);

/// Set the handle pinning writes through, and how many nodes [PINNED] holds at most; see
/// [crate::infra::ledger_db::init].
pub(in crate::infra::ledger_db) fn init(ledger_db: LedgerDb, max_pinned: usize) {
    let _ = PINNER.set(ledger_db);
    PINNED
        .lock()
        .expect("lock pinned nodes")
        .set_max(max_pinned);
}

/// The roots queued to pin and not pinned yet, and the nodes pinned as written since the last
/// [pin_queued].
#[derive(Debug, Default)]
struct Queue {
    roots: HashSet<ArenaHash<DefaultHasher>>,
    pinned_on_write: usize,
    roots_pinned_on_write: usize,
}

/// A set of at most `max` keys, one more if odd, in two generations, approximating least recently
/// used: a key found in the previous generation moves to the current one, and a full current
/// generation becomes the previous one, forgetting the keys left there. Unbounded if `max` is zero,
/// as storage-core's cache is; holds none until `max` is set.
#[derive(Debug, Default)]
struct PinnedNodes {
    current: HashSet<ArenaHash<DefaultHasher>>,
    previous: HashSet<ArenaHash<DefaultHasher>>,
    generation: usize,
}

impl PinnedNodes {
    fn set_max(&mut self, max: usize) {
        self.generation = if max == 0 {
            usize::MAX
        } else {
            max.div_ceil(2)
        };
    }

    fn contains(&mut self, key: &ArenaHash<DefaultHasher>) -> bool {
        if self.current.contains(key) {
            return true;
        }
        if self.previous.remove(key) {
            self.insert(key.clone());
            return true;
        }
        false
    }

    fn insert(&mut self, key: ArenaHash<DefaultHasher>) {
        if self.generation == 0 || self.current.contains(&key) {
            return;
        }
        self.previous.remove(&key);
        if self.current.len() >= self.generation {
            self.previous = std::mem::take(&mut self.current);
        }
        self.current.insert(key);
    }

    fn remove(&mut self, key: &ArenaHash<DefaultHasher>) {
        self.current.remove(key);
        self.previous.remove(key);
    }
}

// Empties [PINNED], to hold `max` nodes at most from now.
#[cfg(test)]
fn reset_pinned(max: usize) {
    let mut pinned = PINNED.lock().expect("lock pinned nodes");
    *pinned = PinnedNodes::default();
    pinned.set_max(max);
}

// Adds `keys` to [PINNED].
fn add_pinned(keys: impl IntoIterator<Item = ArenaHash<DefaultHasher>>) {
    let mut pinned = PINNED.lock().expect("lock pinned nodes");
    keys.into_iter().for_each(|key| pinned.insert(key));
}

/// Removes `keys`, of deleted nodes, from [PINNED]. Nothing deletes a pinned node, but a node in
/// [PINNED] must be stored.
pub(super) fn remove_pinned<'a>(keys: impl IntoIterator<Item = &'a ArenaHash<DefaultHasher>>) {
    let mut pinned = PINNED.lock().expect("lock pinned nodes");
    keys.into_iter().for_each(|key| pinned.remove(key));
}

/// Queue a persisted root to pin, with everything it reaches: by [Pinning::nodes] as its nodes are
/// written, or else by the next [pin_queued].
pub(crate) fn queue(root: ArenaHash<DefaultHasher>) {
    QUEUE.lock().expect("lock pinning queue").roots.insert(root);
}

/// Pins the roots still queued, which [Pinning::nodes] did not pin, by walking the ledger DB, so
/// call it once their nodes are flushed. Returns those nodes and the ones pinned as written since
/// the last call. A pinned node's descendants are pinned already, so the walk stops there; a root
/// reaching a missing node is left unpinned, with all it reaches.
///
/// Panics if the ledger DB is not initialized, rather than pinning nothing and reporting that as
/// done.
pub(crate) fn pin_queued() -> ledger::Pinned {
    let Queue {
        roots,
        pinned_on_write,
        roots_pinned_on_write,
    } = take_queued();
    let roots_walked = roots.len();
    let mut pinned = PINNER
        .get()
        .expect("ledger DB initialized before pinning")
        .pin_reachable(roots);
    pinned.nodes += pinned_on_write;
    pinned.roots_pinned_on_write = roots_pinned_on_write;
    pinned.roots_walked = roots_walked;
    pinned
}

/// Empties the queue, so the roots in it are stored unpinned, as the history written before
/// pinning existed.
#[cfg(test)]
pub(crate) fn clear_queue() {
    take_queued();
}

// The queued roots and what was pinned as written, leaving the queue empty.
fn take_queued() -> Queue {
    std::mem::take(&mut *QUEUE.lock().expect("lock pinning queue"))
}

/// The pins of one flush, between the queue and [PINNED]: the written nodes inserted pinned, see
/// [Pinning::nodes], and the queued roots whose rows are written pinned with their counts, see
/// [Pinning::roots]. Once a flush is committed, [Pinning::commit] takes the roots from the queue
/// and adds the nodes to [PINNED]. A flush that deletes pins nothing as written, see
/// [Pinning::evict].
#[derive(Debug, Default)]
pub(super) struct Pinning {
    nodes: HashSet<ArenaHash<DefaultHasher>>,
    roots: HashSet<ArenaHash<DefaultHasher>>,
    evicted: Vec<ArenaHash<DefaultHasher>>,
}

impl Pinning {
    /// Which of `candidates`, a flush's inserts about to be written, to write pinned: the nodes
    /// queued roots among them reach whose descendants are all written pinned with them or are in
    /// [PINNED]. It reads nothing, and pins none if no queued root is among `candidates` or the
    /// flush deletes. A root reaching a stored node not in [PINNED] stays queued for [pin_queued].
    pub(super) fn nodes(
        &mut self,
        candidates: &[(ArenaHash<DefaultHasher>, OnDiskObject<DefaultHasher>)],
    ) -> &HashSet<ArenaHash<DefaultHasher>> {
        // The queued roots among the candidates, if any, unless this flush deletes.
        if candidates.is_empty() || !self.evicted.is_empty() {
            return &self.nodes;
        }
        let roots = {
            let queue = QUEUE.lock().expect("lock pinning queue");
            if queue.roots.is_empty() {
                return &self.nodes;
            }
            candidates
                .iter()
                .map(|(key, _)| key)
                .filter(|key| queue.roots.contains(key))
                .collect::<Vec<_>>()
        };
        if roots.is_empty() {
            return &self.nodes;
        }

        // Each candidate's object, by key.
        let objects = candidates
            .iter()
            .map(|(key, object)| (key, object))
            .collect::<HashMap<_, _>>();

        // Walk down from the roots through candidates, noting parents and the stored nodes reached.
        let mut reached = HashSet::new();
        let mut parents = HashMap::<_, Vec<_>>::new();
        let mut outside = HashSet::new();
        let mut frontier = roots;
        while let Some(node) = frontier.pop() {
            if !reached.insert(node) {
                continue;
            }
            for child in refs(objects[node]) {
                parents.entry(child).or_default().push(node);
                if objects.contains_key(child) {
                    frontier.push(child);
                } else {
                    outside.insert(child);
                }
            }
        }

        // Mark unpinnable every node reaching a stored node not known to be pinned.
        let mut unpinnable = HashSet::new();
        let mut frontier = {
            let mut pinned = PINNED.lock().expect("lock pinned nodes");
            outside
                .into_iter()
                .filter(|node| !pinned.contains(node))
                .collect::<Vec<_>>()
        };
        while let Some(node) = frontier.pop() {
            for parent in parents.get(node).into_iter().flatten() {
                if unpinnable.insert(*parent) {
                    frontier.push(*parent);
                }
            }
        }

        // Write pinned the reached nodes that are not unpinnable.
        self.nodes.extend(
            reached
                .into_iter()
                .filter(|node| !unpinnable.contains(node))
                .cloned(),
        );

        &self.nodes
    }

    /// Which root rows among `root_counts`, about to be written, to write pinned with their
    /// counts: those of queued roots whose nodes are written pinned in this flush or are in
    /// [PINNED]. That covers a root whose node is stored already and not written again, such as an
    /// unchanged contract state, whose persist writes only its count. A queued root written without
    /// its count stays queued for [pin_queued].
    pub(super) fn roots(
        &mut self,
        root_counts: &[(ArenaHash<DefaultHasher>, u32)],
    ) -> &HashSet<ArenaHash<DefaultHasher>> {
        // Nothing to pin without queued roots, or once this flush deletes.
        if !self.evicted.is_empty() {
            return &self.roots;
        }
        let queue = QUEUE.lock().expect("lock pinning queue");
        if queue.roots.is_empty() {
            return &self.roots;
        }

        // Write pinned the rows of queued roots given a count whose nodes are pinned, going by the
        // last count per root, as it is the one written.
        let mut pinned = PINNED.lock().expect("lock pinned nodes");
        let counts = root_counts
            .iter()
            .map(|(root, count)| (root, *count))
            .collect::<HashMap<_, _>>();
        for (root, count) in counts {
            if count > 0
                && queue.roots.contains(root)
                && (self.nodes.contains(root) || pinned.contains(root))
            {
                self.roots.insert(root.clone());
            }
        }

        &self.roots
    }

    /// A node a flush deletes leaves [PINNED] once it is committed, and from then on the flush
    /// pins nothing as written: the node might be one it writes, or below one, so its roots are
    /// left queued for the walk. Storage-core's flush never deletes, and gc's batches only delete.
    /// Called before any part of the flush is written, as the first delete is what writes the
    /// inserts before it.
    pub(super) fn evict(&mut self, key: &ArenaHash<DefaultHasher>) {
        self.evicted.push(key.clone());
    }

    /// Moves the pins on, once a flush is committed: the roots leave the queue, the nodes join
    /// [PINNED], and the deleted nodes leave it.
    pub(super) fn commit(self) {
        {
            let mut queue = QUEUE.lock().expect("lock pinning queue");
            for root in &self.roots {
                queue.roots.remove(root);
            }
            queue.pinned_on_write += self.nodes.len();
            queue.roots_pinned_on_write += self.roots.len();
        }
        add_pinned(self.nodes);
        remove_pinned(&self.evicted);
    }
}

// The hashes a node references, through its inlined children too.
fn refs(object: &OnDiskObject<DefaultHasher>) -> impl Iterator<Item = &ArenaHash<DefaultHasher>> {
    object.children.iter().flat_map(|child| child.refs())
}

/// Whether the node with `key` is pinned, `None` if it is not stored.
#[cfg(test)]
pub(crate) fn pinned(key: &ArenaHash<DefaultHasher>) -> Option<bool> {
    PINNER
        .get()
        .expect("ledger DB initialized before pinning")
        .pinned(key)
}

// Keys per read while pinning: one bind each on SQLite, one array parameter on Postgres.
#[cfg(feature = "standalone")]
const PIN_READ_SIZE: usize = BATCH_DELETE_SIZE;
#[cfg(feature = "cloud")]
const PIN_READ_SIZE: usize = BATCH_ROWS;

impl LedgerDb {
    /// Pins every node reachable from `roots` that is not pinned yet. A pinned node's descendants
    /// are pinned already, so the walk stops there.
    ///
    /// gc does not mark from pinned roots, so a node reachable only through pinned ones is marked
    /// by nothing, and if it were unpinned, the sweep would cull it. A root reaching a missing node
    /// is therefore left unpinned, with all it reaches, and marked as before; the other roots are
    /// pinned regardless.
    fn pin_reachable(&self, roots: HashSet<ArenaHash<DefaultHasher>>) -> ledger::Pinned {
        if roots.is_empty() {
            return ledger::Pinned::default();
        }

        if let Some(nodes) = self.pin_all_reachable(&roots) {
            return ledger::Pinned {
                nodes,
                ..Default::default()
            };
        }

        // A node is missing below some root: pin root by root, so it holds back only its own.
        roots
            .into_iter()
            .fold(ledger::Pinned::default(), |mut pinned, root| {
                match self.pin_all_reachable([&root]) {
                    Some(nodes) => pinned.nodes += nodes,
                    None => pinned.failed_roots.push(root),
                }
                pinned
            })
    }

    /// Pins every node reachable from `roots` that is not pinned yet, and the root rows of `roots`,
    /// and returns how many nodes, or pins none and returns `None` if any of them is missing.
    fn pin_all_reachable<'a>(
        &self,
        roots: impl IntoIterator<Item = &'a ArenaHash<DefaultHasher>> + Clone,
    ) -> Option<usize> {
        // Walk down from the roots, reading the frontier in batches.
        let mut seen = roots.clone().into_iter().cloned().collect::<HashSet<_>>();
        let mut frontier = seen.iter().cloned().collect::<Vec<_>>();
        let mut to_pin = vec![];

        while !frontier.is_empty() {
            let batch = frontier.split_off(frontier.len().saturating_sub(PIN_READ_SIZE));
            let rows = self.children_rows(&batch);

            // A missing node leaves all the roots unpinned.
            if rows.len() < batch.len() {
                return None;
            }

            // Follow unpinned rows' children, and record pinned rows in `PINNED`.
            let mut found_pinned = vec![];
            for (key, pinned, children) in rows {
                // A pinned node's descendants are pinned already.
                if pinned {
                    found_pinned.push(key);
                    continue;
                }
                for child in children {
                    if seen.insert(child.clone()) {
                        frontier.push(child);
                    }
                }
                to_pin.push(key);
            }
            add_pinned(found_pinned);
        }

        // Flag every root's row, as a root's node may be pinned already through another root.
        let roots = roots
            .into_iter()
            .map(|root| root.0.to_vec())
            .collect::<Vec<_>>();
        let nodes = to_pin.iter().map(|key| key.0.to_vec()).collect::<Vec<_>>();

        // Write the flags, then record the nodes in `PINNED`.
        block_in_place(|| Handle::current().block_on(pin_keys(&self.pool, &roots, &nodes)));
        add_pinned(to_pin.iter().cloned());

        Some(to_pin.len())
    }

    /// Whether the node with `key` is pinned, `None` if it is not stored.
    #[cfg(test)]
    fn pinned(&self, key: &ArenaHash<DefaultHasher>) -> Option<bool> {
        block_in_place(|| {
            Handle::current().block_on(
                sqlx::query_as::<_, (bool,)>("SELECT pinned FROM ledger_db_nodes WHERE key = $1")
                    .bind(key.0.as_slice())
                    .fetch_optional(&*self.pool),
            )
        })
        .unwrap_or_panic("cannot get pinned")
        .map(|(pinned,)| pinned)
    }

    /// The gc roots that are not pinned. What pinned roots reach is pinned, never swept, so gc
    /// need not mark them.
    ///
    /// Reads `ledger_db_roots.pinned` through its partial index, so the cost follows the number of
    /// unpinned roots. The flag is set as the roots are pinned. A root row written for a node
    /// pinned before, without pinning the root itself, is left unflagged: gc then marks from it,
    /// which costs time but culls nothing.
    pub(crate) fn unpinned_roots(&self) -> Vec<ArenaHash<DefaultHasher>> {
        block_in_place(|| {
            Handle::current().block_on(async {
                let query = indoc! {"
                    SELECT key
                    FROM ledger_db_roots
                    WHERE NOT pinned
                "};

                sqlx::query_as::<_, (Vec<u8>,)>(query)
                    .fetch_all(&*self.pool)
                    .await
                    .unwrap_or_panic("cannot get unpinned roots")
                    .into_iter()
                    .map(|(key,)| {
                        ArenaHash::<DefaultHasher>::deserialize(&mut key.as_slice(), 0)
                            .unwrap_or_panic("cannot deserialize key as ArenaHash")
                    })
                    .collect()
            })
        })
    }
}

// Sets `pinned` on the root rows of `roots` not flagged yet, so a root pinned before is not written
// again, and on the nodes with the given keys. SQLite has no array parameters, so keys are bound
// one each in chunks. In one transaction, so no root is flagged without its nodes pinned.
#[cfg(feature = "standalone")]
async fn pin_keys(pool: &sqlx::SqlitePool, roots: &[Vec<u8>], nodes: &[Vec<u8>]) {
    let updates = [
        (
            "UPDATE ledger_db_roots SET pinned = TRUE WHERE NOT pinned AND key IN (",
            roots,
        ),
        (
            "UPDATE ledger_db_nodes SET pinned = TRUE WHERE key IN (",
            nodes,
        ),
    ];

    let mut tx = pool
        .begin()
        .await
        .unwrap_or_panic("begin transaction for pinning");

    for (update, keys) in updates {
        for chunk in keys.chunks(BATCH_DELETE_SIZE) {
            let mut query = QueryBuilder::new(update);
            let mut bindings = query.separated(", ");
            for key in chunk {
                bindings.push_bind(key.as_slice());
            }
            query.push(")");

            query
                .build()
                .execute(&mut *tx)
                .await
                .unwrap_or_panic("cannot pin keys");
        }
    }

    tx.commit()
        .await
        .unwrap_or_panic("commit transaction for pinning");
}

// Sets `pinned` on the root rows of `roots` not flagged yet, so a root pinned before is not written
// again, and on the nodes with the given keys, both in one statement per chunk. A single statement
// is atomic, so it is sent alone, in one round trip; more chunks run in one transaction, so no root
// is flagged without its nodes pinned.
#[cfg(feature = "cloud")]
async fn pin_keys(pool: &sqlx::PgPool, roots: &[Vec<u8>], nodes: &[Vec<u8>]) {
    let chunks = roots
        .chunks(BATCH_ROWS)
        .zip_longest(nodes.chunks(BATCH_ROWS))
        .map(EitherOrBoth::or_default)
        .collect::<Vec<_>>();

    match chunks.as_slice() {
        [] => {}
        [(roots, nodes)] => pin_chunk(pool, roots, nodes).await,
        chunks => {
            let mut tx = pool
                .begin()
                .await
                .unwrap_or_panic("begin transaction for pinning");
            for (roots, nodes) in chunks {
                pin_chunk(&mut *tx, roots, nodes).await;
            }
            tx.commit()
                .await
                .unwrap_or_panic("commit transaction for pinning");
        }
    }
}

#[cfg(feature = "cloud")]
async fn pin_chunk<'e>(executor: impl sqlx::PgExecutor<'e>, roots: &[Vec<u8>], nodes: &[Vec<u8>]) {
    let query = indoc! {"
        WITH roots AS (
            UPDATE ledger_db_roots
            SET pinned = TRUE
            WHERE key = ANY($1::bytea[]) AND NOT pinned
        )
        UPDATE ledger_db_nodes
        SET pinned = TRUE
        WHERE key = ANY($2::bytea[])
    "};

    sqlx::query(query)
        .bind(roots)
        .bind(nodes)
        .execute(executor)
        .await
        .unwrap_or_panic("cannot pin keys");
}

#[cfg(test)]
mod tests {
    use super::{LedgerDb, PIN_READ_SIZE, PINNED, Queue, queue, reset_pinned, take_queued};
    use crate::infra::ledger_db::v1_1::persist_nodes;
    use midnight_storage_core_v1::{
        Storage, arena::Sp, db::DB, db::Update::DeleteNode, storable::SMALL_OBJECT_LIMIT,
    };
    use std::collections::HashSet;

    // gc never culls a pinned node, even one no root it marks from reaches, while it still culls
    // unpinned garbage, and pinned nodes still load. Pinning is all or nothing per root: a root
    // reaching a missing node is left unpinned, without holding back the others. Runs
    // storage-core's own gc, so it fails if a storage-core upgrade stops sweeping through
    // `DB::scan_keys`. Shared by both suites.
    fn assert_gc_keeps_pinned_nodes(storage: &Storage<LedgerDb>, db: &mut LedgerDb) {
        use std::time::Duration;

        // A leaf is stored as its own node; a persisted `Option` holding one references it.
        let leaf = |value| storage.alloc([value; SMALL_OBJECT_LIMIT / 4]);
        let rooted = |value| {
            let mut root = storage.alloc(Some(leaf(value)));
            root.persist();
            root
        };
        let roots = [rooted(1), rooted(2), rooted(3)];
        storage.with_backend(|backend| backend.flush_all_changes_to_db());
        let [contract, ledger, garbage] = roots.map(|root| {
            let leaf = root.as_ref().as_ref().expect("leaf").hash();
            (root.hash(), leaf)
        });

        // A root reaching a missing node is left unpinned, without holding back the others.
        db.batch_update([(garbage.1.clone(), DeleteNode)].into_iter());
        let pinned = db.pin_reachable(HashSet::from([garbage.0.clone(), contract.0.clone()]));
        assert_eq!(pinned.nodes, 2);
        assert_eq!(pinned.failed_roots, vec![garbage.0.clone()]);
        assert_eq!(
            db.pin_reachable(HashSet::from([contract.0.clone()])).nodes,
            0,
            "pinned already"
        );
        assert_eq!(
            db.batch_get_children([contract.0.clone()].into_iter()),
            vec![(contract.0.clone(), vec![])],
            "pinned nodes report no children"
        );

        let (scanned, _) = db.scan_keys(None, 100);
        let scanned = scanned.into_iter().collect::<HashSet<_>>();
        assert_eq!(
            scanned,
            HashSet::from([ledger.0.clone(), ledger.1.clone(), garbage.0.clone()])
        );

        // A root whose node was pinned through another root is flagged as it is pinned itself.
        db.set_root_count(contract.1.clone(), 1);
        assert_eq!(
            db.pin_reachable(HashSet::from([contract.1.clone()])).nodes,
            0
        );

        // The contract's roots are flagged pinned; the garbage's, left unpinned, is not.
        assert_eq!(
            db.unpinned_roots().into_iter().collect::<HashSet<_>>(),
            HashSet::from([ledger.0.clone(), garbage.0.clone()])
        );

        // Neither the contract's nor the garbage's root is marked from any more; only the garbage is
        // unpinned.
        db.set_root_count(contract.0.clone(), 0);
        db.set_root_count(garbage.0.clone(), 0);
        assert_eq!(db.unpinned_roots(), vec![ledger.0.clone()]);
        storage.with_backend(|backend| {
            backend.gc_override_gc_roots(Duration::MAX, |db| db.unpinned_roots())
        });

        for key in [&contract.0, &contract.1, &ledger.0, &ledger.1] {
            assert!(db.get_node(key).is_some(), "{key:?} kept");
        }
        let loaded = db.batch_get_nodes([contract.0.clone(), contract.1.clone()].into_iter());
        assert!(
            loaded.iter().all(|(_, object)| object.is_some()),
            "pinned nodes load"
        );
        for key in [&garbage.0, &garbage.1] {
            assert!(db.get_node(key).is_none(), "{key:?} culled");
        }
    }

    // Pinning crosses every batch: more roots than one read takes, so the walk reads in batches and
    // the write is split over several statements, all committed. A new root above an already pinned
    // node pins only its own node, and its root row is flagged. Shared by both suites.
    fn assert_pinning_crosses_batches(storage: &Storage<LedgerDb>, db: &mut LedgerDb) {
        let count = PIN_READ_SIZE + 1;
        let roots = persist_nodes(storage, db, count)
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>();

        let pinned = db.pin_reachable(roots.iter().cloned().collect());
        assert_eq!(pinned.nodes, count);
        assert!(pinned.failed_roots.is_empty());
        assert!(db.unpinned_roots().is_empty(), "every root row flagged");
        assert_eq!(db.pinned(&roots[0]), Some(true));
        assert_eq!(db.pinned(&roots[count - 1]), Some(true));

        // A value no root above holds, so the leaf is a node of its own.
        let leaf = storage.alloc([u32::MAX; SMALL_OBJECT_LIMIT / 4]);
        let mut first = storage.alloc(Some(leaf.clone()));
        first.persist();
        storage.with_backend(|backend| backend.flush_all_changes_to_db());
        assert_eq!(
            db.pin_reachable(HashSet::from([first.hash()])).nodes,
            2,
            "the root and its leaf"
        );

        // An inlined `Option` in between gives the second root its own node above the same leaf.
        let mut second = storage.alloc(Some(storage.alloc(Some(leaf.clone()))));
        second.persist();
        storage.with_backend(|backend| backend.flush_all_changes_to_db());
        assert_eq!(
            db.pin_reachable(HashSet::from([second.hash()])).nodes,
            1,
            "only the new root"
        );
        assert!(db.unpinned_roots().is_empty(), "the new root's row flagged");
    }

    // Pinning between bounded gc passes: storage-core keeps a cycle's mark set across passes and
    // reads the roots only as a cycle starts, so a root pinned after its cycle started, and one
    // written and pinned after, are both kept by the sweep, which skips pinned rows, as is all a
    // later cycle, which no longer marks from them. `gc` is a second `Storage` on the same ledger
    // DB, with a cold cache, so a pass without budget starts a cycle and stops before marking any
    // node. Shared by both suites.
    fn assert_bounded_gc_keeps_nodes_pinned_between_passes(
        storage: &Storage<LedgerDb>,
        gc: &Storage<LedgerDb>,
        db: &mut LedgerDb,
    ) {
        use std::time::Duration;

        // A leaf is stored as its own node; a persisted `Option` holding one references it.
        let leaf = |value| storage.alloc([value; SMALL_OBJECT_LIMIT / 4]);
        let rooted = |value| {
            let mut root = storage.alloc(Some(leaf(value)));
            root.persist();
            root
        };
        let roots = [rooted(1), rooted(2), rooted(3)];
        storage.with_backend(|backend| backend.flush_all_changes_to_db());
        let [contract, ledger, garbage] = roots.map(|root| {
            let leaf = root.as_ref().as_ref().expect("leaf").hash();
            (root.hash(), leaf)
        });
        db.set_root_count(garbage.0.clone(), 0);
        let gc_pass =
            |bound| gc.with_backend(|b| b.gc_override_gc_roots(bound, |db| db.unpinned_roots()));

        // The cycle starts from the unpinned roots, the contract's among them, and marks nothing.
        assert_eq!(gc_pass(Duration::ZERO), 0);

        // The contract is pinned, and a root is written and pinned, after the cycle started.
        assert_eq!(
            db.pin_reachable(HashSet::from([contract.0.clone()])).nodes,
            2
        );
        let late = rooted(4);
        storage.with_backend(|backend| backend.flush_all_changes_to_db());
        let late = (late.hash(), late.as_ref().as_ref().expect("leaf").hash());
        assert_eq!(db.pin_reachable(HashSet::from([late.0.clone()])).nodes, 2);

        // The cycle completes, culling only the garbage, and a new one culls nothing.
        assert_eq!(gc_pass(Duration::MAX), 2);
        assert_eq!(db.unpinned_roots(), vec![ledger.0.clone()]);
        assert_eq!(gc_pass(Duration::MAX), 0);

        for key in [
            &contract.0,
            &contract.1,
            &late.0,
            &late.1,
            &ledger.0,
            &ledger.1,
        ] {
            assert!(db.get_node(key).is_some(), "{key:?} kept");
        }
        for key in [&garbage.0, &garbage.1] {
            assert!(db.get_node(key).is_none(), "{key:?} culled");
        }
    }

    // Queued roots' written nodes are inserted pinned in a flush, with their root rows, when all
    // they reach is written with them or in `PINNED`; a root reaching a stored node not in it,
    // older and unpinned, missing, or pinned but dropped, stays queued for the walk, which adds
    // what it pins or finds pinned. A stored node written again, unqueued, as by `other`, whose
    // cold cache does not know it is stored, stays pinned. Shared by both suites.
    fn assert_pinning_on_write(
        storage: &Storage<LedgerDb>,
        other: &Storage<LedgerDb>,
        db: &mut LedgerDb,
    ) {
        let flush = || storage.with_backend(|backend| backend.flush_all_changes_to_db());
        let rooted = |leaf: &Sp<[u32; SMALL_OBJECT_LIMIT / 4], LedgerDb>| {
            let mut root = storage.alloc(Some(leaf.clone()));
            root.persist();
            root
        };
        take_queued();
        reset_pinned(1_000);

        // A new root and its leaf are written pinned, the root row flagged.
        let leaf = storage.alloc([u32::MAX; SMALL_OBJECT_LIMIT / 4]);
        let first = rooted(&leaf);
        queue(first.hash());
        flush();
        assert_eq!(db.pinned(&first.hash()), Some(true));
        assert_eq!(db.pinned(&leaf.hash()), Some(true));
        assert!(!db.unpinned_roots().contains(&first.hash()));
        let taken = take_queued();
        assert_eq!(
            (taken.roots, taken.pinned_on_write),
            (HashSet::new(), 2),
            "pinned as written"
        );

        // The pinned leaf written again, unqueued, stays pinned.
        let mut again = other.alloc([u32::MAX; SMALL_OBJECT_LIMIT / 4]);
        again.persist();
        other.with_backend(|backend| backend.flush_all_changes_to_db());
        assert_eq!(db.pinned(&leaf.hash()), Some(true), "written again");

        // A new root above the pinned leaf: only the root is written, pinned.
        let mut second = storage.alloc(Some(storage.alloc(Some(leaf.clone()))));
        second.persist();
        queue(second.hash());
        flush();
        assert_eq!(db.pinned(&second.hash()), Some(true));
        let taken = take_queued();
        assert_eq!(
            (taken.roots, taken.pinned_on_write),
            (HashSet::new(), 1),
            "only the new root"
        );

        // A stored pinned node becoming a root, as an unchanged contract state persisted again, has
        // only its count written: it is pinned there, its row flagged, without the walk.
        let mut leaf_root = leaf.clone();
        leaf_root.persist();
        queue(leaf.hash());
        flush();
        assert!(!db.unpinned_roots().contains(&leaf.hash()), "row flagged");
        let taken = take_queued();
        assert_eq!(
            (
                taken.roots,
                taken.pinned_on_write,
                taken.roots_pinned_on_write
            ),
            (HashSet::new(), 0, 1),
            "pinned with its count"
        );

        // A new root above an older unpinned leaf is written unpinned and stays queued; the walk
        // pins both.
        let mut older = storage.alloc([u32::MAX - 1; SMALL_OBJECT_LIMIT / 4]);
        older.persist();
        flush();
        let third = rooted(&older);
        queue(third.hash());
        flush();
        assert_eq!(db.pinned(&third.hash()), Some(false));
        let Queue {
            roots,
            pinned_on_write,
            ..
        } = take_queued();
        assert_eq!(
            (&roots, pinned_on_write),
            (&HashSet::from([third.hash()]), 0)
        );
        assert_eq!(db.pin_reachable(roots).nodes, 2);
        assert_eq!(db.pinned(&older.hash()), Some(true));

        // A new root above a missing leaf stays queued, and the walk leaves it unpinned.
        let lost = storage.alloc([u32::MAX - 2; SMALL_OBJECT_LIMIT / 4]);
        let mut held = lost.clone();
        held.persist();
        flush();
        db.batch_update([(lost.hash(), DeleteNode)].into_iter());
        let fourth = rooted(&lost);
        queue(fourth.hash());
        flush();
        assert_eq!(db.pinned(&fourth.hash()), Some(false));
        let roots = take_queued().roots;
        let pinned = db.pin_reachable(roots);
        assert_eq!(pinned.nodes, 0);
        assert_eq!(pinned.failed_roots, vec![fourth.hash()]);

        // With the pinned leaf forgotten, as after a restart, a new root above it stays queued; the
        // walk pins only the root and adds the leaf to `PINNED`, so the next new root is pinned as
        // written. Deeper inlined `Option`s give each root its own node above the leaf.
        reset_pinned(1_000);
        let mut fifth = storage.alloc(Some(storage.alloc(Some(storage.alloc(Some(leaf.clone()))))));
        fifth.persist();
        queue(fifth.hash());
        flush();
        assert_eq!(db.pinned(&fifth.hash()), Some(false), "not in PINNED");
        let roots = take_queued().roots;
        assert_eq!(db.pin_reachable(roots).nodes, 1, "only the root");
        let mut sixth = storage.alloc(Some(
            storage.alloc(Some(storage.alloc(Some(storage.alloc(Some(leaf.clone())))))),
        ));
        sixth.persist();
        queue(sixth.hash());
        flush();
        assert_eq!(
            db.pinned(&sixth.hash()),
            Some(true),
            "added to PINNED by the walk"
        );
        assert_eq!(take_queued().roots, HashSet::new());

        // Two levels of new nodes above an older unpinned leaf are written unpinned, while the new
        // leaf beside it is written pinned; the walk then pins the two levels and the older leaf.
        let mut older = storage.alloc([u32::MAX - 10; SMALL_OBJECT_LIMIT / 4]);
        older.persist();
        flush();
        let fresh = storage.alloc([u32::MAX - 13; SMALL_OBJECT_LIMIT / 4]);
        // Persisted, so a row of its own rather than inlined in its parent.
        let mut middle = storage.alloc((older.clone(), fresh.clone()));
        middle.persist();
        let mut top = storage.alloc(Some(middle.clone()));
        top.persist();
        queue(top.hash());
        flush();
        assert_eq!(
            db.pinned(&middle.hash()),
            Some(false),
            "above the older leaf"
        );
        assert_eq!(db.pinned(&top.hash()), Some(false), "two levels above it");
        assert_eq!(db.pinned(&fresh.hash()), Some(true), "the new leaf");
        let roots = take_queued().roots;
        assert_eq!(roots, HashSet::from([top.hash()]));
        assert_eq!(
            db.pin_reachable(roots).nodes,
            3,
            "both levels and the older leaf"
        );

        // Two queued roots share a new node: it and the root above it alone are written pinned,
        // while the root also reaching an older unpinned leaf is not, and stays queued.
        let mut older = storage.alloc([u32::MAX - 11; SMALL_OBJECT_LIMIT / 4]);
        older.persist();
        flush();
        let shared = storage.alloc([u32::MAX - 12; SMALL_OBJECT_LIMIT / 4]);
        let mut alone = storage.alloc(Some(shared.clone()));
        alone.persist();
        let mut mixed = storage.alloc((shared.clone(), older.clone()));
        mixed.persist();
        queue(alone.hash());
        queue(mixed.hash());
        flush();
        assert_eq!(db.pinned(&shared.hash()), Some(true), "the shared node");
        assert_eq!(
            db.pinned(&alone.hash()),
            Some(true),
            "the root above it alone"
        );
        assert_eq!(
            db.pinned(&mixed.hash()),
            Some(false),
            "the root reaching the older leaf"
        );
        assert_eq!(take_queued().roots, HashSet::from([mixed.hash()]));
    }

    // Batches built by hand, some of shapes storage-core 1.2.3 does not send: without deletes, a
    // root's written nodes are pinned and its row flagged, while a root written without a count, or
    // with its count back to zero, stays queued; a flush that deletes pins nothing, and a pinned
    // node it deletes leaves `PINNED`; diamonds above an older unpinned node leave every root above
    // it queued; and the single-node delete takes its key out of `PINNED`. Shared by both suites.
    fn assert_pinning_on_write_batches(db: &mut LedgerDb) {
        use midnight_serialize_v1::Deserializable;
        use midnight_storage_core_v1::{
            DefaultHasher,
            arena::ArenaHash,
            backend::OnDiskObject,
            db::Update::{InsertNode, SetRootCount},
        };

        let key = |byte: u8| {
            ArenaHash::<DefaultHasher>::deserialize(&mut [byte; 32].as_slice(), 0)
                .expect("32 bytes are an ArenaHash")
        };
        // A node of one data byte referencing `children`, in `OnDiskObject`'s encoding: compact
        // lengths, and each reference a zero discriminant then its hash.
        let node = |children: &[u8]| {
            let mut bytes = vec![0x04, 0x00, (children.len() as u8) << 2];
            for child in children {
                bytes.push(0);
                bytes.extend([*child; 32]);
            }
            OnDiskObject::<DefaultHasher>::deserialize(&mut bytes.as_slice(), 0)
                .expect("an OnDiskObject")
        };
        let in_pinned = |byte| {
            PINNED
                .lock()
                .expect("lock pinned nodes")
                .contains(&key(byte))
        };
        take_queued();
        reset_pinned(1_000);

        // Without deletes, written nodes are pinned and a counted root's row flagged; a root
        // without a count, or with its count back to zero, stays queued.
        for byte in [2, 6, 7, 8] {
            queue(key(byte));
        }
        db.batch_update(
            [
                (key(1), InsertNode(node(&[]))),
                (key(2), InsertNode(node(&[1]))),
                (key(2), SetRootCount(1)),
                (key(6), InsertNode(node(&[]))),
                (key(7), InsertNode(node(&[]))),
                (key(7), SetRootCount(1)),
                (key(7), SetRootCount(0)),
                (key(8), InsertNode(node(&[]))),
                (key(8), SetRootCount(1)),
            ]
            .into_iter(),
        );
        assert!(
            [1, 2, 6, 7, 8]
                .iter()
                .all(|byte| db.pinned(&key(*byte)) == Some(true))
        );
        assert!(
            !db.unpinned_roots().contains(&key(2)),
            "counted root flagged"
        );
        let taken = take_queued();
        assert_eq!(
            (taken.roots, taken.pinned_on_write),
            (HashSet::from([key(6), key(7)]), 5)
        );

        // A flush that deletes pins nothing, before the delete or after it, and flags no root row,
        // even a pinned node's, leaving the roots queued; a pinned node it deletes leaves `PINNED`.
        for byte in [1, 3, 17] {
            queue(key(byte));
        }
        db.batch_update(
            [
                (key(3), InsertNode(node(&[1]))),
                (key(3), SetRootCount(1)),
                (key(1), SetRootCount(1)),
                (key(8), DeleteNode),
                (key(17), InsertNode(node(&[]))),
                (key(17), SetRootCount(1)),
            ]
            .into_iter(),
        );
        assert_eq!(
            db.pinned(&key(3)),
            Some(false),
            "inserted before the delete"
        );
        assert_eq!(
            db.pinned(&key(17)),
            Some(false),
            "inserted after the delete"
        );
        assert!(
            db.unpinned_roots().contains(&key(1)),
            "pinned node's row not flagged"
        );
        assert!(!in_pinned(8), "deleted");
        let taken = take_queued();
        assert_eq!(
            (taken.roots, taken.pinned_on_write),
            (HashSet::from([key(1), key(3), key(17)]), 0)
        );

        // Diamonds above an older unpinned node, 10: a new node above it shared by two queued
        // roots leaves both unpinned, as does a root above two new nodes both above it.
        db.batch_update([(key(10), InsertNode(node(&[])))].into_iter());
        for byte in [12, 13, 16] {
            queue(key(byte));
        }
        db.batch_update(
            [
                (key(11), InsertNode(node(&[10]))),
                (key(12), InsertNode(node(&[11]))),
                (key(12), SetRootCount(1)),
                (key(13), InsertNode(node(&[11]))),
                (key(13), SetRootCount(1)),
                (key(14), InsertNode(node(&[10]))),
                (key(15), InsertNode(node(&[10]))),
                (key(16), InsertNode(node(&[14, 15]))),
                (key(16), SetRootCount(1)),
            ]
            .into_iter(),
        );
        assert!(
            [11, 12, 13, 14, 15, 16]
                .iter()
                .all(|byte| db.pinned(&key(*byte)) == Some(false))
        );
        assert_eq!(
            take_queued().roots,
            HashSet::from([key(12), key(13), key(16)])
        );

        // The single-node delete takes its key out of `PINNED`.
        assert!(in_pinned(1));
        db.delete_node(&key(1));
        assert!(!in_pinned(1));
    }

    // `PINNED`'s nodes are kept in two generations of half the limit, rounded up: a full generation
    // is replaced, forgetting the older one's keys unless found since. A limit of zero is
    // unbounded, as storage-core's cache, and none is kept before a limit is set.
    #[test]
    fn pinned_keeps_recent_keys() {
        use super::PinnedNodes;
        use midnight_serialize_v1::Deserializable;

        let key = |byte| {
            midnight_storage_core_v1::arena::ArenaHash::deserialize(&mut [byte; 32].as_slice(), 0)
                .expect("32 bytes are an ArenaHash")
        };
        let mut pinned = PinnedNodes::default();
        pinned.set_max(4);
        pinned.insert(key(1));
        pinned.insert(key(2));
        pinned.insert(key(3));
        assert!(
            pinned.contains(&key(1)),
            "found in the previous generation, so kept"
        );
        pinned.insert(key(4));
        assert!(!pinned.contains(&key(2)), "never found since, so forgotten");
        assert!(pinned.contains(&key(1)) && pinned.contains(&key(3)) && pinned.contains(&key(4)));

        let mut again = PinnedNodes::default();
        again.set_max(4);
        again.insert(key(1));
        again.insert(key(2));
        again.insert(key(2));
        again.insert(key(3));
        again.insert(key(4));
        assert!(again.contains(&key(1)), "a key held again does not rotate");

        let mut one = PinnedNodes::default();
        one.set_max(1);
        one.insert(key(1));
        assert!(one.contains(&key(1)), "a limit of one keeps one");

        let mut unbounded = PinnedNodes::default();
        unbounded.set_max(0);
        (1..=100).for_each(|byte| unbounded.insert(key(byte)));
        assert!(
            (1..=100).all(|byte| unbounded.contains(&key(byte))),
            "zero is unbounded"
        );

        let mut unset = PinnedNodes::default();
        unset.insert(key(1));
        assert!(!unset.contains(&key(1)), "none before a limit is set");
    }

    #[cfg(feature = "standalone")]
    mod sqlite {
        use super::{
            assert_bounded_gc_keeps_nodes_pinned_between_passes, assert_gc_keeps_pinned_nodes,
            assert_pinning_crosses_batches, assert_pinning_on_write,
            assert_pinning_on_write_batches,
        };
        use crate::infra::ledger_db::v1_1::{LedgerDb, sqlite_tests::setup_pool};
        use midnight_storage_core_v1::Storage;
        use std::error::Error as StdError;

        #[tokio::test(flavor = "multi_thread")]
        async fn gc_keeps_pinned_nodes() -> Result<(), Box<dyn StdError>> {
            let pool = setup_pool().await?;
            let storage = Storage::new(16, LedgerDb::new(pool.clone()));
            let mut db = LedgerDb::new(pool);
            assert_gc_keeps_pinned_nodes(&storage, &mut db);
            Ok(())
        }

        // See `assert_pinning_crosses_batches`.
        #[tokio::test(flavor = "multi_thread")]
        async fn pinning_crosses_batches() -> Result<(), Box<dyn StdError>> {
            let pool = setup_pool().await?;
            let storage = Storage::new(16, LedgerDb::new(pool.clone()));
            let mut db = LedgerDb::new(pool);
            assert_pinning_crosses_batches(&storage, &mut db);
            Ok(())
        }

        // See `assert_bounded_gc_keeps_nodes_pinned_between_passes`.
        #[tokio::test(flavor = "multi_thread")]
        async fn bounded_gc_keeps_nodes_pinned_between_passes() -> Result<(), Box<dyn StdError>> {
            let pool = setup_pool().await?;
            let storage = Storage::new(16, LedgerDb::new(pool.clone()));
            let gc = Storage::new(16, LedgerDb::new(pool.clone()));
            let mut db = LedgerDb::new(pool);
            assert_bounded_gc_keeps_nodes_pinned_between_passes(&storage, &gc, &mut db);
            Ok(())
        }

        // See `assert_pinning_on_write`.
        #[tokio::test(flavor = "multi_thread")]
        async fn pinning_on_write() -> Result<(), Box<dyn StdError>> {
            let pool = setup_pool().await?;
            let storage = Storage::new(16, LedgerDb::new(pool.clone()));
            let other = Storage::new(16, LedgerDb::new(pool.clone()));
            let mut db = LedgerDb::new(pool);
            assert_pinning_on_write(&storage, &other, &mut db);
            Ok(())
        }

        // See `assert_pinning_on_write_batches`.
        #[tokio::test(flavor = "multi_thread")]
        async fn pinning_on_write_batches() -> Result<(), Box<dyn StdError>> {
            let pool = setup_pool().await?;
            let mut db = LedgerDb::new(pool);
            assert_pinning_on_write_batches(&mut db);
            Ok(())
        }
    }

    #[cfg(feature = "cloud")]
    mod postgres {
        use super::{
            assert_bounded_gc_keeps_nodes_pinned_between_passes, assert_gc_keeps_pinned_nodes,
            assert_pinning_crosses_batches, assert_pinning_on_write,
            assert_pinning_on_write_batches,
        };
        use crate::infra::ledger_db::v1_1::{LedgerDb, postgres_tests::setup_pool};
        use midnight_storage_core_v1::Storage;
        use std::error::Error as StdError;

        #[tokio::test(flavor = "multi_thread")]
        async fn gc_keeps_pinned_nodes() -> Result<(), Box<dyn StdError>> {
            let (_container, pool) = setup_pool().await?;
            let storage = Storage::new(16, LedgerDb::new(pool.clone()));
            let mut db = LedgerDb::new(pool);
            assert_gc_keeps_pinned_nodes(&storage, &mut db);
            Ok(())
        }

        // See `assert_pinning_crosses_batches`.
        #[tokio::test(flavor = "multi_thread")]
        async fn pinning_crosses_batches() -> Result<(), Box<dyn StdError>> {
            let (_container, pool) = setup_pool().await?;
            let storage = Storage::new(16, LedgerDb::new(pool.clone()));
            let mut db = LedgerDb::new(pool);
            assert_pinning_crosses_batches(&storage, &mut db);
            Ok(())
        }

        // See `assert_bounded_gc_keeps_nodes_pinned_between_passes`.
        #[tokio::test(flavor = "multi_thread")]
        async fn bounded_gc_keeps_nodes_pinned_between_passes() -> Result<(), Box<dyn StdError>> {
            let (_container, pool) = setup_pool().await?;
            let storage = Storage::new(16, LedgerDb::new(pool.clone()));
            let gc = Storage::new(16, LedgerDb::new(pool.clone()));
            let mut db = LedgerDb::new(pool);
            assert_bounded_gc_keeps_nodes_pinned_between_passes(&storage, &gc, &mut db);
            Ok(())
        }

        // See `assert_pinning_on_write`.
        #[tokio::test(flavor = "multi_thread")]
        async fn pinning_on_write() -> Result<(), Box<dyn StdError>> {
            let (_container, pool) = setup_pool().await?;
            let storage = Storage::new(16, LedgerDb::new(pool.clone()));
            let other = Storage::new(16, LedgerDb::new(pool.clone()));
            let mut db = LedgerDb::new(pool);
            assert_pinning_on_write(&storage, &other, &mut db);
            Ok(())
        }

        // See `assert_pinning_on_write_batches`.
        #[tokio::test(flavor = "multi_thread")]
        async fn pinning_on_write_batches() -> Result<(), Box<dyn StdError>> {
            let (_container, pool) = setup_pool().await?;
            let mut db = LedgerDb::new(pool);
            assert_pinning_on_write_batches(&mut db);
            Ok(())
        }
    }
}
