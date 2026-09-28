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

// Reading a wallet's spendable unshielded holdings, on either backend.
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
// So e2e suites discover their token here, from the live balance, and leave the
// fixture to the integration suites.

import { env } from '../../environment/model';
import { listUnshieldedHoldingsViaMoth } from './moth-backend';
import type { ToolkitWrapper } from '../toolkit/toolkit-wrapper';

/** The all-zero token type: NIGHT, which every chain has from genesis. */
export const NIGHT_TOKEN_TYPE = '0'.repeat(64);

/**
 * Total spendable value per unshielded token type for a wallet, keyed by the
 * hex token type the indexer reports as `tokenType`.
 *
 * On the moth backend this reads the synced wallet's own state. On the toolkit
 * backend it is the existing `show-public-wallet-state` exec, so a started
 * toolkit must be supplied.
 *
 * @param seed - Seed of the wallet to read. Never logged.
 * @param address - That seed's unshielded address, used by the toolkit backend.
 * @param toolkit - A started ToolkitWrapper. Required on the toolkit backend;
 *                  ignored on the moth backend.
 */
export async function listUnshieldedHoldings(
  seed: string,
  address: string,
  toolkit?: ToolkitWrapper,
): Promise<Map<string, bigint>> {
  if (env.getTxBackend() === 'moth') {
    return listUnshieldedHoldingsViaMoth(seed);
  }

  if (!toolkit) {
    throw new Error(
      'listUnshieldedHoldings needs a started ToolkitWrapper on the toolkit backend. ' +
        'Pass one, or run with TX_BACKEND=moth.',
    );
  }

  // The toolkit reports UTXO token types under `token_type`, in the same hex
  // encoding the indexer reports as `tokenType`; if a provisioned environment
  // ever looks empty here, compare the two encodings first.
  const walletState = await toolkit.showPublicWalletState(address);
  const totals = new Map<string, bigint>();
  for (const utxo of walletState.utxos) {
    totals.set(utxo.token_type, (totals.get(utxo.token_type) ?? 0n) + BigInt(utxo.value));
  }
  return totals;
}

/**
 * Pick a custom (non-NIGHT) unshielded token the wallet can spend, leaving
 * change behind.
 *
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
