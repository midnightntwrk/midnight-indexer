// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

pub mod application;
#[cfg(feature = "cloud")]
pub mod config;
pub mod domain;
#[cfg(feature = "cloud")]
pub mod infra;
