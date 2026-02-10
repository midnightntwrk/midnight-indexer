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

use crate::error::StdErrorExt;
use derive_more::Debug;
use futures::TryStreamExt;
use indoc::indoc;
use midnight_serialize_v7::{Deserializable, Serializable};
use midnight_storage_v7::{
    DefaultHasher, WellBehavedHasher,
    arena::ArenaHash,
    backend::OnDiskObject,
    db::{DB, DummyArbitrary, Update},
};
use std::{collections::HashMap, future::ready};
use tokio::{runtime::Handle, task::block_in_place};

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
}

impl DB for LedgerDb {
    type Hasher = DefaultHasher;

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

    fn get_unreachable_keys(&self) -> Vec<ArenaHash<Self::Hasher>> {
        block_in_place(|| {
            Handle::current().block_on(async {
                let query = indoc! {"
                    SELECT key
                    FROM ledger_db_nodes
                    WHERE key NOT IN (SELECT key FROM ledger_db_roots)
                    AND ref_count = 0
                "};

                sqlx::query_as::<_, (Vec<u8>,)>(query)
                    .fetch(&*self.pool)
                    .and_then(|(key,)| {
                        let key = ArenaHash::<Self::Hasher>::deserialize(&mut key.as_slice(), 0)
                            .map_err(|error| sqlx::Error::Decode(error.into()));
                        ready(key)
                    })
                    .try_collect::<Vec<_>>()
                    .await
                    .unwrap_or_panic("cannot get unreachable keys")
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

                insert_node(&mut tx, key, object).await;

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

                for (key, update) in updates {
                    match update {
                        Update::InsertNode(object) => insert_node(&mut tx, key, object).await,
                        Update::DeleteNode => delete_node(&mut tx, &key).await,
                        Update::SetRootCount(count) => set_root_count(&mut tx, key, count).await,
                    }
                }

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
                let query = indoc! {"
                    SELECT count(1)
                    FROM ledger_db_roots
                    WHERE key = $1
                "};

                let (count,) = sqlx::query_as::<_, (i64,)>(query)
                    .bind(key.0.as_slice())
                    .fetch_one(&*self.pool)
                    .await
                    .unwrap_or_panic("cannot get root count");

                count as u32
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

async fn insert_node<H>(tx: &mut SqlxTransaction, key: ArenaHash<H>, object: OnDiskObject<H>)
where
    H: WellBehavedHasher,
{
    let mut ser_object = Vec::with_capacity(object.serialized_size());
    Serializable::serialize(&object, &mut ser_object).unwrap_or_panic("cannot serialize object");

    let query = indoc! {"
        INSERT INTO ledger_db_nodes (
            key,
            object,
            ref_count
        )
        VALUES (
            $1,
            $2,
            $3
        )
        ON CONFLICT (key) DO UPDATE
        SET
            object = EXCLUDED.object,
            ref_count = EXCLUDED.ref_count
    "};

    sqlx::query(query)
        .bind(key.0.as_slice())
        .bind(ser_object.as_slice())
        .bind(object.ref_count as i64)
        .execute(&mut **tx)
        .await
        .unwrap_or_panic("cannot insert node");
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
