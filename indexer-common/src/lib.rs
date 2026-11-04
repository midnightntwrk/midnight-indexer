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

// jemalloc backs every Linux executable in the workspace: it is what the allocator metrics in
// `telemetry` read from and what heap profiling (docs/heap-profiling.md) needs. Declared here
// rather than per binary so there is a single allocator and a single profiling feature.
#[cfg(target_os = "linux")]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
