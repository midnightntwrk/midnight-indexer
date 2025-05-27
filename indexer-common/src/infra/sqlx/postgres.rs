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

/// Maps a "deadlock_detected" PosgtreSQL error to the result of the given function wrapped in `Ok`
/// and passes all other errors along.
/// For "40P01" see <https://www.postgresql.org/docs/current/errcodes-appendix.html>.
pub fn ignore_deadlock_detected<F, T>(error: sqlx::Error, on_deadlock: F) -> Result<T, sqlx::Error>
where
    F: FnOnce() -> T,
{
    match error {
        sqlx::Error::Database(e) if e.code().as_deref() == Some("40P01") => Ok(on_deadlock()),
        other => Err(other),
    }
}
