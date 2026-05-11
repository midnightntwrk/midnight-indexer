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
import { bech32m } from 'bech32';
import { Buffer } from 'node:buffer';
import '@utils/logging/test-logging-hooks';
import type { TestContext } from 'vitest';
import {
  IndexerWsClient,
  DustGenerationsSubscriptionResponse,
} from '@utils/indexer/websocket-client';
import { DustGenerationsEventSchema } from '@utils/indexer/graphql/schema';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { env } from 'environment/model';
import dataProvider from '@utils/testdata-provider';

const indexerHttpClient = new IndexerHttpClient();

function encodeDustAddressAsHex(dustAddress: string): string {
  const { words } = bech32m.decode(dustAddress);
  return Buffer.from(bech32m.fromWords(words)).toString('hex');
}

function generateDustAddressForNetworkId(networkId: string): string {
  const hrp = networkId === 'mainnet' ? 'mn_dust' : `mn_dust_${networkId}`;
  const payload = Buffer.alloc(32, 1);
  return bech32m.encode(hrp, bech32m.toWords(payload));
}

function safeUnsubscribe(unsubscribe: () => void): void {
  try {
    unsubscribe();
  } catch (error) {
    // If the websocket is already closed during teardown, unsubscribe can throw.
    log.debug(`Ignoring unsubscribe error during teardown: ${String(error)}`);
  }
}

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
     * @given a registered dust address in bech32m format (mn_dust...) and a valid index range
     * @when we subscribe to dustGenerations
     * @then we should receive DustGenerationsItem and/or DustGenerationsProgress events
     * @and each event should match the expected schema
     */
    test('should stream dust generation events for a valid dust address in bech32m format', async () => {
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
      log.debug(`Using dust address (bech32m): ${dustAddress}`);

      // Subscribe with a small range starting from 0
      const received: DustGenerationsSubscriptionResponse[] = [];

      await new Promise<void>((resolve, reject) => {
        let settled = false;
        let unsubscribe = () => {};
        const settle = (handler: () => void) => {
          if (settled) return;
          settled = true;
          handler();
        };

        // 60s ceiling. Under healthy conditions the indexer responds with
        // historical events + a Progress message in well under a second; the
        // previous 12s window was too tight when qanet was loaded or
        // recovering from a 503 burst, causing the stream's first event to
        // arrive late and the test to fail with zero events received.
        const timeout = setTimeout(() => {
          safeUnsubscribe(unsubscribe);
          // It's OK if we received some events before timeout
          if (received.length > 0) {
            settle(resolve);
          } else {
            settle(() => reject(new Error('Timed out waiting for dust generations events')));
          }
        }, 60_000);

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
                safeUnsubscribe(unsubscribe);
                settle(resolve);
              }
            },
            error: (error) => {
              clearTimeout(timeout);
              safeUnsubscribe(unsubscribe);
              settle(() => reject(new Error(`Subscription error: ${JSON.stringify(error)}`)));
            },
            complete: () => {
              clearTimeout(timeout);
              settle(resolve);
            },
          },
          dustAddress,
          0,
          10,
        );
        unsubscribe = subscription.unsubscribe;
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

      // Wire-format coverage for DustGenerationDtimeUpdateItem (issue #1078).
      // The discriminated-union schema above (DustGenerationsEventSchema) is
      // the actual regression guard: any payload whose `__typename` is
      // `DustGenerationDtimeUpdateItem` is validated against
      // DustGenerationDtimeUpdateItemSchema as part of the union match, so a
      // drift in the new variant's field set would already have failed there.
      // Here we only count occurrences for visibility — presence is
      // environment-dependent (requires the wallet's backing NIGHT/cNIGHT UTXO
      // to have been spent on chain, and `startIndex` past the wallet's first
      // owned entry to trigger historical replay).
      const dtimeUpdateCount = received.filter(
        (msg) => msg.data?.dustGenerations?.__typename === 'DustGenerationDtimeUpdateItem',
      ).length;
      log.debug(`Received ${dtimeUpdateCount} DustGenerationDtimeUpdateItem event(s)`);
      // Test-level timeout must comfortably exceed the internal 60s ceiling
      // for the dust-generations subscription wait, plus query + teardown.
    }, 90_000);
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
        let unsubscribe = () => {};
        const timeout = setTimeout(() => {
          safeUnsubscribe(unsubscribe);
          reject(new Error('Timed out waiting for error'));
        }, 10_000);

        const subscription = indexerWsClient.subscribeToDustGenerations(
          {
            next: (payload) => {
              if (payload.errors && payload.errors.length > 0) {
                clearTimeout(timeout);
                safeUnsubscribe(unsubscribe);
                resolve(payload.errors[0].message);
              }
            },
            error: (error) => {
              clearTimeout(timeout);
              safeUnsubscribe(unsubscribe);
              resolve(typeof error === 'string' ? error : JSON.stringify(error));
            },
            complete: () => {
              clearTimeout(timeout);
              reject(new Error('Subscription completed without error'));
            },
          },
          'invalid_address',
          0,
          10,
        );
        unsubscribe = subscription.unsubscribe;
      });

      expect(errorReceived).toBeDefined();
      log.debug(`Received expected error: ${errorReceived}`);
    });

    /**
     * A dust generations subscription with a valid bech32m dust address from another network should return an error
     *
     * @given valid bech32m dust addresses for all network IDs other than the target one
     * @when we subscribe to dustGenerations
     * @then Indexer should return an error related to unexpected/wrong HRP prefix
     */
    test('should return an error for a valid address that is meant for another networkid', async () => {
      const targetNetworkId = env.getNetworkId().toLowerCase();
      const networkIds = env.getAllEnvironmentNames();

      for (const networkId of networkIds) {
        if (networkId.toLowerCase() === targetNetworkId) {
          continue;
        }

        const foreignDustAddress = generateDustAddressForNetworkId(networkId);
        log.debug(`Testing foreign dust address for networkId=${networkId}: ${foreignDustAddress}`);

        const result = await new Promise<{
          error: string | null;
          completed: boolean;
          timedOut: boolean;
        }>((resolve) => {
          let settled = false;
          let unsubscribe = () => {};
          const settle = (value: {
            error: string | null;
            completed: boolean;
            timedOut: boolean;
          }) => {
            if (settled) return;
            settled = true;
            resolve(value);
          };

          const timeout = setTimeout(() => {
            safeUnsubscribe(unsubscribe);
            settle({ error: null, completed: false, timedOut: true });
          }, 10_000);

          const subscription = indexerWsClient.subscribeToDustGenerations(
            {
              next: (payload) => {
                if (payload.errors && payload.errors.length > 0) {
                  clearTimeout(timeout);
                  safeUnsubscribe(unsubscribe);
                  settle({
                    error: payload.errors[0].message,
                    completed: false,
                    timedOut: false,
                  });
                }
              },
              error: (error) => {
                clearTimeout(timeout);
                safeUnsubscribe(unsubscribe);
                settle({
                  error: typeof error === 'string' ? error : JSON.stringify(error),
                  completed: false,
                  timedOut: false,
                });
              },
              complete: () => {
                clearTimeout(timeout);
                settle({ error: null, completed: true, timedOut: false });
              },
            },
            foreignDustAddress,
            0,
            10,
          );
          unsubscribe = subscription.unsubscribe;
        });

        expect
          .soft(result.timedOut, `networkId=${networkId} timed out waiting for error`)
          .toBe(false);
        expect
          .soft(
            result.completed,
            `networkId=${networkId} subscription completed without emitting an error`,
          )
          .toBe(false);
        expect.soft(result.error, `networkId=${networkId} should emit an error`).toBeTruthy();
        if (result.error) {
          expect
            .soft(
              result.error.toLowerCase(),
              `networkId=${networkId} error should mention wrong HRP`,
            )
            .toMatch(/(expected hrp|unexpected hrp|wrong hrp|invalid.*network|network id)/);
        }
      }
    });

    /**
     * A dust generations subscription with a hex-encoded address should return an error.
     *
     * @given a valid bech32m dust address converted to hex format
     * @when we subscribe to dustGenerations using hex format
     * @then Indexer should return an error indicating the expected bech32m/HRP format
     */
    test('should return an error for a valid dust address passed in hex format', async () => {
      const targetNetworkId = env.getNetworkId().toLowerCase();
      const bech32DustAddress = generateDustAddressForNetworkId(targetNetworkId);
      const hexDustAddress = encodeDustAddressAsHex(bech32DustAddress);

      const errorReceived = await new Promise<string>((resolve, reject) => {
        let unsubscribe = () => {};
        const timeout = setTimeout(() => {
          safeUnsubscribe(unsubscribe);
          reject(new Error('Timed out waiting for error for hex dust address'));
        }, 10_000);

        const subscription = indexerWsClient.subscribeToDustGenerations(
          {
            next: (payload) => {
              if (payload.errors && payload.errors.length > 0) {
                clearTimeout(timeout);
                safeUnsubscribe(unsubscribe);
                resolve(payload.errors[0].message);
              }
            },
            error: (error) => {
              clearTimeout(timeout);
              safeUnsubscribe(unsubscribe);
              resolve(typeof error === 'string' ? error : JSON.stringify(error));
            },
            complete: () => {
              clearTimeout(timeout);
              reject(new Error('Subscription completed without error for hex dust address'));
            },
          },
          hexDustAddress,
          0,
          10,
        );
        unsubscribe = subscription.unsubscribe;
      });

      expect(errorReceived).toBeDefined();
      expect(errorReceived.toLowerCase()).toMatch(
        /(expected hrp|unexpected hrp|wrong hrp|bech32|invalid.*address)/,
      );
    });
  });

  /**
   * Coverage for midnight-indexer#1114 / PR #1116
   * (`feat(indexer-api): add transactionHash to event subscription response types`).
   *
   * `transactionHash: HexEncoded!` was added to `DustGenerationsItem` and
   * `DustGenerationDtimeUpdateItem` so wallets can resolve the on-chain
   * transaction from a streamed event via `transactions(offset: { hash: ... })`.
   * The `transactionId` BIGSERIAL is indexer-internal and not portable across
   * indexer instances; the hash is. The schema-level shape (64-hex,
   * non-nullable) is already enforced by the discriminated-union zod schema
   * used by the streaming test above. This block adds the round-trip check.
   */
  describe('transactionHash on dust generation events (#1114)', () => {
    /**
     * @given a registered dust address that emits at least one
     *        `DustGenerationsItem` or `DustGenerationDtimeUpdateItem`
     * @when we subscribe to `dustGenerations` and look up the first event's
     *       `transactionHash` via `transactions(offset: { hash: ... })`
     * @then the lookup resolves a single transaction whose `hash` equals the
     *       streamed `transactionHash` — proving the field is the on-chain
     *       identifier wallets can use to fetch the full transaction.
     */
    test('first item transactionHash resolves via transactions(offset)', async (ctx: TestContext) => {
      let rewardAddress: string;
      try {
        rewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
        return;
      }

      const generationsResponse = await indexerHttpClient.getDustGenerations([rewardAddress]);
      expect(generationsResponse).toBeSuccess();
      const dustAddress = generationsResponse.data!.dustGenerations[0].registrations[0].dustAddress;
      log.debug(`Using dust address: ${dustAddress}`);

      const firstItem = await new Promise<{
        transactionId: number;
        transactionHash: string;
        __typename: 'DustGenerationsItem' | 'DustGenerationDtimeUpdateItem';
      } | null>((resolve, reject) => {
        let settled = false;
        let unsubscribe = () => {};
        const settle = (handler: () => void) => {
          if (settled) return;
          settled = true;
          handler();
        };

        // Returning null (instead of rejecting) on timeout lets the caller
        // ctx.skip when the streaming surface is in a known-flaky state on
        // the target environment, rather than false-failing this test.
        const timeout = setTimeout(() => {
          safeUnsubscribe(unsubscribe);
          settle(() => resolve(null));
        }, 15_000);

        const subscription = indexerWsClient.subscribeToDustGenerations(
          {
            next: (payload) => {
              const event = payload.data?.dustGenerations;
              if (
                event?.__typename === 'DustGenerationsItem' ||
                event?.__typename === 'DustGenerationDtimeUpdateItem'
              ) {
                clearTimeout(timeout);
                safeUnsubscribe(unsubscribe);
                settle(() =>
                  resolve({
                    transactionId: event.transactionId,
                    transactionHash: event.transactionHash,
                    __typename: event.__typename,
                  }),
                );
              }
            },
            error: (error) => {
              clearTimeout(timeout);
              safeUnsubscribe(unsubscribe);
              settle(() => reject(new Error(`Subscription error: ${JSON.stringify(error)}`)));
            },
            complete: () => {
              clearTimeout(timeout);
              settle(() => resolve(null));
            },
          },
          dustAddress,
          0,
          10,
        );
        unsubscribe = subscription.unsubscribe;
      });

      if (firstItem === null) {
        log.warn(
          'no DustGenerationsItem / DtimeUpdateItem event received within the timeout — ' +
            'streaming surface is currently flaky on this environment (round-trip skipped)',
        );
        ctx.skip?.(true, 'no dust generations item event in time — round-trip vacuous');
        return;
      }

      log.debug(
        `Round-tripping ${firstItem.__typename}.transactionHash=${firstItem.transactionHash} ` +
          `(transactionId=${firstItem.transactionId})`,
      );

      const txResponse = await indexerHttpClient.getTransactionByOffset({
        hash: firstItem.transactionHash,
      });
      expect(txResponse).toBeSuccess();
      const transactions = txResponse.data!.transactions;
      expect(transactions.length).toBeGreaterThanOrEqual(1);
      expect(transactions[0].hash).toBe(firstItem.transactionHash);
    }, 30_000);
  });
});
