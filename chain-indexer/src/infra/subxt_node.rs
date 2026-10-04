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

pub(crate) mod header;
pub mod rpc;
pub(crate) mod runtimes;

use indexer_common::{
    domain::{BlockAuthor, NodeVersion, ProtocolVersionError, ledger},
    error::BoxError,
};
use parity_scale_codec::Decode;
use serde::Deserialize;
use std::{num::NonZeroUsize, time::Duration};
use subxt::config::substrate::{ConsensusEngineId, DigestItem};
use thiserror::Error;

pub(crate) const AURA_ENGINE_ID: ConsensusEngineId = [b'a', b'u', b'r', b'a'];
pub(crate) const BABE_ENGINE_ID: ConsensusEngineId = [b'B', b'A', b'B', b'E'];

/// Name of the node runtime API reporting the active block-production engine, declared in
/// `midnight-primitives-consensus-engine` and implemented alongside the pallet driving the
/// Aura→BABE transition. Its presence in a block's runtime guarantees the correctness of Aura
/// and BABE pre-runtime digests during the transition, so BABE digests are only trusted for
/// author derivation where it exists.
pub(crate) const CONSENSUS_ENGINE_RUNTIME_API: &str = "ConsensusEngineApi";

/// Config for node connection.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub url: String,

    #[serde(with = "humantime_serde")]
    pub reconnect_max_delay: Duration,

    pub reconnect_max_attempts: usize,

    /// How long the finalized-block subscription (`chainHead_v1_follow`) may go without an event
    /// before it is renewed. Defaults to 30 seconds.
    #[serde(
        with = "humantime_serde",
        default = "default_subscription_recovery_timeout"
    )]
    pub subscription_recovery_timeout: Duration,

    /// The most heights per chunk the block sourcing pipeline sources at once. Keep it well above
    /// `application.decode_cpu_threads`, e.g. at least 8 times, so that one chunk keeps the decode
    /// threads busy. Defaults to 64.
    #[serde(default = "default_source_chunk_size")]
    pub source_chunk_size: NonZeroUsize,
    /// The most chunks in progress, and the most sourced chunks waiting to be decoded. Blocks held
    /// in memory are bounded by about twice this many chunks. Defaults to 8.
    #[serde(default = "default_source_chunks_ahead")]
    pub source_chunks_ahead: NonZeroUsize,
    /// The most calls per JSON-RPC batch. A node or proxy may reject large batches: public
    /// endpoints accept 64 over WebSocket, the transport used. Defaults to 64.
    #[serde(default = "default_rpc_batch_size")]
    pub rpc_batch_size: NonZeroUsize,
    /// The most JSON-RPC batches in flight on the connection. Defaults to 16.
    #[serde(default = "default_rpc_batches_in_flight")]
    pub rpc_batches_in_flight: NonZeroUsize,
}

fn default_subscription_recovery_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_source_chunk_size() -> NonZeroUsize {
    NonZeroUsize::new(64).expect("64 is not zero")
}

fn default_source_chunks_ahead() -> NonZeroUsize {
    NonZeroUsize::new(8).expect("8 is not zero")
}

fn default_rpc_batch_size() -> NonZeroUsize {
    NonZeroUsize::new(64).expect("64 is not zero")
}

fn default_rpc_batches_in_flight() -> NonZeroUsize {
    NonZeroUsize::new(16).expect("16 is not zero")
}

/// Error decoding a block's runtime data.
#[derive(Debug, Error)]
pub enum SubxtNodeError {
    #[error("cannot get next extrinsic")]
    GetNextExtrinsic(#[source] Box<subxt::error::ExtrinsicDecodeErrorAt>),

    #[error("cannot decode extrinsic as call")]
    DecodeExtrinsicAsCall(#[source] Box<subxt::error::ExtrinsicError>),

    #[error("cannot get next event")]
    GetNextEvent(#[source] Box<subxt::error::EventsError>),

    #[error("cannot decode subxt event as midnight event")]
    DecodeEvent(#[source] Box<subxt::error::EventsError>),

    #[error("cannot decode bridge recipient from c2m-bridge event")]
    DecodeBridgeRecipient(#[from] indexer_common::domain::bridge::BridgeRecipientError),

    #[error("invalid BABE pre-runtime digest variant tag {0}")]
    InvalidBabePreDigestTag(u8),

    #[error("cannot get zswap state root")]
    GetZswapStateRoot(#[source] BoxError),

    #[error("cannot get D-Parameter")]
    GetDParameter(#[source] BoxError),

    #[error("cannot get Terms and Conditions")]
    GetTermsAndConditions(#[source] BoxError),

    #[error("cannot get ledger state root")]
    GetLedgerStateRoot(#[source] BoxError),

    #[error("cannot decode storage")]
    DecodeStorage(#[source] BoxError),

    #[error(transparent)]
    ProtocolVersion(#[from] ProtocolVersionError),

    #[error("cannot scale decode")]
    ScaleDecode(#[from] parity_scale_codec::Error),

    #[error(transparent)]
    Ledger(#[from] ledger::Error),
}

/// Determine the block author from the pre-runtime digest logs, taking the first log with a
/// recognized consensus engine that yields an author, in digest order (mirroring polkadot-js
/// `extractAuthor`): Aura carries the slot (the author is the slot modulo the authority-set
/// length), BABE carries the authority index explicitly in all of its pre-digest variants. BABE
/// digests are only recognized if `babe_supported`, i.e. if the block's runtime guarantees
/// their correctness (see [CONSENSUS_ENGINE_RUNTIME_API]); otherwise they are skipped like any
/// unrecognized engine.
pub(crate) fn author_from_digest_logs(
    logs: &[DigestItem],
    authorities: &[[u8; 32]],
    content_node_version: NodeVersion,
    babe_supported: bool,
) -> Result<Option<BlockAuthor>, SubxtNodeError> {
    if authorities.is_empty() {
        return Ok(None);
    }

    for log in logs {
        let DigestItem::PreRuntime(engine_id, pre_digest) = log else {
            continue;
        };

        let author = match *engine_id {
            AURA_ENGINE_ID => {
                let slot = runtimes::decode_slot(pre_digest, content_node_version)?;
                let index = slot % authorities.len() as u64;
                authorities.get(index as usize).copied().map(Into::into)
            }

            BABE_ENGINE_ID if babe_supported => babe_author(pre_digest, authorities)?,

            _ => None,
        };

        if author.is_some() {
            return Ok(author);
        }
    }

    Ok(None)
}

/// Determine the block author from a BABE pre-runtime digest. An out-of-range authority index
/// means the cached authority set does not match the block's epoch; report an unknown author
/// instead of failing block processing.
fn babe_author(
    pre_digest: &[u8],
    authorities: &[[u8; 32]],
) -> Result<Option<BlockAuthor>, SubxtNodeError> {
    let index = decode_babe_authority_index(pre_digest)?;

    let author = usize::try_from(index)
        .ok()
        .and_then(|index| authorities.get(index))
        .copied()
        .map(Into::into);

    Ok(author)
}

/// Extract the authority index from a BABE pre-runtime digest. All `PreDigest` variants
/// (`Primary` = 1, `SecondaryPlain` = 2, `SecondaryVRF` = 3, see `sp_consensus_babe::digests`)
/// lead with the SCALE-encoded `authority_index: u32` right after the variant tag, so only that
/// prefix is decoded and the remainder (slot, VRF signature) is ignored.
fn decode_babe_authority_index(mut pre_digest: &[u8]) -> Result<u32, SubxtNodeError> {
    let tag = u8::decode(&mut pre_digest)?;
    if !(1..=3).contains(&tag) {
        return Err(SubxtNodeError::InvalidBabePreDigestTag(tag));
    }

    Ok(u32::decode(&mut pre_digest)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parity_scale_codec::Encode;

    const AUTHORITIES: [[u8; 32]; 3] = [[1; 32], [2; 32], [3; 32]];

    /// A BABE pre-digest prefix: variant tag, then the SCALE-encoded authority index, then
    /// trailing payload (slot, VRF signature) which must be ignored.
    fn babe_pre_digest(tag: u8, authority_index: u32) -> Vec<u8> {
        let mut pre_digest = vec![tag];
        pre_digest.extend(authority_index.encode());
        pre_digest.extend([0xff; 8]);
        pre_digest
    }

    #[test]
    fn author_from_aura_digest() {
        let logs = vec![DigestItem::PreRuntime(AURA_ENGINE_ID, 4u64.encode())];

        let author = author_from_digest_logs(&logs, &AUTHORITIES, NodeVersion::V2_0, false)
            .expect("author can be determined");

        assert_eq!(author, Some([2; 32].into()));
    }

    #[test]
    fn babe_author_for_all_variants() {
        for tag in 1..=3 {
            let author = babe_author(&babe_pre_digest(tag, 2), &AUTHORITIES)
                .expect("author can be determined");

            assert_eq!(author, Some([3; 32].into()));
        }
    }

    #[test]
    fn babe_digest_is_skipped_if_babe_not_supported() {
        let logs = vec![DigestItem::PreRuntime(
            BABE_ENGINE_ID,
            babe_pre_digest(2, 2),
        )];
        let author = author_from_digest_logs(&logs, &AUTHORITIES, NodeVersion::V2_0, false)
            .expect("skipped digest is not an error");
        assert_eq!(author, None);

        let logs = vec![
            DigestItem::PreRuntime(BABE_ENGINE_ID, babe_pre_digest(2, 2)),
            DigestItem::PreRuntime(AURA_ENGINE_ID, 4u64.encode()),
        ];
        let author = author_from_digest_logs(&logs, &AUTHORITIES, NodeVersion::V2_0, false)
            .expect("author can be determined");
        assert_eq!(author, Some([2; 32].into()));
    }

    #[test]
    fn first_pre_runtime_digest_in_digest_order_wins() {
        let logs = vec![
            DigestItem::PreRuntime(BABE_ENGINE_ID, babe_pre_digest(2, 2)),
            DigestItem::PreRuntime(AURA_ENGINE_ID, 4u64.encode()),
        ];
        let author = author_from_digest_logs(&logs, &AUTHORITIES, NodeVersion::V2_0, true)
            .expect("author can be determined");
        assert_eq!(author, Some([3; 32].into()));

        let logs = vec![
            DigestItem::PreRuntime(AURA_ENGINE_ID, 4u64.encode()),
            DigestItem::PreRuntime(BABE_ENGINE_ID, babe_pre_digest(2, 2)),
        ];
        let author = author_from_digest_logs(&logs, &AUTHORITIES, NodeVersion::V2_0, true)
            .expect("author can be determined");
        assert_eq!(author, Some([2; 32].into()));
    }

    #[test]
    fn unrecognized_engine_is_skipped() {
        let logs = vec![
            DigestItem::PreRuntime(*b"test", vec![0xaa]),
            DigestItem::PreRuntime(AURA_ENGINE_ID, 4u64.encode()),
        ];

        let author = author_from_digest_logs(&logs, &AUTHORITIES, NodeVersion::V2_0, true)
            .expect("author can be determined");

        assert_eq!(author, Some([2; 32].into()));
    }

    #[test]
    fn babe_out_of_range_authority_index_yields_no_author() {
        let author = babe_author(&babe_pre_digest(2, 7), &AUTHORITIES)
            .expect("out-of-range index is not an error");

        assert_eq!(author, None);
    }

    #[test]
    fn invalid_babe_pre_digest_tag_is_an_error() {
        for tag in [0, 4] {
            let author = babe_author(&babe_pre_digest(tag, 2), &AUTHORITIES);

            assert!(matches!(
                author,
                Err(SubxtNodeError::InvalidBabePreDigestTag(t)) if t == tag
            ));
        }
    }

    #[test]
    fn truncated_babe_pre_digest_is_an_error() {
        let author = babe_author(&[1, 0xaa], &AUTHORITIES);

        assert!(matches!(author, Err(SubxtNodeError::ScaleDecode(_))));
    }

    #[test]
    fn no_pre_runtime_digest_yields_no_author() {
        let author = author_from_digest_logs(&[], &AUTHORITIES, NodeVersion::V2_0, true)
            .expect("no digest is not an error");

        assert_eq!(author, None);
    }

    #[test]
    fn empty_authorities_yield_no_author() {
        let logs = vec![DigestItem::PreRuntime(AURA_ENGINE_ID, 4u64.encode())];

        let author = author_from_digest_logs(&logs, &[], NodeVersion::V2_0, true)
            .expect("empty authorities are not an error");

        assert_eq!(author, None);
    }
}
