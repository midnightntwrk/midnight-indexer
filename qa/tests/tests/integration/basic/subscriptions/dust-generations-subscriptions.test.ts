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
  DustGenerationsSubscriptionResponse,
} from '@utils/indexer/websocket-client';
import { DustGenerationsEventSchema } from '@utils/indexer/graphql/schema';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import dataProvider from '@utils/testdata-provider';

const indexerHttpClient = new IndexerHttpClient();

describe('dust generations subscription', () => {
  let indexerWsClient: IndexerWsClient;

  beforeEach(async () => {
    indexerWsClient = new IndexerWsClient();
    await indexerWsClient.connectionInit();
  }, 30_000);

  afterEach(async () => {
    await indexerWsClient.connectionClose();
  });

  describe('streaming dust generation entries', () => {
    /**
     * A dust generations subscription streams items and ends with a progress event
     *
     * @given a registered dust address and a valid index range
     * @when we subscribe to dustGenerations
     * @then we should receive DustGenerationsItem and/or DustGenerationsProgress events
     * @and each event should match the expected schema
     */
    test('should stream dust generation events for a valid dust address', async () => {
      let rewardAddress: string;
      try {
        rewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust');
      } catch (error) {
        log.warn(error);
        return;
      }

      // Get the dust address from the generations query
      const generationsResponse = await indexerHttpClient.getDustGenerations([rewardAddress]);
      expect(generationsResponse).toBeSuccess();

      const generations = generationsResponse.data!.dustGenerations;
      expect(generations.length).toBeGreaterThanOrEqual(1);
      expect(generations[0].registrations.length).toBeGreaterThanOrEqual(1);

      const dustAddress = generations[0].registrations[0].dustAddress;
      log.debug(`Using dust address: ${dustAddress}`);

      // Subscribe with a small range starting from 0
      const received: DustGenerationsSubscriptionResponse[] = [];

      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => {
          subscription.unsubscribe();
          // It's OK if we received some events before timeout
          if (received.length > 0) {
            resolve();
          } else {
            reject(new Error('Timed out waiting for dust generations events'));
          }
        }, 15_000);

        const subscription = indexerWsClient.subscribeToDustGenerations(
          {
            next: (payload) => {
              received.push(payload);
              log.debug(
                `Received dust generations event ${received.length}: ${JSON.stringify(payload.data?.dustGenerations?.__typename)}`,
              );

              // Stop after receiving a progress event (indicates completion)
              if (payload.data?.dustGenerations?.__typename === 'DustGenerationsProgress') {
                clearTimeout(timeout);
                subscription.unsubscribe();
                resolve();
              }
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
          dustAddress,
          0,
          10,
        );
      });

      expect(received.length).toBeGreaterThan(0);

      // Validate each event against the schema
      for (const msg of received) {
        expect(msg).toBeSuccess();
        const event = msg.data!.dustGenerations;
        const parsed = DustGenerationsEventSchema.safeParse(event);
        expect(
          parsed.success,
          `Dust generations event schema validation failed: ${JSON.stringify(parsed.error, null, 2)}`,
        ).toBe(true);
      }

      // The last event should be a DustGenerationsProgress
      const lastEvent = received[received.length - 1].data!.dustGenerations;
      expect(lastEvent.__typename).toBe('DustGenerationsProgress');
    });
  });

  describe('subscription error handling', () => {
    /**
     * A dust generations subscription with an invalid dust address should return an error
     *
     * @given an invalid hex-encoded dust address
     * @when we subscribe to dustGenerations
     * @then the subscription should return an error
     */
    test('should return an error for an invalid dust address', async () => {
      const errorReceived = await new Promise<string>((resolve, reject) => {
        const timeout = setTimeout(() => {
          subscription.unsubscribe();
          reject(new Error('Timed out waiting for error'));
        }, 10_000);

        const subscription = indexerWsClient.subscribeToDustGenerations(
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
          'invalid_hex',
          0,
          10,
        );
      });

      expect(errorReceived).toBeDefined();
      log.debug(`Received expected error: ${errorReceived}`);
    });
  });
});
