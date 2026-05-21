// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! GraphQL types for the `contractEvents` query and subscription surface
//! (public contract events, MIP-107 / CoIP-442).
//!
//! `ContractEvent` is the polymorphic interface; concrete types per
//! `LogEventType` variant (`ShieldedSpendEvent`, `MiscContractEvent`, etc.)
//! implement it. Clients discriminate via `__typename`. See
//! `docs/interactions/gh-tasks/8may/contract-events-graphql-draft-v0.7.graphql`
//! for the canonical reference shape.

use crate::{
    domain::ContractEventRow,
    infra::api::v4::{HexEncodable, HexEncoded},
};
use async_graphql::{Enum, InputObject, Interface, SimpleObject};
use indexer_common::domain::{AddressOrContract as DomainAddressOrContract, LedgerEventAttributes};
use thiserror::Error;

/// Tagged-union helper for fields like `Either<ZswapCoinPublicKey, ContractAddress>`
/// used in standard unshielded events.
///
/// Exactly one of `userAddress` or `contractAddress` is non-null; the `kind`
/// discriminator says which.
#[derive(Debug, Clone, SimpleObject)]
pub struct AddressOrContract {
    pub kind: AddressOrContractKind,
    /// Bech32m-encoded user address; populated when kind = USER.
    /// Hex-encoded here at the wire level; clients re-encode if needed.
    pub user_address: Option<HexEncoded>,
    /// Hex-encoded contract address; populated when kind = CONTRACT.
    pub contract_address: Option<HexEncoded>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum AddressOrContractKind {
    User,
    Contract,
}

impl From<DomainAddressOrContract> for AddressOrContract {
    fn from(domain: DomainAddressOrContract) -> Self {
        match domain {
            DomainAddressOrContract::User(b) => Self {
                kind: AddressOrContractKind::User,
                user_address: Some(b.hex_encode()),
                contract_address: None,
            },
            DomainAddressOrContract::Contract(b) => Self {
                kind: AddressOrContractKind::Contract,
                user_address: None,
                contract_address: Some(b.hex_encode()),
            },
        }
    }
}

/// Closed enum of contract event types the indexer surfaces. Used in filter
/// input only, response discrimination is via `__typename`. Mirrors the
/// 11 variants of `LogEventType` (onchain-vm/src/ops.rs, ledger-9 alpha).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum ContractEventType {
    ShieldedSpend,
    ShieldedReceive,
    ShieldedMint,
    ShieldedBurn,
    UnshieldedSpend,
    UnshieldedReceive,
    UnshieldedMint,
    UnshieldedBurn,
    Paused,
    Unpaused,
    Misc,
}

/// Prefix filter on an indexed field of a standard event. Indexer resolves
/// `fieldName` for all standard events from the variant; no descriptor needed.
/// Not supported on Misc events.
#[derive(Debug, Clone, InputObject)]
pub struct FieldPrefixFilter {
    /// Field name (e.g. `nullifier`, `commitment`, `sender`). Must match an
    /// indexed field of the filtered event type.
    pub field_name: String,
    /// Hex-encoded prefix bytes. Empty string matches all values; otherwise
    /// the indexer returns events whose field value starts with this prefix,
    /// client filters to exact match if needed.
    pub prefix: HexEncoded,
}

/// Filter for contract events queries and subscriptions. Block-range bounds
/// live here so the same shape works for both (per Andrzej 21 May review).
#[derive(Debug, Clone, InputObject)]
pub struct ContractEventFilter {
    /// Required: the contract address to filter events for.
    pub contract_address: HexEncoded,
    /// Optional: filter to a subset of contract event types. Indexer translates
    /// to `variant = ANY(...)` against the indexed variant column.
    pub types: Option<Vec<ContractEventType>>,
    /// Optional: prefix-match on indexed fields of the event. Standard events only.
    pub field_prefixes: Option<Vec<FieldPrefixFilter>>,
    /// Optional: lower bound on the block height an event was emitted in. On
    /// subscription, acts as a starting cursor (alternative to `id`).
    pub from_block: Option<u32>,
    /// Optional: upper bound on the block height an event was emitted in. On
    /// subscription, terminates the stream once the chain reaches this block.
    pub to_block: Option<u32>,
}

/// Common interface implemented by every concrete contract event type.
#[derive(Debug, Interface)]
#[allow(clippy::duplicated_attributes)]
#[graphql(
    field(name = "id", ty = "&u64"),
    field(name = "raw", ty = "&HexEncoded"),
    field(name = "max_id", ty = "&u64"),
    field(name = "protocol_version", ty = "&u32"),
    field(name = "version", ty = "&u32"),
    field(name = "contract_address", ty = "&HexEncoded"),
    field(name = "transaction_id", ty = "&u64")
)]
pub enum ContractEvent {
    ShieldedSpend(ShieldedSpendEvent),
    ShieldedReceive(ShieldedReceiveEvent),
    ShieldedMint(ShieldedMintEvent),
    ShieldedBurn(ShieldedBurnEvent),
    UnshieldedSpend(UnshieldedSpendEvent),
    UnshieldedReceive(UnshieldedReceiveEvent),
    UnshieldedMint(UnshieldedMintEvent),
    UnshieldedBurn(UnshieldedBurnEvent),
    Paused(PausedEvent),
    Unpaused(UnpausedEvent),
    Misc(MiscContractEvent),
}

// ============================================================================
// Shared base data extracted once per row, then folded into each concrete type.
// ============================================================================

struct Base {
    id: u64,
    raw: HexEncoded,
    max_id: u64,
    protocol_version: u32,
    version: u32,
    contract_address: HexEncoded,
    transaction_id: u64,
}

impl Base {
    fn from_row(row: &ContractEventRow, version: u32) -> Self {
        Self {
            id: row.id,
            raw: row.raw.hex_encode(),
            max_id: row.max_id,
            protocol_version: row.protocol_version.into(),
            version,
            contract_address: row.contract_address.hex_encode(),
            transaction_id: row.transaction_id,
        }
    }
}

// ============================================================================
// Shielded concrete types
// ============================================================================

#[derive(Debug, SimpleObject)]
pub struct ShieldedSpendEvent {
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    /// Indexed.
    pub nullifier: HexEncoded,
}

#[derive(Debug, SimpleObject)]
pub struct ShieldedReceiveEvent {
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    /// Indexed.
    pub commitment: HexEncoded,
    /// Indexed. Optional ciphertext for shielded coin receipt
    /// (Maybe<Bytes<512>>). Hex-encoded, up to 512 bytes.
    pub ciphertext: Option<HexEncoded>,
    /// Set when received by a contract; null for user recipients
    /// (Maybe<ContractAddress>). Renamed from `contractAddress` in the CoIP
    /// to avoid collision with the top-level emitting `contractAddress`
    /// inherited from the ContractEvent interface.
    pub receiving_contract_address: Option<HexEncoded>,
}

#[derive(Debug, SimpleObject)]
pub struct ShieldedMintEvent {
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    /// Indexed.
    pub commitment: HexEncoded,
    /// Indexed (per Andrzej, useful for token-type queries).
    pub domain_sep: HexEncoded,
    /// Optional, hidden in some shielded mints (Maybe<Uint<128>>).
    pub amount: Option<String>,
}

#[derive(Debug, SimpleObject)]
pub struct ShieldedBurnEvent {
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    /// Indexed.
    pub nullifier: HexEncoded,
    /// Optional, hidden in some shielded burns (Maybe<Uint<128>>).
    pub amount: Option<String>,
}

// ============================================================================
// Unshielded concrete types (CoIP-442 head: Spend/Receive feature both
// domainSep + tokenType; Burn features tokenType only; Mint features both).
// ============================================================================

#[derive(Debug, SimpleObject)]
pub struct UnshieldedSpendEvent {
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    /// Indexed.
    pub sender: AddressOrContract,
    /// Indexed.
    pub domain_sep: HexEncoded,
    /// Indexed; matches existing unshielded_utxos.token_type index.
    pub token_type: HexEncoded,
    pub amount: String,
}

#[derive(Debug, SimpleObject)]
pub struct UnshieldedReceiveEvent {
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    /// Indexed.
    pub recipient: AddressOrContract,
    /// Indexed.
    pub domain_sep: HexEncoded,
    /// Indexed; matches existing unshielded_utxos.token_type index.
    pub token_type: HexEncoded,
    pub amount: String,
}

#[derive(Debug, SimpleObject)]
pub struct UnshieldedMintEvent {
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    /// Indexed.
    pub domain_sep: HexEncoded,
    /// Indexed; matches existing unshielded_utxos.token_type index.
    pub token_type: HexEncoded,
    pub amount: String,
}

#[derive(Debug, SimpleObject)]
pub struct UnshieldedBurnEvent {
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    /// Indexed.
    pub sender: AddressOrContract,
    /// Indexed; matches existing unshielded_utxos.token_type index.
    pub token_type: HexEncoded,
    pub amount: String,
}

// ============================================================================
// Signal-only events (empty payload structs `Paused`, `Unpaused`).
// ============================================================================

#[derive(Debug, SimpleObject)]
pub struct PausedEvent {
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
}

#[derive(Debug, SimpleObject)]
pub struct UnpausedEvent {
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
}

// ============================================================================
// Custom (Misc) event — opaque payload up to 256 bytes.
// ============================================================================

#[derive(Debug, SimpleObject)]
pub struct MiscContractEvent {
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    /// Hex-encoded contract-defined event name (Compact Bytes<32>).
    pub name: HexEncoded,
    /// Hex-encoded opaque payload (Compact Bytes<256>); consumer brings
    /// descriptor to decode.
    pub payload: HexEncoded,
}

// ============================================================================
// TryFrom: discriminate via attributes, fold base + per-variant fields.
// ============================================================================

impl TryFrom<ContractEventRow> for ContractEvent {
    type Error = UnexpectedContractEvent;

    fn try_from(row: ContractEventRow) -> Result<Self, Self::Error> {
        use LedgerEventAttributes::*;
        match &row.attributes {
            ContractShieldedSpend {
                version, nullifier, ..
            } => {
                let base = Base::from_row(&row, *version);
                Ok(ContractEvent::ShieldedSpend(ShieldedSpendEvent {
                    id: base.id,
                    raw: base.raw,
                    max_id: base.max_id,
                    protocol_version: base.protocol_version,
                    version: base.version,
                    contract_address: base.contract_address,
                    transaction_id: base.transaction_id,
                    nullifier: nullifier.hex_encode(),
                }))
            }

            ContractShieldedReceive {
                version,
                commitment,
                ciphertext,
                receiving_contract_address,
                ..
            } => {
                let base = Base::from_row(&row, *version);
                Ok(ContractEvent::ShieldedReceive(ShieldedReceiveEvent {
                    id: base.id,
                    raw: base.raw,
                    max_id: base.max_id,
                    protocol_version: base.protocol_version,
                    version: base.version,
                    contract_address: base.contract_address,
                    transaction_id: base.transaction_id,
                    commitment: commitment.hex_encode(),
                    ciphertext: ciphertext.as_ref().map(|b| b.hex_encode()),
                    receiving_contract_address: receiving_contract_address
                        .as_ref()
                        .map(|b| b.hex_encode()),
                }))
            }

            ContractShieldedMint {
                version,
                commitment,
                domain_sep,
                amount,
                ..
            } => {
                let base = Base::from_row(&row, *version);
                Ok(ContractEvent::ShieldedMint(ShieldedMintEvent {
                    id: base.id,
                    raw: base.raw,
                    max_id: base.max_id,
                    protocol_version: base.protocol_version,
                    version: base.version,
                    contract_address: base.contract_address,
                    transaction_id: base.transaction_id,
                    commitment: commitment.hex_encode(),
                    domain_sep: domain_sep.hex_encode(),
                    amount: amount.clone(),
                }))
            }

            ContractShieldedBurn {
                version,
                nullifier,
                amount,
                ..
            } => {
                let base = Base::from_row(&row, *version);
                Ok(ContractEvent::ShieldedBurn(ShieldedBurnEvent {
                    id: base.id,
                    raw: base.raw,
                    max_id: base.max_id,
                    protocol_version: base.protocol_version,
                    version: base.version,
                    contract_address: base.contract_address,
                    transaction_id: base.transaction_id,
                    nullifier: nullifier.hex_encode(),
                    amount: amount.clone(),
                }))
            }

            ContractUnshieldedSpend {
                version,
                sender,
                domain_sep,
                token_type,
                amount,
                ..
            } => {
                let base = Base::from_row(&row, *version);
                Ok(ContractEvent::UnshieldedSpend(UnshieldedSpendEvent {
                    id: base.id,
                    raw: base.raw,
                    max_id: base.max_id,
                    protocol_version: base.protocol_version,
                    version: base.version,
                    contract_address: base.contract_address,
                    transaction_id: base.transaction_id,
                    sender: sender.clone().into(),
                    domain_sep: domain_sep.hex_encode(),
                    token_type: token_type.hex_encode(),
                    amount: amount.clone(),
                }))
            }

            ContractUnshieldedReceive {
                version,
                recipient,
                domain_sep,
                token_type,
                amount,
                ..
            } => {
                let base = Base::from_row(&row, *version);
                Ok(ContractEvent::UnshieldedReceive(UnshieldedReceiveEvent {
                    id: base.id,
                    raw: base.raw,
                    max_id: base.max_id,
                    protocol_version: base.protocol_version,
                    version: base.version,
                    contract_address: base.contract_address,
                    transaction_id: base.transaction_id,
                    recipient: recipient.clone().into(),
                    domain_sep: domain_sep.hex_encode(),
                    token_type: token_type.hex_encode(),
                    amount: amount.clone(),
                }))
            }

            ContractUnshieldedMint {
                version,
                domain_sep,
                token_type,
                amount,
                ..
            } => {
                let base = Base::from_row(&row, *version);
                Ok(ContractEvent::UnshieldedMint(UnshieldedMintEvent {
                    id: base.id,
                    raw: base.raw,
                    max_id: base.max_id,
                    protocol_version: base.protocol_version,
                    version: base.version,
                    contract_address: base.contract_address,
                    transaction_id: base.transaction_id,
                    domain_sep: domain_sep.hex_encode(),
                    token_type: token_type.hex_encode(),
                    amount: amount.clone(),
                }))
            }

            ContractUnshieldedBurn {
                version,
                sender,
                token_type,
                amount,
                ..
            } => {
                let base = Base::from_row(&row, *version);
                Ok(ContractEvent::UnshieldedBurn(UnshieldedBurnEvent {
                    id: base.id,
                    raw: base.raw,
                    max_id: base.max_id,
                    protocol_version: base.protocol_version,
                    version: base.version,
                    contract_address: base.contract_address,
                    transaction_id: base.transaction_id,
                    sender: sender.clone().into(),
                    token_type: token_type.hex_encode(),
                    amount: amount.clone(),
                }))
            }

            ContractPaused { version, .. } => {
                let base = Base::from_row(&row, *version);
                Ok(ContractEvent::Paused(PausedEvent {
                    id: base.id,
                    raw: base.raw,
                    max_id: base.max_id,
                    protocol_version: base.protocol_version,
                    version: base.version,
                    contract_address: base.contract_address,
                    transaction_id: base.transaction_id,
                }))
            }

            ContractUnpaused { version, .. } => {
                let base = Base::from_row(&row, *version);
                Ok(ContractEvent::Unpaused(UnpausedEvent {
                    id: base.id,
                    raw: base.raw,
                    max_id: base.max_id,
                    protocol_version: base.protocol_version,
                    version: base.version,
                    contract_address: base.contract_address,
                    transaction_id: base.transaction_id,
                }))
            }

            ContractMisc {
                version,
                name,
                payload,
                ..
            } => {
                let base = Base::from_row(&row, *version);
                Ok(ContractEvent::Misc(MiscContractEvent {
                    id: base.id,
                    raw: base.raw,
                    max_id: base.max_id,
                    protocol_version: base.protocol_version,
                    version: base.version,
                    contract_address: base.contract_address,
                    transaction_id: base.transaction_id,
                    name: name.hex_encode(),
                    payload: payload.hex_encode(),
                }))
            }

            other => Err(UnexpectedContractEvent(other.clone())),
        }
    }
}

#[derive(Debug, Error)]
#[error("unexpected ledger event for contract events surface: {0:?}")]
pub struct UnexpectedContractEvent(LedgerEventAttributes);
