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

mod header;
mod runtimes;

use crate::{
    domain::{
        BlockRef, SystemParametersChange,
        node::{Block, Node, RegularTransaction, SystemTransaction, Transaction},
    },
    infra::subxt_node::{header::SubstrateHeaderExt, runtimes::BlockDetails},
};
use async_stream::try_stream;
use const_hex::FromHexError;
use fastrace::trace;
use futures::{Stream, StreamExt, TryStreamExt, stream};
use http::{
    HeaderMap,
    header::{InvalidHeaderValue, USER_AGENT},
};
use indexer_common::{
    domain::{
        BlockAuthor, BlockHash, ByteVec, NodeVersion, ProtocolVersion, ProtocolVersionError,
        SerializedContractAddress,
        ledger::{self, ZswapMerkleTreeRoot},
    },
    error::BoxError,
};
use log::{debug, info, warn};
use serde::Deserialize;
use std::{future::ready, time::Duration};
use subxt::{
    OnlineClient, SubstrateConfig,
    config::{
        Hash, RpcConfigFor,
        substrate::{ConsensusEngineId, DigestItem, SubstrateHeader},
    },
    rpcs::{
        LegacyRpcMethods,
        client::{ReconnectingRpcClient, reconnecting_rpc_client::ExponentialBackoff},
    },
    utils::H256,
};
use thiserror::Error;
use tokio::time::timeout;

type OnlineClientAtBlock = subxt::client::OnlineClientAtBlock<SubstrateConfig>;
type SubxtBlock = subxt::client::Block<SubstrateConfig>;

const AURA_ENGINE_ID: ConsensusEngineId = [b'a', b'u', b'r', b'a'];
const TRAVERSE_BACK_LOG_AFTER: u64 = 1_000;

/// A [Node] implementation based on subxt.
#[derive(Clone)]
pub struct SubxtNode {
    rpc_client: ReconnectingRpcClient,
    online_client: OnlineClient<SubstrateConfig>,
    subscription_recovery_timeout: Duration,
}

impl SubxtNode {
    /// Create a new [SubxtNode] with the given [Config].
    pub async fn new(config: Config) -> Result<Self, Error> {
        let Config {
            url,
            reconnect_max_delay: retry_max_delay,
            reconnect_max_attempts: retry_max_attempts,
            subscription_recovery_timeout,
        } = config;

        let retry_policy = ExponentialBackoff::from_millis(10)
            .max_delay(retry_max_delay)
            .take(retry_max_attempts);
        let user_agent = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")).parse()?;
        let headers = HeaderMap::from_iter([(USER_AGENT, user_agent)]);
        let rpc_client = ReconnectingRpcClient::builder()
            .set_headers(headers)
            .retry_policy(retry_policy)
            .build(&url)
            .await
            .map_err(|error| Error::RpcClient(error.into()))?;

        let online_client =
            OnlineClient::<SubstrateConfig>::from_rpc_client(rpc_client.clone()).await?;

        Ok(Self {
            rpc_client,
            online_client,
            subscription_recovery_timeout,
        })
    }

    /// Returns a clone of the underlying reconnecting RPC client so other
    /// indexers in the same process can share the single websocket connection.
    pub fn rpc_client(&self) -> ReconnectingRpcClient {
        self.rpc_client.clone()
    }

    /// Subscribe to finalized blocks, filtering duplicates and disconnection errors.
    /// Subxt with its reconnecting-rpc-client feature exposes the error case, i.e. yields one `Err`
    /// item, then reconnects and continues with `Ok` items. Therefore we filter out the respective
    /// `Err` item; all other errors need to be propagated as is.
    ///
    /// The `last_height` parameter allows the caller to pass in the last successfully processed
    /// block height, which is used to properly filter duplicates after re-subscribing.
    async fn subscribe_finalized_blocks(
        &self,
        mut last_height: Option<u64>,
    ) -> Result<impl Stream<Item = Result<SubxtBlock, SubxtNodeError>> + use<>, SubxtNodeError>
    {
        let finalized_blocks = self
            .online_client
            .stream_blocks()
            .await
            .map_err(|error| SubxtNodeError::SubscribeFinalizedBlocks(error.into()))?
            .filter(move |block| {
                let pass = match block {
                    Ok(block) => {
                        let height = block.number();

                        if Some(height) <= last_height {
                            warn!(
                                hash:% = block.hash(),
                                height = block.number(),
                                last_height:?;
                                "received duplicate, possibly after reconnect"
                            );
                            false
                        } else {
                            last_height = Some(height);
                            true
                        }
                    }

                    // Filter out reconnect errors; see method comment above.
                    Err(subxt::error::BlocksError::CannotGetBlockHeader(
                        subxt::error::BackendError::Rpc(subxt::error::RpcError::ClientError(
                            subxt::rpcs::Error::DisconnectedWillReconnect(_),
                        )),
                    )) => {
                        warn!("node disconnected, reconnecting");
                        false
                    }

                    _ => true,
                };

                ready(pass)
            })
            .map_err(|error| SubxtNodeError::ReceiveBlock(error.into()));

        Ok(finalized_blocks)
    }

    async fn make_block(
        &mut self,
        authorities: &mut Option<Vec<[u8; 32]>>,
        block: OnlineClientAtBlock,
    ) -> Result<Block, SubxtNodeError> {
        let hash = block.block_hash().0.into();
        let height = block.block_number();
        let header = block_header(&block).await?;
        let parent_hash = header.parent_hash.0.into();
        let protocol_version = header
            .protocol_version()?
            .expect("protocol version header is present");
        let node_version = protocol_version.node_version();
        let ledger_version = protocol_version.ledger_version();

        debug!(
            hash:%,
            height,
            parent_hash:%,
            protocol_version:?,
            node_version:%,
            ledger_version:%;
            "making block"
        );

        // Fetch authorities if `None`, either initially or because of a `NewSession` event (below).
        if authorities.is_none() {
            *authorities = Some(runtimes::fetch_authorities(node_version, &block).await?);
        }
        let author = authorities
            .as_ref()
            .map(|authorities| extract_block_author(&header, authorities, node_version))
            .transpose()?
            .flatten();

        let zswap_merkle_tree_root =
            runtimes::get_zswap_merkle_tree_root(node_version, &block).await?;
        let zswap_merkle_tree_root =
            ZswapMerkleTreeRoot::deserialize(zswap_merkle_tree_root, ledger_version)?;

        let BlockDetails {
            timestamp,
            transactions,
            mut dust_registration_events,
        } = runtimes::make_block_details(authorities, node_version, &block).await?;

        // At genesis, Substrate does not emit events (Parity PR #5463). Fetch cNight
        // registrations from pallet storage instead.
        // Also fetch the ledger state root for genesis ledger state detection.
        let ledger_state_root = if height == 0 {
            let genesis_registrations =
                runtimes::fetch_genesis_cnight_registrations(node_version, &block).await?;
            dust_registration_events.extend(genesis_registrations);

            runtimes::get_ledger_state_root(node_version, &block)
                .await?
                .map(Into::into)
        } else {
            None
        };

        let transactions = stream::iter(transactions)
            .then(|t| make_transaction(t, protocol_version, &block))
            .try_collect::<Vec<_>>()
            .await?;

        let block = Block {
            hash,
            height,
            parent_hash,
            protocol_version,
            author,
            timestamp: timestamp.unwrap_or(0),
            zswap_merkle_tree_root,
            ledger_state_root,
            transactions,
            dust_registration_events,
        };

        debug!(
            hash:% = block.hash,
            height = block.height,
            parent_hash:% = block.parent_hash,
            transactions_len = block.transactions.len();
            "block made"
        );

        Ok(block)
    }

    #[trace]
    async fn block_at(&self, hash: H256) -> Result<OnlineClientAtBlock, SubxtNodeError> {
        self.online_client
            .at_block(hash)
            .await
            .map_err(|error| SubxtNodeError::GetOnlineClientAt(hash, error.into()))
    }
}

impl Node for SubxtNode {
    type Error = SubxtNodeError;

    async fn highest_blocks(
        &self,
    ) -> Result<impl Stream<Item = Result<BlockRef, Self::Error>> + Send, Self::Error> {
        let highest_blocks = self
            .subscribe_finalized_blocks(None)
            .await?
            .map_ok(|block| BlockRef {
                hash: block.hash().0.into(),
                height: block.number(),
            });

        Ok(highest_blocks)
    }

    fn finalized_blocks<'a>(
        &'a mut self,
        after: Option<BlockRef>,
    ) -> impl Stream<Item = Result<Block, Self::Error>> + use<'a> {
        let (after_hash, after_height) = after
            .map(|BlockRef { hash, height }| (hash, height))
            .unzip();
        debug!(
            after_hash:?,
            after_height:?;
            "subscribing to finalized blocks"
        );

        let after_hash = after_hash.unwrap_or_default();
        let mut authorities = None;

        try_stream! {
            let mut finalized_blocks = self.subscribe_finalized_blocks(after_height).await?;
            let mut last_yielded_height = after_height;

            // First we receive the first finalized block.
            let Some(first_block) = receive_block(&mut finalized_blocks).await? else {
                return;
            };
            debug!(
                hash:% = first_block.hash(),
                height = first_block.number(),
                parent_hash:% = first_block.header().parent_hash;
                "block received"
            );

            // Then we fetch and yield earlier blocks and then yield the first finalized block,
            // unless the highest stored block matches the first finalized block.
            if first_block.hash().0 != after_hash.0 {
                // If we have not already stored the first finalized block, we fetch all blocks
                // walking backwards from the one with the parent hash of the first finalized block
                // until we arrive at the highest stored block (excluded) or at genesis (included).
                // For these we store the hashes; one hash is 32 bytes, i.e. one year is ~ 160MB.
                // (one year ~ 5,256,000 blocks).
                let genesis = self.block_at(self.online_client.genesis_hash()).await?;
                let genesis_parent_hash = block_header(&genesis).await?.parent_hash;

                let capacity = match after_height {
                    Some(after_height) if after_height < first_block.number() => {
                        (first_block.number() - after_height) as usize + 1
                    }
                    _ => first_block.number() as usize + 1,
                };
                // Cap at one year, see comment above.
                let capacity = capacity.min(5_256_000);
                let mut hashes = Vec::with_capacity(capacity);

                let mut parent_hash = first_block.header().parent_hash;
                while parent_hash.0 != after_hash.0 && parent_hash != genesis_parent_hash {
                    let parent = self.block_at(parent_hash).await?;
                    if parent.block_number() % TRAVERSE_BACK_LOG_AFTER == 0 {
                        info!(
                            highest_stored_height:? = after_height,
                            current_height = parent.block_number(),
                            first_finalized_height = first_block.number();
                            "traversing back via parent hashes"
                        );
                    }
                    parent_hash = block_header(&parent).await?.parent_hash;
                    hashes.push(parent.block_hash());
                }

                // We fetch and yield the blocks for the stored block hashes.
                for hash in hashes.into_iter().rev() {
                    let block = self.block_at(hash).await?;
                    debug!(
                        hash:% = block.block_hash(),
                        height = block.block_number();
                        "block fetched"
                    );
                    yield self.make_block(&mut authorities, block).await?;
                }

                // Then we yield the first finalized block.
                let first_block = first_block.at().await.map_err(|error| {
                    SubxtNodeError::GetOnlineClientAt(first_block.hash(), error.into())
                })?;
                let first_block = self.make_block(&mut authorities, first_block).await?;
                last_yielded_height = Some(first_block.height);
                yield first_block;
            }

            // Finally we emit all other finalized ones.
            // If no block is received within the recovery timeout, re-subscribe to recover
            // from potentially stuck subscriptions (e.g., after a reconnect).
            let recovery_timeout = self.subscription_recovery_timeout;
            loop {
                match timeout(recovery_timeout, receive_block(&mut finalized_blocks)).await {
                    Ok(Ok(Some(block))) => {
                        debug!(
                            hash:% = block.hash(),
                            height = block.number(),
                            parent_hash:% = block.header().parent_hash;
                            "block received"
                        );
                        let block = block.at().await.map_err(|error| {
                            SubxtNodeError::GetOnlineClientAt(block.hash(), error.into())
                        })?;
                        let block = self.make_block(&mut authorities, block).await?;
                        last_yielded_height = Some(block.height);
                        yield block;
                    }

                    // Stream completed normally.
                    Ok(Ok(None)) => break,

                    // Stream completed with error.
                    Ok(Err(e)) => Err(e)?,

                    // Timeout: no block received within recovery_timeout => resubscribe.
                    Err(_) => {
                        warn!(
                            last_yielded_height:?,
                            recovery_timeout:?;
                            "subscription appears stuck, re-subscribing"
                        );
                        finalized_blocks =
                            self.subscribe_finalized_blocks(last_yielded_height).await?;
                    }
                }
            }
        }
    }

    async fn fetch_system_parameters(
        &self,
        block_hash: BlockHash,
        block_height: u64,
        timestamp: u64,
        node_version: NodeVersion,
    ) -> Result<SystemParametersChange, Self::Error> {
        let block = self.block_at(H256(block_hash.0)).await?;

        let (d_parameter, terms_and_conditions) = tokio::try_join!(
            runtimes::get_d_parameter(node_version, &block),
            runtimes::get_terms_and_conditions(node_version, &block),
        )?;

        Ok(SystemParametersChange {
            block_height,
            block_hash,
            timestamp,
            d_parameter: Some(d_parameter),
            terms_and_conditions,
        })
    }

    async fn fetch_ledger_parameters(&self, block_hash: BlockHash) -> Result<ByteVec, Self::Error> {
        let block = self.block_at(H256(block_hash.0)).await?;
        let header = block_header(&block).await?;
        let node_version = header
            .protocol_version()?
            .expect("protocol version header is present")
            .node_version();

        let parameters = runtimes::get_ledger_parameters(node_version, &block).await?;
        Ok(parameters.into())
    }

    async fn fetch_genesis_ledger_state(&self) -> Result<ByteVec, Self::Error> {
        let legacy_rpc_methods = LegacyRpcMethods::<RpcConfigFor<SubstrateConfig>>::new(
            self.rpc_client.to_owned().into(),
        );
        let properties = legacy_rpc_methods
            .system_properties()
            .await
            .map_err(SubxtNodeError::FetchSystemProperties)?;

        let genesis_ledger_state = properties
            .get("genesis_state")
            .and_then(|value| value.as_str())
            .map(Ok)
            .unwrap_or_else(|| Err(SubxtNodeError::GenesisLedgerStateNotFound))?;

        let genesis_ledger_state = genesis_ledger_state
            .strip_prefix("0x")
            .unwrap_or(genesis_ledger_state);
        let genesis_ledger_state = const_hex::decode(genesis_ledger_state)
            .map_err(SubxtNodeError::HexDecodeGenesisLedgerState)?;
        let genesis_ledger_state = ByteVec::from(genesis_ledger_state);

        info!(
            genesis_ledger_state_len = genesis_ledger_state.len();
            "fetched genesis ledger state from system properties"
        );

        Ok(genesis_ledger_state)
    }
}

/// Config for node connection.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub url: String,

    #[serde(with = "humantime_serde")]
    pub reconnect_max_delay: Duration,

    pub reconnect_max_attempts: usize,

    /// Timeout for receiving a valid block after a reconnect or duplicate event.
    /// If no valid block is received within this duration, the subscription is considered
    /// stuck and will be re-established. Defaults to 30 seconds.
    #[serde(
        with = "humantime_serde",
        default = "default_subscription_recovery_timeout"
    )]
    pub subscription_recovery_timeout: Duration,
}

fn default_subscription_recovery_timeout() -> Duration {
    Duration::from_secs(30)
}

/// Error possibly returned by [SubxtNode::new].
#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot create reconnecting subxt RPC client")]
    RpcClient(#[source] BoxError),

    #[error("cannot create subxt online client")]
    OnlineClient(#[from] subxt::error::OnlineClientError),

    #[error("cannot create HTTP header")]
    InvalidHeaderValue(#[from] InvalidHeaderValue),
}

/// Error possibly returned by each item of the [Block]s stream.
#[derive(Debug, Error)]
pub enum SubxtNodeError {
    #[error("cannot subscribe to finalized blocks")]
    SubscribeFinalizedBlocks(#[source] Box<subxt::error::BlocksError>),

    #[error("cannot receive finalized block")]
    ReceiveBlock(#[source] Box<subxt::error::BlocksError>),

    #[error("cannot get online client at block {0}")]
    GetOnlineClientAt(H256, #[source] Box<subxt::error::OnlineClientAtBlockError>),

    #[error("cannot fetch extrinsics")]
    FetchExtrinsics(#[source] Box<subxt::error::ExtrinsicError>),

    #[error("cannot fetch events")]
    FetchEvents(#[source] Box<subxt::error::EventsError>),

    #[error("cannot get block header")]
    GetBlockHeader(#[source] Box<subxt::error::BlockError>),

    #[error("cannot get next extrinsic")]
    GetNextExtrinsic(#[source] Box<subxt::error::ExtrinsicDecodeErrorAt>),

    #[error("cannot decode extrinsic as call")]
    DecodeExtrinsicAsCall(#[source] Box<subxt::error::ExtrinsicError>),

    #[error("cannot get next event")]
    GetNextEvent(#[source] Box<subxt::error::EventsError>),

    #[error("cannot decode subxt event as midnight event")]
    DecodeEvent(#[source] Box<subxt::error::EventsError>),

    #[error("cannot fetch authorities")]
    FetchAuthorities(#[source] Box<subxt::error::StorageError>),

    #[error("cannot decode authorities")]
    DecodeAuthorities(#[source] Box<subxt::error::StorageValueError>),

    #[error("cannot fetch genesis cNight registrations")]
    FetchGenesisCnightRegistrations(#[source] Box<subxt::error::StorageError>),

    #[error("cannot decode genesis cNight registrations")]
    DecodeGenesisCnightRegistrations(#[source] Box<subxt::error::StorageValueError>),

    #[error("cannot get contract state for address {0}")]
    GetContractState(SerializedContractAddress, #[source] BoxError),

    #[error("cannot get zswap state root")]
    GetZswapStateRoot(#[source] BoxError),

    #[error("cannot get D-Parameter")]
    GetDParameter(#[source] BoxError),

    #[error("cannot get Terms and Conditions")]
    GetTermsAndConditions(#[source] BoxError),

    #[error("cannot hex decode genesis ledger state")]
    HexDecodeGenesisLedgerState(#[source] FromHexError),

    #[error("cannot get ledger state root")]
    GetLedgerStateRoot(#[source] BoxError),

    #[error("cannot get ledger parameters")]
    GetLedgerParameters(#[source] BoxError),

    #[error("cannot fetch system properties")]
    FetchSystemProperties(#[source] subxt::rpcs::Error),

    #[error("no String type genesis ledger state in system parameters")]
    GenesisLedgerStateNotFound,

    #[error(transparent)]
    ProtocolVersion(#[from] ProtocolVersionError),

    #[error("cannot scale decode")]
    ScaleDecode(#[from] parity_scale_codec::Error),

    #[error(transparent)]
    Ledger(#[from] ledger::Error),
}

#[trace]
async fn receive_block(
    finalized_blocks: &mut (impl Stream<Item = Result<SubxtBlock, SubxtNodeError>> + Unpin),
) -> Result<Option<SubxtBlock>, SubxtNodeError> {
    finalized_blocks.try_next().await
}

/// Check an authority set against a block header's digest logs to determine the author of that
/// block.
fn extract_block_author<H>(
    header: &SubstrateHeader<H>,
    authorities: &[[u8; 32]],
    node_version: NodeVersion,
) -> Result<Option<BlockAuthor>, SubxtNodeError>
where
    H: Hash,
{
    if authorities.is_empty() {
        return Ok(None);
    }

    let block_author = header
        .digest
        .logs
        .iter()
        .find_map(|log| match log {
            DigestItem::PreRuntime(AURA_ENGINE_ID, inner) => Some(inner.as_slice()),
            _ => None,
        })
        .map(|slot| runtimes::decode_slot(slot, node_version))
        .transpose()?
        .and_then(|slot| {
            let index = slot % authorities.len() as u64;
            authorities.get(index as usize).copied().map(Into::into)
        });

    Ok(block_author)
}

async fn make_transaction(
    transaction: runtimes::Transaction,
    protocol_version: ProtocolVersion,
    block: &OnlineClientAtBlock,
) -> Result<Transaction, SubxtNodeError> {
    match transaction {
        runtimes::Transaction::Regular(transaction) => {
            make_regular_transaction(transaction, protocol_version, block).await
        }

        runtimes::Transaction::System(transaction) => {
            make_system_transaction(transaction, protocol_version).await
        }
    }
}

async fn make_regular_transaction(
    transaction: ByteVec,
    protocol_version: ProtocolVersion,
    block: &OnlineClientAtBlock,
) -> Result<Transaction, SubxtNodeError> {
    let node_version = protocol_version.node_version();

    let ledger_transaction =
        ledger::Transaction::deserialize(&transaction, protocol_version.ledger_version())?;

    let hash = ledger_transaction.hash();

    let identifiers = ledger_transaction.identifiers()?;

    let contract_actions = ledger_transaction
        .contract_actions(|address| async move {
            runtimes::get_contract_state(address, node_version, block).await
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect();

    let transaction = RegularTransaction {
        hash,
        protocol_version,
        identifiers,
        contract_actions,
        raw: transaction,
    };

    Ok(Transaction::Regular(transaction))
}

async fn make_system_transaction(
    transaction: ByteVec,
    protocol_version: ProtocolVersion,
) -> Result<Transaction, SubxtNodeError> {
    let ledger_transaction =
        ledger::SystemTransaction::deserialize(&transaction, protocol_version.ledger_version())?;

    let hash = ledger_transaction.hash();

    let transaction = SystemTransaction {
        hash,
        protocol_version,
        raw: transaction,
    };

    Ok(Transaction::System(transaction))
}

#[trace]
async fn block_header(
    block: &OnlineClientAtBlock,
) -> Result<SubstrateHeader<H256>, SubxtNodeError> {
    block
        .block_header()
        .await
        .map_err(|error| SubxtNodeError::GetBlockHeader(error.into()))
}
