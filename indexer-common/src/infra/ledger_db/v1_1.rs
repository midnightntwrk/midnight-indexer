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

use crate::error::StdErrorExt;
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
#[cfg(feature = "cloud")]
use std::collections::HashSet;
use std::{collections::HashMap, future::ready};
use tokio::{runtime::Handle, task::block_in_place};

// Serialized node bytes per Postgres insert. Far below Postgres' 1 GB limit on one parameter value,
// it also bounds the memory a statement takes in the indexer and in Postgres.
#[cfg(feature = "cloud")]
const BATCH_BYTES: usize = 64 * 1024 * 1024;
// Keys per SQLite `DELETE … IN (…)`, one bind each.
#[cfg(feature = "standalone")]
const BATCH_DELETE_SIZE: usize = SQLITE_MAX_BINDS;
// Rows per SQLite `INSERT … VALUES`, two binds each.
#[cfg(feature = "standalone")]
const BATCH_INSERT_SIZE: usize = SQLITE_MAX_BINDS / 2;
// Rows per Postgres statement taking its rows as array parameters.
#[cfg(feature = "cloud")]
const BATCH_ROWS: usize = 10_000;
// SQLite's maximum number of binds per statement.
#[cfg(feature = "standalone")]
const SQLITE_MAX_BINDS: usize = 32_766;

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

    async fn begin_write(&self, msg: &'static str) -> SqlxTransaction {
        #[cfg(feature = "cloud")]
        let tx = crate::infra::pool::postgres::begin_write(&self.pool).await;
        #[cfg(feature = "standalone")]
        let tx = self.pool.begin().await;
        tx.unwrap_or_panic(msg)
    }
}

impl DB for LedgerDb {
    type Hasher = DefaultHasher;
    type ScanResumeHandle = ArenaHash<DefaultHasher>;

    fn get_node(&self, key: &ArenaHash<Self::Hasher>) -> Option<OnDiskObject<Self::Hasher>> {
        block_in_place(|| {
            Handle::current().block_on(async {
                let query = indoc! {"
                    SELECT object
                    FROM ledger_db_nodes
                    WHERE key = $1
                "};

                sqlx::query_as::<_, (Vec<u8>,)>(query)
                    .bind(key.0.as_slice())
                    .fetch_optional(&*self.pool)
                    .await
                    .unwrap_or_panic("cannot get node")
                    .map(|(object,)| {
                        OnDiskObject::<Self::Hasher>::deserialize(&mut object.as_slice(), 0)
                    })
                    .transpose()
                    .unwrap_or_panic("cannot deserialize node as OnDiskObject")
            })
        })
    }

    fn insert_node(&mut self, key: ArenaHash<Self::Hasher>, object: OnDiskObject<Self::Hasher>) {
        block_in_place(|| {
            Handle::current().block_on(async {
                let mut tx = self.begin_write("begin transaction for insert node").await;

                let mut ser_object = Vec::with_capacity(object.serialized_size());
                Serializable::serialize(&object, &mut ser_object)
                    .unwrap_or_panic("cannot serialize object");

                let query = indoc! {"
                    INSERT INTO ledger_db_nodes (
                        key,
                        object
                    )
                    VALUES (
                        $1,
                        $2
                    )
                    ON CONFLICT (key) DO UPDATE
                    SET object = EXCLUDED.object
                "};

                sqlx::query(query)
                    .bind(key.0.as_slice())
                    .bind(ser_object.as_slice())
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
                let mut tx = self.begin_write("begin transaction for delete node").await;

                delete_node(&mut tx, key).await;

                tx.commit()
                    .await
                    .unwrap_or_panic("commit transaction for delete node");
            })
        })
    }

    fn batch_update<I>(&mut self, updates: I)
    where
        I: Iterator<Item = (ArenaHash<Self::Hasher>, Update<Self::Hasher>)>,
    {
        block_in_place(|| {
            Handle::current().block_on(async {
                let mut tx = self.begin_write("begin transaction for batch update").await;

                // Consecutive inserts and consecutive deletes are each sent in batches. Flushing one
                // buffer before the other grows keeps the updates in their original order.
                let mut inserts = Vec::new();
                let mut deletes = Vec::new();
                let mut root_counts = Vec::new();

                for (key, update) in updates {
                    match update {
                        InsertNode(object) => {
                            flush_deletes(&mut tx, &mut deletes).await;
                            inserts.push((key, object));
                        }
                        DeleteNode => {
                            flush_inserts(&mut tx, &mut inserts).await;
                            deletes.push(key);
                        }
                        // Root counts live in `ledger_db_roots`, which no node insert or delete
                        // touches, so they are written together after the nodes.
                        SetRootCount(count) => root_counts.push((key, count)),
                    }
                }

                flush_inserts(&mut tx, &mut inserts).await;
                flush_deletes(&mut tx, &mut deletes).await;
                flush_root_counts(&mut tx, root_counts).await;

                tx.commit()
                    .await
                    .unwrap_or_panic("commit transaction for batch update");
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
                        SELECT key, object
                        FROM ledger_db_nodes
                        WHERE key = ANY($1::bytea[])
                    "};

                    let mut nodes = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(query)
                        .bind(keys.iter().map(|key| key.0.as_slice()).collect::<Vec<_>>())
                        .fetch(&*self.pool)
                        .and_then(|(key, object)| {
                            let key_and_object =
                                ArenaHash::<Self::Hasher>::deserialize(&mut key.as_slice(), 0)
                                    .map_err(|error| sqlx::Error::Decode(error.into()))
                                    .and_then(|key| {
                                        let object = OnDiskObject::<Self::Hasher>::deserialize(
                                            &mut object.as_slice(),
                                            0,
                                        )
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
                        SELECT key, object
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
                        .build_query_as::<(Vec<u8>, Vec<u8>)>()
                        .fetch(&*self.pool)
                        .and_then(|(key, object)| {
                            let key_and_object =
                                ArenaHash::<Self::Hasher>::deserialize(&mut key.as_slice(), 0)
                                    .map_err(|error| sqlx::Error::Decode(error.into()))
                                    .and_then(|key| {
                                        let object = OnDiskObject::<Self::Hasher>::deserialize(
                                            &mut object.as_slice(),
                                            0,
                                        )
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

    fn set_root_count(&mut self, key: ArenaHash<Self::Hasher>, count: u32) {
        block_in_place(|| {
            Handle::current().block_on(async {
                let mut tx = self
                    .begin_write("begin transaction for set root count")
                    .await;

                set_root_count(&mut tx, key, count).await;

                tx.commit()
                    .await
                    .unwrap_or_panic("commit transaction for set root count");
            })
        })
    }

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
                let mut query = QueryBuilder::new("SELECT key, object FROM ledger_db_nodes");
                if let Some(handle) = resume_from.as_ref() {
                    query.push(" WHERE key > ");
                    query.push_bind(handle.0.as_slice());
                }
                query.push(" ORDER BY key LIMIT ");
                query.push_bind(batch_size as i64);

                let raw = query
                    .build_query_as::<(Vec<u8>, Vec<u8>)>()
                    .fetch_all(&*self.pool)
                    .await
                    .unwrap_or_panic("cannot scan ledger_db_nodes");

                let batch = raw
                    .into_iter()
                    .map(|(key, object)| {
                        let key = ArenaHash::<Self::Hasher>::deserialize(&mut key.as_slice(), 0)
                            .unwrap_or_panic("cannot deserialize key as ArenaHash");
                        let object =
                            OnDiskObject::<Self::Hasher>::deserialize(&mut object.as_slice(), 0)
                                .unwrap_or_panic("cannot deserialize node as OnDiskObject");
                        (key, object)
                    })
                    .collect::<Vec<_>>();

                let next_handle = batch.last().map(|(k, _)| k.clone());

                (batch, next_handle)
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

// SQLite has no array parameters, so nodes are bound in chunks.
#[cfg(feature = "standalone")]
async fn flush_inserts<Hasher: WellBehavedHasher>(
    tx: &mut SqlxTransaction,
    inserts: &mut Vec<(ArenaHash<Hasher>, OnDiskObject<Hasher>)>,
) {
    for chunk in inserts.chunks(BATCH_INSERT_SIZE) {
        QueryBuilder::new("INSERT INTO ledger_db_nodes (key, object) ")
            .push_values(chunk, |mut q, (key, object)| {
                let ser_object = serialize_object(object);
                q.push_bind(key.0.as_slice()).push_bind(ser_object);
            })
            .push(" ON CONFLICT (key) DO UPDATE SET object = EXCLUDED.object")
            .build()
            .execute(&mut **tx)
            .await
            .unwrap_or_panic("cannot batch insert nodes");
    }

    inserts.clear();
}

// Postgres takes the keys and the nodes as two `bytea[]` parameters, so every statement has the same
// SQL whatever its number of nodes.
#[cfg(feature = "cloud")]
async fn flush_inserts<Hasher: WellBehavedHasher>(
    tx: &mut SqlxTransaction,
    inserts: &mut Vec<(ArenaHash<Hasher>, OnDiskObject<Hasher>)>,
) {
    let query = indoc! {"
        INSERT INTO ledger_db_nodes (key, object)
        SELECT * FROM UNNEST($1::bytea[], $2::bytea[])
        ON CONFLICT (key) DO UPDATE SET object = EXCLUDED.object
    "};

    for chunk in insert_chunks(inserts, |(_, object)| object.serialized_size()) {
        // Keeps one row per key, as an upsert fails when two of its rows share a key. Nodes are
        // content-addressed and, with storage-core's `layout-v2`, carry no reference count, so rows
        // sharing a key are identical.
        let mut unique = HashSet::with_capacity(chunk.len());
        let (keys, objects) = chunk
            .iter()
            .filter(|(key, _)| unique.insert(key.0.as_slice()))
            .map(|(key, object)| (key.0.as_slice(), serialize_object(object)))
            .unzip::<_, _, Vec<_>, Vec<_>>();

        sqlx::query(query)
            .bind(keys)
            .bind(objects)
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

async fn flush_root_counts<Hasher: WellBehavedHasher>(
    tx: &mut SqlxTransaction,
    root_counts: Vec<(ArenaHash<Hasher>, u32)>,
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
        .map(|(key, count)| (key.0.as_slice(), *count as i64))
        .collect::<Vec<_>>();
    upsert_root_counts(tx, &set).await;
}

// SQLite has no array parameters, so root counts are bound in chunks.
#[cfg(feature = "standalone")]
async fn upsert_root_counts(tx: &mut SqlxTransaction, root_counts: &[(&[u8], i64)]) {
    for chunk in root_counts.chunks(BATCH_INSERT_SIZE) {
        QueryBuilder::new("INSERT INTO ledger_db_roots (key, count) ")
            .push_values(chunk, |mut q, (key, count)| {
                q.push_bind(*key).push_bind(*count);
            })
            .push(" ON CONFLICT (key) DO UPDATE SET count = EXCLUDED.count")
            .build()
            .execute(&mut **tx)
            .await
            .unwrap_or_panic("cannot batch set root counts");
    }
}

// Postgres takes the keys and the counts as two array parameters, so every statement has the same
// SQL whatever its number of root counts.
#[cfg(feature = "cloud")]
async fn upsert_root_counts(tx: &mut SqlxTransaction, root_counts: &[(&[u8], i64)]) {
    let query = indoc! {"
        INSERT INTO ledger_db_roots (key, count)
        SELECT * FROM UNNEST($1::bytea[], $2::bigint[])
        ON CONFLICT (key) DO UPDATE SET count = EXCLUDED.count
    "};

    for chunk in root_counts.chunks(BATCH_ROWS) {
        let (keys, counts) = chunk.iter().copied().unzip::<_, _, Vec<_>, Vec<_>>();

        sqlx::query(query)
            .bind(keys)
            .bind(counts)
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

#[cfg(test)]
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

    // Sets then clears `ROOTS` root counts, each in one batch.
    let roots = (0..ROOTS as u32)
        .map(|index| {
            let mut bytes = [0; 32];
            bytes[..4].copy_from_slice(&index.to_be_bytes());
            arena_hash(bytes)
        })
        .collect::<Vec<_>>();
    db.batch_update(roots.iter().map(|root| (root.clone(), SetRootCount(1))));
    assert_eq!(db.get_roots().len(), ROOTS + 2);
    db.batch_update(roots.iter().map(|root| (root.clone(), SetRootCount(0))));
    assert_eq!(db.get_roots(), HashMap::from([(first, 5), (third, 7)]));
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

#[cfg(all(test, feature = "standalone"))]
pub(crate) mod sqlite_tests {
    use super::{
        SQLITE_MAX_BINDS, assert_batch_update_deletes_in_chunks_and_keeps_update_order,
        assert_batch_update_inserts_in_chunks, assert_batch_update_sets_root_counts,
        assert_get_root_count_reads_stored_count,
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
        BATCH_BYTES, BATCH_ROWS, assert_batch_update_deletes_in_chunks_and_keeps_update_order,
        assert_batch_update_inserts_in_chunks, assert_batch_update_sets_root_counts,
        assert_get_root_count_reads_stored_count, insert_chunks,
    };
    use crate::infra::{
        ledger_db::v1_1::LedgerDb,
        migrations::postgres::run as run_migrations,
        pool::{self, postgres::PostgresPool},
    };
    use anyhow::Context;
    use midnight_storage_core_v1::{DefaultHasher, Storage, arena::ArenaHash, db::DB};
    use sqlx::postgres::PgSslMode;
    use std::{error::Error as StdError, time::Duration};
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres;

    async fn setup_pool()
    -> Result<(testcontainers::ContainerAsync<Postgres>, PostgresPool), Box<dyn StdError>> {
        let container = Postgres::default()
            .with_db_name("indexer")
            .with_user("indexer")
            .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
            .with_tag("17.1-alpine")
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
