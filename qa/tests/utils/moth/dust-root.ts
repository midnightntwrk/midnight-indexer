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

// Comparing a moth wallet's DUST Merkle roots with the roots the indexer
// reports for a block. Kept free of wallet and environment imports so the unit
// suite can test it directly.

/** Little-endian bytes as an unsigned integer. */
const leToBigInt = (bytes: Uint8Array): bigint =>
  bytes.reduceRight((acc, byte) => (acc << 8n) | BigInt(byte), 0n);

/**
 * Decode a DUST Merkle root as the indexer prints it: the hex of the ledger's
 * `ScaleBigInt` serialization of the field element (SCALE compact).
 *
 * The encoding drops the value's trailing (most significant) zero bytes, so
 * its length varies. Values of five bytes and more carry the header
 * `((n - 5) << 2) | 0b11`, where n is the encoded length, followed by the
 * remaining little-endian bytes. A full 32-byte root is `73` + 32 bytes; a root
 * whose top byte is zero is `6f` + 31 bytes. Smaller values use SCALE's 1-, 2-
 * and 4-byte modes, with the value shifted left by two.
 *
 * @param hex - The encoded root, with or without a `0x` prefix.
 * @returns The root's value.
 * @throws If the string is empty or the header disagrees with its length.
 */
export const decodeScaleBigInt = (hex: string): bigint => {
  const bytes = Buffer.from(hex.replace(/^0x/i, ''), 'hex');
  if (bytes.length === 0) {
    throw new Error(`empty SCALE big-int '${hex}'`);
  }
  const expectLength = (length: number) => {
    if (bytes.length !== length) {
      throw new Error(
        `malformed SCALE big-int ${hex}: its header implies ${length} bytes, got ${bytes.length}`,
      );
    }
  };
  switch (bytes[0] & 0b11) {
    case 0:
      expectLength(1);
      return BigInt(bytes[0] >> 2);
    case 1:
      expectLength(2);
      return leToBigInt(bytes) >> 2n;
    case 2:
      expectLength(4);
      return leToBigInt(bytes) >> 2n;
    default:
      expectLength(1 + (bytes[0] >> 2) + 4);
      return leToBigInt(bytes.subarray(1));
  }
};

/**
 * A 32-byte field element as little-endian hex, the byte order the indexer
 * uses. For messages only: the indexer's encoding is shorter when the top byte
 * is zero, so never compare against this string.
 */
export const rootHexLe = (root: bigint | undefined): string =>
  root === undefined
    ? ''
    : Buffer.from(root.toString(16).padStart(64, '0'), 'hex').reverse().toString('hex');

/**
 * Whether the indexer's encoded root is the wallet's root.
 *
 * Compared by value, not by hex: the indexer's encoding omits a zero top byte
 * (about 1 root in 256), so its hex is not always the wallet's 32-byte hex.
 *
 * @param wallet - The root as the dust wallet holds it.
 * @param chain - The root as the indexer prints it.
 * @returns False when either side is missing or the chain side does not decode.
 */
export const sameRoot = (wallet: bigint | undefined, chain: string | null | undefined): boolean => {
  if (wallet === undefined || !chain) return false;
  try {
    return decodeScaleBigInt(chain) === wallet;
  } catch {
    return false;
  }
};
