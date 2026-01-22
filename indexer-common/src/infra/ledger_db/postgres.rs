use crate::infra::pool::postgres::PostgresPool;
use derive_more::Debug;
use indoc::indoc;
use midnight_serialize_v7_0_0::{Deserializable, Serializable as SerializableV6};
use midnight_storage_v7_0_0::{
    DefaultHasher, Storage, WellBehavedHasher,
    arena::ArenaHash,
    backend::OnDiskObject,
    db::{DB, DummyArbitrary, Update},
    storage::set_default_storage,
};
use std::{collections::HashMap, marker::PhantomData};

#[derive(Debug)]
pub struct PostgresDb<H = DefaultHasher> {
    pool: PostgresPool,

    #[debug(skip)]
    _h: PhantomData<H>,
}

impl<H> PostgresDb<H>
where
    H: WellBehavedHasher,
{
    pub fn new(pool: PostgresPool) -> PostgresDb<H> {
        Self {
            pool,
            _h: PhantomData,
        }
    }

    pub fn set_as_default_storage(self, cache_size: usize) {
        let _ = set_default_storage(|| Storage::new(cache_size, self));
    }
}

impl<H> DB for PostgresDb<H>
where
    H: WellBehavedHasher,
{
    type Hasher = H;

    fn get_node(&self, key: &ArenaHash<Self::Hasher>) -> Option<OnDiskObject<Self::Hasher>> {
        let query = indoc! {"
            SELECT object
            FROM ledger_db_nodes
            WHERE key = $1
        "};

        sqlx::query_as::<_, (Vec<u8>,)>(query)
            .bind(key.0.as_ref())
            .await;

        todo!()
    }

    fn get_unreachable_keys(&self) -> Vec<ArenaHash<Self::Hasher>> {
        todo!()
    }

    fn insert_node(&mut self, key: ArenaHash<Self::Hasher>, object: OnDiskObject<Self::Hasher>) {
        todo!()
    }

    fn delete_node(&mut self, key: &ArenaHash<Self::Hasher>) {
        todo!()
    }

    fn batch_update<I>(&mut self, updates: I)
    where
        I: Iterator<Item = (ArenaHash<Self::Hasher>, Update<Self::Hasher>)>,
    {
        todo!()
    }

    fn batch_get_nodes<I>(
        &self,
        keys: I,
    ) -> Vec<(ArenaHash<Self::Hasher>, Option<OnDiskObject<Self::Hasher>>)>
    where
        I: Iterator<Item = ArenaHash<Self::Hasher>>,
    {
        todo!()
    }

    fn get_root_count(&self, key: &ArenaHash<Self::Hasher>) -> u32 {
        todo!()
    }

    fn set_root_count(&mut self, key: ArenaHash<Self::Hasher>, count: u32) {
        todo!()
    }

    fn get_roots(&self) -> HashMap<ArenaHash<Self::Hasher>, u32> {
        todo!()
    }

    fn size(&self) -> usize {
        todo!()
    }
}

impl<H> Default for PostgresDb<H> {
    fn default() -> Self {
        panic!("PostgresDb cannot be constructed by default");
    }
}

impl<H> DummyArbitrary for PostgresDb<H> {}

trait ResultExt<T> {
    fn unwrap_or_panic(self, msg: &'static str) -> T;
}

impl<T, E> ResultExt<T> for Result<T, E>
where
    E: std::error::Error,
{
    fn unwrap_or_panic(self, msg: &'static str) -> T {
        self.unwrap_or_else(|error| panic!("{msg}: {error}"))
    }
}
