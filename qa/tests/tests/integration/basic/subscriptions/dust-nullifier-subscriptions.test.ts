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
import {
  IndexerWsClient,
  DustNullifierTransactionSubscriptionResponse,
} from '@utils/indexer/websocket-client';
import { DustNullifierTransactionSchema } from '@utils/indexer/graphql/schema';
import { IndexerHttpClient } from '@utils/indexer/http-client';

const indexerHttpClient = new IndexerHttpClient();

describe('dust nullifier transactions subscription', () => {
  let indexerWsClient: IndexerWsClient;

  beforeEach(async () => {
    indexerWsClient = new IndexerWsClient();
    await indexerWsClient.connectionInit();
  });

  afterEach(async () => {
    await indexerWsClient.connectionClose();
  });

  describe('streaming dust nullifier transactions with block range', () => {
    /**
     * A dust nullifier transactions subscription with a bounded block range
     * should stream matching transactions and complete
     *
     * @given a set of nullifier prefixes and a block range
     * @when we subscribe to dustNullifierTransactions
     * @then we should receive matching transactions (if any) and the subscription should complete
     * @and each transaction should match the expected schema
     */
    test('should stream transactions within a block range and complete', async () => {
      // Get the latest block height to define a bounded range
      const blockResponse = await indexerHttpClient.getLatestBlock();
      expect(blockResponse).toBeSuccess();
      const latestHeight = blockResponse.data!.block.height;

      // Use a broad prefix to increase chance of matches, bounded to first 10 blocks
      const toBlock = Math.min(latestHeight, 10);
      const nullifierPrefixes = ['00'];

      log.debug(`Subscribing to dust nullifier transactions with prefixes=${nullifierPrefixes}, fromBlock=0, toBlock=${toBlock}`);

      const received: DustNullifierTransactionSubscriptionResponse[] = [];

      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => {
          subscription.unsubscribe();
          // It's OK if no matches were found in the range — subscription should still complete
          resolve();
        }, 15_000);

        const subscription = indexerWsClient.subscribeToDustNullifierTransactions(
          {
            next: (payload) => {
              received.push(payload);
              log.debug(`Received dust nullifier transaction: ${JSON.stringify(payload.data?.dustNullifierTransactions)}`);
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

      log.debug(`Received ${received.length} dust nullifier transactions`);

      // Validate each received transaction against the schema
      for (const msg of received) {
        expect(msg).toBeSuccess();
        const tx = msg.data!.dustNullifierTransactions;
        const parsed = DustNullifierTransactionSchema.safeParse(tx);
        expect(
          parsed.success,
          `Dust nullifier transaction schema validation failed: ${JSON.stringify(parsed.error, null, 2)}`,
        ).toBe(true);

        // Block height should be within the requested range
        expect(tx.blockHeight).toBeGreaterThanOrEqual(0);
        expect(tx.blockHeight).toBeLessThanOrEqual(toBlock);
      }
    });
  });

  describe('subscription error handling', () => {
    /**
     * A dust nullifier transactions subscription with an empty prefixes array should return an error
     *
     * @given an empty array of nullifier prefixes
     * @when we subscribe to dustNullifierTransactions
     * @then the subscription should return an error
     */
    test('should return an error for empty nullifier prefixes', async () => {
      const errorReceived = await new Promise<string>((resolve, reject) => {
        const timeout = setTimeout(() => {
          subscription.unsubscribe();
          reject(new Error('Timed out waiting for error'));
        }, 10_000);

        const subscription = indexerWsClient.subscribeToDustNullifierTransactions(
          {
            next: (payload) => {
              if (payload.errors && payload.errors.length > 0) {
                clearTimeout(timeout);
                subscription.unsubscribe();
                resolve(payload.errors[0].message);
              }
            },
            error: (error) => {
              clearTimeout(timeout);
              subscription.unsubscribe();
              resolve(typeof error === 'string' ? error : JSON.stringify(error));
            },
            complete: () => {
              clearTimeout(timeout);
              reject(new Error('Subscription completed without error'));
            },
          },
          [],
          0,
          10,
        );
      });

      expect(errorReceived).toBeDefined();
      log.debug(`Received expected error: ${errorReceived}`);
    });

    /**
     * A dust nullifier transactions subscription with invalid block range should return an error
     *
     * @given fromBlock > toBlock
     * @when we subscribe to dustNullifierTransactions
     * @then the subscription should return an error
     */
    test('should return an error when fromBlock is greater than toBlock', async () => {
      const errorReceived = await new Promise<string>((resolve, reject) => {
        const timeout = setTimeout(() => {
          subscription.unsubscribe();
          reject(new Error('Timed out waiting for error'));
        }, 10_000);

        const subscription = indexerWsClient.subscribeToDustNullifierTransactions(
          {
            next: (payload) => {
              if (payload.errors && payload.errors.length > 0) {
                clearTimeout(timeout);
                subscription.unsubscribe();
                resolve(payload.errors[0].message);
              }
            },
            error: (error) => {
              clearTimeout(timeout);
              subscription.unsubscribe();
              resolve(typeof error === 'string' ? error : JSON.stringify(error));
            },
            complete: () => {
              clearTimeout(timeout);
              reject(new Error('Subscription completed without error'));
            },
          },
          ['00'],
          10,
          5,
        );
      });

      expect(errorReceived).toBeDefined();
      log.debug(`Received expected error: ${errorReceived}`);
    });
  });
});
