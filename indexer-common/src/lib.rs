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

#![allow(clippy::zero_prefixed_literal)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod cipher;
pub mod config;
pub mod domain;
pub mod error;
pub mod infra;
pub mod stream;
pub mod telemetry;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod version;

// The allocator the telemetry metrics and heap profiling rely on. Binaries install it themselves,
// since a library-level global allocator would clash with test binaries that bring their own.
#[cfg(target_os = "linux")]
pub use tikv_jemallocator::Jemalloc;
