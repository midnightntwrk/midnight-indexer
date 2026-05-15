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

import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import type { TestContext } from 'vitest';
import {
  IndexerWsClient,
  ShieldedNullifierTransactionSubscriptionResponse,
} from '@utils/indexer/websocket-client';
import { ShieldedNullifierTransactionSchema } from '@utils/indexer/graphql/schema';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { ShieldedNullifierTransaction } from '@utils/indexer/indexer-types';

const indexerHttpClient = new IndexerHttpClient();

/**
 * Coverage for midnight-indexer#994 / PR #996
 * (`feat: add shieldedNullifierTransactions subscription`).
 *
 * The subscription returns transaction references for any shielded (Zswap)
 * transaction whose nullifiers match one of the provided hex prefixes, within
 * an optional block range. It mirrors `dustNullifierTransactions` but on the
 * shielded surface. Wallets use it to detect spends of coins they discovered
 * via trial-decryption that the regular shielded sync wouldn't otherwise
 * surface (the shielded sync only catches transactions with outputs for the
 * provided viewing key, not pure nullifier-only spends).
 *
 * Mirrors the structure of `dust-nullifier-subscriptions.test.ts` so the two
 * surfaces stay symmetric.
 */
describe('shielded nullifier transactions subscription', () => {
  let indexerWsClient: IndexerWsClient;

  beforeEach(async () => {
    indexerWsClient = new IndexerWsClient();
    await indexerWsClient.connectionInit();
  }, 30_000);

  afterEach(async () => {
    await indexerWsClient.connectionClose();
  });

  describe('streaming shielded nullifier transactions with block range', () => {
    /**
     * @given a set of nullifier prefixes and a bounded block range
     * @when we subscribe to shieldedNullifierTransactions
     * @then we should receive matching transactions (if any) and the
     *       subscription should complete once `toBlock` is reached
     * @and each transaction should match the expected schema
     */
    test('should stream transactions within a block range and complete', async () => {
      const blockResponse = await indexerHttpClient.getLatestBlock();
      expect(blockResponse).toBeSuccess();
      const latestHeight = blockResponse.data!.block.height;

      // Use a broad prefix to maximise the chance of matches in the early
      // window. Bounded to first 10 blocks for determinism.
      const toBlock = Math.min(latestHeight, 10);
      const nullifierPrefixes = ['00'];

      log.debug(
        `Subscribing to shielded nullifier transactions with prefixes=${nullifierPrefixes}, fromBlock=0, toBlock=${toBlock}`,
      );

      const received: ShieldedNullifierTransactionSubscriptionResponse[] = [];

      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => {
          subscription.unsubscribe();
          // OK if no matches found in the range — subscription still
          // completes; absent matches is a valid outcome.
          resolve();
        }, 15_000);

        const subscription = indexerWsClient.subscribeToShieldedNullifierTransactions(
          {
            next: (payload) => {
              received.push(payload);
              log.debug(
                `Received shielded nullifier transaction: ${JSON.stringify(payload.data?.shieldedNullifierTransactions)}`,
              );
            },
            error: (error) => {
              clearTimeout(timeout);
              subscription.unsubscribe();
              reject(new Error(`Subscription error: ${JSON.stringify(error)}`));
            },
            complete: () => {
              clearTimeout(timeout);
              resolve();
            },
          },
          nullifierPrefixes,
          0,
          toBlock,
        );
      });

      log.debug(`Received ${received.length} shielded nullifier transactions`);

      for (const msg of received) {
        expect(msg).toBeSuccess();
        const tx = msg.data!.shieldedNullifierTransactions;
        const parsed = ShieldedNullifierTransactionSchema.safeParse(tx);
        expect(
          parsed.success,
          `Shielded nullifier transaction schema validation failed: ${JSON.stringify(parsed.error, null, 2)}`,
        ).toBe(true);

        // Block height should be within the requested range.
        expect(tx.blockHeight).toBeGreaterThanOrEqual(0);
        expect(tx.blockHeight).toBeLessThanOrEqual(toBlock);
      }
    });
  });

  /**
   * Behaviour divergence vs `dustNullifierTransactions`:
   *
   * `dustNullifierTransactions` (since midnight-indexer#1089 / PR #1090)
   * rejects two malformed inputs as client errors:
   *   - empty `nullifierPrefixes` array (`[]`)
   *   - `fromBlock > toBlock`
   *
   * `shieldedNullifierTransactions` does NOT enforce either guard at the time
   * of writing. The tests below intentionally record the current behaviour
   * (subscription accepts the input without surfacing an error) so that:
   *   1. Future symmetric hardening on the shielded surface is caught — these
   *      tests will start failing when validation is added, prompting an
   *      update to mirror the dust pattern.
   *   2. The asymmetry is visible in the QA suite rather than silently
   *      tolerated.
   *
   * If/when an issue is filed to harden the shielded validation, link it
   * here and convert the assertions to expect the matching client errors.
   */
  describe('input handling (currently permissive vs dust)', () => {
    /**
     * @given an empty array of nullifier prefixes
     * @when we subscribe to shieldedNullifierTransactions with toBlock set
     * @then the subscription does NOT raise a client error (recorded
     *       behaviour; dust rejects this since #1089). It either completes
     *       cleanly or stays open without emitting an event for the
     *       observation window.
     */
    test('should not raise an error for empty nullifier prefixes (records divergence from dust)', async () => {
      const blockResponse = await indexerHttpClient.getLatestBlock();
      const toBlock = Math.min(blockResponse.data!.block.height, 5);

      const settled = await new Promise<{
        completed: boolean;
        error: string | null;
        eventCount: number;
      }>((resolve) => {
        let eventCount = 0;
        const timeout = setTimeout(() => {
          subscription.unsubscribe();
          resolve({ completed: false, error: null, eventCount });
        }, 8_000);

        const subscription = indexerWsClient.subscribeToShieldedNullifierTransactions(
          {
            next: (payload) => {
              eventCount++;
              if (payload.errors && payload.errors.length > 0) {
                clearTimeout(timeout);
                subscription.unsubscribe();
                resolve({
                  completed: false,
                  error: payload.errors[0].message,
                  eventCount,
                });
              }
            },
            error: (error) => {
              clearTimeout(timeout);
              subscription.unsubscribe();
              resolve({
                completed: false,
                error: typeof error === 'string' ? error : JSON.stringify(error),
                eventCount,
              });
            },
            complete: () => {
              clearTimeout(timeout);
              resolve({ completed: true, error: null, eventCount });
            },
          },
          [],
          0,
          toBlock,
        );
      });

      log.debug(`empty-prefix shielded outcome: ${JSON.stringify(settled)}`);
      // Permissive: no error message surfaced. If this changes, the shielded
      // surface has gained validation and this test should be reworked to
      // assert the new error message (see header comment).
      expect(settled.error).toBeNull();
      expect(settled.eventCount).toBe(0);
      expect(settled.completed).toBe(true);
    });

    /**
     * @given fromBlock greater than toBlock
     * @when we subscribe to shieldedNullifierTransactions
     * @then the subscription does NOT raise a client error (recorded
     *       behaviour; dust rejects this since #1089).
     */
    test('should not raise an error when fromBlock is greater than toBlock (records divergence from dust)', async () => {
      const settled = await new Promise<{
        completed: boolean;
        error: string | null;
        eventCount: number;
      }>((resolve) => {
        let eventCount = 0;
        const timeout = setTimeout(() => {
          subscription.unsubscribe();
          resolve({ completed: false, error: null, eventCount });
        }, 8_000);

        const subscription = indexerWsClient.subscribeToShieldedNullifierTransactions(
          {
            next: (payload) => {
              eventCount++;
              if (payload.errors && payload.errors.length > 0) {
                clearTimeout(timeout);
                subscription.unsubscribe();
                resolve({
                  completed: false,
                  error: payload.errors[0].message,
                  eventCount,
                });
              }
            },
            error: (error) => {
              clearTimeout(timeout);
              subscription.unsubscribe();
              resolve({
                completed: false,
                error: typeof error === 'string' ? error : JSON.stringify(error),
                eventCount,
              });
            },
            complete: () => {
              clearTimeout(timeout);
              resolve({ completed: true, error: null, eventCount });
            },
          },
          ['00'],
          10,
          5,
        );
      });

      log.debug(`fromBlock>toBlock shielded outcome: ${JSON.stringify(settled)}`);
      expect(settled.error).toBeNull();
      expect(settled.eventCount).toBe(0);
      expect(settled.completed).toBe(true);
    });
  });

  /**
   * Coverage for midnight-indexer#1114 / PR #1116
   * (`feat(indexer-api): add transactionHash to event subscription response types`).
   *
   * `transactionHash: HexEncoded!` was added to `ShieldedNullifierTransaction`.
   * The schema-level shape (64-hex, non-nullable) is already enforced by the
   * `ShieldedNullifierTransactionSchema` used by the streaming test above.
   * This block adds the round-trip check: the streamed hash must resolve a
   * transaction via `transactions(offset: { hash: ... })`.
   *
   * Match presence is environment-dependent. If no transactions match within
   * the timeout, the round-trip is vacuous and we skip rather than asserting
   * against an empty stream.
   */
  describe('transactionHash on shielded nullifier events (#1114)', () => {
    /**
     * @given a wide prefix scan of the full chain
     * @when we subscribe to `shieldedNullifierTransactions` and look up the
     *       first streamed event's `transactionHash` via `transactions(offset)`
     * @then the lookup resolves a single transaction whose `hash` equals the
     *       streamed `transactionHash` — proving the field is the on-chain
     *       identifier.
     */
    test('first event transactionHash resolves via transactions(offset)', async (ctx: TestContext) => {
      const blockResponse = await indexerHttpClient.getLatestBlock();
      expect(blockResponse).toBeSuccess();
      const latestHeight = blockResponse.data!.block.height;

      const received: ShieldedNullifierTransaction[] = [];

      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => {
          subscription.unsubscribe();
          resolve();
        }, 15_000);

        const subscription = indexerWsClient.subscribeToShieldedNullifierTransactions(
          {
            next: (payload) => {
              const tx = payload.data?.shieldedNullifierTransactions;
              if (tx) {
                received.push(tx);
                clearTimeout(timeout);
                subscription.unsubscribe();
                resolve();
              }
            },
            error: (err) => {
              clearTimeout(timeout);
              subscription.unsubscribe();
              reject(new Error(`Subscription error: ${JSON.stringify(err)}`));
            },
            complete: () => {
              clearTimeout(timeout);
              resolve();
            },
          },
          ['00'],
          0,
          latestHeight,
        );
      });

      if (received.length === 0) {
        log.warn(
          'no shieldedNullifierTransactions matched prefix "00" within the timeout; ' +
            'round-trip skipped (environment has no shielded nullifier transactions in range)',
        );
        ctx.skip?.(
          true,
          'no shielded nullifier transactions matched within timeout — round-trip vacuous',
        );
        return;
      }

      const first = received[0];
      log.debug(
        `Round-tripping ShieldedNullifierTransaction.transactionHash=${first.transactionHash} ` +
          `(transactionId=${first.transactionId})`,
      );

      const txResponse = await indexerHttpClient.getTransactionByOffset({
        hash: first.transactionHash,
      });
      expect(txResponse).toBeSuccess();
      const transactions = txResponse.data!.transactions;
      expect(transactions).toHaveLength(1);
      expect(transactions[0].hash).toBe(first.transactionHash);
    }, 30_000);
  });
});
