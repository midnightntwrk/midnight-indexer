use crate::{error::StdErrorExt, infra::pool::postgres::PostgresPool};
use derive_more::Debug;
use futures::TryStreamExt;
use indoc::indoc;
use midnight_serialize_v7_0_0::{Deserializable, Serializable};
use midnight_storage_v7_0_0::{
    DefaultHasher, Storage, WellBehavedHasher,
    arena::ArenaHash,
    backend::OnDiskObject,
    db::{DB, DummyArbitrary, Update},
    storage::set_default_storage,
};
use sqlx::QueryBuilder;
use std::{collections::HashMap, future::ready};
use tokio::{runtime::Handle, task::block_in_place};

type SqlxTransaction = sqlx::Transaction<'static, sqlx::Postgres>;

#[derive(Debug)]
pub struct PostgresDb {
    pool: PostgresPool,
}

impl PostgresDb {
    pub fn new(pool: PostgresPool) -> PostgresDb {
        Self { pool }
    }

    pub fn set_as_default_storage(self, cache_size: usize) {
        let _ = set_default_storage(|| Storage::new(cache_size, self));
    }
}

impl DB for PostgresDb {
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

                let query = indoc! {"
                    SELECT key, object
                    FROM ledger_db_nodes
                    WHERE key = ANY($1::bytea[])
                "};

                let mut nodes = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(query)
                    .bind(keys.iter().map(|key| key.0.to_vec()).collect::<Vec<_>>())
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
            })
        })
    }

    fn get_root_count(&self, key: &ArenaHash<Self::Hasher>) -> u32 {
        block_in_place(|| {
            Handle::current().block_on(async {
                let query = indoc! {"
                    SELECT count(1)
                    FROM ledger_db_roots
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

impl Default for PostgresDb {
    fn default() -> Self {
        panic!("PostgresDb cannot be constructed by default");
    }
}

impl DummyArbitrary for PostgresDb {}

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
        INSERT INTO ledger_db_nodes
        (key, object, ref_count)
    "};

    QueryBuilder::new(query)
        .push_values([()], |mut q, _| {
            q.push_bind(key.0.as_ref())
                .push_bind(ser_object.as_slice())
                .push_bind(object.ref_count as i64);
        })
        .build()
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
        .bind(key.0.iter().as_ref())
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
            .bind(key.0.iter().as_ref())
            .execute(&mut **tx)
            .await
            .unwrap_or_panic("cannot set root count");
    } else {
        let query = indoc! {"
            DELETE FROM ledger_db_roots
            WHERE key = $1
        "};

        sqlx::query(query)
            .bind(key.0.iter().as_ref())
            .execute(&mut **tx)
            .await
            .unwrap_or_panic("cannot set root count");
    }
}
