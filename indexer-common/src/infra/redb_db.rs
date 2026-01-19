use derive_more::Debug;
use midnight_serialize_v6::{Deserializable, Serializable as SerializableV6};
use midnight_storage_v6::{
    DefaultHasher, Storage, WellBehavedHasher,
    arena::ArenaHash,
    backend::OnDiskObject,
    db::{DB, DummyArbitrary, Update},
    storage::set_default_storage,
};
use redb::{ReadableDatabase, ReadableTable, TableDefinition, WriteTransaction};
use sha2::digest::generic_array::GenericArray;
use std::{collections::HashMap, marker::PhantomData, path::Path};
use tempfile::TempDir;

const NODES: TableDefinition<&[u8], Vec<u8>> = TableDefinition::new("nodes");
const REF_COUNT_ZERO: TableDefinition<&[u8], ()> = TableDefinition::new("ref-count-zero");
const ROOT_COUNT: TableDefinition<&[u8], u32> = TableDefinition::new("root-count");

macro_rules! open_nodes_table {
    ($tx:ident) => {
        (&$tx)
            .open_table(NODES)
            .unwrap_or_panic("cannot open nodes table")
    };
}

macro_rules! open_ref_count_zero_table {
    ($tx:ident) => {
        (&$tx)
            .open_table(REF_COUNT_ZERO)
            .unwrap_or_panic("cannot open ref_count_zero table")
    };
}

macro_rules! open_root_count_table {
    ($tx:ident) => {
        (&$tx)
            .open_table(ROOT_COUNT)
            .unwrap_or_panic("cannot open root-count table")
    };
}

#[derive(Debug)]
pub struct RedbDb<H = DefaultHasher> {
    #[debug(skip)]
    inner: redb::Database,

    #[debug(skip)]
    _h: PhantomData<H>,
}

impl<H> RedbDb<H>
where
    H: WellBehavedHasher,
{
    pub fn new(file: impl AsRef<Path>) -> RedbDb<H> {
        let inner = redb::Database::create(file).unwrap_or_panic("cannot open redb");

        let tx = inner.begin_write().unwrap_or_panic("cannot begin write tx");

        open_nodes_table!(tx);
        open_ref_count_zero_table!(tx);
        open_root_count_table!(tx);

        tx.commit().unwrap_or_panic("cannot commit write tx");

        Self {
            inner,
            _h: PhantomData,
        }
    }

    pub fn set_as_default_storage(self) {
        set_default_storage(|| Storage::new(1_024, self))
            .expect("RedbDb can be set as default storage");
    }
}

impl<H> DB for RedbDb<H>
where
    H: WellBehavedHasher,
{
    type Hasher = H;

    fn get_node(&self, key: &ArenaHash<Self::Hasher>) -> Option<OnDiskObject<Self::Hasher>> {
        let tx = self
            .inner
            .begin_read()
            .unwrap_or_panic("cannot begin read tx for get_node");

        open_nodes_table!(tx)
            .get(key.0.as_ref())
            .unwrap_or_panic("cannot read from nodes table")
            .map(|object| {
                OnDiskObject::<Self::Hasher>::deserialize(&mut object.value().as_slice(), 0)
                    .unwrap_or_panic("cannot deserialize as OnDiskObject")
            })
    }

    fn get_unreachable_keys(&self) -> Vec<ArenaHash<Self::Hasher>> {
        let tx = self
            .inner
            .begin_read()
            .unwrap_or_panic("cannot begin read tx for get_unreachable_keys");

        let root_count_table = open_root_count_table!(tx);

        open_ref_count_zero_table!(tx)
            .iter()
            .unwrap_or_panic("cannot iterate ref-count-zero table")
            .filter_map(|entry| {
                let (key, _) =
                    entry.unwrap_or_panic("cannot get next entry of ref-count-zero table");
                let key = ArenaHash::<H>(GenericArray::from_iter(key.value().iter().copied()));

                let root_count = root_count_table
                    .get(key.0.as_ref())
                    .unwrap_or_panic("cannot read from root-count table")
                    .map(|count| count.value())
                    .unwrap_or_default();

                (root_count == 0).then_some(key)
            })
            .collect()
    }

    fn insert_node(&mut self, key: ArenaHash<Self::Hasher>, object: OnDiskObject<Self::Hasher>) {
        let tx = self
            .inner
            .begin_write()
            .unwrap_or_panic("cannot begin write tx for insert_node");

        insert_node(&tx, key, object);

        tx.commit()
            .unwrap_or_panic("cannot commit write tx for insert_node");
    }

    fn delete_node(&mut self, key: &ArenaHash<Self::Hasher>) {
        let tx = self
            .inner
            .begin_write()
            .unwrap_or_panic("cannot begin write tx for delete_node");

        delete_node(&tx, key);

        tx.commit()
            .unwrap_or_panic("cannot commit write tx for delete_node");
    }

    fn batch_update<I>(&mut self, updates: I)
    where
        I: Iterator<Item = (ArenaHash<Self::Hasher>, Update<Self::Hasher>)>,
    {
        let tx = self
            .inner
            .begin_write()
            .unwrap_or_panic("cannot begin write tx for batch_update");

        for (key, update) in updates {
            match update {
                Update::InsertNode(object) => insert_node(&tx, key, object),
                Update::DeleteNode => delete_node(&tx, &key),
                Update::SetRootCount(count) => set_root_count(&tx, key, count),
            }
        }

        tx.commit()
            .unwrap_or_panic("cannot commit write tx for batch_update");
    }

    fn batch_get_nodes<I>(
        &self,
        keys: I,
    ) -> Vec<(ArenaHash<Self::Hasher>, Option<OnDiskObject<Self::Hasher>>)>
    where
        I: Iterator<Item = ArenaHash<Self::Hasher>>,
    {
        let tx = self
            .inner
            .begin_read()
            .unwrap_or_panic("cannot begin read tx for batch_get_nodes");

        let nodes_table = open_nodes_table!(tx);

        keys.map(|key| {
            let object = nodes_table
                .get(key.0.as_ref())
                .unwrap_or_panic("cannot read from nodes table")
                .map(|object| {
                    OnDiskObject::<Self::Hasher>::deserialize(&mut object.value().as_slice(), 0)
                        .unwrap_or_panic("cannot deserialize as OnDiskObject")
                });

            (key, object)
        })
        .collect()
    }

    fn get_root_count(&self, key: &ArenaHash<Self::Hasher>) -> u32 {
        let tx = self
            .inner
            .begin_read()
            .unwrap_or_panic("cannot begin read tx for get_root_count");

        open_root_count_table!(tx)
            .get(key.0.as_ref())
            .unwrap_or_panic("cannot read from root-count table")
            .map(|count| count.value())
            .unwrap_or_default()
    }

    fn set_root_count(&mut self, key: ArenaHash<Self::Hasher>, count: u32) {
        let tx = self
            .inner
            .begin_write()
            .unwrap_or_panic("cannot begin write tx for set_root_count");

        set_root_count(&tx, key, count);

        tx.commit()
            .unwrap_or_panic("cannot commit write tx for set_root_count");
    }

    fn get_roots(&self) -> HashMap<ArenaHash<Self::Hasher>, u32> {
        let tx = self
            .inner
            .begin_read()
            .unwrap_or_panic("cannot begin read tx for get_roots");

        open_root_count_table!(tx)
            .iter()
            .unwrap_or_panic("cannot iterate root-count table")
            .map(|entry| {
                let (key, count) =
                    entry.unwrap_or_panic("cannot get next entry of root-count table");
                let key = ArenaHash::<H>(GenericArray::from_iter(key.value().iter().copied()));

                (key, count.value())
            })
            .collect()
    }

    fn size(&self) -> usize {
        let tx = self
            .inner
            .begin_read()
            .unwrap_or_panic("cannot begin read tx for size");

        open_nodes_table!(tx)
            .iter()
            .unwrap_or_panic("cannot iterate nodes table")
            .map(|entry| {
                entry.unwrap_or_panic("cannot get next entry of nodes table");
            })
            .count()
    }
}

impl<H> Default for RedbDb<H>
where
    H: WellBehavedHasher,
{
    fn default() -> Self {
        let dir = TempDir::new()
            .unwrap_or_panic("cannot create tempdir")
            .keep();

        Self::new(&dir)
    }
}

impl<H> DummyArbitrary for RedbDb<H> {}

pub fn new_redb_db(file: impl AsRef<Path>) -> RedbDb<DefaultHasher> {
    RedbDb::<DefaultHasher>::new(file)
}

fn insert_node<H>(tx: &WriteTransaction, key: ArenaHash<H>, object: OnDiskObject<H>)
where
    H: WellBehavedHasher,
{
    let mut serialized_object = Vec::with_capacity(object.serialized_size());
    SerializableV6::serialize(&object, &mut serialized_object)
        .unwrap_or_panic("cannot serialize object");

    open_nodes_table!(tx)
        .insert(key.0.as_ref(), serialized_object)
        .unwrap_or_panic("cannot insert key {key:?} into nodes table");

    let mut ref_count_zero = open_ref_count_zero_table!(tx);
    if object.ref_count == 0 {
        ref_count_zero
            .insert(key.0.as_ref(), ())
            .unwrap_or_panic("cannot insert key {key:?} into ref-count-zero table");
    } else {
        ref_count_zero
            .remove(key.0.as_ref())
            .unwrap_or_panic("cannot remove key {key:?} from ref-count-zero table");
    }
}

fn delete_node<H>(tx: &WriteTransaction, key: &ArenaHash<H>)
where
    H: WellBehavedHasher,
{
    open_nodes_table!(tx)
        .remove(key.0.as_ref())
        .unwrap_or_panic("cannot remove key {key:?} from nodes table");

    open_ref_count_zero_table!(tx)
        .remove(key.0.as_ref())
        .unwrap_or_panic("cannot remove key {key:?} from ref-count-zero table");
}

fn set_root_count<H>(tx: &WriteTransaction, key: ArenaHash<H>, count: u32)
where
    H: WellBehavedHasher,
{
    open_root_count_table!(tx)
        .insert(key.0.as_ref(), count)
        .unwrap_or_panic("cannot insert key {key:?} into root-count table");
}

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
