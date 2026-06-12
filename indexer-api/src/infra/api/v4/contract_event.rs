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
//! `docs/interactions/event-epic/8may-2026/contract-events-graphql-draft-v0.7.graphql`
//! for the canonical reference shape.

use crate::{
    domain::{
        ContractEventRow,
        storage::{
            Storage,
            contract_event::{
                ContractEventFilter as DomainContractEventFilter, FieldPrefix as DomainFieldPrefix,
            },
        },
    },
    infra::api::{
        ApiResult,
        v4::{
            HexEncodable, HexEncoded, contract_action::get_transaction_by_id, directives::beta,
            transaction::Transaction,
        },
    },
};
use async_graphql::{ComplexObject, Context, Enum, InputObject, Interface, SimpleObject};
use derive_more::Debug;
use indexer_common::domain::{
    AddressOrContract as DomainAddressOrContract, ByteVec, LedgerEventAttributes,
};
use std::marker::PhantomData;
use thiserror::Error;

/// Tagged-union helper for fields like `Either<ZswapCoinPublicKey, ContractAddress>`
/// used in standard unshielded events.
///
/// Exactly one of `userAddress` or `contractAddress` is non-null; the `kind`
/// discriminator says which.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(directive = beta::apply())]
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
    /// Optional: hex-encoded transaction hash; narrows to events emitted from
    /// transactions with this hash ("I just submitted tx X, give me its
    /// events").
    pub transaction_hash: Option<HexEncoded>,
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
    field(name = "transaction_id", ty = "&u64"),
    field(name = "transaction", ty = "ApiResult<Transaction<S>>")
)]
pub enum ContractEvent<S: Storage> {
    ShieldedSpend(ShieldedSpendEvent<S>),
    ShieldedReceive(ShieldedReceiveEvent<S>),
    ShieldedMint(ShieldedMintEvent<S>),
    ShieldedBurn(ShieldedBurnEvent<S>),
    UnshieldedSpend(UnshieldedSpendEvent<S>),
    UnshieldedReceive(UnshieldedReceiveEvent<S>),
    UnshieldedMint(UnshieldedMintEvent<S>),
    UnshieldedBurn(UnshieldedBurnEvent<S>),
    Paused(PausedEvent<S>),
    Unpaused(UnpausedEvent<S>),
    Misc(MiscContractEvent<S>),
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
#[graphql(complex, directive = beta::apply())]
pub struct ShieldedSpendEvent<S>
where
    S: Storage,
{
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    /// Indexed.
    pub nullifier: HexEncoded,
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> ShieldedSpendEvent<S>
where
    S: Storage,
{
    /// The transaction this event was emitted from.
    async fn transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

#[derive(Debug, SimpleObject)]
#[graphql(complex, directive = beta::apply())]
pub struct ShieldedReceiveEvent<S>
where
    S: Storage,
{
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
    /// (`Maybe<Bytes<512>>`). Hex-encoded, up to 512 bytes.
    pub ciphertext: Option<HexEncoded>,
    /// Set when received by a contract; null for user recipients
    /// (`Maybe<ContractAddress>`). Renamed from `contractAddress` in the CoIP
    /// to avoid collision with the top-level emitting `contractAddress`
    /// inherited from the ContractEvent interface.
    pub receiving_contract_address: Option<HexEncoded>,
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> ShieldedReceiveEvent<S>
where
    S: Storage,
{
    /// The transaction this event was emitted from.
    async fn transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

#[derive(Debug, SimpleObject)]
#[graphql(complex, directive = beta::apply())]
pub struct ShieldedMintEvent<S>
where
    S: Storage,
{
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
    /// Optional, hidden in some shielded mints (`Maybe<Uint<128>>`).
    pub amount: Option<String>,
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> ShieldedMintEvent<S>
where
    S: Storage,
{
    /// The transaction this event was emitted from.
    async fn transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

#[derive(Debug, SimpleObject)]
#[graphql(complex, directive = beta::apply())]
pub struct ShieldedBurnEvent<S>
where
    S: Storage,
{
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    /// Indexed.
    pub nullifier: HexEncoded,
    /// Optional, hidden in some shielded burns (`Maybe<Uint<128>>`).
    pub amount: Option<String>,
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> ShieldedBurnEvent<S>
where
    S: Storage,
{
    /// The transaction this event was emitted from.
    async fn transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

// ============================================================================
// Unshielded concrete types (CoIP-442 head: Spend/Receive feature both
// domainSep + tokenType; Burn features tokenType only; Mint features both).
// ============================================================================

#[derive(Debug, SimpleObject)]
#[graphql(complex, directive = beta::apply())]
pub struct UnshieldedSpendEvent<S>
where
    S: Storage,
{
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
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> UnshieldedSpendEvent<S>
where
    S: Storage,
{
    /// The transaction this event was emitted from.
    async fn transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

#[derive(Debug, SimpleObject)]
#[graphql(complex, directive = beta::apply())]
pub struct UnshieldedReceiveEvent<S>
where
    S: Storage,
{
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
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> UnshieldedReceiveEvent<S>
where
    S: Storage,
{
    /// The transaction this event was emitted from.
    async fn transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

#[derive(Debug, SimpleObject)]
#[graphql(complex, directive = beta::apply())]
pub struct UnshieldedMintEvent<S>
where
    S: Storage,
{
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
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> UnshieldedMintEvent<S>
where
    S: Storage,
{
    /// The transaction this event was emitted from.
    async fn transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

#[derive(Debug, SimpleObject)]
#[graphql(complex, directive = beta::apply())]
pub struct UnshieldedBurnEvent<S>
where
    S: Storage,
{
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
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> UnshieldedBurnEvent<S>
where
    S: Storage,
{
    /// The transaction this event was emitted from.
    async fn transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

// ============================================================================
// Signal-only events (empty payload structs `Paused`, `Unpaused`).
// ============================================================================

#[derive(Debug, SimpleObject)]
#[graphql(complex, directive = beta::apply())]
pub struct PausedEvent<S>
where
    S: Storage,
{
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> PausedEvent<S>
where
    S: Storage,
{
    /// The transaction this event was emitted from.
    async fn transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

#[derive(Debug, SimpleObject)]
#[graphql(complex, directive = beta::apply())]
pub struct UnpausedEvent<S>
where
    S: Storage,
{
    pub id: u64,
    pub raw: HexEncoded,
    pub max_id: u64,
    pub protocol_version: u32,
    pub version: u32,
    pub contract_address: HexEncoded,
    pub transaction_id: u64,
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> UnpausedEvent<S>
where
    S: Storage,
{
    /// The transaction this event was emitted from.
    async fn transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

// ============================================================================
// Custom (Misc) event — opaque payload up to 256 bytes.
// ============================================================================

#[derive(Debug, SimpleObject)]
#[graphql(complex, directive = beta::apply())]
pub struct MiscContractEvent<S>
where
    S: Storage,
{
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
    #[graphql(skip)]
    _s: PhantomData<S>,
}

#[ComplexObject]
impl<S> MiscContractEvent<S>
where
    S: Storage,
{
    /// The transaction this event was emitted from.
    async fn transaction(&self, cx: &Context<'_>) -> ApiResult<Transaction<S>> {
        get_transaction_by_id(self.transaction_id, cx).await
    }
}

// ============================================================================
// TryFrom: discriminate via attributes, fold base + per-variant fields.
// ============================================================================

impl<S> TryFrom<ContractEventRow> for ContractEvent<S>
where
    S: Storage,
{
    type Error = Box<UnexpectedContractEvent>;

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
                    _s: PhantomData,
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
                    _s: PhantomData,
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
                    _s: PhantomData,
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
                    _s: PhantomData,
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
                    _s: PhantomData,
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
                    _s: PhantomData,
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
                    _s: PhantomData,
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
                    _s: PhantomData,
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
                    _s: PhantomData,
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
                    _s: PhantomData,
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
                    _s: PhantomData,
                }))
            }

            other => Err(Box::new(UnexpectedContractEvent(other.clone()))),
        }
    }
}

#[derive(Debug, Error)]
#[error("unexpected ledger event for contract events surface: {0:?}")]
pub struct UnexpectedContractEvent(LedgerEventAttributes);

// ============================================================================
// GraphQL filter → domain filter conversion.
// ============================================================================

impl ContractEventFilter {
    /// Convert into the domain filter shape consumed by the storage layer.
    /// Validates that `contract_address` is non-empty and hex-decodable.
    pub fn into_domain(self) -> Result<DomainContractEventFilter, ContractEventFilterError> {
        let contract_address: ByteVec = self
            .contract_address
            .hex_decode()
            .map_err(|e| ContractEventFilterError::InvalidContractAddress(e.to_string()))?;
        if contract_address.as_ref().is_empty() {
            return Err(ContractEventFilterError::EmptyContractAddress);
        }

        let variants = self.types.map(|ts| {
            ts.into_iter()
                .map(contract_event_type_variant_name)
                .collect::<Vec<_>>()
        });

        let field_prefixes = match self.field_prefixes {
            None => Vec::new(),
            Some(fps) => {
                fps.into_iter()
                    .map(|fp| {
                        let prefix: ByteVec = fp.prefix.hex_decode().map_err(|e| {
                            ContractEventFilterError::InvalidFieldPrefix(e.to_string())
                        })?;
                        Ok(DomainFieldPrefix {
                            field_name: fp.field_name,
                            prefix: prefix.as_ref().to_vec(),
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };

        let transaction_hash = self
            .transaction_hash
            .map(|hash| {
                hash.hex_decode::<ByteVec>()
                    .map(|hash| hash.as_ref().to_vec())
                    .map_err(|e| ContractEventFilterError::InvalidTransactionHash(e.to_string()))
            })
            .transpose()?;

        Ok(DomainContractEventFilter {
            contract_address: contract_address.as_ref().to_vec(),
            variants,
            field_prefixes,
            from_block: self.from_block,
            to_block: self.to_block,
            transaction_hash,
        })
    }
}

fn contract_event_type_variant_name(t: ContractEventType) -> &'static str {
    match t {
        ContractEventType::ShieldedSpend => "ShieldedSpend",
        ContractEventType::ShieldedReceive => "ShieldedReceive",
        ContractEventType::ShieldedMint => "ShieldedMint",
        ContractEventType::ShieldedBurn => "ShieldedBurn",
        ContractEventType::UnshieldedSpend => "UnshieldedSpend",
        ContractEventType::UnshieldedReceive => "UnshieldedReceive",
        ContractEventType::UnshieldedMint => "UnshieldedMint",
        ContractEventType::UnshieldedBurn => "UnshieldedBurn",
        ContractEventType::Paused => "Paused",
        ContractEventType::Unpaused => "Unpaused",
        ContractEventType::Misc => "Misc",
    }
}

#[derive(Debug, Error)]
pub enum ContractEventFilterError {
    #[error("invalid contractAddress: {0}")]
    InvalidContractAddress(String),

    #[error("contractAddress is required and must be non-empty")]
    EmptyContractAddress,

    #[error("invalid fieldPrefix.prefix: {0}")]
    InvalidFieldPrefix(String),

    #[error("invalid transactionHash: {0}")]
    InvalidTransactionHash(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::storage::NoopStorage;
    use indexer_common::domain::{ByteVec, ProtocolVersion, SerializedLedgerEvent};

    fn bv(bytes: &[u8]) -> ByteVec {
        ByteVec::from(bytes.to_vec())
    }

    fn make_row(attributes: LedgerEventAttributes) -> ContractEventRow {
        ContractEventRow {
            id: 42,
            contract_address: bv(&[0x01; 32]),
            transaction_id: 7,
            contract_action_id: Some(99),
            raw: SerializedLedgerEvent::from(vec![0xab, 0xcd]),
            attributes,
            max_id: 100,
            protocol_version: ProtocolVersion::V1_0(0),
        }
    }

    #[test]
    fn try_from_shielded_spend_yields_correct_variant() {
        let row = make_row(LedgerEventAttributes::ContractShieldedSpend {
            version: 1,
            entry_point: bv(b"spend"),
            nullifier: bv(&[0xaa; 32]),
        });

        let event = ContractEvent::<NoopStorage>::try_from(row).expect("try_from");
        assert!(matches!(event, ContractEvent::ShieldedSpend(_)));
        if let ContractEvent::ShieldedSpend(e) = event {
            assert_eq!(e.id, 42);
            assert_eq!(e.version, 1);
            assert_eq!(e.transaction_id, 7);
            assert_eq!(e.max_id, 100);
        }
    }

    #[test]
    fn try_from_unshielded_receive_includes_domain_sep() {
        let row = make_row(LedgerEventAttributes::ContractUnshieldedReceive {
            version: 2,
            entry_point: bv(b"recv"),
            recipient: indexer_common::domain::AddressOrContract::Contract(bv(&[0xbb; 32])),
            domain_sep: bv(&[0xcc; 32]),
            token_type: bv(&[0xdd; 32]),
            amount: "1000".into(),
        });

        let event = ContractEvent::<NoopStorage>::try_from(row).expect("try_from");
        if let ContractEvent::UnshieldedReceive(e) = event {
            assert_eq!(e.version, 2);
            assert_eq!(e.amount, "1000");
            assert!(matches!(e.recipient.kind, AddressOrContractKind::Contract));
            assert!(e.recipient.user_address.is_none());
            assert!(e.recipient.contract_address.is_some());
        } else {
            panic!("expected UnshieldedReceive");
        }
    }

    #[test]
    fn try_from_paused_carries_only_interface_fields() {
        let row = make_row(LedgerEventAttributes::ContractPaused {
            version: 1,
            entry_point: bv(b"pause"),
        });
        let event = ContractEvent::<NoopStorage>::try_from(row).expect("try_from");
        assert!(matches!(event, ContractEvent::Paused(_)));
    }

    #[test]
    fn try_from_non_contract_attribute_returns_error() {
        let row = make_row(LedgerEventAttributes::ZswapOutput);
        let err = ContractEvent::<NoopStorage>::try_from(row).expect_err("must error");
        assert!(format!("{err}").contains("unexpected ledger event"));
    }

    #[test]
    fn contract_event_type_variant_name_is_stable() {
        assert_eq!(
            contract_event_type_variant_name(ContractEventType::ShieldedSpend),
            "ShieldedSpend"
        );
        assert_eq!(
            contract_event_type_variant_name(ContractEventType::Misc),
            "Misc"
        );
        assert_eq!(
            contract_event_type_variant_name(ContractEventType::UnshieldedReceive),
            "UnshieldedReceive"
        );
    }

    #[test]
    fn filter_into_domain_rejects_empty_contract_address() {
        let filter = ContractEventFilter {
            contract_address: HexEncoded::try_from(String::new()).expect("empty hex parses"),
            types: None,
            field_prefixes: None,
            from_block: None,
            to_block: None,
            transaction_hash: None,
        };
        let err = filter.into_domain().expect_err("should reject");
        assert!(matches!(
            err,
            ContractEventFilterError::EmptyContractAddress
        ));
    }

    #[test]
    fn filter_into_domain_threads_block_range_and_types() {
        let filter = ContractEventFilter {
            contract_address: HexEncoded::try_from("ab".repeat(32)).expect("valid hex"),
            types: Some(vec![
                ContractEventType::ShieldedSpend,
                ContractEventType::Misc,
            ]),
            field_prefixes: None,
            from_block: Some(100),
            to_block: Some(200),
            transaction_hash: None,
        };
        let domain = filter.into_domain().expect("valid");
        assert_eq!(domain.from_block, Some(100));
        assert_eq!(domain.to_block, Some(200));
        assert_eq!(
            domain.variants.as_deref(),
            Some(&["ShieldedSpend", "Misc"][..])
        );
        assert_eq!(domain.contract_address.len(), 32);
        assert_eq!(domain.transaction_hash, None);
    }

    #[test]
    fn filter_into_domain_threads_transaction_hash() {
        let filter = ContractEventFilter {
            contract_address: HexEncoded::try_from("ab".repeat(32)).expect("valid hex"),
            types: None,
            field_prefixes: None,
            from_block: None,
            to_block: None,
            transaction_hash: Some(HexEncoded::try_from("cd".repeat(32)).expect("valid hex")),
        };
        let domain = filter.into_domain().expect("valid");
        assert_eq!(domain.transaction_hash, Some(vec![0xcd; 32]));
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    use crate::domain::storage::NoopStorage;
    use indexer_common::domain::{
        AddressOrContract as DomainAOC, ByteVec, ProtocolVersion, SerializedLedgerEvent,
    };

    fn bv(bytes: &[u8]) -> ByteVec {
        ByteVec::from(bytes.to_vec())
    }

    fn make_row(attributes: LedgerEventAttributes) -> ContractEventRow {
        ContractEventRow {
            id: 1,
            contract_address: bv(&[0x01; 32]),
            transaction_id: 1,
            contract_action_id: Some(1),
            raw: SerializedLedgerEvent::from(vec![0]),
            attributes,
            max_id: 1,
            protocol_version: ProtocolVersion::V1_0(0),
        }
    }

    #[test]
    #[allow(clippy::type_complexity)]
    fn try_from_every_variant_succeeds() {
        let cases: Vec<(
            LedgerEventAttributes,
            fn(&ContractEvent<NoopStorage>) -> bool,
        )> = vec![
            (
                LedgerEventAttributes::ContractShieldedSpend {
                    version: 1,
                    entry_point: bv(b""),
                    nullifier: bv(&[0; 32]),
                },
                |e| matches!(e, ContractEvent::ShieldedSpend(_)),
            ),
            (
                LedgerEventAttributes::ContractShieldedReceive {
                    version: 1,
                    entry_point: bv(b""),
                    commitment: bv(&[0; 32]),
                    ciphertext: None,
                    receiving_contract_address: None,
                },
                |e| matches!(e, ContractEvent::ShieldedReceive(_)),
            ),
            (
                LedgerEventAttributes::ContractShieldedMint {
                    version: 1,
                    entry_point: bv(b""),
                    commitment: bv(&[0; 32]),
                    domain_sep: bv(&[0; 32]),
                    amount: None,
                },
                |e| matches!(e, ContractEvent::ShieldedMint(_)),
            ),
            (
                LedgerEventAttributes::ContractShieldedBurn {
                    version: 1,
                    entry_point: bv(b""),
                    nullifier: bv(&[0; 32]),
                    amount: None,
                },
                |e| matches!(e, ContractEvent::ShieldedBurn(_)),
            ),
            (
                LedgerEventAttributes::ContractUnshieldedSpend {
                    version: 1,
                    entry_point: bv(b""),
                    sender: DomainAOC::User(bv(&[0; 32])),
                    domain_sep: bv(&[0; 32]),
                    token_type: bv(&[0; 32]),
                    amount: "0".into(),
                },
                |e| matches!(e, ContractEvent::UnshieldedSpend(_)),
            ),
            (
                LedgerEventAttributes::ContractUnshieldedReceive {
                    version: 1,
                    entry_point: bv(b""),
                    recipient: DomainAOC::Contract(bv(&[0; 32])),
                    domain_sep: bv(&[0; 32]),
                    token_type: bv(&[0; 32]),
                    amount: "0".into(),
                },
                |e| matches!(e, ContractEvent::UnshieldedReceive(_)),
            ),
            (
                LedgerEventAttributes::ContractUnshieldedMint {
                    version: 1,
                    entry_point: bv(b""),
                    domain_sep: bv(&[0; 32]),
                    token_type: bv(&[0; 32]),
                    amount: "0".into(),
                },
                |e| matches!(e, ContractEvent::UnshieldedMint(_)),
            ),
            (
                LedgerEventAttributes::ContractUnshieldedBurn {
                    version: 1,
                    entry_point: bv(b""),
                    sender: DomainAOC::User(bv(&[0; 32])),
                    token_type: bv(&[0; 32]),
                    amount: "0".into(),
                },
                |e| matches!(e, ContractEvent::UnshieldedBurn(_)),
            ),
            (
                LedgerEventAttributes::ContractPaused {
                    version: 1,
                    entry_point: bv(b""),
                },
                |e| matches!(e, ContractEvent::Paused(_)),
            ),
            (
                LedgerEventAttributes::ContractUnpaused {
                    version: 1,
                    entry_point: bv(b""),
                },
                |e| matches!(e, ContractEvent::Unpaused(_)),
            ),
            (
                LedgerEventAttributes::ContractMisc {
                    version: 1,
                    entry_point: bv(b""),
                    name: bv(&[0; 32]),
                    payload: bv(&[0; 32]),
                },
                |e| matches!(e, ContractEvent::Misc(_)),
            ),
        ];

        for (attrs, check) in cases {
            let row = make_row(attrs.clone());
            let event = ContractEvent::<NoopStorage>::try_from(row).unwrap_or_else(|_| {
                panic!("try_from failed for {:?}", attrs);
            });
            assert!(check(&event), "wrong variant for {:?}", attrs);
        }
    }

    #[test]
    fn address_or_contract_kind_round_trip_through_graphql_conversion() {
        let user_gql: AddressOrContract = DomainAOC::User(bv(&[0xab; 32])).into();
        let contract_gql: AddressOrContract = DomainAOC::Contract(bv(&[0xcd; 32])).into();

        assert!(matches!(user_gql.kind, AddressOrContractKind::User));
        assert!(user_gql.user_address.is_some());
        assert!(user_gql.contract_address.is_none());

        assert!(matches!(contract_gql.kind, AddressOrContractKind::Contract));
        assert!(contract_gql.user_address.is_none());
        assert!(contract_gql.contract_address.is_some());
    }

    #[test]
    fn shielded_receive_with_all_optional_fields_present() {
        let attrs = LedgerEventAttributes::ContractShieldedReceive {
            version: 1,
            entry_point: bv(b"r"),
            commitment: bv(&[0x11; 32]),
            ciphertext: Some(bv(&[0x22; 64])),
            receiving_contract_address: Some(bv(&[0x33; 32])),
        };
        let event = ContractEvent::<NoopStorage>::try_from(make_row(attrs)).unwrap();
        if let ContractEvent::ShieldedReceive(e) = event {
            assert!(e.ciphertext.is_some());
            assert!(e.receiving_contract_address.is_some());
        } else {
            panic!("wrong variant");
        }
    }
}
