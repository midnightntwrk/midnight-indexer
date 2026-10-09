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

use crate::{
    domain::{self, storage::Storage},
    infra::api::{
        ApiError, ApiResult, ContextExt, ResultExt,
        v4::{HexEncoded, block::BlockOffset, contract_action::ContractAction, resolve_height},
    },
};
use async_graphql::{Context, Subscription};
use async_stream::try_stream;
use fastrace::{Span, future::FutureExt, prelude::SpanContext};
use futures::{Stream, TryStreamExt};
use indexer_common::domain::{BlockIndexed, Subscriber};
use log::{debug, warn};
use std::{collections::VecDeque, pin::pin};

pub struct ContractActionSubscription<S, B> {
    _storage: std::marker::PhantomData<S>,
    _subscriber: std::marker::PhantomData<B>,
}

impl<S, B> Default for ContractActionSubscription<S, B> {
    fn default() -> Self {
        Self {
            _storage: std::marker::PhantomData,
            _subscriber: std::marker::PhantomData,
        }
    }
}

#[Subscription]
impl<S, B> ContractActionSubscription<S, B>
where
    S: Storage,
    B: Subscriber,
{
    /// Subscribe to contract actions with the given address starting at the given offset or at the
    /// latest block if the offset is omitted.
    ///
    /// At a ledger hard fork, a contract without an action in the fork block has its latest action
    /// re-emitted with the translated `state` and `stateAt` set to the fork block.
    async fn contract_actions<'a>(
        &self,
        cx: &'a Context<'a>,
        address: HexEncoded,
        offset: Option<BlockOffset>,
    ) -> Result<impl Stream<Item = ApiResult<ContractAction<S>>> + use<'a, S, B>, ApiError> {
        let address = address
            .hex_decode()
            .map_err_into_client_error(|| "invalid address")?;

        let quota_guard = cx
            .get_subscription_quotas()
            .try_acquire(cx.get_per_connection_counter(), None)
            .map_err_into_client_error(|| "subscription limit exceeded")?;

        let storage = cx.get_storage::<S>();
        let subscriber = cx.get_subscriber::<B>();
        let batch_size = cx.get_subscription_config().contract_actions.batch_size;

        let block_indexed_stream = subscriber.subscribe::<BlockIndexed>();
        let height = resolve_height::<S>(offset, cx).await?;
        let mut contract_action_id = storage
            .get_contract_action_id_by_block_height(height)
            .await
            .map_err_into_server_error(|| {
                format!("get contract action id by block height {height}")
            })?;

        // The replay serves the blocks up to this tip, the live side those after it. Translations
        // are read after the tip, so they cover it.
        let tip = storage
            .get_latest_block()
            .await
            .map_err_into_server_error(|| "get latest block")?;
        let mut seen = tip
            .map(|block| Seen {
                height: block.height,
                protocol_version: Some(block.protocol_version.into()),
            })
            .unwrap_or_default();
        let translations = storage
            .get_contract_state_translations_by_address(&address)
            .await
            .map_err_into_server_error(|| {
                format!("get contract state translations for address {address}")
            })?;
        let mut replay_translations = pending_translations(translations, height);

        let contract_actions = try_stream! {
            let _hold = quota_guard;

            // Stream existing contract actions up to the tip, each preceded by the translations
            // due.
            debug!(contract_action_id; "streaming existing contract actions");

            let contract_actions = storage.get_contract_actions_by_address(
                &address,
                contract_action_id,
                batch_size,
            );
            let mut contract_actions = pin!(contract_actions);
            while let Some(contract_action) = get_next_contract_action(&mut contract_actions)
                .await
                .map_err_into_server_error(|| {
                    format!("get next contract action for ID {contract_action_id}")
                })?
                .filter(|contract_action| contract_action.block.height <= seen.height)
            {
                for translation in due(&mut replay_translations, contract_action.block.height) {
                    yield translation.into();
                }
                contract_action_id = contract_action.action.id + 1;
                yield contract_action.into();
            }
            for translation in due(&mut replay_translations, seen.height) {
                yield translation.into();
            }
            // Any left were recorded after the tip: the live side reads those by range.
            drop(replay_translations);

            // Stream live contract actions up to each message's block, preceded on a protocol
            // version change by the translations recorded since the last block seen.
            debug!(contract_action_id; "streaming live contract actions");
            let mut block_indexed_stream = pin!(block_indexed_stream);
            while let Some(BlockIndexed { height, protocol_version, .. }) = block_indexed_stream
                .try_next()
                .await
                .map_err_into_server_error(|| "get next BlockIndexed event")?
            {
                let height = u32::try_from(height)
                    .map_err_into_server_error(|| format!("block height {height} out of range"))?;
                if height <= seen.height {
                    continue;
                }

                debug!(height; "streaming next contract actions");

                let mut live_translations = match seen.translation_range(protocol_version) {
                    Some(after) => storage
                        .get_contract_state_translations_between(&address, after, height)
                        .await
                        .map_err_into_server_error(|| {
                            format!("get contract state translations after height {after} through {height}")
                        })?
                        .into(),

                    None => VecDeque::new(),
                };

                let contract_actions = storage.get_contract_actions_by_address(
                    &address,
                    contract_action_id,
                    batch_size,
                    );
                let mut contract_actions = pin!(contract_actions);

                while let Some(contract_action) = get_next_contract_action(&mut contract_actions)
                    .await
                    .map_err_into_server_error(|| {
                        format!("get next contract action for ID {contract_action_id}")
                    })?
                    .filter(|contract_action| contract_action.block.height <= height)
                {
                    for translation in due(&mut live_translations, contract_action.block.height) {
                        yield translation.into();
                    }
                    contract_action_id = contract_action.action.id + 1;
                    yield contract_action.into();
                }
                for translation in due(&mut live_translations, height) {
                    yield translation.into();
                }

                seen = Seen {
                    height,
                    protocol_version: Some(protocol_version),
                };
            }

            warn!("stream of BlockIndexed events completed unexpectedly");
        };

        Ok(contract_actions)
    }
}

/// The last block a stream has served and its protocol version.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Seen {
    height: u32,
    protocol_version: Option<u32>,
}

impl Seen {
    /// The height after which to read translations for a block with the given protocol version: the
    /// last block seen if the version changed, otherwise none. Ranging from the last block seen
    /// covers a lost message.
    fn translation_range(&self, protocol_version: u32) -> Option<u32> {
        (self.protocol_version != Some(protocol_version)).then_some(self.height)
    }
}

/// The translations recorded at or after `start_height`, oldest first.
fn pending_translations(
    translations: Vec<domain::ContractActionAtBlock>,
    start_height: u32,
) -> VecDeque<domain::ContractActionAtBlock> {
    translations
        .into_iter()
        .filter(|translation| translation.block.height >= start_height)
        .collect()
}

/// Remove and return the pending translations recorded at or before `height`, in order.
fn due(
    pending: &mut VecDeque<domain::ContractActionAtBlock>,
    height: u32,
) -> Vec<domain::ContractActionAtBlock> {
    let mut due = Vec::new();
    while pending
        .front()
        .is_some_and(|translation| translation.block.height <= height)
    {
        due.extend(pending.pop_front());
    }
    due
}

async fn get_next_contract_action<E>(
    contract_actions: &mut (impl Stream<Item = Result<domain::ContractActionAtBlock, E>> + Unpin),
) -> Result<Option<domain::ContractActionAtBlock>, E> {
    contract_actions
        .try_next()
        .in_span(Span::root(
            "subscription.contract-actions.get-next-contract-action",
            SpanContext::random(),
        ))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexer_common::domain::{ContractAttributes, ProtocolVersion};

    fn translation(id: u64, translated_at: u32) -> domain::ContractActionAtBlock {
        domain::ContractActionAtBlock {
            action: domain::ContractAction {
                id,
                address: vec![0xa; 32].into(),
                state_key: Some(vec![0x80].into()),
                attributes: ContractAttributes::Deploy,
                zswap_state_key: None,
                transaction_id: id,
                translated_at: None,
            },
            block: domain::BlockReference {
                height: translated_at,
                hash: [0; 32].into(),
                protocol_version: ProtocolVersion::try_from(2_000_000_u32)
                    .expect("a known protocol version"),
            },
        }
    }

    fn heights(actions: &[domain::ContractActionAtBlock]) -> Vec<u32> {
        actions.iter().map(|action| action.block.height).collect()
    }

    /// Translations interleave with actions by height.
    #[test]
    fn translations_are_due_before_the_first_action_at_or_after_their_block() {
        let mut pending = pending_translations(vec![translation(1, 500), translation(1, 900)], 0);

        assert!(due(&mut pending, 100).is_empty());
        assert_eq!(heights(&due(&mut pending, 700)), [500]);
        assert!(
            due(&mut pending, 700).is_empty(),
            "each translation is due once"
        );
        assert_eq!(heights(&due(&mut pending, 1000)), [900]);
        assert!(pending.is_empty());
    }

    /// A replay ending before a translation does not emit it.
    #[test]
    fn a_replay_ending_before_a_translation_does_not_emit_it() {
        let mut pending = pending_translations(vec![translation(1, 500), translation(1, 900)], 0);

        assert_eq!(heights(&due(&mut pending, 800)), [500]);
        assert_eq!(heights(&pending.iter().cloned().collect::<Vec<_>>()), [900]);
    }

    /// Translations are read from the last block seen, only on a protocol version change.
    #[test]
    fn translations_are_looked_for_from_the_last_block_seen_across_a_version_change() {
        let seen = Seen {
            height: 499,
            protocol_version: Some(1_000_000),
        };
        assert_eq!(seen.translation_range(1_000_000), None);
        assert_eq!(seen.translation_range(2_000_000), Some(499));

        assert_eq!(Seen::default().translation_range(1_000_000), Some(0));
    }

    /// A stream from the fork block replays its translation; one from after it does not.
    #[test]
    fn a_stream_starting_after_a_translation_does_not_replay_it() {
        let translations = vec![translation(1, 500), translation(1, 900)];

        assert_eq!(
            heights(
                &pending_translations(translations.clone(), 500)
                    .into_iter()
                    .collect::<Vec<_>>()
            ),
            [500, 900]
        );
        assert_eq!(
            heights(
                &pending_translations(translations.clone(), 501)
                    .into_iter()
                    .collect::<Vec<_>>()
            ),
            [900]
        );
        assert!(pending_translations(translations, 901).is_empty());
    }
}
