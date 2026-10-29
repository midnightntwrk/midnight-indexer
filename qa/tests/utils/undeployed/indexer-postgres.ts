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

/**
 * Access to the undeployed stack's indexer Postgres via `docker exec … psql`.
 *
 * Reads are for tests that need to observe state the GraphQL API does not
 * expose (e.g. the wallet keep-alive heartbeat). Writes are limited to seeding
 * relevant transactions for a wallet session, see `seedRelevantTransactions`
 * for the invariants that keep seeded rows out of the indexers' way.
 *
 * Undeployed-only: it requires the docker compose stack of this repository
 * to be running on the local docker daemon.
 */

import { randomBytes, randomInt } from 'crypto';
import { execFile } from 'child_process';
import path from 'path';
import { fileURLToPath } from 'url';
import { promisify } from 'util';

const execFileAsync = promisify(execFile);

const POSTGRES_SERVICE = 'postgres';
const POSTGRES_USER = 'indexer';
const POSTGRES_DB = 'indexer';

let cachedContainerId: string | undefined;

function resolveRepoRoot(): string {
  // This file lives at qa/tests/utils/undeployed/indexer-postgres.ts —
  // four levels up reaches the repo root.
  const here = path.dirname(fileURLToPath(import.meta.url));
  return path.resolve(here, '..', '..', '..', '..');
}

/**
 * Resolve the running indexer Postgres container of the undeployed stack.
 * The compose project name derives from the checkout directory, so the
 * container is looked up through `docker compose ps` instead of a
 * hardcoded name.
 */
async function getPostgresContainerId(): Promise<string> {
  if (!cachedContainerId) {
    const { stdout } = await execFileAsync(
      'docker',
      ['compose', '--profile', 'cloud', 'ps', '-q', POSTGRES_SERVICE],
      { cwd: resolveRepoRoot() },
    );
    const [containerId] = stdout.trim().split('\n');
    if (!containerId) {
      throw new Error('Indexer postgres container not found — is the undeployed stack running?');
    }
    cachedContainerId = containerId;
  }
  return cachedContainerId;
}

/**
 * Run `sql` through psql and return its trimmed, unaligned, tuples-only output.
 * Several statements in one string run in a single transaction, and the first
 * SQL error fails the call (`ON_ERROR_STOP`), so a partial seed never lands.
 */
async function runSql(sql: string): Promise<string> {
  const containerId = await getPostgresContainerId();
  const { stdout } = await execFileAsync('docker', [
    'exec',
    containerId,
    'psql',
    '-U',
    POSTGRES_USER,
    '-d',
    POSTGRES_DB,
    '-v',
    'ON_ERROR_STOP=1',
    '-t',
    '-A',
    '-c',
    sql,
  ]);
  return stdout.trim();
}

async function queryScalar(sql: string): Promise<string> {
  return runSql(sql);
}

function assertLowercaseHex(value: string, what: string): void {
  if (!/^[0-9a-f]+$/.test(value)) {
    throw new Error(`${what} is not lowercase hex: ${value}`);
  }
}

/**
 * Read `wallets.last_active` for the wallet bound to the given API session,
 * as epoch microseconds, or undefined when no wallet row matches the session.
 *
 * `last_active` is written by the `connect` mutation and by the shielded
 * subscription's keep-alive heartbeat, and read by the wallet-indexer to
 * decide which wallets are still actively indexed.
 */
export async function getWalletLastActiveEpochMicros(
  sessionId: string,
): Promise<number | undefined> {
  assertLowercaseHex(sessionId, 'Session ID');
  const value = await queryScalar(
    'SELECT (extract(epoch FROM last_active) * 1000000)::bigint ' +
      `FROM wallets WHERE session_id = decode('${sessionId}', 'hex');`,
  );
  return value === '' ? undefined : Number(value);
}

/**
 * Read `wallets.id` for the wallet bound to the given API session, or undefined
 * when no wallet row matches the session. The `connect` mutation writes the row
 * before it returns the session ID, so a fresh session resolves immediately.
 */
export async function getWalletIdBySessionId(sessionId: string): Promise<string | undefined> {
  assertLowercaseHex(sessionId, 'Session ID');
  const value = await queryScalar(
    `SELECT id FROM wallets WHERE session_id = decode('${sessionId}', 'hex');`,
  );
  return value === '' ? undefined : value;
}

/**
 * Lowest transaction ID of rows seeded by tests. The chain-indexer assigns IDs
 * from a sequence that a test run never gets anywhere near this value, so
 * everything at or above it is a seeded row and can be deleted wholesale.
 */
export const SEEDED_TRANSACTION_ID_BASE = 2 ** 40;

const SEEDED_TRANSACTION_ID_BLOCK = 1_000;

/**
 * Allocate `count` consecutive seeded transaction IDs in a random block above
 * `SEEDED_TRANSACTION_ID_BASE`, so that two seeding tests in one run cannot
 * collide. At most `SEEDED_TRANSACTION_ID_BLOCK` per call.
 */
export function allocateSeededTransactionIds(count: number): number[] {
  if (!Number.isInteger(count) || count < 1 || count > SEEDED_TRANSACTION_ID_BLOCK) {
    throw new Error(`count must be an integer in 1..${SEEDED_TRANSACTION_ID_BLOCK}, got ${count}`);
  }
  const base = SEEDED_TRANSACTION_ID_BASE + randomInt(0, 2 ** 20) * SEEDED_TRANSACTION_ID_BLOCK;
  return Array.from({ length: count }, (_, i) => base + i);
}

export interface SeededRelevantTransactionSpec {
  /** From `allocateSeededTransactionIds`. */
  id: number;
  zswapStartIndex: number;
  /** Exclusive. Equal to `zswapStartIndex` for a transaction that adds no commitment. */
  zswapEndIndex: number;
}

export interface SeededRelevantTransaction extends SeededRelevantTransactionSpec {
  /** Random, so it clashes with no real transaction's hash. */
  hash: string;
}

/**
 * Seed transactions that are relevant for the given wallet with the given zswap
 * indices, as the shielded transactions subscription reads them, without going
 * through a node. Returns the seeded rows with their generated hashes.
 *
 * The rows are shaped so that nothing but that subscription ever acts on them:
 *
 * - `transactions.variant` is `System`, although a `regular_transactions` row is
 *   attached. The subscription's query joins `regular_transactions` without
 *   checking the variant, whereas the wallet-indexer scans `transactions` with
 *   `variant = 'Regular'` and would otherwise sweep the seeded rows into its
 *   next batch and move every wallet's `last_indexed_transaction_id` past the
 *   real chain for good.
 * - IDs are above `SEEDED_TRANSACTION_ID_BASE`, so they never collide with the
 *   chain-indexer's sequence; `BlockIndexed.max_transaction_id` comes from that
 *   indexer's own inserts, not from the table, so seeded IDs never reach the
 *   wallet-indexer's scan bound either.
 * - `raw`, `protocol_version` and the Merkle tree root are copied from the first
 *   real regular transaction so that any column-level consumer sees well-formed
 *   bytes; the hash is random and `identifiers` is empty so no query by hash or
 *   identifier of a real transaction picks a seeded row up.
 * - The rows sit in the block at height 1, which no test queries by height, and
 *   not in the genesis or latest block, which several do. A block query for
 *   height 1 would list a `System` transaction without a system row while the
 *   seeded rows exist, so callers delete them in `afterAll` via
 *   `deleteSeededTransactions`.
 */
export async function seedRelevantTransactions(
  walletId: string,
  specs: SeededRelevantTransactionSpec[],
): Promise<SeededRelevantTransaction[]> {
  if (!/^[0-9a-f-]{36}$/.test(walletId)) {
    throw new Error(`Wallet ID is not a UUID: ${walletId}`);
  }
  for (const { id, zswapStartIndex, zswapEndIndex } of specs) {
    if (!Number.isSafeInteger(id) || id < SEEDED_TRANSACTION_ID_BASE) {
      throw new Error(`Seeded transaction ID must be >= ${SEEDED_TRANSACTION_ID_BASE}, got ${id}`);
    }
    if (
      !Number.isSafeInteger(zswapStartIndex) ||
      !Number.isSafeInteger(zswapEndIndex) ||
      zswapStartIndex < 0 ||
      zswapEndIndex < zswapStartIndex
    ) {
      throw new Error(`Invalid zswap indices for ${id}: ${zswapStartIndex}..${zswapEndIndex}`);
    }
  }

  const referenceId = await queryScalar(
    "SELECT id FROM transactions WHERE variant = 'Regular' ORDER BY id LIMIT 1;",
  );
  if (referenceId === '') {
    throw new Error('No regular transaction indexed yet to copy raw bytes from');
  }

  const rows = specs.map((spec) => ({ ...spec, hash: randomBytes(32).toString('hex') }));
  const statements = rows.flatMap(({ id, hash, zswapStartIndex, zswapEndIndex }) => [
    `INSERT INTO transactions (id, block_id, variant, hash, protocol_version, raw)
     SELECT ${id},
            COALESCE((SELECT id FROM blocks WHERE height = 1), block_id),
            'System',
            decode('${hash}', 'hex'),
            protocol_version,
            raw
     FROM transactions WHERE id = ${referenceId};`,
    `INSERT INTO regular_transactions (
       id, transaction_result, zswap_merkle_tree_root,
       zswap_start_index, zswap_end_index,
       dust_commitment_start_index, dust_commitment_end_index,
       dust_generation_start_index, dust_generation_end_index,
       paid_fees, estimated_fees, identifiers
     )
     SELECT ${id}, transaction_result, zswap_merkle_tree_root,
            ${zswapStartIndex}, ${zswapEndIndex},
            dust_commitment_end_index, dust_commitment_end_index,
            dust_generation_end_index, dust_generation_end_index,
            NULL, NULL, '{}'
     FROM regular_transactions WHERE id = ${referenceId};`,
    `INSERT INTO relevant_transactions (wallet_id, transaction_id)
     VALUES ('${walletId}', ${id});`,
  ]);
  await runSql(statements.join('\n'));
  return rows;
}

/**
 * Delete every row seeded by `seedRelevantTransactions`, in this or any earlier
 * run against the same stack.
 */
export async function deleteSeededTransactions(): Promise<void> {
  await runSql(
    [
      `DELETE FROM relevant_transactions WHERE transaction_id >= ${SEEDED_TRANSACTION_ID_BASE};`,
      `DELETE FROM regular_transactions WHERE id >= ${SEEDED_TRANSACTION_ID_BASE};`,
      `DELETE FROM transactions WHERE id >= ${SEEDED_TRANSACTION_ID_BASE};`,
    ].join('\n'),
  );
}
