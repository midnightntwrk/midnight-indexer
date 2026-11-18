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

// The moth backend holds every spend until the dust wallet's Merkle roots
// equal the chain tip's. If the comparison is wrong in the "never equal"
// direction, every moth transfer, deploy and call waits out the 3-minute
// ceiling and fails. These cases pin the comparison to the indexer's real
// encoding, using roots captured on undeployed on 2026-11-05.

import { decodeScaleBigInt, sameRoot } from '@utils/moth/dust-root';

/** The wallet's view of a root, from the little-endian hex it is printed as. */
const fromLeHex = (hex: string): bigint =>
  BigInt('0x' + Buffer.from(hex, 'hex').reverse().toString('hex'));

const GENERATION_WALLET_LE = 'fc852a67f5f1dbe26486bc4f06ff6b3618facdb3810012c71d2bcc8098522400';
const GENERATION_CHAIN = '6ffc852a67f5f1dbe26486bc4f06ff6b3618facdb3810012c71d2bcc80985224';
const COMMITMENT_WALLET_LE = '94d67e7cd9edfb3f013d770afda6d1a5b2e686cd17eda26b3a7dc2d671015f52';
const COMMITMENT_CHAIN = '7394d67e7cd9edfb3f013d770afda6d1a5b2e686cd17eda26b3a7dc2d671015f52';

describe('a DUST root reported by the indexer', () => {
  describe('whose top byte is zero', () => {
    test('should equal the wallet root although the encoding is one byte shorter', () => {
      expect(sameRoot(fromLeHex(GENERATION_WALLET_LE), GENERATION_CHAIN)).toBe(true);
    });
  });

  describe('that fills all 32 bytes', () => {
    test('should equal the wallet root', () => {
      expect(sameRoot(fromLeHex(COMMITMENT_WALLET_LE), COMMITMENT_CHAIN)).toBe(true);
    });

    test('should equal the wallet root when written with a 0x prefix', () => {
      expect(sameRoot(fromLeHex(COMMITMENT_WALLET_LE), '0x' + COMMITMENT_CHAIN)).toBe(true);
    });

    test('should not equal a different wallet root', () => {
      expect(sameRoot(fromLeHex(COMMITMENT_WALLET_LE) + 1n, COMMITMENT_CHAIN)).toBe(false);
    });
  });

  describe('that is missing or malformed', () => {
    test.each([
      ['a missing chain root', fromLeHex(COMMITMENT_WALLET_LE), null],
      ['an empty chain root', fromLeHex(COMMITMENT_WALLET_LE), ''],
      ['a missing wallet root', undefined, COMMITMENT_CHAIN],
      ['a truncated chain root', fromLeHex(COMMITMENT_WALLET_LE), COMMITMENT_CHAIN.slice(0, -2)],
    ])('should not match for %s', (_case, wallet, chain) => {
      expect(sameRoot(wallet, chain)).toBe(false);
    });
  });
});

describe('the SCALE big-int decoder', () => {
  test.each([
    ['00', 0n],
    ['fc', 63n],
    ['0101', 64n],
    ['fdff', 16_383n],
    ['02000100', 16_384n],
    ['feffffff', 1_073_741_823n],
    ['0300000040', 1_073_741_824n],
  ])('should decode %s as %s', (hex, value) => {
    expect(decodeScaleBigInt(hex)).toBe(value);
  });

  test.each([
    ['an empty string', ''],
    ['a one-byte header followed by data', '0000'],
    ['a two-byte header cut short', '01'],
    ['a long header with too few bytes', '73' + '00'.repeat(31)],
  ])('should reject %s', (_case, hex) => {
    expect(() => decodeScaleBigInt(hex)).toThrow();
  });
});
