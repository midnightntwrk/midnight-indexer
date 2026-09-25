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

// Address derivation that works without a toolkit container.
//
// WHY THIS IS NOT A METHOD ON ToolkitWrapper. `tests/e2e/toolkit/**` calls
// `ToolkitWrapper.showAddress` in order to test the *toolkit*. Dispatching on
// TX_BACKEND inside that method would quietly stop those suites exercising the
// thing they exist to check. So the switch lives here and callers opt in: tests
// that only need an address use `deriveAddresses`, tests that are about the
// toolkit keep calling the toolkit.
//
// Verified against `midnight-node-toolkit:1.0.0 show-address` on preview
// (2026-09-17): moth's nightExternal / zswap / dust bech32m values are
// character-for-character the toolkit's unshielded / shielded / dust.

import { deriveAllAddressesFromSeed } from '@shieldedtech/moth-wallet';
import { env } from '../../environment/model';
import type { ToolkitWrapper } from '../toolkit/toolkit-wrapper';

/**
 * The address fields available from both backends.
 *
 * The toolkit also returns `coinPublic`, `coinPublicTagged`, `verifyingKey`,
 * `userAddress` and `unshieldedUserAddressUntagged`. moth does not expose those
 * — its per-address `hex` field is empty — and no test needs them today, so
 * they are deliberately absent here rather than filled with an empty string a
 * caller would silently assert against. See `mothOnlyFieldError`.
 */
export interface DerivedAddresses {
  shielded: string;
  unshielded: string;
  dust: string;
}

/**
 * Derive a seed's addresses using whichever backend is selected.
 *
 * On the moth backend this is pure local key derivation: no container, no
 * network, sub-millisecond. On the toolkit backend it is the existing
 * `show-address` exec, so a started toolkit must be supplied.
 *
 * @param seed - The seed to derive from. Never logged.
 * @param toolkit - A started ToolkitWrapper. Required on the toolkit backend;
 *                  ignored on the moth backend.
 * @param networkId - Network to encode for (default: the target environment).
 */
export async function deriveAddresses(
  seed: string,
  toolkit?: ToolkitWrapper,
  networkId?: string,
): Promise<DerivedAddresses> {
  const network = (networkId ?? env.getNetworkId()).toLowerCase();

  if (env.getTxBackend() === 'moth') {
    const addrs = deriveAllAddressesFromSeed(seed);
    const pick = (group: { bech32m: Record<string, string> }, name: string): string => {
      // `bech32m` is a map keyed by network, NOT a flat string. Passing the
      // object on to anything that expects an address yields the unhelpful
      // "bech32.decode input: string expected".
      const value = group?.bech32m?.[network];
      if (!value) {
        throw new Error(
          `moth derived no ${name} address for network "${network}". ` +
            `Available networks: ${Object.keys(group?.bech32m ?? {}).join(', ') || 'none'}.`,
        );
      }
      return value;
    };
    return {
      unshielded: pick(addrs.nightExternal, 'unshielded'),
      shielded: pick(addrs.zswap, 'shielded'),
      dust: pick(addrs.dust, 'dust'),
    };
  }

  if (!toolkit) {
    throw new Error(
      'deriveAddresses needs a started ToolkitWrapper on the toolkit backend. ' +
        'Pass one, or run with TX_BACKEND=moth.',
    );
  }
  const info = await toolkit.showAddress(seed, networkId);
  return { unshielded: info.unshielded, shielded: info.shielded, dust: info.dust };
}

/**
 * The message to throw when something needs a key material field the moth
 * backend cannot produce. Kept here so the gap is named in one place.
 */
export function mothOnlyFieldError(field: string): Error {
  return new Error(
    `"${field}" is not available on the moth backend: moth exposes bech32m addresses only ` +
      '(its hex fields are empty) and does not export this value. Use the toolkit backend ' +
      'for it, or derive it from @midnight-ntwrk/wallet-sdk and add it to deriveAddresses.',
  );
}
