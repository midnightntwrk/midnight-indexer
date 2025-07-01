use crate::{
    domain::{self, storage::Storage},
    infra::api::{AsBytesExt, HexEncoded, v1::transaction::Transaction},
};
use async_graphql::{SimpleObject, Union};
use derive_more::Debug;

/// A wallet synchronization event, either a viewing update or a progress update.
#[derive(Debug, Union)]
pub enum WalletSyncEvent<S: Storage> {
    ViewingUpdate(ViewingUpdate<S>),
    ProgressUpdate(WalletProgressUpdate),
}

/// Aggregates a relevant transaction with the next start index and an optional collapsed
/// Merkle-Tree update.
#[derive(Debug, SimpleObject)]
pub struct ViewingUpdate<S: Storage> {
    /// Next start index into the zswap state to be queried. Usually the end index of the included
    /// relevant transaction plus one unless that is a failure in which case just its end
    /// index.
    pub index: u64,

    /// Relevant transaction for the wallet and maybe a collapsed Merkle-Tree update.
    pub update: Vec<ZswapChainStateUpdate<S>>,
}

/// Aggregates information about the wallet indexing progress.
#[derive(Debug, SimpleObject)]
pub struct WalletProgressUpdate {
    /// The highest end index into the zswap state of all currently known transactions.
    pub highest_index: u64,

    /// The highest end index into the zswap state of all currently known relevant transactions,
    /// i.e. those that belong to any known wallet. Less or equal `highest_index`.
    pub highest_relevant_index: u64,

    /// The highest end index into the zswap state of all currently known relevant transactions for
    /// a particular wallet. Less or equal `highest_relevant_index`.
    pub highest_relevant_wallet_index: u64,
}

#[derive(Debug, Union)]
#[allow(clippy::large_enum_variant)]
pub enum ZswapChainStateUpdate<S: Storage> {
    MerkleTreeCollapsedUpdate(MerkleTreeCollapsedUpdate),
    RelevantTransaction(RelevantTransaction<S>),
}

#[derive(Debug, SimpleObject)]
pub struct MerkleTreeCollapsedUpdate {
    /// The start index into the zswap state.
    start: u64,

    /// The end index into the zswap state.
    end: u64,

    /// The hex-encoded merkle-tree collapsed update.
    #[debug(skip)]
    update: HexEncoded,

    /// The protocol version.
    protocol_version: u32,
}

impl From<domain::MerkleTreeCollapsedUpdate> for MerkleTreeCollapsedUpdate {
    fn from(value: domain::MerkleTreeCollapsedUpdate) -> Self {
        let domain::MerkleTreeCollapsedUpdate {
            start_index,
            end_index,
            update,
            protocol_version,
        } = value;

        Self {
            start: start_index,
            end: end_index,
            update: update.hex_encode(),
            protocol_version: protocol_version.0,
        }
    }
}

#[derive(Debug, SimpleObject)]
pub struct RelevantTransaction<S: Storage> {
    /// Relevant transaction for the wallet.
    transaction: Transaction<S>,

    /// The start index.
    start: u64,

    /// The end index.
    end: u64,
}

impl<S> From<domain::Transaction> for RelevantTransaction<S>
where
    S: Storage,
{
    fn from(value: domain::Transaction) -> Self {
        Self {
            start: value.start_index,
            end: value.end_index,
            transaction: value.into(),
        }
    }
}
