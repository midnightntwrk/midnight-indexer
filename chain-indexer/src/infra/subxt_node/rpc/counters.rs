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

//! Request and byte counts per call, kept for reports and exported as metrics.

use crate::infra::subxt_node::rpc::{Call, method, metric};
use metrics::counter;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::BTreeMap;

/// Requests and bytes of one method, or of one storage item.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Count {
    pub requests: u64,
    pub request_bytes: u64,
    pub response_bytes: u64,
}

/// Batches sent, and the largest batch response.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BatchCount {
    pub batches: u64,
    pub largest_response_bytes: u64,
}

/// Request and byte counts, keyed by method, by method and runtime function, or by storage item.
/// Bytes are JSON wire bytes, except for storage items, whose bytes are the decoded values.
#[derive(Debug, Default)]
pub struct Counters {
    counts: Mutex<BTreeMap<String, Count>>,
    batches: Mutex<BatchCount>,
}

impl Counters {
    /// Add to the count of the given key, and to the metrics labelled with it.
    pub fn record(&self, key: &str, requests: u64, request_bytes: u64, response_bytes: u64) {
        let label = [("call", key.to_owned())];
        counter!(metric::REQUEST_COUNT, &label).increment(requests);
        counter!(metric::REQUEST_BYTES, &label).increment(request_bytes);
        counter!(metric::RESPONSE_BYTES, &label).increment(response_bytes);

        let mut counts = self.counts.lock();
        let count = counts.entry(key.to_owned()).or_default();
        count.requests += requests;
        count.request_bytes += request_bytes;
        count.response_bytes += response_bytes;
    }

    /// The counts so far.
    pub fn counts(&self) -> BTreeMap<String, Count> {
        self.counts.lock().clone()
    }

    /// The batch counts so far.
    pub fn batches(&self) -> BatchCount {
        *self.batches.lock()
    }

    pub(super) fn record_batch(&self, response_bytes: u64) {
        counter!(metric::BATCH_COUNT).increment(1);
        let mut batches = self.batches.lock();
        batches.batches += 1;
        batches.largest_response_bytes = batches.largest_response_bytes.max(response_bytes);
    }
}

/// The key a call is counted under: its method, and for runtime calls also the function.
pub(super) fn count_key(call: &Call) -> String {
    match (call.method, call.params.get(1).and_then(Value::as_str)) {
        (method::ARCHIVE_CALL, Some(function)) => format!("{} {function}", call.method),
        (method, _) => method.to_owned(),
    }
}

/// The size of a JSON value as serialized, without serializing it.
pub(super) fn json_size(value: &Value) -> usize {
    use Value::*;
    match value {
        Null => 4,
        Bool(true) => 4,
        Bool(false) => 5,
        Number(number) => number.to_string().len(),
        String(string) => string.len() + 2,
        Array(values) => {
            values.iter().map(json_size).sum::<usize>() + values.len().saturating_sub(1) + 2
        }
        Object(entries) => {
            entries
                .iter()
                .map(|(key, value)| key.len() + 3 + json_size(value))
                .sum::<usize>()
                + entries.len().saturating_sub(1)
                + 2
        }
    }
}
