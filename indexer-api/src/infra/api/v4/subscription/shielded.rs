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

use super::polling::{jittered, jittered_interval, latest_protocol_version, next_poll_interval};
use crate::{
    domain::{self, LedgerStateCache, storage::Storage},
    infra::api::{
        ApiError, ApiResult, ContextExt, OptionExt, ResultExt,
        ledger_query_limit::LedgerQueryLimiter,
        v4::{
            merkle_tree_collapsed_update::{CollapsedMerkleTree, MerkleTreeCollapsedUpdate},
            session_id::SessionId,
            transaction::RegularTransaction,
        },
    },
};
use async_graphql::{Context, SimpleObject, Subscription, Union};
use async_stream::try_stream;
use derive_more::Debug;

use drop_stream::DropStreamExt;
use fastrace::{Span, future::FutureExt, prelude::SpanContext, trace};
use futures::{
    Stream, StreamExt,
    future::ok,
    stream::{self, TryStreamExt},
};
use indexer_common::domain::{Subscriber, WalletIndexed};
use log::{debug, warn};
use std::{future::ready, marker::PhantomData, num::NonZeroU32, pin::pin};
use stream_cancel::{StreamExt as _, Trigger, Tripwire};
use tokio::time::sleep;
use uuid::Uuid;

/// An event of the shielded transactions subscription.
#[derive(Debug, Union)]
enum ShieldedTransactionsEvent<S: Storage> {
    // Boxing RelevantTransaction to reduce variant size (clippy warning).
    RelevantTransaction(Box<RelevantTransaction<S>>),
    ShieldedTransactionsProgress(ShieldedTransactionsProgress),
}

/// A transaction relevant for the subscribing wallet and an optional zswap state Merkle tree
/// collapsed update.
#[derive(Debug, SimpleObject)]
struct RelevantTransaction<S>
where
    S: Storage,
{
    /// A transaction relevant for the subscribing wallet.
    transaction: RegularTransaction<S>,

    /// Only include a zswap state Merkle tree collapsed update if there is a gap between the
    /// current zswap index "driving" the subscription and the zswap start index of the
    /// transaction.
    zswap_collapsed_update: Option<MerkleTreeCollapsedUpdate>,

    /// An optional collapsed Merkle tree.
    #[graphql(deprecation = "Use zswapCollapsedUpdate instead")]
    collapsed_merkle_tree: Option<CollapsedMerkleTree>,
}

/// Information about the shielded transactions indexing progress.
#[derive(Debug, SimpleObject)]
struct ShieldedTransactionsProgress {
    /// The highest end index into the zswap state for all transactions. It represents the known
    /// state of the blockchain. A value of zero (completely unlikely) means that no shielded
    /// transactions have been indexed yet.
    highest_zswap_end_index: u64,

    /// The highest end index into the zswap state for all transactions. It represents the known
    /// state of the blockchain. A value of zero (completely unlikely) means that no shielded
    /// transactions have been indexed yet.
    #[graphql(deprecation = "Use highestZswapEndIndex instead")]
    highest_end_index: u64,

    /// The highest highest end index into the zswap state for all transactions checked for
    /// relevance. Initially less than and eventually (when some wallet has been fully indexed)
    /// equal to `highest_end_index`. A value of zero (very unlikely) means that no wallet
    /// has subscribed before and indexing for the subscribing wallet has not yet started.
    highest_checked_zswap_end_index: u64,

    /// The highest highest end index into the zswap state for all transactions checked for
    /// relevance. Initially less than and eventually (when some wallet has been fully indexed)
    /// equal to `highest_end_index`. A value of zero (very unlikely) means that no wallet
    /// has subscribed before and indexing for the subscribing wallet has not yet started.
    #[graphql(deprecation = "Use highestCheckedZswapEndIndex instead")]
    highest_checked_end_index: u64,

    /// The highest highest end index into the zswap state for all relevant transactions for the
    /// subscribing wallet. Usually less than `highest_checked_end_index` unless the latest
    /// checked transaction is relevant for the subscribing wallet. A value of zero means that
    /// no relevant transactions have been indexed for the subscribing wallet.
    highest_relevant_zswap_end_index: u64,

    /// The highest highest end index into the zswap state for all relevant transactions for the
    /// subscribing wallet. Usually less than `highest_checked_end_index` unless the latest
    /// checked transaction is relevant for the subscribing wallet. A value of zero means that
    /// no relevant transactions have been indexed for the subscribing wallet.
    #[graphql(deprecation = "Use highestRelevantZswapEndIndex instead")]
    highest_relevant_end_index: u64,

    /// The protocol version at the tip of the chain, i.e. the one currently in effect, not the
    /// one the transactions of this subscription were indexed at. Allows a wallet without any
    /// relevant transactions to observe a protocol upgrade, which it would otherwise only learn
    /// about from the transactions it receives. A value of zero means that no block has been
    /// indexed yet.
    protocol_version: u32,
}

pub struct ShieldedTransactionsSubscription<S, B> {
    _s: PhantomData<S>,
    _b: PhantomData<B>,
}

impl<S, B> Default for ShieldedTransactionsSubscription<S, B> {
    fn default() -> Self {
        Self {
            _s: PhantomData,
            _b: PhantomData,
        }
    }
}

#[Subscription]
impl<S, B> ShieldedTransactionsSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribe to shielded transaction events for the given session ID starting at the given
    /// index or at zero if omitted.
    async fn shielded_transactions<'a>(
        &self,
        cx: &'a Context<'a>,
        #[graphql(secret)] session_id: SessionId,
        index: Option<u64>,
    ) -> Result<impl Stream<Item = ApiResult<ShieldedTransactionsEvent<S>>> + use<'a, S, B>, ApiError>
    {
        cx.get_metrics().wallets_connected.increment(1);

        let session_id = session_id
            .try_into()
            .map_err_into_client_error(|| "invalid session ID")?;

        let wallet_id = cx
            .get_storage::<S>()
            .resolve_session_id(&session_id)
            .await
            .map_err_into_server_error(|| "resolve session ID")?
            .some_or_client_error(|| "unknown or expired session ID")?;

        // Only acquire a per-session quota bucket for a known session, so that arbitrary session
        // IDs cannot create buckets. The bucket is keyed by wallet ID, so reconnecting with a new
        // session does not reset it.
        let quota_guard = cx
            .get_subscription_quotas()
            .try_acquire(cx.get_per_connection_counter(), Some(wallet_id))
            .map_err_into_client_error(|| "subscription limit exceeded")?;

        let index = index.unwrap_or_default();

        // Build a stream of shielded transaction events by merging relevant transactions and
        // progress items. The relevant transactions stream should be infinite by definition (see
        // the trait). However, if it nevertheless completes, we use a tripwire to ensure the
        // progress stream also completes, preventing the merged stream from hanging indefinitely
        // waiting for both streams to complete.
        let (trigger, tripwire) = Tripwire::new();

        let relevant_transactions = make_relevant_transactions::<S, B>(
            cx, wallet_id, index, trigger,
        )
        .map_ok(|relevant_transaction| {
            ShieldedTransactionsEvent::RelevantTransaction(relevant_transaction.into())
        });

        let progress = make_progress::<S>(cx, wallet_id)
            .take_until_if(tripwire)
            .map_ok(ShieldedTransactionsEvent::ShieldedTransactionsProgress)
            .boxed();

        let events = tokio_stream::StreamExt::merge(relevant_transactions, progress);

        // As long as the subscription is alive, the wallet is periodically kept active, even if
        // there are no new transactions.
        let storage = cx.get_storage::<S>();
        let keep_wallet_alive_interval = cx
            .get_subscription_config()
            .shielded_transactions
            .keep_wallet_alive_interval;
        let keep_wallet_active = jittered_interval(keep_wallet_alive_interval)
            .then(move |_| async move { storage.keep_wallet_active(wallet_id).await })
            .map_err(|error| ApiError::server("keep wallet active", error));
        let events = stream::select(events.map_ok(Some), keep_wallet_active.map_ok(|_| None))
            .try_filter_map(ok)
            .on_drop(move || {
                cx.get_metrics().wallets_connected.decrement(1);
                drop(quota_guard);
                debug!(wallet_id:%; "shielded transaction subscription ended");
            });

        Ok(events)
    }
}

fn make_relevant_transactions<'a, S, B>(
    cx: &'a Context<'a>,
    wallet_id: Uuid,
    index: u64,
    trigger: Trigger,
) -> impl Stream<Item = ApiResult<RelevantTransaction<S>>> + use<'a, S, B>
where
    S: Storage,
    B: Subscriber,
{
    let storage = cx.get_storage::<S>();
    let subscriber = cx.get_subscriber::<B>();
    let ledger_state_cache = cx.get_ledger_state_cache();
    let ledger_query_limiter = cx.get_ledger_query_limiter();
    let batch_size = cx
        .get_subscription_config()
        .shielded_transactions
        .batch_size;

    try_stream! {
        // Subscribe before reading existing items, so none indexed in between is missed.
        let wallet_indexed_events = subscriber
            .subscribe::<WalletIndexed>()
            .await
            .try_filter(move |wallet_indexed| ready(wallet_indexed.wallet_id == wallet_id));

        // Stream exiting transactions.
        debug!(wallet_id:%, index; "streaming existing transactions");

        let mut cursor = Cursor::new(index);

        let transactions = cursor.transactions(storage, wallet_id, batch_size);
        let mut transactions = pin!(transactions);
        while let Some(transaction) = get_next_transaction(&mut transactions)
            .await
            .map_err_into_server_error(|| "get next transaction")?
        {
            let index = cursor.index;
            cursor = cursor.after(&transaction);

            yield make_relevant_transaction(
                index,
                transaction,
                storage,
                ledger_state_cache,
                ledger_query_limiter,
            )
            .await?;
        }

        // Stream live transactions.
        debug!(wallet_id:%, cursor:?; "streaming live transactions");
        let mut wallet_indexed_events = pin!(wallet_indexed_events);
        while wallet_indexed_events
            .try_next()
            .await
            .map_err_into_server_error(|| "get next WalletIndexed event")?
            .is_some()
        {
            debug!(cursor:?; "streaming next live transactions");

            let transactions = cursor.transactions(storage, wallet_id, batch_size);
            let mut transactions = pin!(transactions);
            while let Some(transaction) =  get_next_transaction(&mut transactions)
                .await
                .map_err_into_server_error(|| "get next transaction")?
            {
                let index = cursor.index;
                cursor = cursor.after(&transaction);

                yield make_relevant_transaction(
                    index,
                    transaction,
                    storage,
                    ledger_state_cache,
                    ledger_query_limiter,
                )
                .await?;
            }
        }

        warn!("stream of WalletIndexed events completed unexpectedly");
        trigger.cancel();
    }
}

/// Where the relevant transactions of a shielded transactions subscription continue.
#[derive(Debug, Clone, Copy)]
struct Cursor {
    /// The zswap index up to which the client has the zswap state Merkle tree.
    index: u64,
    /// The lowest ID of the transactions still to stream, i.e. one past the last streamed
    /// transaction: re-queries resume there rather than at the zswap index, which does not advance
    /// for a transaction without commitments.
    from_transaction_id: u64,
}

impl Cursor {
    fn new(index: u64) -> Self {
        Self {
            index,
            from_transaction_id: 0,
        }
    }

    /// The cursor after streaming the given transaction.
    fn after(self, transaction: &domain::RegularTransaction) -> Self {
        // The end index is "exclusive", i.e. the next free index; hence no +1 here!
        Self {
            index: transaction.zswap_end_index,
            from_transaction_id: transaction.id + 1,
        }
    }

    /// The relevant transactions for the given wallet which follow this cursor.
    fn transactions<S: Storage>(
        self,
        storage: &S,
        wallet_id: Uuid,
        batch_size: NonZeroU32,
    ) -> impl Stream<Item = Result<domain::RegularTransaction, sqlx::Error>> + Send + use<'_, S>
    {
        storage.get_relevant_transactions(
            wallet_id,
            self.index,
            self.from_transaction_id,
            batch_size,
        )
    }
}

#[trace(properties = { "index": "{index:?}" })]
async fn make_relevant_transaction<S>(
    index: u64,
    transaction: domain::RegularTransaction,
    storage: &S,
    ledger_state_cache: &LedgerStateCache,
    ledger_query_limiter: &LedgerQueryLimiter,
) -> ApiResult<RelevantTransaction<S>>
where
    S: Storage,
{
    debug!(index, transaction:?; "making relevant transaction");

    // Only include a zswap state Merkle tree collapsed update if there is a gap between the queried
    // index and the start index of the transaction.
    let zswap_collapsed_update = if index < transaction.zswap_start_index {
        // Bound concurrent ledger-DB work (issue #595): hold a permit only while building the
        // Merkle-tree update, so it is released before this transaction is yielded to the client.
        let _ledger_permit = ledger_query_limiter.acquire().await;

        // Read the highest ledger state after acquiring the permit: the wait for a permit is
        // unbounded, so a key read before the wait may be garbage collected by the time the ledger
        // state is loaded.
        let (height, protocol_version, key) = storage
            .get_highest_ledger_state()
            .await
            .map_err_into_server_error(|| "get highest ledger state")?
            .some_or_server_error(|| "no ledger state available")?;

        let zswap_collapsed_update = ledger_state_cache
            .at_least(height, &key, protocol_version)
            .await
            .map(|(ledger_state, _)| ledger_state)
            .map_err_into_server_error(|| "load ledger state")?
            .make_zswap_collapsed_update(
                index,
                transaction.zswap_start_index - 1,
                transaction.protocol_version,
            )
            .map_err_into_server_error(|| "create zswap state Merkle tree collapsed update")?;
        Some(MerkleTreeCollapsedUpdate::from(zswap_collapsed_update))
    } else {
        None
    };

    let collapsed_merkle_tree = zswap_collapsed_update.as_ref().map(|u| u.to_owned().into());

    let relevant_transaction = RelevantTransaction {
        transaction: transaction.into(),
        zswap_collapsed_update,
        collapsed_merkle_tree,
    };

    debug!(relevant_transaction:?; "made relevant transaction");

    Ok(relevant_transaction)
}

fn make_progress<'a, S>(
    cx: &'a Context<'a>,
    wallet_id: Uuid,
) -> impl Stream<Item = ApiResult<ShieldedTransactionsProgress>> + use<'a, S>
where
    S: Storage,
{
    let storage = cx.get_storage::<S>();
    let progress_cache = cx.get_progress_cache();
    let base = cx
        .get_subscription_config()
        .shielded_transactions
        .progress_update_interval;

    // Emit progress immediately, then re-poll after a jittered interval that
    // backs off while neither the indices nor the protocol version have changed (idle) and
    // resets when either moves. A protocol upgrade counts as movement, so a subscription that
    // was idle through a fork returns to the base interval instead of staying backed off. The
    // cache lets concurrent subscribers for the same wallet share one query.
    let mut current_interval = base;
    let mut last_progress = None;
    try_stream! {
        loop {
            let indices = progress_cache
                .shielded_indices(wallet_id, async {
                    storage
                        .get_highest_zswap_end_indices(wallet_id)
                        .await
                        .map_err_into_server_error(|| "get highest indices")
                })
                .await?;
            let protocol_version = latest_protocol_version::<S>(cx).await?;
            let progress = (indices, protocol_version);
            let changed = last_progress.as_ref() != Some(&progress);
            current_interval = next_poll_interval(current_interval, base, changed);
            last_progress = Some(progress);
            yield to_progress(indices, protocol_version);
            sleep(jittered(current_interval)).await;
        }
    }
}

fn to_progress(
    (highest, highest_checked, highest_relevant): (Option<u64>, Option<u64>, Option<u64>),
    protocol_version: u32,
) -> ShieldedTransactionsProgress {
    let highest_zswap_end_index = highest.unwrap_or_default();
    let highest_checked_zswap_end_index = highest_checked.unwrap_or_default();
    let highest_relevant_zswap_end_index = highest_relevant.unwrap_or_default();

    ShieldedTransactionsProgress {
        highest_zswap_end_index,
        highest_end_index: highest_zswap_end_index,
        highest_checked_zswap_end_index,
        highest_checked_end_index: highest_checked_zswap_end_index,
        highest_relevant_zswap_end_index,
        highest_relevant_end_index: highest_relevant_zswap_end_index,
        protocol_version,
    }
}

async fn get_next_transaction<E>(
    transactions: &mut (impl Stream<Item = Result<domain::RegularTransaction, E>> + Unpin),
) -> Result<Option<domain::RegularTransaction>, E> {
    transactions
        .try_next()
        .in_span(Span::root(
            "subscription.shielded-transactions.get-next-transaction",
            SpanContext::random(),
        ))
        .await
}

#[cfg(all(test, feature = "standalone"))]
mod tests {
    use crate::infra::{api::v4::subscription::shielded::Cursor, storage::Storage};
    use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit};
    use futures::{StreamExt, TryStreamExt};
    use indexer_common::infra::{
        migrations,
        pool::sqlite::{Config, SqlitePool},
    };
    use indoc::indoc;
    use sqlx::types::{Uuid, time::OffsetDateTime};
    use std::{error::Error as StdError, num::NonZeroU32, time::Duration};
    use tokio::time::timeout;

    /// Relevant transactions which add no zswap commitment, i.e. have equal zswap start and end
    /// indices, must each be streamed once, also when one is the last of a batch or a whole batch
    /// shares one start index.
    #[tokio::test]
    async fn existing_transactions_without_commitments_are_streamed_once()
    -> Result<(), Box<dyn StdError>> {
        let (storage, pool) = new_storage().await?;
        let wallet_id = Uuid::now_v7();
        seed(&pool, wallet_id).await?;
        for (id, start, end) in [(1, 2, 5), (2, 5, 5), (3, 5, 5), (4, 5, 7)] {
            seed_relevant_transaction(&pool, wallet_id, id, start, end).await?;
        }

        for batch_size in [NonZeroU32::MIN, NonZeroU32::MIN.saturating_add(1)] {
            let transactions = Cursor::new(5)
                .transactions(&storage, wallet_id, batch_size)
                // Take one more than expected so a repeating stream still terminates.
                .take(4)
                .try_collect::<Vec<_>>();
            let transactions = timeout(Duration::from_secs(10), transactions).await??;

            let ids = transactions
                .iter()
                .map(|transaction| transaction.id)
                .collect::<Vec<_>>();
            assert_eq!(
                ids,
                vec![2, 3, 4],
                "each transaction from index 5 once, batch size {batch_size}"
            );
        }

        Ok(())
    }

    /// A live re-query after a relevant transaction which adds no zswap commitment must not return
    /// that transaction again, but only the relevant transactions indexed since.
    #[tokio::test]
    async fn live_requery_resumes_after_last_streamed_transaction() -> Result<(), Box<dyn StdError>>
    {
        let (storage, pool) = new_storage().await?;
        let wallet_id = Uuid::now_v7();
        seed(&pool, wallet_id).await?;
        seed_relevant_transaction(&pool, wallet_id, 1, 5, 5).await?;

        let batch_size = NonZeroU32::MIN;
        let cursor = Cursor::new(0);

        // Existing transactions.
        let transactions = cursor
            .transactions(&storage, wallet_id, batch_size)
            .take(1)
            .try_collect::<Vec<_>>();
        let transactions = timeout(Duration::from_secs(10), transactions).await??;
        let cursor = transactions
            .iter()
            .fold(cursor, |cursor, transaction| cursor.after(transaction));

        // A WalletIndexed event for a new relevant transaction triggers a live re-query.
        seed_relevant_transaction(&pool, wallet_id, 2, 5, 7).await?;
        let transactions = cursor
            .transactions(&storage, wallet_id, batch_size)
            // Take one more than expected so a repeating stream still terminates.
            .take(2)
            .try_collect::<Vec<_>>();
        let transactions = timeout(Duration::from_secs(10), transactions).await??;

        let ids = transactions
            .iter()
            .map(|transaction| transaction.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![2], "only the newly indexed transaction");

        Ok(())
    }

    async fn new_storage() -> Result<(Storage, SqlitePool), Box<dyn StdError>> {
        // A single connection, because every connection to `sqlite::memory:` gets its own empty
        // database.
        let config = Config {
            cnn_url: "sqlite::memory:".to_string(),
            max_connections: 1,
            ..Default::default()
        };
        let pool = SqlitePool::new(config).await?;
        migrations::sqlite::run(&pool).await?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&[0u8; 32]));
        Ok((Storage::new(cipher, pool.clone()), pool))
    }

    /// Seed a block and a connected wallet.
    async fn seed(pool: &SqlitePool, wallet_id: Uuid) -> Result<(), sqlx::Error> {
        let query = indoc! {"
            INSERT INTO blocks (
                id, hash, height, protocol_version, parent_hash, author,
                timestamp, zswap_merkle_tree_root, ledger_parameters, ledger_state_key
            )
            VALUES (1, zeroblob(32), 0, 1000000, zeroblob(32), NULL, 0, X'00', X'00', X'00')
        "};
        sqlx::query(query).execute(&**pool).await?;

        let query = indoc! {"
            INSERT INTO wallets (
                id, viewing_key_hash, viewing_key,
                wanted_start_index, first_indexed_transaction_id, last_indexed_transaction_id,
                last_active, session_id
            )
            VALUES ($1, X'00', X'00', 0, 0, 0, $2, X'01')
        "};
        sqlx::query(query)
            .bind(wallet_id)
            .bind(OffsetDateTime::now_utc())
            .execute(&**pool)
            .await?;

        Ok(())
    }

    /// Seed a regular transaction in block 1 which is relevant for the given wallet.
    async fn seed_relevant_transaction(
        pool: &SqlitePool,
        wallet_id: Uuid,
        id: i64,
        zswap_start_index: i64,
        zswap_end_index: i64,
    ) -> Result<(), sqlx::Error> {
        let query = indoc! {"
            INSERT INTO transactions (id, block_id, variant, hash, protocol_version, raw)
            VALUES ($1, 1, 'Regular', zeroblob(32), 1000000, X'00')
        "};
        sqlx::query(query).bind(id).execute(&**pool).await?;

        let query = indoc! {"
            INSERT INTO regular_transactions (
                id, transaction_result, zswap_merkle_tree_root,
                zswap_start_index, zswap_end_index,
                dust_commitment_start_index, dust_commitment_end_index,
                dust_generation_start_index, dust_generation_end_index,
                paid_fees, estimated_fees
            )
            VALUES ($1, '\"Success\"', X'00', $2, $3, 0, 0, 0, 0, NULL, NULL)
        "};
        sqlx::query(query)
            .bind(id)
            .bind(zswap_start_index)
            .bind(zswap_end_index)
            .execute(&**pool)
            .await?;

        let query = indoc! {"
            INSERT INTO relevant_transactions (wallet_id, transaction_id)
            VALUES ($1, $2)
        "};
        sqlx::query(query)
            .bind(wallet_id)
            .bind(id)
            .execute(&**pool)
            .await?;

        Ok(())
    }
}
