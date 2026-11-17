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

pub(crate) mod pinning;

use crate::{
    error::StdErrorExt,
    infra::ledger_db::object_parts::{self, Parts},
};
use derive_more::Debug;
use futures::TryStreamExt;
use indoc::indoc;
use midnight_serialize_v1::{Deserializable, Serializable};
use midnight_storage_core_v1::{
    DefaultHasher, WellBehavedHasher,
    arena::ArenaHash,
    backend::OnDiskObject,
    db::{
        DB, DummyArbitrary,
        Update::{self, DeleteNode, InsertNode, SetRootCount},
    },
};
#[cfg(test)]
use midnight_storage_core_v1::{Storage, storable::SMALL_OBJECT_LIMIT};
use sqlx::QueryBuilder;
use std::{
    collections::{HashMap, HashSet},
    future::ready,
};
use tokio::{runtime::Handle, task::block_in_place};

// Serialized node bytes per Postgres insert. Far below Postgres' 1 GB limit on one parameter value,
// it also bounds the memory a statement takes in the indexer and in Postgres.
#[cfg(feature = "cloud")]
const BATCH_BYTES: usize = 64 * 1024 * 1024;
// Keys per SQLite `DELETE … IN (…)`, one bind each.
#[cfg(feature = "standalone")]
const BATCH_DELETE_SIZE: usize = SQLITE_MAX_BINDS;
// Rows per SQLite `INSERT … VALUES`, four binds each.
#[cfg(feature = "standalone")]
const BATCH_INSERT_SIZE: usize = SQLITE_MAX_BINDS / 4;
// Rows per Postgres statement taking its rows as array parameters.
#[cfg(feature = "cloud")]
const BATCH_ROWS: usize = 10_000;
// SQLite's maximum number of binds per statement.
#[cfg(feature = "standalone")]
const SQLITE_MAX_BINDS: usize = 32_766;

// A row read for a node's children: key, pinned, `child_keys` and, only without them, object.
type ChildrenRow = (Vec<u8>, bool, Option<Vec<u8>>, Option<Vec<u8>>);
// A stored node: `object`, `child_keys` and `child_values`; see [node_object].
type NodeRow = (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>);
// A stored node with its key in front.
type NodeKeyRow = (Vec<u8>, Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>);

#[cfg(feature = "cloud")]
type SqlxTransaction = sqlx::Transaction<'static, sqlx::Postgres>;

#[cfg(feature = "standalone")]
type SqlxTransaction = sqlx::Transaction<'static, sqlx::Sqlite>;

#[derive(Debug)]
pub struct LedgerDb {
    #[cfg(feature = "cloud")]
    pool: crate::infra::pool::postgres::PostgresPool,

    #[cfg(feature = "standalone")]
    pool: crate::infra::pool::sqlite::SqlitePool,
}

impl LedgerDb {
    #[cfg(feature = "cloud")]
    pub fn new(pool: crate::infra::pool::postgres::PostgresPool) -> Self {
        Self { pool }
    }

    #[cfg(feature = "standalone")]
    pub fn new(pool: crate::infra::pool::sqlite::SqlitePool) -> Self {
        Self { pool }
    }

    /// The number of gc roots, counted in the database rather than read back as [DB::get_roots]
    /// does.
    ///
    /// Only root counts flushed to the database are seen: unlike storage-core's backend
    /// `get_roots`, this does not apply changes still pending in its write cache, e.g. an unpersist
    /// since the last flush.
    pub fn root_count(&self) -> usize {
        block_in_place(|| {
            Handle::current().block_on(async {
                let count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ledger_db_roots")
                    .fetch_one(&*self.pool)
                    .await
                    .unwrap_or_panic("cannot count roots");

                count as usize
            })
        })
    }
}

impl DB for LedgerDb {
    type Hasher = DefaultHasher;
    type ScanResumeHandle = ArenaHash<DefaultHasher>;

    fn get_node(&self, key: &ArenaHash<Self::Hasher>) -> Option<OnDiskObject<Self::Hasher>> {
        block_in_place(|| {
            Handle::current().block_on(async {
                let query = indoc! {"
                    SELECT object, child_keys, child_values
                    FROM ledger_db_nodes
                    WHERE key = $1
                "};

                sqlx::query_as::<_, NodeRow>(query)
                    .bind(key.0.as_slice())
                    .fetch_optional(&*self.pool)
                    .await
                    .unwrap_or_panic("cannot get node")
                    .map(node_object)
                    .transpose()
                    .unwrap_or_panic("cannot deserialize node as OnDiskObject")
            })
        })
    }

    fn insert_node(&mut self, key: ArenaHash<Self::Hasher>, object: OnDiskObject<Self::Hasher>) {
        block_in_place(|| {
            Handle::current().block_on(async {
                let mut tx = self
                    .pool
                    .begin()
                    .await
                    .unwrap_or_panic("begin transaction for insert node");

                let parts = node_parts(&object);

                let query = indoc! {"
                    INSERT INTO ledger_db_nodes (
                        key,
                        object,
                        child_keys,
                        child_values
                    )
                    VALUES (
                        $1,
                        $2,
                        $3,
                        $4
                    )
                    ON CONFLICT (key) DO UPDATE
                    SET object = EXCLUDED.object,
                        child_keys = EXCLUDED.child_keys,
                        child_values = EXCLUDED.child_values
                "};

                sqlx::query(query)
                    .bind(key.0.as_slice())
                    .bind(parts.data)
                    .bind(parts.child_keys)
                    .bind(parts.child_values)
                    .execute(&mut *tx)
                    .await
                    .unwrap_or_panic("cannot insert node");

                tx.commit()
                    .await
                    .unwrap_or_panic("commit transaction for insert node");
            })
        })
    }

    fn delete_node(&mut self, key: &ArenaHash<Self::Hasher>) {
        block_in_place(|| {
            Handle::current().block_on(async {
                let mut tx = self
                    .pool
                    .begin()
                    .await
                    .unwrap_or_panic("begin transaction for delete node");

                delete_node(&mut tx, key).await;

                tx.commit()
                    .await
                    .unwrap_or_panic("commit transaction for delete node");
                pinning::remove_pinned([key]);
            })
        })
    }

    fn batch_update<I>(&mut self, updates: I)
    where
        I: Iterator<Item = (ArenaHash<Self::Hasher>, Update<Self::Hasher>)>,
    {
        block_in_place(|| {
            Handle::current().block_on(async {
                let mut tx = self
                    .pool
                    .begin()
                    .await
                    .unwrap_or_panic("begin transaction for batch update");

                // Consecutive inserts and consecutive deletes are each sent in batches. Flushing one
                // buffer before the other grows keeps the updates in their original order.
                let mut inserts = Vec::new();
                let mut deletes = Vec::new();
                let mut root_counts = Vec::new();
                // The pins of this flush, which leave the queue once it is committed.
                let mut pinning = pinning::Pinning::default();

                for (key, update) in updates {
                    match update {
                        InsertNode(object) => {
                            flush_deletes(&mut tx, &mut deletes).await;
                            inserts.push((key, object));
                        }
                        // A flush that deletes pins nothing as written.
                        DeleteNode => {
                            pinning.evict(&key);
                            flush_inserts(&mut tx, &mut inserts, &HashSet::new()).await;
                            deletes.push(key);
                        }
                        // Root counts live in `ledger_db_roots`, which no node insert or delete
                        // touches, so they are written together after the nodes.
                        SetRootCount(count) => root_counts.push((key, count)),
                    }
                }

                let pinned = pinning.nodes(&inserts);
                flush_inserts(&mut tx, &mut inserts, pinned).await;
                flush_deletes(&mut tx, &mut deletes).await;
                let pinned = pinning.roots(&root_counts);
                flush_root_counts(&mut tx, root_counts, pinned).await;

                tx.commit()
                    .await
                    .unwrap_or_panic("commit transaction for batch update");
                pinning.commit();
            })
        })
    }

    fn batch_get_nodes<I>(
        &self,
        keys: I,
    ) -> Vec<(ArenaHash<Self::Hasher>, Option<OnDiskObject<Self::Hasher>>)>
    where
        I: Iterator<Item = ArenaHash<Self::Hasher>>,
    {
        block_in_place(|| {
            Handle::current().block_on(async {
                let keys = keys.collect::<Vec<_>>();

                if keys.is_empty() {
                    return vec![];
                }

                #[cfg(feature = "cloud")]
                {
                    let query = indoc! {"
                        SELECT key, object, child_keys, child_values
                        FROM ledger_db_nodes
                        WHERE key = ANY($1::bytea[])
                    "};

                    let mut nodes = sqlx::query_as::<_, NodeKeyRow>(query)
                        .bind(keys.iter().map(|key| key.0.as_slice()).collect::<Vec<_>>())
                        .fetch(&*self.pool)
                        .and_then(|(key, object, child_keys, child_values)| {
                            let key_and_object =
                                ArenaHash::<Self::Hasher>::deserialize(&mut key.as_slice(), 0)
                                    .map_err(|error| sqlx::Error::Decode(error.into()))
                                    .and_then(|key| {
                                        let object =
                                            node_object((object, child_keys, child_values))
                                                .map_err(|error| sqlx::Error::Decode(error.into()));
                                        object.map(|object| (key, object))
                                    });
                            ready(key_and_object)
                        })
                        .try_collect::<HashMap<_, _>>()
                        .await
                        .unwrap_or_panic("cannot batch get nodes");

                    keys.into_iter()
                        .map(|key| {
                            let node = nodes.remove(&key);
                            (key, node)
                        })
                        .collect()
                }

                #[cfg(feature = "standalone")]
                {
                    use sqlx::QueryBuilder;

                    let query = indoc! {"
                        SELECT key, object, child_keys, child_values
                        FROM ledger_db_nodes
                        WHERE key IN (
                    "};

                    let mut query = QueryBuilder::new(query);
                    let mut bindings = query.separated(", ");
                    for key in &keys {
                        bindings.push_bind(key.0.as_slice());
                    }
                    query.push(")");

                    let mut nodes = query
                        .build_query_as::<NodeKeyRow>()
                        .fetch(&*self.pool)
                        .and_then(|(key, object, child_keys, child_values)| {
                            let key_and_object =
                                ArenaHash::<Self::Hasher>::deserialize(&mut key.as_slice(), 0)
                                    .map_err(|error| sqlx::Error::Decode(error.into()))
                                    .and_then(|key| {
                                        let object =
                                            node_object((object, child_keys, child_values))
                                                .map_err(|error| sqlx::Error::Decode(error.into()));
                                        object.map(|object| (key, object))
                                    });
                            ready(key_and_object)
                        })
                        .try_collect::<HashMap<_, _>>()
                        .await
                        .unwrap_or_panic("cannot batch get nodes");

                    keys.into_iter()
                        .map(|key| {
                            let node = nodes.remove(&key);
                            (key, node)
                        })
                        .collect()
                }
            })
        })
    }

    fn get_root_count(&self, key: &ArenaHash<Self::Hasher>) -> u32 {
        block_in_place(|| {
            Handle::current().block_on(async {
                // Read the stored `count` column, not `count(1)`: the row-count aggregate
                // is 0/1 (key is the primary key) and under-reports true counts >= 2, which
                // storage-core's flush then drives negative. A missing row means count 0.
                let query = indoc! {"
                    SELECT count
                    FROM ledger_db_roots
                    WHERE key = $1
                "};

                sqlx::query_as::<_, (i64,)>(query)
                    .bind(key.0.as_slice())
                    .fetch_optional(&*self.pool)
                    .await
                    .unwrap_or_panic("cannot get root count")
                    .map_or(0, |(count,)| count as u32)
            })
        })
    }

    /// The root counts of `keys` in one query, or one per chunk of binds on SQLite, rather than one
    /// per key. Every key is in the result, with 0 for a key that is not a root.
    fn batch_get_root_counts<I>(&self, keys: I) -> HashMap<ArenaHash<Self::Hasher>, u32>
    where
        I: Iterator<Item = ArenaHash<Self::Hasher>>,
    {
        let keys = keys.collect::<Vec<_>>();
        if keys.is_empty() {
            return HashMap::new();
        }

        block_in_place(|| {
            Handle::current().block_on(async {
                #[cfg(feature = "standalone")]
                let rows = {
                    let mut rows = vec![];
                    for chunk in keys.chunks(BATCH_DELETE_SIZE) {
                        let mut query = QueryBuilder::new(indoc! {"
                            SELECT key, count
                            FROM ledger_db_roots
                            WHERE key IN (
                        "});
                        let mut bindings = query.separated(", ");
                        for key in chunk {
                            bindings.push_bind(key.0.as_slice());
                        }
                        query.push(")");
                        rows.extend(
                            query
                                .build_query_as::<(Vec<u8>, i64)>()
                                .fetch_all(&*self.pool)
                                .await
                                .unwrap_or_panic("cannot batch get root counts"),
                        );
                    }
                    rows
                };

                #[cfg(feature = "cloud")]
                let rows = {
                    let query = indoc! {"
                        SELECT key, count
                        FROM ledger_db_roots
                        WHERE key = ANY($1::bytea[])
                    "};

                    sqlx::query_as::<_, (Vec<u8>, i64)>(query)
                        .bind(keys.iter().map(|key| key.0.as_slice()).collect::<Vec<_>>())
                        .fetch_all(&*self.pool)
                        .await
                        .unwrap_or_panic("cannot batch get root counts")
                };

                let counts = rows.into_iter().collect::<HashMap<_, _>>();
                keys.into_iter()
                    .map(|key| {
                        let count = counts
                            .get(key.0.as_slice())
                            .map_or(0, |count| *count as u32);
                        (key, count)
                    })
                    .collect()
            })
        })
    }

    fn set_root_count(&mut self, key: ArenaHash<Self::Hasher>, count: u32) {
        block_in_place(|| {
            Handle::current().block_on(async {
                let mut tx = self
                    .pool
                    .begin()
                    .await
                    .unwrap_or_panic("begin transaction for set root count");

                set_root_count(&mut tx, key, count).await;

                tx.commit()
                    .await
                    .unwrap_or_panic("commit transaction for set root count");
            })
        })
    }

    /// Reads every row of `ledger_db_roots` and decodes its key, so its cost grows with the number
    /// of roots. storage-core's gc calls it once per cycle. To count the roots, use
    /// [LedgerDb::root_count] instead.
    fn get_roots(&self) -> HashMap<ArenaHash<Self::Hasher>, u32> {
        block_in_place(|| {
            Handle::current().block_on(async {
                let query = indoc! {"
                    SELECT key, count
                    FROM ledger_db_roots
                "};

                sqlx::query_as::<_, (Vec<u8>, i64)>(query)
                    .fetch(&*self.pool)
                    .and_then(|(key, count)| {
                        let key = ArenaHash::<Self::Hasher>::deserialize(&mut key.as_slice(), 0)
                            .map_err(|error| sqlx::Error::Decode(error.into()));
                        ready(key.map(|key| (key, count as u32)))
                    })
                    .try_collect::<HashMap<_, _>>()
                    .await
                    .unwrap_or_panic("cannot get roots")
            })
        })
    }

    fn size(&self) -> usize {
        block_in_place(|| {
            Handle::current().block_on(async {
                let query = indoc! {"
                    SELECT count(1)
                    FROM ledger_db_nodes
                "};

                let (count,) = sqlx::query_as::<_, (i64,)>(query)
                    .fetch_one(&*self.pool)
                    .await
                    .unwrap_or_panic("cannot get size");

                count as usize
            })
        })
    }

    /// Called by storage-core's gc-v1 sweep phase (`gc.rs`): after marking
    /// reachable nodes, `gc()` scans the DB in batches, culls every scanned
    /// node not in the mark set, and uses the returned resume handle to pick
    /// up where a time-bounded pass left off. Not called by indexer code.
    fn scan(
        &self,
        resume_from: Option<Self::ScanResumeHandle>,
        batch_size: usize,
    ) -> (
        Vec<(ArenaHash<Self::Hasher>, OnDiskObject<Self::Hasher>)>,
        Option<Self::ScanResumeHandle>,
    ) {
        block_in_place(|| {
            Handle::current().block_on(async {
                let mut query = QueryBuilder::new(
                    "SELECT key, object, child_keys, child_values FROM ledger_db_nodes",
                );
                if let Some(handle) = resume_from.as_ref() {
                    query.push(" WHERE key > ");
                    query.push_bind(handle.0.as_slice());
                }
                query.push(" ORDER BY key LIMIT ");
                query.push_bind(batch_size as i64);

                let raw = query
                    .build_query_as::<NodeKeyRow>()
                    .fetch_all(&*self.pool)
                    .await
                    .unwrap_or_panic("cannot scan ledger_db_nodes");

                let batch = raw
                    .into_iter()
                    .map(|(key, object, child_keys, child_values)| {
                        let key = ArenaHash::<Self::Hasher>::deserialize(&mut key.as_slice(), 0)
                            .unwrap_or_panic("cannot deserialize key as ArenaHash");
                        let object = node_object((object, child_keys, child_values))
                            .unwrap_or_panic("cannot deserialize node as OnDiskObject");
                        (key, object)
                    })
                    .collect::<Vec<_>>();

                let next_handle = batch.last().map(|(k, _)| k.clone());

                (batch, next_handle)
            })
        })
    }

    /// The keys of the next `batch_size` unpinned nodes after `resume_from`, in key order, from
    /// the partial index alone, without reading any object. Pinned nodes are reachable from roots
    /// that are never unpersisted, and storage-core lets a scan leave out such nodes.
    fn scan_keys(
        &self,
        resume_from: Option<Self::ScanResumeHandle>,
        batch_size: usize,
    ) -> (Vec<ArenaHash<Self::Hasher>>, Option<Self::ScanResumeHandle>) {
        block_in_place(|| {
            Handle::current().block_on(async {
                let mut query =
                    QueryBuilder::new("SELECT key FROM ledger_db_nodes WHERE NOT pinned");
                if let Some(handle) = resume_from.as_ref() {
                    query.push(" AND key > ");
                    query.push_bind(handle.0.as_slice());
                }
                query.push(" ORDER BY key LIMIT ");
                query.push_bind(batch_size as i64);

                let keys = query
                    .build_query_as::<(Vec<u8>,)>()
                    .fetch_all(&*self.pool)
                    .await
                    .unwrap_or_panic("cannot scan ledger_db_nodes keys")
                    .into_iter()
                    .map(|(key,)| {
                        ArenaHash::<Self::Hasher>::deserialize(&mut key.as_slice(), 0)
                            .unwrap_or_panic("cannot deserialize key as ArenaHash")
                    })
                    .collect::<Vec<_>>();
                let next_handle = keys.last().cloned();

                (keys, next_handle)
            })
        })
    }

    /// The children of each stored key among `keys`; see `LedgerDb::children_rows`. Keys that are
    /// not stored are left out.
    ///
    /// Pinned nodes report none: they are left out of [DB::scan_keys], as are all their
    /// descendants, so gc's mark need not reach below them.
    fn batch_get_children<I>(
        &self,
        keys: I,
    ) -> Vec<(ArenaHash<Self::Hasher>, Vec<ArenaHash<Self::Hasher>>)>
    where
        I: Iterator<Item = ArenaHash<Self::Hasher>>,
    {
        self.children_rows(&keys.collect::<Vec<_>>())
            .into_iter()
            .map(|(key, _, children)| (key, children))
            .collect()
    }
}

impl LedgerDb {
    /// `(key, pinned, children)` of the stored ones among `keys`: an unpinned node's by-reference
    /// children, from `child_keys`, or from its object for a row written before that column
    /// existed, and none for a pinned node.
    fn children_rows(
        &self,
        keys: &[ArenaHash<DefaultHasher>],
    ) -> Vec<(
        ArenaHash<DefaultHasher>,
        bool,
        Vec<ArenaHash<DefaultHasher>>,
    )> {
        if keys.is_empty() {
            return vec![];
        }

        block_in_place(|| {
            Handle::current().block_on(async {
                #[cfg(feature = "standalone")]
                let rows = {
                    let mut rows = vec![];
                    for chunk in keys.chunks(BATCH_DELETE_SIZE) {
                        let mut query = QueryBuilder::new(indoc! {"
                            SELECT
                                key,
                                pinned,
                                CASE WHEN NOT pinned THEN child_keys END,
                                CASE WHEN NOT pinned AND child_keys IS NULL THEN object END
                            FROM ledger_db_nodes
                            WHERE key IN (
                        "});
                        let mut bindings = query.separated(", ");
                        for key in chunk {
                            bindings.push_bind(key.0.as_slice());
                        }
                        query.push(")");
                        rows.extend(
                            query
                                .build_query_as::<ChildrenRow>()
                                .fetch_all(&*self.pool)
                                .await
                                .unwrap_or_panic("cannot batch get node children"),
                        );
                    }
                    rows
                };

                #[cfg(feature = "cloud")]
                let rows = {
                    let query = indoc! {"
                        SELECT
                            key,
                            pinned,
                            CASE WHEN NOT pinned THEN child_keys END,
                            CASE WHEN NOT pinned AND child_keys IS NULL THEN object END
                        FROM ledger_db_nodes
                        WHERE key = ANY($1::bytea[])
                    "};

                    sqlx::query_as::<_, ChildrenRow>(query)
                        .bind(keys.iter().map(|key| key.0.as_slice()).collect::<Vec<_>>())
                        .fetch_all(&*self.pool)
                        .await
                        .unwrap_or_panic("cannot batch get node children")
                };

                rows.into_iter()
                    .map(|(key, pinned, children, object)| {
                        let key = ArenaHash::<DefaultHasher>::deserialize(&mut key.as_slice(), 0)
                            .unwrap_or_panic("cannot deserialize key as ArenaHash");
                        (key, pinned, row_children(children, object))
                    })
                    .collect()
            })
        })
    }
}

impl Default for LedgerDb {
    fn default() -> Self {
        panic!("LedgerDb cannot be constructed by default");
    }
}

impl DummyArbitrary for LedgerDb {}

trait ResultExt<T> {
    fn unwrap_or_panic(self, msg: &'static str) -> T;
}

impl<T, E> ResultExt<T> for Result<T, E>
where
    E: std::error::Error,
{
    fn unwrap_or_panic(self, msg: &'static str) -> T {
        self.unwrap_or_else(|error| panic!("{msg}: {}", error.as_chain()))
    }
}

// SQLite has no array parameters, so nodes are bound in chunks. Nodes in `pinned` are inserted
// pinned, and a stored node stays pinned.
#[cfg(feature = "standalone")]
async fn flush_inserts<Hasher: WellBehavedHasher>(
    tx: &mut SqlxTransaction,
    inserts: &mut Vec<(ArenaHash<Hasher>, OnDiskObject<Hasher>)>,
    pinned: &HashSet<ArenaHash<Hasher>>,
) {
    for chunk in inserts.chunks(BATCH_INSERT_SIZE) {
        QueryBuilder::new(
            "INSERT INTO ledger_db_nodes (key, object, child_keys, child_values, pinned) ",
        )
        .push_values(chunk, |mut q, (key, object)| {
            let parts = node_parts(object);
            q.push_bind(key.0.as_slice())
                .push_bind(parts.data)
                .push_bind(parts.child_keys)
                .push_bind(parts.child_values)
                .push(if pinned.contains(key) {
                    "TRUE"
                } else {
                    "FALSE"
                });
        })
        .push(indoc! {"
             ON CONFLICT (key) DO UPDATE
            SET object = EXCLUDED.object,
                child_keys = EXCLUDED.child_keys,
                child_values = EXCLUDED.child_values,
                pinned = ledger_db_nodes.pinned OR EXCLUDED.pinned
        "})
        .build()
        .execute(&mut **tx)
        .await
        .unwrap_or_panic("cannot batch insert nodes");
    }

    inserts.clear();
}

// Postgres takes the keys, the nodes' parts and their pinned flags as five array parameters, so
// every statement has the same SQL whatever its number of nodes. Nodes in `pinned` are inserted
// pinned, and a stored node stays pinned.
#[cfg(feature = "cloud")]
async fn flush_inserts<Hasher: WellBehavedHasher>(
    tx: &mut SqlxTransaction,
    inserts: &mut Vec<(ArenaHash<Hasher>, OnDiskObject<Hasher>)>,
    pinned: &HashSet<ArenaHash<Hasher>>,
) {
    let query = indoc! {"
        INSERT INTO ledger_db_nodes (key, object, child_keys, child_values, pinned)
        SELECT * FROM UNNEST($1::bytea[], $2::bytea[], $3::bytea[], $4::bytea[], $5::boolean[])
        ON CONFLICT (key) DO UPDATE
        SET object = EXCLUDED.object,
            child_keys = EXCLUDED.child_keys,
            child_values = EXCLUDED.child_values,
            pinned = ledger_db_nodes.pinned OR EXCLUDED.pinned
    "};

    for chunk in insert_chunks(inserts, |(_, object)| object.serialized_size()) {
        // Keeps one row per key, as an upsert fails when two of its rows share a key. Nodes are
        // content-addressed and, with storage-core's `layout-v2`, carry no reference count, so rows
        // sharing a key are identical.
        let mut unique = HashSet::with_capacity(chunk.len());
        let (keys, (objects, (child_keys, (child_values, pinned)))) = chunk
            .iter()
            .filter(|(key, _)| unique.insert(key.0.as_slice()))
            .map(|(key, object)| {
                let parts = node_parts(object);
                (
                    key.0.as_slice(),
                    (
                        parts.data,
                        (parts.child_keys, (parts.child_values, pinned.contains(key))),
                    ),
                )
            })
            .unzip::<_, _, Vec<_>, (Vec<_>, (Vec<_>, (Vec<_>, Vec<_>)))>();

        sqlx::query(query)
            .bind(keys)
            .bind(objects)
            .bind(child_keys)
            .bind(child_values)
            .bind(pinned)
            .execute(&mut **tx)
            .await
            .unwrap_or_panic("cannot batch insert nodes");
    }

    inserts.clear();
}

// Splits nodes into chunks of at most `BATCH_ROWS` rows and `BATCH_BYTES` serialized bytes, as
// measured by `size`. A single node larger than the byte limit gets a chunk of its own.
#[cfg(feature = "cloud")]
fn insert_chunks<Node>(nodes: &[Node], size: impl Fn(&Node) -> usize) -> Vec<&[Node]> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut bytes = 0;

    for (index, node) in nodes.iter().enumerate() {
        let node_size = size(node);
        // Ends the chunk before this node once the chunk holds `BATCH_ROWS` nodes or the node would
        // take it past `BATCH_BYTES`, unless the chunk is empty.
        if index > start && (index - start == BATCH_ROWS || bytes + node_size > BATCH_BYTES) {
            chunks.push(&nodes[start..index]);
            start = index;
            bytes = 0;
        }
        bytes += node_size;
    }
    // Pushes the last chunk, which no node after it ended.
    if start < nodes.len() {
        chunks.push(&nodes[start..]);
    }

    chunks
}

/// The parts `object` is stored as.
fn node_parts<Hasher: WellBehavedHasher>(object: &OnDiskObject<Hasher>) -> Parts {
    object_parts::split(&serialize_object(object)).unwrap_or_panic("cannot split object into parts")
}

/// The node stored as `object`, `child_keys` and `child_values`: the parts joined, or `object`
/// alone for a row written before the parts existed, which has the other two NULL.
fn node_object<Hasher: WellBehavedHasher>(
    (object, child_keys, child_values): NodeRow,
) -> std::io::Result<OnDiskObject<Hasher>> {
    let object = match (child_keys, child_values) {
        (Some(child_keys), Some(child_values)) => object_parts::join(&Parts {
            data: object,
            child_keys,
            child_values,
        })?,
        _ => object,
    };
    OnDiskObject::deserialize(&mut object.as_slice(), 0)
}

/// The by-reference children of a row: from its `child_keys`, or from its object for a row written
/// before that column existed.
///
/// Panics on a `child_keys` value that is not whole hashes: gc's mark follows these lists, so a
/// short one would let gc delete nodes still in use.
fn row_children(
    children: Option<Vec<u8>>,
    object: Option<Vec<u8>>,
) -> Vec<ArenaHash<DefaultHasher>> {
    match (children, object) {
        (Some(children), _) => {
            let (hashes, rest) = children.as_chunks::<32>();
            assert!(
                rest.is_empty(),
                "ledger_db_nodes.child_keys is {} bytes, not whole 32-byte hashes",
                children.len()
            );
            hashes.iter().map(|hash| arena_hash(*hash)).collect()
        }

        (None, Some(object)) => {
            OnDiskObject::<DefaultHasher>::deserialize(&mut object.as_slice(), 0)
                .unwrap_or_panic("cannot deserialize node as OnDiskObject")
                .children
                .iter()
                .flat_map(|child| child.refs().into_iter().cloned())
                .collect()
        }

        (None, None) => vec![],
    }
}

fn serialize_object<Hasher: WellBehavedHasher>(object: &OnDiskObject<Hasher>) -> Vec<u8> {
    let mut serialized = Vec::with_capacity(object.serialized_size());
    Serializable::serialize(object, &mut serialized).unwrap_or_panic("cannot serialize object");
    serialized
}

async fn flush_deletes<Hasher: WellBehavedHasher>(
    tx: &mut SqlxTransaction,
    deletes: &mut Vec<ArenaHash<Hasher>>,
) {
    let keys = deletes
        .iter()
        .map(|key| key.0.as_slice())
        .collect::<Vec<_>>();
    delete_keys(tx, "ledger_db_nodes", &keys).await;
    deletes.clear();
}

// SQLite has no array parameters, so keys are bound one each in chunks.
#[cfg(feature = "standalone")]
async fn delete_keys(tx: &mut SqlxTransaction, table: &'static str, keys: &[&[u8]]) {
    for chunk in keys.chunks(BATCH_DELETE_SIZE) {
        let mut query = QueryBuilder::new(format!("DELETE FROM {table} WHERE key IN ("));
        let mut keys = query.separated(", ");
        chunk.iter().for_each(|key| {
            keys.push_bind(*key);
        });
        keys.push_unseparated(")");

        query
            .build()
            .execute(&mut **tx)
            .await
            .unwrap_or_panic("cannot batch delete keys");
    }
}

// Postgres takes the keys as one `bytea[]` parameter, so every statement has the same SQL whatever
// its number of keys.
#[cfg(feature = "cloud")]
async fn delete_keys(tx: &mut SqlxTransaction, table: &'static str, keys: &[&[u8]]) {
    let query = format!("DELETE FROM {table} WHERE key = ANY($1::bytea[])");

    for chunk in keys.chunks(BATCH_ROWS) {
        sqlx::query(&query)
            .bind(chunk)
            .execute(&mut **tx)
            .await
            .unwrap_or_panic("cannot batch delete keys");
    }
}

// Roots in `pinned` are written pinned with their counts, and a stored root stays pinned.
async fn flush_root_counts<Hasher: WellBehavedHasher>(
    tx: &mut SqlxTransaction,
    root_counts: Vec<(ArenaHash<Hasher>, u32)>,
    pinned: &HashSet<ArenaHash<Hasher>>,
) {
    // Collecting into a map keeps the last count per key, as applying the counts in order would. A
    // count of zero means the key is no longer a root.
    let (set, cleared) = root_counts
        .into_iter()
        .collect::<HashMap<_, _>>()
        .into_iter()
        .partition::<Vec<_>, _>(|(_, count)| *count > 0);

    let cleared = cleared
        .iter()
        .map(|(key, _)| key.0.as_slice())
        .collect::<Vec<_>>();
    delete_keys(tx, "ledger_db_roots", &cleared).await;

    let set = set
        .iter()
        .map(|(key, count)| (key.0.as_slice(), *count as i64, pinned.contains(key)))
        .collect::<Vec<_>>();
    upsert_root_counts(tx, &set).await;
}

// SQLite has no array parameters, so root counts are bound in chunks.
#[cfg(feature = "standalone")]
async fn upsert_root_counts(tx: &mut SqlxTransaction, root_counts: &[(&[u8], i64, bool)]) {
    for chunk in root_counts.chunks(BATCH_INSERT_SIZE) {
        QueryBuilder::new("INSERT INTO ledger_db_roots (key, count, pinned) ")
            .push_values(chunk, |mut q, (key, count, pinned)| {
                q.push_bind(*key)
                    .push_bind(*count)
                    .push(if *pinned { "TRUE" } else { "FALSE" });
            })
            .push(indoc! {"
                 ON CONFLICT (key) DO UPDATE
                SET count = EXCLUDED.count, pinned = ledger_db_roots.pinned OR EXCLUDED.pinned
            "})
            .build()
            .execute(&mut **tx)
            .await
            .unwrap_or_panic("cannot batch set root counts");
    }
}

// Postgres takes the keys, the counts and their pinned flags as three array parameters, so every
// statement has the same SQL whatever its number of root counts.
#[cfg(feature = "cloud")]
async fn upsert_root_counts(tx: &mut SqlxTransaction, root_counts: &[(&[u8], i64, bool)]) {
    let query = indoc! {"
        INSERT INTO ledger_db_roots (key, count, pinned)
        SELECT * FROM UNNEST($1::bytea[], $2::bigint[], $3::boolean[])
        ON CONFLICT (key) DO UPDATE
        SET count = EXCLUDED.count, pinned = ledger_db_roots.pinned OR EXCLUDED.pinned
    "};

    for chunk in root_counts.chunks(BATCH_ROWS) {
        let (keys, (counts, pinned)) = chunk
            .iter()
            .map(|(key, count, pinned)| (*key, (*count, *pinned)))
            .unzip::<_, _, Vec<_>, (Vec<_>, Vec<_>)>();

        sqlx::query(query)
            .bind(keys)
            .bind(counts)
            .bind(pinned)
            .execute(&mut **tx)
            .await
            .unwrap_or_panic("cannot batch set root counts");
    }
}

async fn delete_node<H>(tx: &mut SqlxTransaction, key: &ArenaHash<H>)
where
    H: WellBehavedHasher,
{
    let query = indoc! {"
        DELETE FROM ledger_db_nodes
        WHERE key = $1
    "};

    sqlx::query(query)
        .bind(key.0.as_slice())
        .execute(&mut **tx)
        .await
        .unwrap_or_panic("cannot delete node");
}

async fn set_root_count<H>(tx: &mut SqlxTransaction, key: ArenaHash<H>, count: u32)
where
    H: WellBehavedHasher,
{
    if count > 0 {
        let query = indoc! {"
            INSERT INTO ledger_db_roots (key, count)
            VALUES ($1, $2)
            ON CONFLICT (key)
            DO UPDATE SET count = $2
        "};

        sqlx::query(query)
            .bind(key.0.as_slice())
            .bind(count as i64)
            .execute(&mut **tx)
            .await
            .unwrap_or_panic("cannot set root count");
    } else {
        let query = indoc! {"
            DELETE FROM ledger_db_roots
            WHERE key = $1
        "};

        sqlx::query(query)
            .bind(key.0.as_slice())
            .execute(&mut **tx)
            .await
            .unwrap_or_panic("cannot set root count");
    }
}

pub(crate) fn arena_hash(bytes: [u8; 32]) -> ArenaHash<DefaultHasher> {
    ArenaHash::deserialize(&mut bytes.as_slice(), 0).expect("32 bytes are an ArenaHash")
}

// Persists `count` roots, flushes their nodes to the database and reads them back.
#[cfg(test)]
fn persist_nodes(
    storage: &Storage<LedgerDb>,
    db: &mut LedgerDb,
    count: usize,
) -> Vec<(ArenaHash<DefaultHasher>, OnDiskObject<DefaultHasher>)> {
    // Each array of `SMALL_OBJECT_LIMIT / 4` `u32`s holds `SMALL_OBJECT_LIMIT` bytes, which with its
    // header exceeds the inline limit, so it is stored as its own node. A distinct `u32` per array
    // gives each node its own key.
    let _roots = (0..count as u32)
        .map(|value| {
            let mut root = storage.alloc([value; SMALL_OBJECT_LIMIT / 4]);
            root.persist();
            root
        })
        .collect::<Vec<_>>();
    storage.with_backend(|backend| backend.flush_all_changes_to_db());

    let (nodes, _) = db.scan(None, 2 * count);
    assert_eq!(nodes.len(), count);
    nodes
}

// `scan_keys` pages through the same keys as `scan`, in the same order, resuming where it left off.
// Shared by both suites, like `assert_get_root_count_reads_stored_count`.
#[cfg(test)]
fn assert_scan_keys_pages_like_scan(storage: &Storage<LedgerDb>, db: &mut LedgerDb) {
    let nodes = persist_nodes(storage, db, 10);
    let scanned = nodes.into_iter().map(|(key, _)| key).collect::<Vec<_>>();

    let pages = std::iter::successors(Some(db.scan_keys(None, 3)), |(_, handle)| {
        handle.clone().map(|handle| db.scan_keys(Some(handle), 3))
    })
    .take_while(|(keys, _)| !keys.is_empty())
    .collect::<Vec<_>>();

    assert!(pages.iter().all(|(keys, _)| keys.len() <= 3));
    let keys = pages
        .into_iter()
        .flat_map(|(keys, _)| keys)
        .collect::<Vec<_>>();
    assert_eq!(keys, scanned);
}

// `batch_get_children` reports each stored node's children as its object does, reached through
// inlined children too, falls back to the object for a row stored whole, as written before the
// parts existed, which still loads, and leaves out keys that are not stored. Shared by both suites,
// like `assert_get_root_count_reads_stored_count`.
#[cfg(test)]
fn assert_batch_get_children_matches_objects(storage: &Storage<LedgerDb>, db: &mut LedgerDb) {
    use midnight_storage_core_v1::arena::ArenaKey;

    // A leaf is stored as its own node, while an `Option` holding one is small enough to be
    // inlined: the first root reaches its leaf only through an inlined node, the second directly.
    let leaf = |value| storage.alloc([value; SMALL_OBJECT_LIMIT / 4]);
    let mut through_inlined = storage.alloc(Some(storage.alloc(Some(leaf(1)))));
    let mut direct = storage.alloc(Some(leaf(2)));
    through_inlined.persist();
    direct.persist();
    storage.with_backend(|backend| backend.flush_all_changes_to_db());

    let (stored, _) = db.scan(None, 100);
    assert!(
        stored.iter().any(|(_, object)| object
            .children
            .iter()
            .any(|child| matches!(child, ArenaKey::Direct(_)))),
        "some children are inlined"
    );
    let expected = stored
        .iter()
        .map(|(key, object)| {
            let children = object
                .children
                .iter()
                .flat_map(|child| child.refs().into_iter().cloned())
                .collect::<Vec<_>>();
            (key.clone(), children)
        })
        .collect::<HashMap<_, _>>();

    let (legacy, legacy_object) = stored
        .iter()
        .find(|(key, _)| !expected[key].is_empty())
        .expect("a node with children");
    let whole = serialize_object(legacy_object);
    block_in_place(|| {
        Handle::current().block_on(
            sqlx::query(indoc! {"
                UPDATE ledger_db_nodes
                SET object = $1, child_keys = NULL, child_values = NULL
                WHERE key = $2
            "})
            .bind(whole.as_slice())
            .bind(legacy.0.as_slice())
            .execute(&*db.pool),
        )
    })
    .expect("store the node whole");
    let loaded = db.get_node(legacy).expect("the whole node loads");
    assert_eq!(serialize_object(&loaded), whole);

    let absent = arena_hash([0xff; 32]);
    let reported = db
        .batch_get_children(expected.keys().cloned().chain([absent]))
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(reported, expected);
}

// `batch_get_root_counts` reads the stored count of every key, in chunks on SQLite, and 0 for keys
// that are not roots, as `get_root_count` does. Shared by both suites, like
// `assert_get_root_count_reads_stored_count`.
#[cfg(test)]
fn assert_batch_get_root_counts_reads_every_key(db: &mut LedgerDb) {
    // Enough keys to span several SQLite statements; every third one is not a root.
    #[cfg(feature = "standalone")]
    const KEYS: usize = 2 * SQLITE_MAX_BINDS + 1;
    #[cfg(feature = "cloud")]
    const KEYS: usize = 1_000;

    let key = |i: usize| {
        let mut bytes = [0; 32];
        bytes[..8].copy_from_slice(&(i as u64).to_le_bytes());
        arena_hash(bytes)
    };
    let count = |i: usize| {
        if i.is_multiple_of(3) {
            0
        } else {
            (i % 7 + 1) as u32
        }
    };
    db.batch_update((0..KEYS).map(|i| (key(i), SetRootCount(count(i)))));

    let counts = db.batch_get_root_counts((0..KEYS).map(key));
    assert_eq!(counts.len(), KEYS);
    assert!((0..KEYS).all(|i| counts.get(&key(i)) == Some(&count(i))));
}

// `batch_update` inserts in chunks, storing every node as given, and accepts a key inserted twice
// in one batch. Shared by both suites, like `assert_get_root_count_reads_stored_count`.
#[cfg(test)]
fn assert_batch_update_inserts_in_chunks(storage: &Storage<LedgerDb>, db: &mut LedgerDb) {
    // Inserting every node spans several insert chunks.
    #[cfg(feature = "standalone")]
    const NODES: usize = 2 * BATCH_INSERT_SIZE + 1;
    #[cfg(feature = "cloud")]
    const NODES: usize = 2 * BATCH_ROWS + 1;

    let nodes = persist_nodes(storage, db, NODES);

    // Deletes every node, then inserts them all again in one batch, the first node twice in the
    // same chunk.
    db.batch_update(nodes.iter().map(|(key, _)| (key.clone(), DeleteNode)));
    assert!(db.scan(None, 1).0.is_empty(), "every node is deleted");
    db.batch_update(
        nodes
            .first()
            .into_iter()
            .chain(&nodes)
            .map(|(key, object)| (key.clone(), InsertNode(object.clone()))),
    );

    // Compares serialized bytes, as `OnDiskObject` has no `PartialEq`.
    let serialized = |nodes: &[(ArenaHash<_>, _)]| {
        nodes
            .iter()
            .map(|(key, object)| (key.clone(), serialize_object(object)))
            .collect::<Vec<_>>()
    };
    let (inserted, _) = db.scan(None, 2 * NODES);
    assert!(
        serialized(&inserted) == serialized(&nodes),
        "inserted nodes match the originals"
    );
}

// `batch_update` deletes in chunks, removing exactly the deleted keys, and applies inserts, deletes
// and root counts of the same key in their original order. Shared by both suites, like
// `assert_get_root_count_reads_stored_count`.
#[cfg(test)]
fn assert_batch_update_deletes_in_chunks_and_keeps_update_order(
    storage: &Storage<LedgerDb>,
    db: &mut LedgerDb,
) {
    // Inserting every node spans several insert chunks, and deleting every other node spans two
    // delete chunks.
    #[cfg(feature = "standalone")]
    const NODES: usize = 2 * BATCH_DELETE_SIZE + 2;
    #[cfg(feature = "cloud")]
    const NODES: usize = 2 * BATCH_ROWS + 2;

    let nodes = persist_nodes(storage, db, NODES);

    // Deletes every other node in one batch.
    let (deleted, kept) = nodes
        .into_iter()
        .enumerate()
        .partition::<Vec<_>, _>(|(index, _)| index % 2 == 0);
    db.batch_update(
        deleted
            .iter()
            .map(|(_, (key, _))| (key.clone(), DeleteNode)),
    );

    let (remaining, _) = db.scan(None, 2 * NODES);
    assert_eq!(
        remaining.iter().map(|(key, _)| key).collect::<Vec<_>>(),
        kept.iter().map(|(_, (key, _))| key).collect::<Vec<_>>()
    );

    // A delete then an insert of a present key leaves it, and an insert then a delete of an absent
    // key leaves it absent. Root counts set between the node updates are still written.
    let (_, (present, present_object)) = kept.into_iter().next().expect("kept node");
    let (_, (absent, absent_object)) = deleted.into_iter().next().expect("deleted node");
    db.batch_update(
        [
            (present.clone(), DeleteNode),
            (present.clone(), SetRootCount(3)),
            (present.clone(), InsertNode(present_object)),
            (absent.clone(), InsertNode(absent_object)),
            (absent.clone(), SetRootCount(0)),
            (absent.clone(), DeleteNode),
        ]
        .into_iter(),
    );
    assert!(
        db.get_node(&present).is_some(),
        "re-inserted node is present"
    );
    assert!(db.get_node(&absent).is_none(), "deleted node is absent");
    assert_eq!(db.get_root_count(&present), 3);
    assert_eq!(db.get_root_count(&absent), 0);
}

// `batch_update` writes root counts in chunks: a count above zero is stored, zero removes the root,
// and a later count for the same key replaces an earlier one. Shared by both suites, like
// `assert_get_root_count_reads_stored_count`.
#[cfg(test)]
fn assert_batch_update_sets_root_counts(db: &mut LedgerDb) {
    // Setting every root count spans two upsert chunks.
    #[cfg(feature = "standalone")]
    const ROOTS: usize = BATCH_INSERT_SIZE + 1;
    #[cfg(feature = "cloud")]
    const ROOTS: usize = BATCH_ROWS + 1;

    let [first, second, third, absent] =
        [0xa1, 0xa2, 0xa3, 0xa4].map(|byte| arena_hash([byte; 32]));

    db.batch_update(
        [
            (first.clone(), SetRootCount(1)),
            (second.clone(), SetRootCount(2)),
            (third.clone(), SetRootCount(3)),
        ]
        .into_iter(),
    );
    assert_eq!(
        db.get_roots(),
        HashMap::from([(first.clone(), 1), (second.clone(), 2), (third.clone(), 3)])
    );

    db.batch_update(
        [
            (first.clone(), SetRootCount(5)),
            (second.clone(), SetRootCount(0)),
            (third.clone(), SetRootCount(4)),
            (third.clone(), SetRootCount(7)),
            (absent.clone(), SetRootCount(0)),
        ]
        .into_iter(),
    );

    assert_eq!(db.get_root_count(&first), 5);
    assert_eq!(db.get_root_count(&second), 0);
    assert_eq!(db.get_root_count(&third), 7);
    assert_eq!(db.get_root_count(&absent), 0);
    assert_eq!(
        db.get_roots(),
        HashMap::from([(first.clone(), 5), (third.clone(), 7)])
    );
    assert_eq!(db.root_count(), 2);

    // Sets then clears `ROOTS` root counts, each in one batch.
    let roots = (0..ROOTS as u32)
        .map(|index| {
            let mut bytes = [0; 32];
            bytes[..4].copy_from_slice(&index.to_be_bytes());
            arena_hash(bytes)
        })
        .collect::<Vec<_>>();
    db.batch_update(roots.iter().map(|root| (root.clone(), SetRootCount(1))));
    assert_eq!(db.root_count(), ROOTS + 2);
    db.batch_update(roots.iter().map(|root| (root.clone(), SetRootCount(0))));
    assert_eq!(db.get_roots(), HashMap::from([(first, 5), (third, 7)]));
    assert_eq!(db.root_count(), 2);
}

/// `get_root_count` must return the stored count, not the number of matching rows: `key` is the
/// primary key, so a row-count aggregate can only ever be 0 or 1.
///
/// Shared by the sqlite and postgres suites rather than written twice: `LedgerDb` is one type under
/// both features - only its pool field is feature-gated - so the assertions need neither generics
/// nor a macro. Only acquiring the pool differs, and that stays in each suite.
#[cfg(test)]
fn assert_get_root_count_reads_stored_count(db: &mut LedgerDb) {
    let key = arena_hash([0xaa; 32]);
    let other_key = arena_hash([0xbb; 32]);

    assert_eq!(db.get_root_count(&key), 0, "missing key is not a root");

    db.set_root_count(key.clone(), 7);
    db.set_root_count(other_key.clone(), 1);
    assert_eq!(db.get_root_count(&key), 7);
    assert_eq!(db.get_root_count(&other_key), 1);

    db.set_root_count(key.clone(), 0);
    assert_eq!(db.get_root_count(&key), 0, "count 0 deletes the row");
    assert_eq!(db.get_root_count(&other_key), 1);
}

#[cfg(test)]
#[test]
#[should_panic(expected = "not whole 32-byte hashes")]
fn row_children_rejects_partial_hashes() {
    row_children(Some(vec![0; 33]), None);
}

#[cfg(all(test, feature = "standalone"))]
pub(crate) mod sqlite_tests {
    use super::{
        SQLITE_MAX_BINDS, assert_batch_get_children_matches_objects,
        assert_batch_get_root_counts_reads_every_key,
        assert_batch_update_deletes_in_chunks_and_keeps_update_order,
        assert_batch_update_inserts_in_chunks, assert_batch_update_sets_root_counts,
        assert_get_root_count_reads_stored_count, assert_scan_keys_pages_like_scan,
    };
    use crate::infra::{
        ledger_db::v1_1::LedgerDb,
        migrations::sqlite::run_for_ledger_db,
        pool::sqlite::{Config, SqlitePool},
    };
    use anyhow::Context;
    use indoc::indoc;
    use midnight_storage_core_v1::{DefaultHasher, Storage, arena::ArenaHash, db::DB};
    use sqlx::{QueryBuilder, Sqlite};
    use std::error::Error as StdError;

    /// Number of times a root is persisted before it is unpersisted again; stands in for the
    /// retention window (a ledger state root is persisted once per block and unpersisted once
    /// the block leaves the window). Any value >= 3 exposes the read bug.
    const PERSISTS: u32 = 5;

    pub(crate) async fn setup_pool() -> Result<SqlitePool, Box<dyn StdError>> {
        let pool = SqlitePool::new(Config::default())
            .await
            .context("create sqlite pool")?;
        run_for_ledger_db(&pool)
            .await
            .context("run ledger_db migrations")?;

        Ok(pool)
    }

    /// The stored root count for `key`, read with plain SQL so assertions do not go through the
    /// code under test. `None` means there is no row, i.e. the key is not a root.
    pub(crate) async fn stored_root_count(
        pool: &SqlitePool,
        key: &ArenaHash<DefaultHasher>,
    ) -> Result<Option<i64>, Box<dyn StdError>> {
        let query = indoc! {"
            SELECT count
            FROM ledger_db_roots
            WHERE key = $1
        "};

        let count = sqlx::query_as::<_, (i64,)>(query)
            .bind(key.0.as_slice())
            .fetch_optional(&**pool)
            .await
            .context("select stored root count")?
            .map(|(count,)| count);

        Ok(count)
    }

    // See `assert_batch_update_sets_root_counts`.
    #[tokio::test(flavor = "multi_thread")]
    async fn batch_update_sets_root_counts() -> Result<(), Box<dyn StdError>> {
        let pool = setup_pool().await?;
        let mut db = LedgerDb::new(pool);
        assert_batch_update_sets_root_counts(&mut db);
        Ok(())
    }

    // See `assert_batch_update_deletes_in_chunks_and_keeps_update_order`.
    #[tokio::test(flavor = "multi_thread")]
    async fn batch_update_deletes_in_chunks_and_keeps_update_order() -> Result<(), Box<dyn StdError>>
    {
        let pool = setup_pool().await?;
        let storage = Storage::new(16, LedgerDb::new(pool.clone()));
        let mut db = LedgerDb::new(pool);
        assert_batch_update_deletes_in_chunks_and_keeps_update_order(&storage, &mut db);
        Ok(())
    }

    // See `assert_batch_get_children_matches_objects`.
    #[tokio::test(flavor = "multi_thread")]
    async fn batch_get_children_matches_objects() -> Result<(), Box<dyn StdError>> {
        let pool = setup_pool().await?;
        let storage = Storage::new(16, LedgerDb::new(pool.clone()));
        let mut db = LedgerDb::new(pool);
        assert_batch_get_children_matches_objects(&storage, &mut db);
        Ok(())
    }

    // See `assert_batch_get_root_counts_reads_every_key`.
    #[tokio::test(flavor = "multi_thread")]
    async fn batch_get_root_counts_reads_every_key() -> Result<(), Box<dyn StdError>> {
        let pool = setup_pool().await?;
        let mut db = LedgerDb::new(pool);
        assert_batch_get_root_counts_reads_every_key(&mut db);
        Ok(())
    }

    // See `assert_scan_keys_pages_like_scan`.
    #[tokio::test(flavor = "multi_thread")]
    async fn scan_keys_pages_like_scan() -> Result<(), Box<dyn StdError>> {
        let pool = setup_pool().await?;
        let storage = Storage::new(16, LedgerDb::new(pool.clone()));
        let mut db = LedgerDb::new(pool);
        assert_scan_keys_pages_like_scan(&storage, &mut db);
        Ok(())
    }

    // See `assert_batch_update_inserts_in_chunks`.
    #[tokio::test(flavor = "multi_thread")]
    async fn batch_update_inserts_in_chunks() -> Result<(), Box<dyn StdError>> {
        let pool = setup_pool().await?;
        let storage = Storage::new(16, LedgerDb::new(pool.clone()));
        let mut db = LedgerDb::new(pool);
        assert_batch_update_inserts_in_chunks(&storage, &mut db);
        Ok(())
    }

    #[tokio::test]
    async fn scan_empty_db_returns_no_rows_and_no_handle() -> Result<(), Box<dyn StdError>> {
        let pool = SqlitePool::new(Config::default())
            .await
            .context("create sqlite pool")?;
        run_for_ledger_db(&pool)
            .await
            .context("run ledger_db migrations")?;

        let db = LedgerDb::new(pool);

        let (batch, handle) = tokio::task::spawn_blocking(move || {
            // scan() uses block_in_place internally, which requires being on a
            // multi-thread runtime; spawn_blocking is a clean way to satisfy that.
            db.scan(None, 100)
        })
        .await
        .context("await scan")?;

        assert!(batch.is_empty());
        assert!(handle.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn scan_with_resume_on_empty_db_still_returns_no_rows() -> Result<(), Box<dyn StdError>> {
        let pool = SqlitePool::new(Config::default())
            .await
            .context("create sqlite pool")?;
        run_for_ledger_db(&pool)
            .await
            .context("run ledger_db migrations")?;

        let db = LedgerDb::new(pool);
        let fake_resume: ArenaHash<DefaultHasher> = ArenaHash::default();

        let (batch, handle) = tokio::task::spawn_blocking(move || db.scan(Some(fake_resume), 50))
            .await
            .context("await scan")?;

        assert!(batch.is_empty());
        assert!(handle.is_none());

        Ok(())
    }

    /// `get_root_count` must return the stored count, not the number of matching rows: `key` is
    /// the primary key, so a row-count aggregate can only ever be 0 or 1.
    #[tokio::test(flavor = "multi_thread")]
    async fn get_root_count_returns_stored_count_not_row_count() -> Result<(), Box<dyn StdError>> {
        let pool = setup_pool().await?;
        let mut db = LedgerDb::new(pool);

        assert_get_root_count_reads_stored_count(&mut db);

        Ok(())
    }

    /// Root counts must accumulate across persists: storage-core's flush computes
    /// `stored count + delta`, so an under-reading `get_root_count` writes back a count that is
    /// too low. With a read saturating at 1 every persist stores `1 + 1`, and the count caps at
    /// 2 however many blocks reference the root.
    #[tokio::test(flavor = "multi_thread")]
    async fn repeated_persist_accumulates_stored_root_count() -> Result<(), Box<dyn StdError>> {
        let pool = setup_pool().await?;
        let storage = Storage::new(16, LedgerDb::new(pool.clone()));

        let mut root = storage.alloc(42u32);
        for _ in 0..PERSISTS {
            root.persist();
            storage.with_backend(|backend| backend.flush_all_changes_to_db());
        }

        assert_eq!(
            stored_root_count(&pool, &root.hash()).await?,
            Some(PERSISTS.into())
        );

        Ok(())
    }

    /// Balanced persist/unpersist must bring the root count back to zero. Under-read counts make
    /// storage-core's flush compute a negative count and panic ("roots counts can't be negative"),
    /// which is what takes the chain-indexer down once a root leaves the retention window.
    #[tokio::test(flavor = "multi_thread")]
    async fn balanced_unpersist_returns_root_count_to_zero() -> Result<(), Box<dyn StdError>> {
        let pool = setup_pool().await?;
        let storage = Storage::new(16, LedgerDb::new(pool.clone()));

        let mut root = storage.alloc(42u32);
        for _ in 0..PERSISTS {
            root.persist();
            storage.with_backend(|backend| backend.flush_all_changes_to_db());
        }

        for _ in 0..PERSISTS {
            root.unpersist();
            storage.with_backend(|backend| backend.flush_all_changes_to_db());
        }

        assert_eq!(
            stored_root_count(&pool, &root.hash()).await?,
            None,
            "no longer a root"
        );

        Ok(())
    }

    // `SQLITE_MAX_BINDS` is the linked SQLite's limit: a statement with that many binds runs, and
    // one with a bind more fails.
    #[tokio::test]
    async fn sqlite_max_binds_is_the_linked_limit() -> Result<(), Box<dyn StdError>> {
        let pool = setup_pool().await?;
        let query = |binds: usize| {
            let mut query = QueryBuilder::<Sqlite>::new("SELECT 1 WHERE 1 IN (");
            let mut values = query.separated(", ");
            (0..binds as i64).for_each(|value| {
                values.push_bind(value);
            });
            values.push_unseparated(")");
            query
        };

        query(SQLITE_MAX_BINDS)
            .build()
            .execute(&*pool)
            .await
            .context("run statement with SQLITE_MAX_BINDS binds")?;

        let error = query(SQLITE_MAX_BINDS + 1)
            .build()
            .execute(&*pool)
            .await
            .expect_err("statement with more than SQLITE_MAX_BINDS binds fails");
        assert!(
            error.to_string().contains("too many SQL variables"),
            "unexpected error: {error}"
        );

        Ok(())
    }
}

#[cfg(all(test, feature = "cloud"))]
mod postgres_tests {
    use super::{
        BATCH_BYTES, BATCH_ROWS, assert_batch_get_children_matches_objects,
        assert_batch_get_root_counts_reads_every_key,
        assert_batch_update_deletes_in_chunks_and_keeps_update_order,
        assert_batch_update_inserts_in_chunks, assert_batch_update_sets_root_counts,
        assert_get_root_count_reads_stored_count, assert_scan_keys_pages_like_scan, insert_chunks,
    };
    use crate::infra::{
        ledger_db::v1_1::LedgerDb,
        migrations::postgres::run as run_migrations,
        pool::{self, postgres::PostgresPool},
    };
    use crate::testing::POSTGRES_IMAGE_TAG;
    use anyhow::Context;
    use midnight_storage_core_v1::{DefaultHasher, Storage, arena::ArenaHash, db::DB};
    use sqlx::postgres::PgSslMode;
    use std::{error::Error as StdError, time::Duration};
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres;

    pub(super) async fn setup_pool()
    -> Result<(testcontainers::ContainerAsync<Postgres>, PostgresPool), Box<dyn StdError>> {
        let container = Postgres::default()
            .with_db_name("indexer")
            .with_user("indexer")
            .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
            .with_tag(POSTGRES_IMAGE_TAG)
            .start()
            .await
            .context("start Postgres container")?;
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .context("get Postgres port")?;

        let config = pool::postgres::Config {
            host: "localhost".to_string(),
            port,
            dbname: "indexer".to_string(),
            user: "indexer".to_string(),
            password: env!("APP__INFRA__STORAGE__PASSWORD").into(),
            sslmode: PgSslMode::Prefer,
            max_connections: 10,
            idle_timeout: Duration::from_secs(60),
            max_lifetime: Duration::from_secs(5 * 60),
            test_before_acquire: true,
        };
        let pool = PostgresPool::new(config).await?;
        run_migrations(&pool).await.context("run migrations")?;

        Ok((container, pool))
    }

    #[tokio::test]
    async fn scan_empty_db_returns_no_rows_and_no_handle() -> Result<(), Box<dyn StdError>> {
        let (_container, pool) = setup_pool().await?;
        let db = LedgerDb::new(pool);

        let (batch, handle) = tokio::task::spawn_blocking(move || db.scan(None, 100))
            .await
            .context("await scan")?;

        assert!(batch.is_empty());
        assert!(handle.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn scan_with_resume_on_empty_db_still_returns_no_rows() -> Result<(), Box<dyn StdError>> {
        let (_container, pool) = setup_pool().await?;
        let db = LedgerDb::new(pool);
        let fake_resume: ArenaHash<DefaultHasher> = ArenaHash::default();

        let (batch, handle) = tokio::task::spawn_blocking(move || db.scan(Some(fake_resume), 50))
            .await
            .context("await scan")?;

        assert!(batch.is_empty());
        assert!(handle.is_none());

        Ok(())
    }

    /// See the standalone counterpart: `key` is the primary key, so a row-count aggregate can
    /// only ever be 0 or 1, never the stored count.
    #[tokio::test(flavor = "multi_thread")]
    async fn get_root_count_reads_the_stored_count() -> Result<(), Box<dyn StdError>> {
        let (_container, pool) = setup_pool().await?;
        let mut db = LedgerDb::new(pool);

        assert_get_root_count_reads_stored_count(&mut db);

        Ok(())
    }

    // Postgres insert chunks respect both the row limit and the byte limit, and a node larger than
    // the byte limit gets a chunk of its own.
    #[test]
    fn insert_chunks_respect_row_and_byte_limits() {
        let lengths = |sizes: &[usize]| {
            insert_chunks(sizes, |size| *size)
                .iter()
                .map(|chunk| chunk.len())
                .collect::<Vec<_>>()
        };

        assert_eq!(lengths(&[1; BATCH_ROWS + 1]), vec![BATCH_ROWS, 1]);
        assert_eq!(lengths(&[BATCH_BYTES / 2; 3]), vec![2, 1]);
        assert_eq!(lengths(&[BATCH_BYTES / 2 + 1; 3]), vec![1, 1, 1]);
        assert_eq!(lengths(&[BATCH_BYTES; 2]), vec![1, 1]);
        assert_eq!(
            lengths(&[1, BATCH_BYTES + 1, BATCH_BYTES + 1, 1]),
            vec![1, 1, 1, 1]
        );

        assert!(lengths(&[]).is_empty());
    }

    // See `assert_batch_update_sets_root_counts`.
    #[tokio::test(flavor = "multi_thread")]
    async fn batch_update_sets_root_counts() -> Result<(), Box<dyn StdError>> {
        let (_container, pool) = setup_pool().await?;
        let mut db = LedgerDb::new(pool);
        assert_batch_update_sets_root_counts(&mut db);
        Ok(())
    }

    // See `assert_batch_update_deletes_in_chunks_and_keeps_update_order`.
    #[tokio::test(flavor = "multi_thread")]
    async fn batch_update_deletes_in_chunks_and_keeps_update_order() -> Result<(), Box<dyn StdError>>
    {
        let (_container, pool) = setup_pool().await?;
        let storage = Storage::new(16, LedgerDb::new(pool.clone()));
        let mut db = LedgerDb::new(pool);
        assert_batch_update_deletes_in_chunks_and_keeps_update_order(&storage, &mut db);
        Ok(())
    }

    // See `assert_batch_get_children_matches_objects`.
    #[tokio::test(flavor = "multi_thread")]
    async fn batch_get_children_matches_objects() -> Result<(), Box<dyn StdError>> {
        let (_container, pool) = setup_pool().await?;
        let storage = Storage::new(16, LedgerDb::new(pool.clone()));
        let mut db = LedgerDb::new(pool);
        assert_batch_get_children_matches_objects(&storage, &mut db);
        Ok(())
    }

    // See `assert_batch_get_root_counts_reads_every_key`.
    #[tokio::test(flavor = "multi_thread")]
    async fn batch_get_root_counts_reads_every_key() -> Result<(), Box<dyn StdError>> {
        let (_container, pool) = setup_pool().await?;
        let mut db = LedgerDb::new(pool);
        assert_batch_get_root_counts_reads_every_key(&mut db);
        Ok(())
    }

    // See `assert_scan_keys_pages_like_scan`.
    #[tokio::test(flavor = "multi_thread")]
    async fn scan_keys_pages_like_scan() -> Result<(), Box<dyn StdError>> {
        let (_container, pool) = setup_pool().await?;
        let storage = Storage::new(16, LedgerDb::new(pool.clone()));
        let mut db = LedgerDb::new(pool);
        assert_scan_keys_pages_like_scan(&storage, &mut db);
        Ok(())
    }

    // See `assert_batch_update_inserts_in_chunks`.
    #[tokio::test(flavor = "multi_thread")]
    async fn batch_update_inserts_in_chunks() -> Result<(), Box<dyn StdError>> {
        let (_container, pool) = setup_pool().await?;
        let storage = Storage::new(16, LedgerDb::new(pool.clone()));
        let mut db = LedgerDb::new(pool);
        assert_batch_update_inserts_in_chunks(&storage, &mut db);
        Ok(())
    }
}
