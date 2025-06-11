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
    domain::{Block, BlockInfo, ContractAction, ContractAttributes, Node, Transaction},
    infra::node::{
        header::SubstrateHeaderExt,
        runtimes::{BlockDetails, UtxoInfo},
    },
};
use async_stream::try_stream;
use fastrace::trace;
use futures::{Stream, StreamExt, TryStreamExt};
use indexer_common::{
    LedgerTransaction,
    domain::{
        BlockAuthor, BlockHash, ByteVec, NetworkId, ProtocolVersion, RawTransaction,
        ScaleDecodeProtocolVersionError, TransactionHash, UnshieldedAddress,
    },
    error::{BoxError, StdErrorExt},
    serialize::SerializableExt,
};
use log::{debug, error, info, warn};
use midnight_coin_structure::contract::ContractAddress;
use midnight_ledger::structure::{ContractAction as LedgerContractAction, ProofMarker};
use midnight_serialize::deserialize;
use midnight_storage::DefaultDB;
use midnight_transient_crypto::merkle_tree::MerkleTreeDigest;
use serde::Deserialize;
use std::{
    collections::HashMap,
    future::ready,
    io::{self},
    time::Duration,
};
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

type SubxtBlock = subxt::blocks::Block<SubstrateConfig, OnlineClient<SubstrateConfig>>;

const AURA_ENGINE_ID: ConsensusEngineId = [b'a', b'u', b'r', b'a'];
const TRAVERSE_BACK_LOG_AFTER: u32 = 1_000;

/// A [Node] implementation based on subxt.
#[derive(Clone)]
pub struct SubxtNode {
    genesis_protocol_version: ProtocolVersion,
    rpc_client: RpcClient,
    default_online_client: OnlineClient<SubstrateConfig>,
    compatible_online_client: Option<(ProtocolVersion, OnlineClient<SubstrateConfig>)>,
}

impl SubxtNode {
    /// Create a new [SubxtNode] with the given [Config].
    pub async fn new(config: Config) -> Result<Self, Error> {
        let Config {
            url,
            genesis_protocol_version,
            reconnect_max_delay: retry_max_delay,
            reconnect_max_attempts: retry_max_attempts,
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
            genesis_protocol_version,
            default_online_client,
            compatible_online_client: None,
        })
    }

    async fn compatible_online_client(
        &mut self,
        protocol_version: ProtocolVersion,
        hash: BlockHash,
    ) -> Result<&OnlineClient<SubstrateConfig>, SubxtNodeError> {
        if !self
            .compatible_online_client
            .as_ref()
            .map(|&(v, _)| protocol_version.is_compatible(v))
            .unwrap_or_default()
        {
            let genesis_hash = self.default_online_client.genesis_hash();

            // Version must be greater or equal 15. This is a substrate/subxt detail.
            let metadata = self
                .default_online_client
                .backend()
                .metadata_at_version(15, H256(hash.0))
                .await
                .map_err(Box::new)?;

            let legacy_rpc_methods =
                LegacyRpcMethods::<SubstrateConfig>::new(self.rpc_client.to_owned().into());
            let runtime_version = legacy_rpc_methods
                .state_get_runtime_version(Some(H256(hash.0)))
                .await?;
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
            .map_err(Box::new)?;

            self.compatible_online_client = Some((protocol_version, online_client));
        }

        let compatible_online_client = self
            .compatible_online_client
            .as_ref()
            .map(|(_, c)| c)
            .expect("compatible_online_client is defined");

        Ok(compatible_online_client)
    }

    /// Subscribe to finalizded blocks, filtering duplicates and disconnection errors.
    async fn subscribe_finalized_blocks(
        &self,
    ) -> Result<impl Stream<Item = Result<SubxtBlock, subxt::Error>> + use<>, subxt::Error> {
        let mut last_block_height = None;

        let subscribe_finalized_blocks = self
            .default_online_client
            .blocks()
            .subscribe_finalized()
            .await?
            .filter(move |block| {
                let pass = match block {
                    Ok(block) => {
                        let height = block.number();

                        if Some(height) <= last_block_height {
                            warn!(
                                hash:% = block.hash(),
                                height = block.number();
                                "received duplicate, possibly after reconnect"
                            );
                            false
                        } else {
                            last_block_height = Some(height);
                            true
                        }
                    }

                    Err(subxt::Error::Rpc(subxt::error::RpcError::ClientError(
                        subxt_rpcs::Error::DisconnectedWillReconnect(_),
                    ))) => {
                        warn!("node disconnected, reconnecting");
                        false
                    }

                    Err(_) => true,
                };

                ready(pass)
            });

        Ok(subscribe_finalized_blocks)
    }

    #[trace]
    async fn fetch_block(&self, hash: H256) -> Result<SubxtBlock, subxt::Error> {
        self.default_online_client.blocks().at(hash).await
    }

    async fn make_block(
        &mut self,
        block: SubxtBlock,
        authorities: &mut Option<Vec<[u8; 32]>>,
        network_id: NetworkId,
    ) -> Result<Block, SubxtNodeError> {
        let hash = block.hash().0.into();
        let height = block.number();
        let parent_hash = block.header().parent_hash.0.into();
        let protocol_version = block
            .header()
            .protocol_version()?
            .unwrap_or(self.genesis_protocol_version);

        info!(
            hash:%,
            height,
            parent_hash:%,
            protocol_version:%;
            "making block"
        );

        // Fetch authorities if `None`, either initially or because of a `NewSession` event (below).
        if authorities.is_none() {
            // Safe to use self.online_client? Probably yes, because using storage at latest block.
            *authorities =
                runtimes::fetch_authorities(&self.default_online_client, protocol_version).await?;
        }
        let author = authorities
            .as_ref()
            .map(|authorities| extract_block_author(block.header(), authorities, protocol_version))
            .transpose()?
            .flatten();

        let online_client = self
            .compatible_online_client(protocol_version, hash)
            .await?;

        let zswap_state_root =
            runtimes::get_zswap_state_root(online_client, hash, protocol_version).await?;
        let zswap_state_root =
            deserialize::<MerkleTreeDigest, _>(&mut zswap_state_root.as_slice(), network_id.into())
                .map_err(|error| {
                    SubxtNodeError::Io("cannot deserialize zswap state root", error)
                })?;

        let extrinsics = block.extrinsics().await.map_err(Box::new)?;
        let events = block.events().await.map_err(Box::new)?;
        let BlockDetails {
            timestamp,
            raw_transactions,
            created_unshielded_utxos_info,
            spent_unshielded_utxos_info,
        } = runtimes::make_block_details(extrinsics, events, authorities, protocol_version).await?;

        let mut transactions = Vec::with_capacity(raw_transactions.len());
        for (n, raw_transaction) in raw_transactions.into_iter().enumerate() {
            let tx = make_transaction(
                n,
                raw_transaction,
                hash,
                protocol_version,
                &created_unshielded_utxos_info,
                &spent_unshielded_utxos_info,
                network_id,
                online_client,
            )
            .await?;

            if let Some(tx) = tx {
                transactions.push(tx);
            }
        }

        let block = Block {
            hash,
            height,
            parent_hash,
            protocol_version,
            author,
            timestamp: timestamp.unwrap_or(0),
            zswap_state_root,
            transactions,
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
}

impl Node for SubxtNode {
    type Error = SubxtNodeError;

    async fn highest_blocks(
        &self,
    ) -> Result<impl Stream<Item = Result<BlockInfo, Self::Error>> + Send, Self::Error> {
        let highest_blocks = self
            .subscribe_finalized_blocks()
            .await
            .map_err(Box::new)?
            .map_ok(|block| BlockInfo {
                hash: block.hash().0.into(),
                height: block.number(),
            })
            .map_err(|error| Box::new(error).into());

        Ok(highest_blocks)
    }

    fn finalized_blocks(
        &mut self,
        after: Option<BlockInfo>,
        network_id: NetworkId,
    ) -> impl Stream<Item = Result<Block, Self::Error>> {
        let (after_hash, after_height) = after
            .map(|BlockInfo { hash, height }| (hash, height))
            .unzip();
        debug!(
            after_hash:?,
            after_height:?;
            "subscribing to finalized blocks"
        );

        let after_hash = after_hash.unwrap_or_default();
        let mut authorities = None;

        try_stream! {
            let mut finalized_blocks = self.subscribe_finalized_blocks().await.map_err(Box::new)?;

            // First we receive the first finalized block.
            let Some(first_block) = receive_block(&mut finalized_blocks)
                .await
                .map_err(Box::new)?
            else {
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
                // starting with the one with the parent hash of the first finalized block, until
                // we arrive at the highest stored block hash (excluded) or at genesis (included).
                // For these we store the hashes; one hash is 32 bytes, i.e. one year is ~ 156MB.
                let genesis_parent_hash = self
                    .fetch_block(self.default_online_client.genesis_hash())
                    .await
                    .map_err(Box::new)?
                    .header()
                    .parent_hash;

                let capacity = match after_height {
                    Some(highest_height) if highest_height < first_block.number() => {
                        (first_block.number() - highest_height) as usize + 1
                    }
                    _ => first_block.number() as usize + 1,
                };
                info!(
                    highest_stored_height:? = after_height,
                    first_finalized_height = first_block.number();
                    "traversing back via parent hashes, this may take some time"
                );

                let mut hashes = Vec::with_capacity(capacity);
                let mut parent_hash = first_block.header().parent_hash;
                while parent_hash.0 != after_hash.0 && parent_hash != genesis_parent_hash {
                    let block = self.fetch_block(parent_hash).await.map_err(Box::new)?;
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
                    let block = self.fetch_block(hash).await.map_err(Box::new)?;
                    debug!(
                        hash:% = block.hash(),
                        height = block.number(),
                        parent_hash:% = block.header().parent_hash;
                        "block fetched"
                    );
                    yield self.make_block(block, &mut authorities, network_id).await?;
                }

                // Then we yield the first finalized block.
                yield self
                    .make_block(first_block, &mut authorities, network_id)
                    .await?;
            }

            // Finally we emit all other finalized ones.
            while let Some(block) = receive_block(&mut finalized_blocks)
                .await
                .map_err(Box::new)?
            {
                debug!(
                    hash:% = block.hash(),
                    height = block.number(),
                    parent_hash:% = block.header().parent_hash;
                    "block received"
                );

                yield self.make_block(block, &mut authorities, network_id).await?;
            }
        }
    }
}

/// Config for node connection.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub url: String,

    pub genesis_protocol_version: ProtocolVersion,

    #[serde(with = "humantime_serde")]
    pub reconnect_max_delay: Duration,

    pub reconnect_max_attempts: usize,
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
    #[error(transparent)]
    Subxt(#[from] Box<subxt::Error>),

    #[error(transparent)]
    SubxtRcps(#[from] subxt::ext::subxt_rpcs::Error),

    #[error("cannot scale decode")]
    ScaleDecode(#[from] parity_scale_codec::Error),

    #[error(transparent)]
    DecodeProtocolVersion(#[from] ScaleDecodeProtocolVersionError),

    #[error("{0}")]
    Io(&'static str, #[source] io::Error),

    #[error("cannot get contract state: {0}")]
    GetContractState(String),

    #[error("cannot get zswap state root: {0}")]
    GetZswapStateRoot(String),

    #[error("cannot get transaction cost: {0}")]
    GetTransactionCost(#[source] Box<subxt::Error>),

    #[error("block with hash {0} not found")]
    BlockNotFound(BlockHash),

    #[error("invalid protocol version {0}")]
    InvalidProtocolVersion(ProtocolVersion),
}

#[trace]
async fn receive_block(
    finalized_blocks: &mut (impl Stream<Item = Result<SubxtBlock, subxt::Error>> + Unpin),
) -> Result<Option<SubxtBlock>, subxt::Error> {
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

#[allow(clippy::too_many_arguments)]
async fn make_transaction(
    transaction_idx: usize,
    raw_transaction: Vec<u8>,
    block_hash: BlockHash,
    protocol_version: ProtocolVersion,
    created_info_map: &HashMap<TransactionHash, Vec<UtxoInfo>>,
    spent_info_map: &HashMap<TransactionHash, Vec<UtxoInfo>>,
    network_id: NetworkId,
    online_client: &OnlineClient<SubstrateConfig>,
) -> Result<Option<Transaction>, SubxtNodeError> {
    let raw_transaction = match const_hex::decode(raw_transaction) {
        Ok(hex_decoded_transaction) => hex_decoded_transaction,

        Err(error) => {
            warn!(
                error = error.as_chain(),
                block_hash:%,
                transaction_idx;
                "skipping midnight transaction that cannot be hex-decoded"
            );

            return Ok(None);
        }
    };

    let raw = RawTransaction::from(raw_transaction);
    let ledger_transaction =
        deserialize::<LedgerTransaction, _>(&mut raw.as_ref(), network_id.into())
            .map_err(|error| SubxtNodeError::Io("cannot deserialize ledger transaction", error))?;

    let hash: TransactionHash = ledger_transaction.transaction_hash().0.0.into();

    let identifiers = ledger_transaction
        .identifiers()
        .map(|identifier| {
            Ok::<_, SubxtNodeError>(
                identifier
                    .serialize(network_id)
                    .map_err(|error| SubxtNodeError::Io("cannot serialize identifier", error))?
                    .into(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let contract_actions = match &ledger_transaction {
        LedgerTransaction::Standard(standard_transaction) => {
            let contract_actions = standard_transaction.actions().map(|(_, action)| action);

            futures::stream::iter(contract_actions)
                .then(|contract_action| async {
                    ledger_contract_action_into_domain(
                        contract_action,
                        block_hash,
                        network_id,
                        online_client,
                        protocol_version,
                    )
                    .await
                })
                .try_collect::<Vec<_>>()
                .await?
        }

        LedgerTransaction::ClaimMint(_) => vec![],
    };

    let created_unshielded_utxos = created_info_map
        .get(&hash)
        .map_or(&[] as &[_], |v| v.as_slice())
        .iter()
        .map(|info| crate::domain::UnshieldedUtxo {
            creating_transaction_id: 0,
            output_index: info.output_no,
            owner_address: UnshieldedAddress::from(info.address.as_ref()),
            token_type: info.token_type,
            intent_hash: info.intent_hash,
            value: info.value,
        })
        .collect();

    let spent_unshielded_utxos = spent_info_map
        .get(&hash)
        .map_or(&[] as &[_], |v| v.as_slice())
        .iter()
        .map(|info| crate::domain::UnshieldedUtxo {
            creating_transaction_id: 0,
            output_index: info.output_no,
            owner_address: UnshieldedAddress::from(info.address.as_ref()),
            token_type: info.token_type,
            intent_hash: info.intent_hash,
            value: info.value,
        })
        .collect();

    let fees = match runtimes::get_transaction_cost(
        online_client,
        raw.as_ref(),
        block_hash,
        protocol_version,
    )
    .await
    {
        Ok(fees) => crate::domain::TransactionFees {
            paid_fees: fees,
            estimated_fees: fees,
        },

        Err(error) => {
            warn!(
                "runtime API fees calculation failed, using fallback: {}, block_hash = {}, transaction_size = {}",
                error,
                block_hash,
                raw.as_ref().len()
            );
            use crate::domain::TransactionFees;
            TransactionFees::extract_from_ledger_transaction(
                &ledger_transaction,
                raw.as_ref().len(),
            )
        }
    };

    let transaction = Transaction {
        id: 0,
        hash,
        transaction_result: Default::default(),
        protocol_version,
        identifiers,
        contract_actions,
        raw,
        merkle_tree_root: Default::default(),
        start_index: Default::default(),
        end_index: Default::default(),
        created_unshielded_utxos,
        spent_unshielded_utxos,
        paid_fees: fees.paid_fees,
        estimated_fees: fees.estimated_fees,
    };

    Ok(Some(transaction))
}

async fn ledger_contract_action_into_domain(
    contract_action: LedgerContractAction<ProofMarker, DefaultDB>,
    block_hash: BlockHash,
    network_id: NetworkId,
    online_client: &OnlineClient<SubstrateConfig>,
    protocol_version: ProtocolVersion,
) -> Result<ContractAction, SubxtNodeError> {
    match contract_action {
        LedgerContractAction::Call(call) => {
            let address = serialize_address(call.address, network_id)?;
            let state =
                runtimes::get_contract_state(online_client, &address, block_hash, protocol_version)
                    .await?;
            let entry_point = call.entry_point.as_ref().into();

            Ok(ContractAction {
                address,
                state,
                zswap_state: Default::default(),
                attributes: ContractAttributes::Call { entry_point },
            })
        }

        LedgerContractAction::Deploy(deploy) => {
            let address = serialize_address(deploy.address(), network_id)?;
            let state =
                runtimes::get_contract_state(online_client, &address, block_hash, protocol_version)
                    .await?;

            Ok(ContractAction {
                address,
                state,
                zswap_state: Default::default(),
                attributes: ContractAttributes::Deploy,
            })
        }

        LedgerContractAction::Maintain(update) => {
            let address = serialize_address(update.address, network_id)?;
            let state =
                runtimes::get_contract_state(online_client, &address, block_hash, protocol_version)
                    .await?;

            Ok(ContractAction {
                address,
                state,
                zswap_state: Default::default(),
                attributes: ContractAttributes::Update,
            })
        }
    }
}

fn serialize_address(
    address: ContractAddress,
    network_id: NetworkId,
) -> Result<ByteVec, SubxtNodeError> {
    let address = address
        .serialize(network_id)
        .map_err(|error| SubxtNodeError::Io("cannot serialize address", error))?;
    Ok(address.into())
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{BlockInfo, Node, Transaction},
        infra::node::{Config, LedgerTransaction, SubxtNode},
    };
    use assert_matches::assert_matches;
    use fs_extra::dir::{CopyOptions, copy};
    use futures::{StreamExt, TryStreamExt};
    use indexer_common::{
        domain::{NetworkId, PROTOCOL_VERSION_000_013_000, ProtocolVersion},
        error::BoxError,
    };
    use midnight_serialize::deserialize;
    use std::{env, path::Path, pin::pin, time::Duration};
    use testcontainers::{
        GenericImage, ImageExt,
        core::{Mount, WaitFor},
        runners::AsyncRunner,
    };

    #[tokio::test]
    async fn test_finalized_blocks_0_13() -> Result<(), BoxError> {
        test_finalized_blocks(
            PROTOCOL_VERSION_000_013_000,
            Some("alpha.3"),
            "0ac58cba8956851701915291080437a42993e094e0a2792f64c28ebcf977dc7c",
            8,
            "9c5e8f3c01f6691ce4682442fdbd752f40975f165465f631d56d72741da283f8",
            28,
        )
        .await
    }

    async fn test_finalized_blocks(
        genesis_protocol_version: ProtocolVersion,
        suffix: Option<&'static str>,
        before_first_tx_block_hash: &'static str,
        before_first_tx_height: u32,
        first_tx_hash: &'static str,
        last_tx_height: u32,
    ) -> Result<(), BoxError> {
        let mut node_version = genesis_protocol_version.to_string();
        if let Some(suffix) = suffix {
            node_version = format!("{node_version}-{suffix}");
        }

        let node_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.node")
            .join(&node_version)
            .canonicalize()?;
        let tmp_dir = tempfile::tempdir()?;
        copy(node_dir, &tmp_dir, &CopyOptions::default())?;

        let host_path = tmp_dir.path().join(&node_version).display().to_string();
        let node_container = GenericImage::new(
            "ghcr.io/midnight-ntwrk/midnight-node".to_string(),
            node_version,
        )
        .with_wait_for(WaitFor::message_on_stderr("9944"))
        .with_mount(Mount::bind_mount(host_path, "/node"))
        .with_env_var("SHOW_CONFIG", "false")
        .with_env_var("CFG_PRESET", "dev")
        .start()
        .await?;
        let node_port = node_container.get_host_port_ipv4(9944).await?;
        let node_url = format!("ws://localhost:{node_port}");

        let config = Config {
            url: node_url,
            genesis_protocol_version,
            reconnect_max_delay: Duration::from_secs(1),
            reconnect_max_attempts: 3,
        };
        let mut subxt_node = SubxtNode::new(config).await?;

        // Assert that the first block is genesis if we start fresh!

        let mut subxt_node_2 = subxt_node.clone();
        let blocks = subxt_node_2.finalized_blocks(None, NetworkId::Undeployed);
        let mut blocks = pin!(blocks);
        let genesis = blocks.try_next().await?;
        // The genesis block has a "zero" parent hash, i.e. `[0; 32]`.
        assert_matches!(genesis, Some(block) if block.parent_hash == [0; 32].into());

        // Assert that we can start with stored blocks and receive the expected ones.

        let hash = const_hex::decode(before_first_tx_block_hash)
            .expect("block hash can be hex-decoded")
            .try_into()
            .expect("block hash has 32 bytes");
        let blocks = subxt_node.finalized_blocks(
            Some(BlockInfo {
                hash,
                height: before_first_tx_height,
            }),
            NetworkId::Undeployed,
        );

        let blocks = blocks
            .take((last_tx_height - before_first_tx_height) as usize)
            .try_collect::<Vec<_>>()
            .await?;

        let heights = blocks.iter().map(|block| block.height).collect::<Vec<_>>();
        assert_eq!(
            heights,
            (before_first_tx_height + 1..=last_tx_height).collect::<Vec<_>>()
        );

        let transactions = blocks
            .into_iter()
            .flat_map(|block| block.transactions)
            .collect::<Vec<_>>();
        // 6 unshielded token transactions, 1 address, 3 contract actions.
        assert_eq!(transactions.len(), 10);

        assert_matches!(
            transactions.as_slice(),
            [
                Transaction {
                    hash: hash_0,
                    contract_actions: contract_actions_0,
                    ..
                },
                Transaction {..},
                Transaction {..},
                Transaction {..},
                Transaction {..},
                Transaction {..},
                Transaction {..},
                Transaction {
                    contract_actions: contract_actions_1,
                    ..
                },
                Transaction {
                    contract_actions: contract_actions_2,
                    ..
                },
                Transaction {
                    contract_actions: contract_actions_3,
                    ..
                },
            ] if
                hash_0.to_string() == first_tx_hash &&
                contract_actions_0.is_empty() &&
                contract_actions_1.len() == 1 &&
                contract_actions_2.len() == 1 &&
                contract_actions_3.len() == 1
        );
        let ledger_transaction = deserialize::<LedgerTransaction, _>(
            &mut transactions[0].raw.as_ref(),
            NetworkId::Undeployed.into(),
        );
        assert!(ledger_transaction.is_ok());

        Ok(())
    }
}
