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
import { env } from 'environment/model';
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import { IndexerWsClient } from '@utils/indexer/websocket-client';
import { extractSubscriptionErrorMessage } from '@utils/indexer/subscription-error';
import { contractEventsSurfacePresent } from '@utils/indexer/contract-events-support';
import { ContractEventUnionSchema } from '@utils/indexer/graphql/schema';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import type { ContractEvent } from '@utils/indexer/indexer-types';
import dataProvider, { type EventEmittingContractInfo } from '@utils/testdata-provider';

const httpClient = new IndexerHttpClient();

describe('contract event subscription', () => {
  let surfacePresent = false;
  let wsClient: IndexerWsClient;

  beforeAll(async () => {
    surfacePresent = await contractEventsSurfacePresent();
    if (!surfacePresent) {
      log.warn(
        `Contract events surface absent on ${env.getCurrentEnvironmentName()}; ` +
          `contract event subscription tests will be skipped.`,
      );
    }
  }, 30_000);

  beforeEach(async () => {
    if (!surfacePresent) return;
    wsClient = new IndexerWsClient();
    await wsClient.connectionInit();
  }, 30_000);

  afterEach(async () => {
    if (!surfacePresent) return;
    await wsClient.connectionClose();
  });

  describe('a contract events subscription for an address with no emitted events', () => {
    /**
     * A bounded subscription for an idle address streams nothing and terminates.
     *
     * @given a valid-format contract address that has emitted no events and a
     *        toBlock the chain has already passed
     * @when a contract events subscription is opened with that bounded filter
     * @then the stream completes via the toBlock terminator without delivering events
     */
    test('should complete via the toBlock terminator without delivering events', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'ContractEvents'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const blockResponse = await httpClient.getLatestBlock();
      expect(blockResponse).toBeSuccess();
      const toBlock = Math.min(blockResponse.data!.block.height, 5);
      const contractAddress = dataProvider.getNonExistingContractAddress();

      const settled = await new Promise<{ completed: boolean; eventCount: number }>(
        (resolve, reject) => {
          let eventCount = 0;
          const timeout = setTimeout(() => {
            subscription.unsubscribe();
            reject(new Error('Timed out waiting for the toBlock terminator'));
          }, 20_000);

          const subscription = wsClient.subscribeToContractEvents(
            {
              next: () => {
                eventCount++;
              },
              error: (error) => {
                clearTimeout(timeout);
                subscription.unsubscribe();
                reject(new Error(`Subscription error: ${JSON.stringify(error)}`));
              },
              complete: () => {
                clearTimeout(timeout);
                resolve({ completed: true, eventCount });
              },
            },
            { contractAddress, fromBlock: 0, toBlock },
            0,
          );
        },
      );

      expect(settled.completed).toBe(true);
      expect(settled.eventCount).toBe(0);
    }, 30_000);
  });

  describe('a contract events subscription with an invalid filter', () => {
    /**
     * An empty contractAddress is rejected by the subscription.
     *
     * @given a contract events filter whose contractAddress is the empty string
     * @when a contract events subscription is opened with that filter
     * @then the subscription surfaces an error rather than streaming events
     */
    test('should return an error when the contract address is empty', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'ContractEvents', 'Negative'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      const settled = await new Promise<{ error: string | null; eventCount: number }>((resolve) => {
        let eventCount = 0;
        const timeout = setTimeout(() => {
          subscription.unsubscribe();
          resolve({ error: null, eventCount });
        }, 10_000);

        const subscription = wsClient.subscribeToContractEvents(
          {
            next: () => {
              eventCount++;
            },
            error: (error) => {
              clearTimeout(timeout);
              subscription.unsubscribe();
              resolve({ error: extractSubscriptionErrorMessage(error), eventCount });
            },
            complete: () => {
              clearTimeout(timeout);
              resolve({ error: null, eventCount });
            },
          },
          { contractAddress: '' },
          0,
        );
      });

      expect(settled.error).not.toBeNull();
      expect(settled.eventCount).toBe(0);
    }, 20_000);
  });

  describe('a contract events subscription for a contract with emitted events', () => {
    /**
     * Streamed events conform to the contract event schema and belong to the contract.
     *
     * Skipped until an event-emitting contract fixture is configured for the
     * environment (see testdata-provider.getEventEmittingContracts); the
     * emit-bearing toolchain path is tracked by midnight-indexer#1163.
     *
     * @given a contract known to have emitted public contract events
     * @when a contract events subscription is opened for that contract from the
     *       start of the chain and bounded by the latest block
     * @then the stream delivers at least one event, each conforming to the
     *       contract event schema and reporting the subscribed contract address
     */
    test('should stream events conforming to the contract event schema', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'ContractEvents'] };
      if (!surfacePresent) return ctx.skip?.(true, 'contract events surface not present');

      let contracts: EventEmittingContractInfo[];
      try {
        contracts = dataProvider.getEventEmittingContracts();
      } catch (error) {
        log.warn(error);
        return ctx.skip?.(true, (error as Error).message);
      }

      const blockResponse = await httpClient.getLatestBlock();
      expect(blockResponse).toBeSuccess();
      const toBlock = blockResponse.data!.block.height;
      const contractAddress = contracts[0]['contract-address'];

      const received: ContractEvent[] = [];
      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => {
          subscription.unsubscribe();
          resolve();
        }, 20_000);

        const subscription = wsClient.subscribeToContractEvents(
          {
            next: (payload) => {
              const event = payload.data?.contractEvents;
              if (event) received.push(event);
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
          { contractAddress, fromBlock: 0, toBlock },
          0,
        );
      });

      expect(received.length).toBeGreaterThan(0);
      for (const event of received) {
        const parsed = ContractEventUnionSchema.safeParse(event);
        expect(
          parsed.success,
          `Contract event schema validation failed: ${JSON.stringify(parsed.error, null, 2)}`,
        ).toBe(true);
        expect(event.contractAddress).toBe(contractAddress);
      }
    }, 30_000);
  });
});
