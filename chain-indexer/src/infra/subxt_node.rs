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

mod header;
mod runtimes;

use crate::{
    domain::{
        BlockRef, SystemParametersChange, TransactionFees,
        node::{Block, Node, RegularTransaction, SystemTransaction, Transaction},
    },
    infra::subxt_node::{header::SubstrateHeaderExt, runtimes::BlockDetails},
};
use async_stream::try_stream;
use fastrace::trace;
use futures::{Stream, StreamExt, TryStreamExt, stream};
use indexer_common::{
    domain::{
        BlockAuthor, BlockHash, ByteVec, GenesisSettings, NodeVersion, ProtocolVersion,
        ScaleDecodeProtocolVersionError, SerializedContractAddress, UnsupportedProtocolVersion,
        ledger::{self, ZswapStateRoot},
    },
    error::BoxError,
};
use log::{debug, info, warn};
use serde::Deserialize;
use std::{future::ready, time::Duration};
use subxt::{
    OnlineClient, SubstrateConfig,
    backend::{
        BackendExt,
        legacy::LegacyRpcMethods,
        rpc::reconnecting_rpc_client::{ExponentialBackoff, RpcClient},
    },
    config::{
        Hasher,
        substrate::{ConsensusEngineId, DigestItem, SubstrateHeader},
    },
    ext::subxt_rpcs,
    utils::H256,
};
use thiserror::Error;
use tokio::time::timeout;

type SubxtBlock = subxt::blocks::Block<SubstrateConfig, OnlineClient<SubstrateConfig>>;

const AURA_ENGINE_ID: ConsensusEngineId = [b'a', b'u', b'r', b'a'];
const TRAVERSE_BACK_LOG_AFTER: u32 = 1_000;

/// A [Node] implementation based on subxt.
#[derive(Clone)]
pub struct SubxtNode {
    rpc_client: RpcClient,
    default_online_client: OnlineClient<SubstrateConfig>,
    compatible_online_client: Option<(NodeVersion, OnlineClient<SubstrateConfig>)>,
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
        let rpc_client = RpcClient::builder()
            .retry_policy(retry_policy)
            .build(&url)
            .await
            .map_err(|error| Error::RpcClient(error.into()))?;

        let default_online_client =
            OnlineClient::<SubstrateConfig>::from_rpc_client(rpc_client.clone()).await?;

        Ok(Self {
            rpc_client,
            default_online_client,
            compatible_online_client: None,
            subscription_recovery_timeout,
        })
    }

    async fn compatible_online_client(
        &mut self,
        protocol_version: ProtocolVersion,
        hash: BlockHash,
    ) -> Result<&OnlineClient<SubstrateConfig>, SubxtNodeError> {
        let node_version = protocol_version.node_version()?;
        if !self
            .compatible_online_client
            .as_ref()
            .map(|&(v, _)| v == node_version)
            .unwrap_or_default()
        {
            let genesis_hash = self.default_online_client.genesis_hash();

            // Version must be greater or equal 15. This is a substrate/subxt detail.
            let metadata = self
                .default_online_client
                .backend()
                .metadata_at_version(15, H256(hash.0))
                .await
                .map_err(|error| SubxtNodeError::GetMetadata(error.into()))?;

            let legacy_rpc_methods =
                LegacyRpcMethods::<SubstrateConfig>::new(self.rpc_client.to_owned().into());
            let runtime_version = legacy_rpc_methods
                .state_get_runtime_version(Some(H256(hash.0)))
                .await
                .map_err(SubxtNodeError::GetRuntimeVersion)?;
            let runtime_version = subxt::client::RuntimeVersion {
                spec_version: runtime_version.spec_version,
                transaction_version: runtime_version.transaction_version,
            };

            let online_client = OnlineClient::<SubstrateConfig>::from_rpc_client_with(
                genesis_hash,
                runtime_version,
                metadata,
                self.rpc_client.to_owned(),
            )
            .map_err(|error| SubxtNodeError::MakeOnlineClient(error.into()))?;

            self.compatible_online_client = Some((node_version, online_client));
        }

        let compatible_online_client = self
            .compatible_online_client
            .as_ref()
            .map(|(_, c)| c)
            .expect("compatible_online_client is defined");

        Ok(compatible_online_client)
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
        mut last_height: Option<u32>,
    ) -> Result<impl Stream<Item = Result<SubxtBlock, SubxtNodeError>> + use<>, SubxtNodeError>
    {
        let finalized_blocks = self
            .default_online_client
            .blocks()
            .subscribe_finalized()
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
                    Err(subxt::Error::Rpc(subxt::error::RpcError::ClientError(
                        subxt_rpcs::Error::DisconnectedWillReconnect(_),
                    ))) => {
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
        block: SubxtBlock,
        authorities: &mut Option<Vec<[u8; 32]>>,
    ) -> Result<Block, SubxtNodeError> {
        let hash = block.hash().0.into();
        let height = block.number();
        let parent_hash = block.header().parent_hash.0.into();
        let protocol_version = block
            .header()
            .protocol_version()?
            .expect("protocol version header is present");

        debug!(
            hash:%,
            height,
            parent_hash:%,
            protocol_version:%;
            "making block"
        );

        let online_client = self
            .compatible_online_client(protocol_version, hash)
            .await?;

        // Fetch authorities if `None`, either initially or because of a `NewSession` event (below).
        if authorities.is_none() {
            *authorities =
                runtimes::fetch_authorities(hash, protocol_version, online_client).await?;
        }
        let author = authorities
            .as_ref()
            .map(|authorities| extract_block_author(block.header(), authorities, protocol_version))
            .transpose()?
            .flatten();

        let zswap_state_root =
            runtimes::get_zswap_state_root(hash, protocol_version, online_client).await?;
        let zswap_state_root = ZswapStateRoot::deserialize(zswap_state_root, protocol_version)?;

        let extrinsics = block
            .extrinsics()
            .await
            .map_err(|error| SubxtNodeError::GetExtrinsics(error.into()))?;
        let events = block
            .events()
            .await
            .map_err(|error| SubxtNodeError::GetEvents(error.into()))?;
        let BlockDetails {
            timestamp,
            transactions,
            mut dust_registration_events,
        } = runtimes::make_block_details(extrinsics, events, authorities, protocol_version).await?;

        // At genesis, Substrate does not emit events (Parity PR #5463). Fetch cNight
        // registrations from pallet storage instead.
        if height == 0 {
            let genesis_registrations =
                runtimes::fetch_genesis_cnight_registrations(hash, protocol_version, online_client)
                    .await?;
            dust_registration_events.extend(genesis_registrations);
        }

        let transactions = stream::iter(transactions)
            .then(|t| make_transaction(t, hash, protocol_version, online_client))
            .try_collect::<Vec<_>>()
            .await?;

        let block = Block {
            hash,
            height,
            parent_hash,
            protocol_version,
            author,
            timestamp: timestamp.unwrap_or(0),
            zswap_state_root,
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
    async fn fetch_block(&self, hash: H256) -> Result<SubxtBlock, SubxtNodeError> {
        self.default_online_client
            .blocks()
            .at(hash)
            .await
            .map_err(|error| SubxtNodeError::FetchBlock(hash, error.into()))
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
                let genesis_parent_hash = self
                    .fetch_block(self.default_online_client.genesis_hash())
                    .await?
                    .header()
                    .parent_hash;

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
                    let block = self.fetch_block(parent_hash).await?;
                    if block.number() % TRAVERSE_BACK_LOG_AFTER == 0 {
                        info!(
                            highest_stored_height:? = after_height,
                            current_height = block.number(),
                            first_finalized_height = first_block.number();
                            "traversing back via parent hashes"
                        );
                    }
                    parent_hash = block.header().parent_hash;
                    hashes.push(block.hash());
                }

                // We fetch and yield the blocks for the stored block hashes.
                for hash in hashes.into_iter().rev() {
                    let block = self.fetch_block(hash).await?;
                    debug!(
                        hash:% = block.hash(),
                        height = block.number(),
                        parent_hash:% = block.header().parent_hash;
                        "block fetched"
                    );
                    yield self.make_block(block, &mut authorities).await?;
                }

                // Then we yield the first finalized block.
                let block = self.make_block(first_block, &mut authorities).await?;
                last_yielded_height = Some(block.height);
                yield block;
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
                        let block = self.make_block(block, &mut authorities).await?;
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
        block_height: u32,
        timestamp: u64,
        protocol_version: ProtocolVersion,
    ) -> Result<SystemParametersChange, Self::Error> {
        let (d_parameter, terms_and_conditions) = tokio::try_join!(
            runtimes::get_d_parameter(block_hash, protocol_version, &self.default_online_client),
            runtimes::get_terms_and_conditions(
                block_hash,
                protocol_version,
                &self.default_online_client
            ),
        )?;

        Ok(SystemParametersChange {
            block_height,
            block_hash,
            timestamp,
            d_parameter: Some(d_parameter),
            terms_and_conditions,
        })
    }

    async fn fetch_genesis_settings(&self) -> Result<Option<GenesisSettings>, Self::Error> {
        let legacy_rpc_methods =
            LegacyRpcMethods::<SubstrateConfig>::new(self.rpc_client.to_owned().into());
        let properties = legacy_rpc_methods
            .system_properties()
            .await
            .map_err(|error| SubxtNodeError::FetchGenesisSettings(error.into()))?;

        let Some(genesis_state_value) = properties.get("genesis_state") else {
            debug!("no genesis_state in system properties");
            return Ok(None);
        };

        let Some(hex_str) = genesis_state_value.as_str() else {
            warn!("genesis_state in system properties is not a string");
            return Ok(None);
        };

        let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        let bytes = const_hex::decode(hex_str).map_err(|error| {
            SubxtNodeError::FetchGenesisSettings(
                format!("cannot hex-decode genesis_state: {error}").into(),
            )
        })?;

        info!(
            genesis_state_bytes = bytes.len();
            "fetched genesis state from chain spec system properties"
        );

        match ledger::LedgerState::extract_genesis_settings(&bytes) {
            Ok(settings) => {
                info!(
                    locked_pool = settings.locked_pool,
                    reserve_pool = settings.reserve_pool,
                    treasury = settings.treasury;
                    "extracted genesis settings from chain spec"
                );
                Ok(Some(settings))
            }
            Err(error) => {
                warn!(error:%; "cannot extract genesis settings from chain spec, ignoring");
                Ok(None)
            }
        }
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
    OnlineClient(#[from] subxt::Error),
}

/// Error possibly returned by each item of the [Block]s stream.
#[derive(Debug, Error)]
pub enum SubxtNodeError {
    #[error("cannot subscribe to finalized blocks")]
    SubscribeFinalizedBlocks(#[source] Box<subxt::Error>),

    #[error("cannot receive finalized block")]
    ReceiveBlock(#[source] Box<subxt::Error>),

    #[error("cannot fetch block at hash {0}")]
    FetchBlock(H256, #[source] Box<subxt::Error>),

    #[error("cannot get extrinsics")]
    GetExtrinsics(#[source] Box<subxt::Error>),

    #[error("cannot get events")]
    GetEvents(#[source] Box<subxt::Error>),

    #[error("cannot next event")]
    GetNextEvent(#[source] Box<subxt::Error>),

    #[error("cannot decode event as root event")]
    AsRootEvent(#[source] Box<subxt::Error>),

    #[error("cannot get node metadata")]
    GetMetadata(#[source] Box<subxt::Error>),

    #[error("cannot make compatible subxt online client")]
    MakeOnlineClient(#[source] Box<subxt::Error>),

    #[error("cannot fetch authorities")]
    FetchAuthorities(#[source] Box<subxt::Error>),

    #[error("cannot use extrinsic as root extrinsic")]
    AsRootExtrinsic(#[source] Box<subxt::Error>),

    #[error("cannot get runtime version")]
    GetRuntimeVersion(#[source] subxt::ext::subxt_rpcs::Error),

    #[error("cannot scale decode")]
    ScaleDecode(#[from] parity_scale_codec::Error),

    #[error(transparent)]
    DecodeProtocolVersion(#[from] ScaleDecodeProtocolVersionError),

    #[error(transparent)]
    Ledger(#[from] ledger::Error),

    #[error("cannot get contract state for address {0} at block {1}")]
    GetContractState(SerializedContractAddress, BlockHash, #[source] BoxError),

    #[error("cannot get zswap state root")]
    GetZswapStateRoot(#[source] BoxError),

    #[error("cannot get transaction cost")]
    GetTransactionCost(#[source] BoxError),

    #[error("block with hash {0} not found")]
    BlockNotFound(BlockHash),

    #[error(transparent)]
    UnsupportedProtocolVersion(#[from] UnsupportedProtocolVersion),

    #[error("invalid DUST address length: expected 32 bytes, was {0}")]
    InvalidDustAddress(usize),

    #[error("cannot get D-Parameter")]
    GetDParameter(#[source] BoxError),

    #[error("cannot get Terms and Conditions")]
    GetTermsAndConditions(#[source] BoxError),

    #[error("cannot fetch genesis cNight registrations")]
    FetchGenesisCnightRegistrations(#[source] BoxError),

    #[error("cannot fetch genesis settings")]
    FetchGenesisSettings(#[source] BoxError),
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
    header: &SubstrateHeader<u32, H>,
    authorities: &[[u8; 32]],
    protocol_version: ProtocolVersion,
) -> Result<Option<BlockAuthor>, SubxtNodeError>
where
    H: Hasher,
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
        .map(|slot| runtimes::decode_slot(slot, protocol_version))
        .transpose()?
        .and_then(|slot| {
            let index = slot % authorities.len() as u64;
            authorities.get(index as usize).copied().map(Into::into)
        });

    Ok(block_author)
}

async fn make_transaction(
    transaction: runtimes::Transaction,
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Transaction, SubxtNodeError> {
    match transaction {
        runtimes::Transaction::Regular(transaction) => {
            make_regular_transaction(transaction, block_hash, protocol_version, online_client).await
        }

        runtimes::Transaction::System(transaction) => {
            make_system_transaction(transaction, protocol_version).await
        }
    }
}

async fn make_regular_transaction(
    transaction: ByteVec,
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Transaction, SubxtNodeError> {
    let ledger_transaction = ledger::Transaction::deserialize(&transaction, protocol_version)?;

    let hash = ledger_transaction.hash();

    let identifiers = ledger_transaction.identifiers()?;

    let contract_actions = ledger_transaction
        .contract_actions(|address| async move {
            runtimes::get_contract_state(address, block_hash, protocol_version, online_client).await
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect();

    let fees = match runtimes::get_transaction_cost(
        &transaction,
        block_hash,
        protocol_version,
        online_client,
    )
    .await
    {
        Ok(fees) => TransactionFees {
            paid_fees: fees,
            estimated_fees: fees,
        },

        Err(error) => {
            warn!(
                error:%, block_hash:%, transaction_size = transaction.len();
                "cannot get runtime API fees, using fallback"
            );
            TransactionFees::from_ledger_transaction(&ledger_transaction, transaction.len())
        }
    };

    let transaction = RegularTransaction {
        hash,
        protocol_version,
        identifiers,
        contract_actions,
        raw: transaction,
        paid_fees: fees.paid_fees,
        estimated_fees: fees.estimated_fees,
    };

    Ok(Transaction::Regular(transaction))
}

async fn make_system_transaction(
    transaction: ByteVec,
    protocol_version: ProtocolVersion,
) -> Result<Transaction, SubxtNodeError> {
    let ledger_transaction =
        ledger::SystemTransaction::deserialize(&transaction, protocol_version)?;

    let hash = ledger_transaction.hash();

    let transaction = SystemTransaction {
        hash,
        protocol_version,
        raw: transaction,
    };

    Ok(Transaction::System(transaction))
}
