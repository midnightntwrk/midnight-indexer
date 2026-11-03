// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Choosing a custom token the wallet under test can spend.
//
// WHY THIS EXISTS RATHER THAN A STATIC FIXTURE. `data/static/<env>/
// unshielded-token-types.jsonc` is produced by the block scanner: it records
// which custom token types the *chain* holds and who held them at scan time.
// That is the right input for query and subscription tests, which only need a
// token type that exists.
//
// An e2e transfer test needs more than existence — it has to *spend* the token,
// so it needs a token the wallet under test owns right now. The scan cannot
// establish that: it may not cover the wallet's holdings at all, and anything
// it did record can have been spent since. Asking the wallet is the only
// answer that is true at the moment the test runs.
//
// So e2e suites discover their token from the live balance
// (`TxBackend.unshieldedHoldings`) and leave the fixture to the integration suites.

/** The all-zero token type: NIGHT, which every chain has from genesis. */
export const NIGHT_TOKEN_TYPE = '0'.repeat(64);

/**
 * Strictly greater than `amount`: the transfer must create a destination output
 * *and* a source change output, exactly as the NIGHT transfer does, so the
 * suite's assertions about both sides hold.
 *
 * @returns The best candidate's token type, or undefined when the wallet holds
 *          no custom token it can spend.
 */
export function pickSpendableCustomToken(
  holdings: Map<string, bigint>,
  amount: bigint,
): string | undefined {
  const candidates = [...holdings.entries()]
    .filter(([tokenType]) => tokenType !== NIGHT_TOKEN_TYPE)
    .filter(([, value]) => value > amount)
    // Largest holding first: the one most likely to survive concurrent runs
    // spending from the same shared funding wallet.
    .sort(([, a], [, b]) => (b > a ? 1 : b < a ? -1 : 0));

  return candidates[0]?.[0];
}
