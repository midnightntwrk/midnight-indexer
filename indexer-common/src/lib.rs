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

#![cfg_attr(docsrs, feature(doc_cfg))]

use midnight_base_crypto::signatures::Signature;
use midnight_ledger::structure::ProofMarker;
use midnight_storage::DefaultDB;
use midnight_transient_crypto::commitment::PedersenRandomness;

pub mod cipher;
pub mod config;
pub mod domain;
pub mod error;
pub mod infra;
pub mod serialize;
pub mod stream;
pub mod telemetry;

pub type LedgerTransaction =
    midnight_ledger::structure::Transaction<Signature, ProofMarker, PedersenRandomness, DefaultDB>;
