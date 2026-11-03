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

// Key material derived locally from a seed, with no container and no network.
// `tests/unit/utils/moth/moth-addresses.test.ts` pins both functions to output captured
// from `midnight-node-toolkit show-address` / `show-viewing-key`.

import { deriveAllAddressesFromSeed, deriveWalletKeys } from '@shieldedtech/moth-wallet';
import { ZswapSecretKeys } from '@midnightntwrk/wallet-sdk/ledger/v9';
import { ShieldedEncryptionSecretKey } from '@midnightntwrk/wallet-sdk/address-format';
import type { DerivedAddresses } from '../tx-backend/tx-backend';

/** The unshielded, shielded and DUST bech32m addresses of `seed` on `networkId`. */
export function deriveMothAddresses(seed: string, networkId: string): DerivedAddresses {
  const network = networkId.toLowerCase();
  const addrs = deriveAllAddressesFromSeed(seed);
  const pick = (group: { bech32m: Record<string, string> }, name: string): string => {
    // `bech32m` maps network to address; handing the whole map on yields the
    // unhelpful "bech32.decode input: string expected" far from here.
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

/** The shielded viewing key (`mn_shield-esk_<network>…`) of `seed` on `networkId`. */
export function deriveMothViewingKey(seed: string, networkId: string): string {
  const encryptionSecretKey = ZswapSecretKeys.fromSeed(
    deriveWalletKeys(seed).shielded,
  ).encryptionSecretKey;
  return ShieldedEncryptionSecretKey.codec
    .encode(networkId.toLowerCase(), new ShieldedEncryptionSecretKey(encryptionSecretKey))
    .asString();
}
