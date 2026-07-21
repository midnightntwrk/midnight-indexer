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
import { extractSubscriptionErrorMessage } from '@utils/indexer/subscription-error';
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

/**
 * Resolves the dust address a wallet registered for the given Cardano reward address.
 */
async function fetchDustAddress(rewardAddress: string): Promise<string> {
  const generationsResponse = await indexerHttpClient.getDustGenerations([rewardAddress]);
  expect(generationsResponse).toBeSuccess();
  const generations = generationsResponse.data!.dustGenerations;
  expect(generations.length).toBeGreaterThanOrEqual(1);
  expect(generations[0].registrations.length).toBeGreaterThanOrEqual(1);
  return generations[0].registrations[0].dustAddress;
}

/**
 * Fetches the block the subscription snapshot is pinned to.
 */
async function fetchBlock(offset?: {
  height: number;
}): Promise<{ hash: string; height: number; dustGenerationEndIndex: number }> {
  const response = offset
    ? await indexerHttpClient.getBlockByOffset(offset)
    : await indexerHttpClient.getLatestBlock();
  expect(response).toBeSuccess();
  const block = response.data!.block!;
  return {
    hash: block.hash,
    height: block.height,
    dustGenerationEndIndex: block.dustGenerationEndIndex!,
  };
}

interface DustGenerationsSubscriptionArgs {
  dustAddress: string;
  blockHash: string;
  dtimeCutoffHeight: number;
}

/**
 * Subscribes to dustGenerations and collects every event until the server completes
 * the subscription. The block-hash-scoped subscription is finite by design, so
 * completion is the expected terminal signal; an error or a timeout rejects.
 */
function collectDustGenerations(
  wsClient: IndexerWsClient,
  args: DustGenerationsSubscriptionArgs,
  timeoutMs = 30_000,
): Promise<DustGenerationsSubscriptionResponse[]> {
  return new Promise((resolve, reject) => {
    const events: DustGenerationsSubscriptionResponse[] = [];
    let settled = false;
    let unsubscribe = () => {};
    const settle = (handler: () => void) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      handler();
    };

    const timeout = setTimeout(() => {
      safeUnsubscribe(unsubscribe);
      settle(() =>
        reject(
          new Error(
            `Timed out after ${timeoutMs}ms waiting for the dust generations subscription ` +
              `to complete (received ${events.length} events)`,
          ),
        ),
      );
    }, timeoutMs);

    const subscription = wsClient.subscribeToDustGenerations(
      {
        next: (payload) => {
          events.push(payload);
        },
        error: (error) => {
          safeUnsubscribe(unsubscribe);
          settle(() => reject(new Error(`Subscription error: ${JSON.stringify(error)}`)));
        },
        complete: () => {
          settle(() => resolve(events));
        },
      },
      args.dustAddress,
      args.blockHash,
      args.dtimeCutoffHeight,
    );
    unsubscribe = subscription.unsubscribe;
  });
}

/**
 * Subscribes to dustGenerations and resolves with the subscription error message.
 * Completion without an error, or a timeout, rejects.
 */
function collectDustGenerationsError(
  wsClient: IndexerWsClient,
  args: DustGenerationsSubscriptionArgs,
  timeoutMs = 10_000,
): Promise<string> {
  return new Promise((resolve, reject) => {
    let unsubscribe = () => {};
    const timeout = setTimeout(() => {
      safeUnsubscribe(unsubscribe);
      reject(new Error('Timed out waiting for a subscription error'));
    }, timeoutMs);

    const subscription = wsClient.subscribeToDustGenerations(
      {
        error: (error) => {
          clearTimeout(timeout);
          safeUnsubscribe(unsubscribe);
          resolve(extractSubscriptionErrorMessage(error));
        },
        complete: () => {
          clearTimeout(timeout);
          reject(new Error('Subscription completed without error'));
        },
      },
      args.dustAddress,
      args.blockHash,
      args.dtimeCutoffHeight,
    );
    unsubscribe = subscription.unsubscribe;
  });
}

function assertEventsMatchSchema(events: DustGenerationsSubscriptionResponse[]): void {
  for (const msg of events) {
    expect(msg).toBeSuccess();
    const event = msg.data!.dustGenerations;
    const parsed = DustGenerationsEventSchema.safeParse(event);
    expect(
      parsed.success,
      `Dust generations event schema validation failed: ${JSON.stringify(parsed.error, null, 2)}`,
    ).toBe(true);
  }
}

function eventsOfType(
  events: DustGenerationsSubscriptionResponse[],
  typename: string,
): DustGenerationsSubscriptionResponse[] {
  return events.filter((msg) => msg.data?.dustGenerations?.__typename === typename);
}

// Dust generation registrations require a Cardano-side mapping which has no
// counterpart in the `undeployed` environment. Skip the whole surface there;
// re-enable once #1152 lands local Cardano test-data provisioning.
describe.skipIf(env.isUndeployedEnv())('dust generations subscription', () => {
  let indexerWsClient: IndexerWsClient;

  beforeEach(async () => {
    indexerWsClient = new IndexerWsClient();
    await indexerWsClient.connectionInit();
  }, 30_000);

  afterEach(async () => {
    await indexerWsClient.connectionClose();
  });

  describe('a subscription at the latest block hash', () => {
    /**
     * A dust generations subscription streams a finite snapshot and completes.
     *
     * @given a registered dust address and the latest block hash
     * @when a dustGenerations subscription is opened at that block with dtimeCutoffHeight 0
     * @then generation events are streamed, the last event is a DustGenerationsProgress,
     *       and the subscription completes on its own
     * @and each event matches the expected schema
     */
    test('should stream a complete generation snapshot for a registered dust address', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Dust', 'Generations'] };

      let rewardAddress: string;
      try {
        rewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
        return;
      }

      const dustAddress = await fetchDustAddress(rewardAddress);
      const block = await fetchBlock();
      log.debug(`Subscribing for ${dustAddress} at block ${block.height} (${block.hash})`);

      const events = await collectDustGenerations(indexerWsClient, {
        dustAddress,
        blockHash: block.hash,
        dtimeCutoffHeight: 0,
      });

      expect(events.length).toBeGreaterThan(0);
      assertEventsMatchSchema(events);

      const lastEvent = events[events.length - 1].data!.dustGenerations;
      expect(lastEvent.__typename).toBe('DustGenerationsProgress');
    }, 60_000);
  });

  describe('a subscription pinned to a block hash', () => {
    /**
     * The block-hash snapshot is deterministic: the same block yields the same events.
     *
     * @given a registered dust address and a fixed block hash
     * @when two dustGenerations subscriptions are opened with identical arguments
     * @then both deliver exactly the same event sequence
     *
     * midnight-indexer#1283
     */
    test('should deliver identical events for repeated subscriptions at the same block', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Dust', 'Generations'] };

      let rewardAddress: string;
      try {
        rewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
        return;
      }

      const dustAddress = await fetchDustAddress(rewardAddress);
      const block = await fetchBlock();
      const args = { dustAddress, blockHash: block.hash, dtimeCutoffHeight: 0 };

      const firstRun = await collectDustGenerations(indexerWsClient, args);
      const secondRun = await collectDustGenerations(indexerWsClient, args);

      expect(firstRun.length).toBeGreaterThan(0);
      expect(secondRun.length).toBe(firstRun.length);
      expect(secondRun.map((msg) => msg.data!.dustGenerations)).toStrictEqual(
        firstRun.map((msg) => msg.data!.dustGenerations),
      );
    }, 90_000);

    /**
     * The snapshot reflects the queried block's generation tree, not the tip's.
     *
     * @given two blocks between which the dust generation tree has grown
     *        (their dustGenerationEndIndex values differ)
     * @when a dustGenerations subscription is opened at each block's hash
     * @then each final progress event reports highestIndex equal to that block's
     *       dustGenerationEndIndex - 1, so the earlier block yields the smaller snapshot
     *
     * midnight-indexer#1283
     */
    test('should snapshot the generation tree at the queried block rather than the tip', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Dust', 'Generations'] };

      let rewardAddress: string;
      try {
        rewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
        return;
      }

      const dustAddress = await fetchDustAddress(rewardAddress);
      const tipBlock = await fetchBlock();
      const earlierBlock = await fetchBlock({ height: Math.floor(tipBlock.height / 2) });

      if (
        earlierBlock.dustGenerationEndIndex === 0 ||
        earlierBlock.dustGenerationEndIndex === tipBlock.dustGenerationEndIndex
      ) {
        ctx.skip?.(
          true,
          `generation tree did not grow between block ${earlierBlock.height} ` +
            `(endIndex ${earlierBlock.dustGenerationEndIndex}) and block ${tipBlock.height} ` +
            `(endIndex ${tipBlock.dustGenerationEndIndex}) — snapshot comparison is vacuous`,
        );
        return;
      }

      for (const block of [earlierBlock, tipBlock]) {
        const events = await collectDustGenerations(indexerWsClient, {
          dustAddress,
          blockHash: block.hash,
          dtimeCutoffHeight: 0,
        });

        const progressEvents = eventsOfType(events, 'DustGenerationsProgress');
        expect(progressEvents).toHaveLength(1);
        const progress = progressEvents[0].data!.dustGenerations as { highestIndex: number };
        expect(
          progress.highestIndex,
          `highestIndex at block ${block.height} should reflect that block's tree size`,
        ).toBe(block.dustGenerationEndIndex - 1);
      }
    }, 90_000);
  });

  describe('dtime update delivery relative to the cutoff height', () => {
    /**
     * A zero cutoff replays the wallet's full owned dtime history before the tree events.
     *
     * @given a wallet with spent backing NIGHT UTXOs (registered-with-dust-and-spent)
     * @when a dustGenerations subscription is opened with dtimeCutoffHeight 0
     * @then at least one DustGenerationDtimeUpdateItem is delivered
     * @and every dtime update precedes the first DustGenerationsItem in the stream
     *
     * midnight-indexer#1283 (supersedes the startIndex-based #1167 regression guard)
     */
    test('should replay owned dtime updates before generation items when the cutoff is zero', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Dust', 'Generations'] };

      let rewardAddress: string;
      try {
        rewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust-and-spent');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
        return;
      }

      const dustAddress = await fetchDustAddress(rewardAddress);
      const block = await fetchBlock();

      const events = await collectDustGenerations(indexerWsClient, {
        dustAddress,
        blockHash: block.hash,
        dtimeCutoffHeight: 0,
      });

      assertEventsMatchSchema(events);

      const typenames = events.map((msg) => msg.data!.dustGenerations.__typename);
      const dtimeCount = typenames.filter((t) => t === 'DustGenerationDtimeUpdateItem').length;
      log.debug(`Received ${dtimeCount} DustGenerationDtimeUpdateItem event(s)`);
      expect(
        dtimeCount,
        'Expected ≥1 DustGenerationDtimeUpdateItem with dtimeCutoffHeight=0 ' +
          'for a wallet with spent NIGHT UTXOs',
      ).toBeGreaterThanOrEqual(1);

      const firstItemIndex = typenames.indexOf('DustGenerationsItem');
      const lastDtimeIndex = typenames.lastIndexOf('DustGenerationDtimeUpdateItem');
      if (firstItemIndex !== -1) {
        expect(
          lastDtimeIndex,
          'All dtime updates should be issued before the first DustGenerationsItem',
        ).toBeLessThan(firstItemIndex);
      }
    }, 60_000);

    /**
     * A cutoff at the snapshot block suppresses the dtime delta entirely.
     *
     * @given a wallet with spent backing NIGHT UTXOs (registered-with-dust-and-spent)
     * @when a dustGenerations subscription is opened with the dtimeCutoffHeight equal to
     *       the snapshot block's height
     * @then no DustGenerationDtimeUpdateItem is delivered, while the generation snapshot
     *       (items and final progress) still streams and completes
     *
     * midnight-indexer#1283
     */
    test('should deliver no dtime updates when the cutoff equals the snapshot block height', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Dust', 'Generations'] };

      let rewardAddress: string;
      try {
        rewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust-and-spent');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
        return;
      }

      const dustAddress = await fetchDustAddress(rewardAddress);
      const block = await fetchBlock();

      const events = await collectDustGenerations(indexerWsClient, {
        dustAddress,
        blockHash: block.hash,
        dtimeCutoffHeight: block.height,
      });

      assertEventsMatchSchema(events);
      expect(eventsOfType(events, 'DustGenerationDtimeUpdateItem')).toHaveLength(0);
      expect(eventsOfType(events, 'DustGenerationsProgress')).toHaveLength(1);
    }, 60_000);
  });

  describe('subscription error handling', () => {
    /**
     * A dust generations subscription with an invalid dust address returns an error.
     *
     * @given an invalid dust address and a valid block hash
     * @when a dustGenerations subscription is opened
     * @then the subscription returns an error
     */
    test('should return an error for an invalid dust address', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Dust', 'Generations', 'Negative'] };

      const block = await fetchBlock();
      const errorReceived = await collectDustGenerationsError(indexerWsClient, {
        dustAddress: 'invalid_address',
        blockHash: block.hash,
        dtimeCutoffHeight: 0,
      });

      expect(errorReceived).toBeDefined();
      log.debug(`Received expected error: ${errorReceived}`);
    });

    /**
     * A valid bech32m dust address from another network returns an HRP error.
     *
     * @given valid bech32m dust addresses for all network IDs other than the target one
     *        and a valid block hash
     * @when a dustGenerations subscription is opened for each foreign address
     * @then the indexer returns an error related to an unexpected/wrong HRP prefix
     */
    test('should return an error for a valid address that is meant for another networkid', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Dust', 'Generations', 'Negative'] };

      const targetNetworkId = env.getNetworkId().toLowerCase();
      const networkIds = env.getAllEnvironmentNames();
      const block = await fetchBlock();

      for (const networkId of networkIds) {
        if (networkId.toLowerCase() === targetNetworkId) {
          continue;
        }

        const foreignDustAddress = generateDustAddressForNetworkId(networkId);
        log.debug(`Testing foreign dust address for networkId=${networkId}: ${foreignDustAddress}`);

        const result = await collectDustGenerationsError(indexerWsClient, {
          dustAddress: foreignDustAddress,
          blockHash: block.hash,
          dtimeCutoffHeight: 0,
        }).then(
          (error) => ({ error, failure: null as string | null }),
          (failure: Error) => ({ error: null as string | null, failure: failure.message }),
        );

        expect.soft(result.failure, `networkId=${networkId}: ${result.failure}`).toBeNull();
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
     * A dust address passed in hex format returns a bech32m/HRP error.
     *
     * @given a valid bech32m dust address converted to hex format and a valid block hash
     * @when a dustGenerations subscription is opened using the hex format
     * @then the indexer returns an error indicating the expected bech32m/HRP format
     */
    test('should return an error for a valid dust address passed in hex format', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Dust', 'Generations', 'Negative'] };

      const targetNetworkId = env.getNetworkId().toLowerCase();
      const bech32DustAddress = generateDustAddressForNetworkId(targetNetworkId);
      const hexDustAddress = encodeDustAddressAsHex(bech32DustAddress);
      const block = await fetchBlock();

      const errorReceived = await collectDustGenerationsError(indexerWsClient, {
        dustAddress: hexDustAddress,
        blockHash: block.hash,
        dtimeCutoffHeight: 0,
      });

      expect(errorReceived).toBeDefined();
      expect(errorReceived.toLowerCase()).toMatch(
        /(expected hrp|unexpected hrp|wrong hrp|bech32|invalid.*address)/,
      );
    });

    /**
     * A well-formed block hash that matches no indexed block returns an error.
     *
     * @given a valid dust address and a 32-byte hex block hash unknown to the indexer
     * @when a dustGenerations subscription is opened at that block hash
     * @then the indexer returns an "unknown block hash" error
     *
     * midnight-indexer#1283
     */
    test('should return an error for an unknown block hash', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Dust', 'Generations', 'Negative'] };

      const targetNetworkId = env.getNetworkId().toLowerCase();
      const dustAddress = generateDustAddressForNetworkId(targetNetworkId);
      const unknownBlockHash = '00'.repeat(32);

      const errorReceived = await collectDustGenerationsError(indexerWsClient, {
        dustAddress,
        blockHash: unknownBlockHash,
        dtimeCutoffHeight: 0,
      });

      expect(errorReceived.toLowerCase()).toMatch(/unknown block hash/);
    });

    /**
     * A block hash that is not valid hex returns an error.
     *
     * @given a valid dust address and a block hash that cannot be hex-decoded
     * @when a dustGenerations subscription is opened at that block hash
     * @then the indexer returns an invalid block hash error
     *
     * midnight-indexer#1283
     */
    test('should return an error for a malformed block hash', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Dust', 'Generations', 'Negative'] };

      const targetNetworkId = env.getNetworkId().toLowerCase();
      const dustAddress = generateDustAddressForNetworkId(targetNetworkId);

      const errorReceived = await collectDustGenerationsError(indexerWsClient, {
        dustAddress,
        blockHash: 'not-a-hex-block-hash',
        dtimeCutoffHeight: 0,
      });

      expect(errorReceived.toLowerCase()).toMatch(/(invalid block hash|hex)/);
    });
  });

  /**
   * Coverage for `transactionHash` on dust generation events
   * (`feat(indexer-api): add transactionHash to event subscription response types`).
   *
   * `transactionHash: HexEncoded!` was added to `DustGenerationsItem` and
   * `DustGenerationDtimeUpdateItem` so wallets can resolve the on-chain
   * transaction from a streamed event via `transactions(offset: { hash: ... })`.
   * The `transactionId` BIGSERIAL is indexer-internal and not portable across
   * indexer instances; the hash is. The schema-level shape (64-hex,
   * non-nullable) is already enforced by the discriminated-union zod schema
   * used by the streaming tests above. This block adds the round-trip check.
   *
   * midnight-indexer#1114
   */
  describe('transactionHash on dust generation events', () => {
    /**
     * @given a registered dust address that emits at least one
     *        `DustGenerationsItem` or `DustGenerationDtimeUpdateItem`
     * @when the first event's `transactionHash` is looked up via
     *       `transactions(offset: { hash: ... })`
     * @then the lookup resolves a single transaction whose `hash` equals the
     *       streamed `transactionHash` — proving the field is the on-chain
     *       identifier wallets can use to fetch the full transaction
     */
    test('first item transactionHash resolves via transactions(offset)', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'Dust', 'Generations', 'Transaction'] };

      let rewardAddress: string;
      try {
        rewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
        return;
      }

      const dustAddress = await fetchDustAddress(rewardAddress);
      const block = await fetchBlock();

      const events = await collectDustGenerations(indexerWsClient, {
        dustAddress,
        blockHash: block.hash,
        dtimeCutoffHeight: 0,
      });

      const firstItem = events
        .map((msg) => msg.data!.dustGenerations)
        .find(
          (event) =>
            event.__typename === 'DustGenerationsItem' ||
            event.__typename === 'DustGenerationDtimeUpdateItem',
        ) as { transactionId: number; transactionHash: string; __typename: string } | undefined;

      if (firstItem === undefined) {
        ctx.skip?.(
          true,
          'no DustGenerationsItem / DtimeUpdateItem event for this address — round-trip vacuous',
        );
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
      expect(transactions).toHaveLength(1);
      expect(transactions[0].hash).toBe(firstItem.transactionHash);
    }, 60_000);
  });
});
