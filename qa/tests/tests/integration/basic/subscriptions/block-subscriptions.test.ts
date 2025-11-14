// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) 2025 Midnight Foundation
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
import dataProvider from '@utils/testdata-provider';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import type { Block, BlockOffset } from '@utils/indexer/indexer-types';
import {
  IndexerWsClient,
  SubscriptionHandlers,
  BlockSubscriptionResponse,
  GraphQLCompleteMessage,
} from '@utils/indexer/websocket-client';
import { EventCoordinator } from '@utils/event-coordinator';
import type { TestContext } from 'vitest';
import { BlockSchema } from '@utils/indexer/graphql/schema';

describe('block subscriptions', () => {
  let indexerWsClient: IndexerWsClient;
  let indexerHttpClient: IndexerHttpClient;
  let eventCoordinator: EventCoordinator;

  beforeEach(async () => {
    indexerHttpClient = new IndexerHttpClient();
    indexerWsClient = new IndexerWsClient();
    eventCoordinator = new EventCoordinator();
    await indexerWsClient.connectionInit();
  });

  afterEach(async () => {
    await indexerWsClient.connectionClose();
    eventCoordinator.clear();
  });

  /**
   * Helper to subscribe to block events and wait for a specific number of blocks.
   */
  async function collectBlocks(
    expectedCount: number,
    fromHeight?: number,
  ): Promise<BlockSubscriptionResponse[]> {
    // We wait until expected number of blocks has been recieved, as we want to make sure that
    // the subscription is working and we are receiving blocks
    const receivedBlocks: BlockSubscriptionResponse[] = [];
    const eventName = `${expectedCount}BlocksReceived`;
    const blockSubscriptionHandler: SubscriptionHandlers<BlockSubscriptionResponse> = {
      next: (payload: BlockSubscriptionResponse) => {
        log.debug(`Received data:\n${JSON.stringify(payload)}`);
        receivedBlocks.push(payload);
        if (receivedBlocks.length === expectedCount) {
          eventCoordinator.notify(eventName);
          log.debug(`${expectedCount} blocks received`);
          indexerWsClient.send<GraphQLCompleteMessage>({
            id: '1',
            type: 'complete',
          });
        }
      },
    };
    // If a starting height is provided, build a BlockOffset object using that height.
    // This will fetch the latest block and stream the new blocks as they are produced
    const blockOffset = fromHeight ? { height: fromHeight } : undefined;

    const unsubscribe = indexerWsClient.subscribeToBlockEvents(
      blockSubscriptionHandler,
      blockOffset,
    );

    // Blocks on MN are produced 6 secs apart. Taking into account the time indexer
    // takes to process blocks when they are produced, we should expect a similar
    // interval. Just to be on the safe side (a block full of unshielded transaction
    // might take up to a sec) we give it a couple of seconds more, so 8 secs in total.
    // For historical subscriptions, blocks are replayed instantly, so only a short grace period (~2s) is applied.
    const maxTimeBetweenBlocks = fromHeight ? 2_000 : 8_000;
    await eventCoordinator.waitForAll([eventName], maxTimeBetweenBlocks);

    unsubscribe();
    return receivedBlocks;
  }

  describe('a subscription to block updates without parameters', () => {
    /**
     * Subscribing to block updates without any offset parameters should stream
     * blocks starting from the latest block and continue streaming new blocks
     * as they are produced.
     *
     * @given no block offset parameters are provided
     * @when we subscribe to block events
     * @then we should receive blocks starting from the latest block
     * @and we should receive new blocks as they are produced
     */
    test('should stream blocks starting from the latest block', async () => {
      const receivedBlocks = await collectBlocks(2);

      // In 6 seconds window we should have received at
      // least 1 block, maybe 2 but no more than that
      expect(receivedBlocks.length).toBe(2);
    });

    /**
     * Validates that all streamed blocks conform to the expected schema.
     *
     * @given a block subscription without any offset parameters
     * @when blocks are streamed from the indexer in real time
     * @then each received block should match the BlockSchema definition
     */
    test('should stream blocks adhering to the expected schema', async () => {
      const latestResponse = await indexerHttpClient.getLatestBlock();
      expect(latestResponse).toBeSuccess();

      const latestHeight = latestResponse.data?.block?.height;
      expect(latestHeight).toBeDefined();

      if (!latestHeight) {
        throw new Error('latestHeight is undefined');
      }

      const startHeight = latestHeight - 10;
      //Stream a set of historical blocks from that height
      const receivedBlocks = await collectBlocks(5, startHeight);
      receivedBlocks
        .filter((msg) => msg?.data?.blocks)
        .forEach((msg) => {
          expect.soft(msg).toBeSuccess();
          const parsed = BlockSchema.safeParse(msg.data?.blocks);
          expect(
            parsed.success,
            `Block subscription schema validation failed: ${JSON.stringify(parsed.error?.format(), null, 2)}`,
          ).toBe(true);
        });
    });
  });

  describe('a subscription to block updates by hash', () => {
    /**
     * Subscribing to block updates with a valid block hash should stream
     * blocks starting from the specified block and continue streaming
     * subsequent blocks.
     *
     * @given a valid block hash that exists in the blockchain
     * @when we subscribe to block events with that hash as offset
     * @then we should receive blocks starting from the block with that hash
     */
    test('should stream blocks starting from the block with that hash, given that hash exists', async () => {
      // Let's get the hash of the genesis block ...
      const genesisBlockResponse = await indexerHttpClient.getBlockByOffset({
        height: 0,
      });
      expect(genesisBlockResponse).toBeSuccess();
      const genesisBlock = genesisBlockResponse.data?.block;
      expect(genesisBlock).toBeDefined();

      //... and extract its hash
      const knownHash = genesisBlock?.hash;
      expect(knownHash).toBeDefined();
      const blockOffset: BlockOffset = {
        hash: knownHash,
      };

      const messagesReceived: BlockSubscriptionResponse[] = [];
      const blockSubscriptionHandler: SubscriptionHandlers<BlockSubscriptionResponse> = {
        next: (payload: BlockSubscriptionResponse) => {
          log.debug(`Received data: ${JSON.stringify(payload)}`);

          messagesReceived.push(payload);

          if (payload.errors) {
            eventCoordinator.notify('error');
            log.error(`Error received: ${JSON.stringify(payload.errors)}`);
          }

          if (messagesReceived.length === 10) {
            eventCoordinator.notify('expectedBlocksReceived');
            log.debug('Expected # of blocks received');
          }
        },
      };

      const unsubscribe = indexerWsClient.subscribeToBlockEvents(
        blockSubscriptionHandler,
        blockOffset,
      );

      await eventCoordinator.waitForAny(['expectedBlocksReceived', 'error']);

      unsubscribe();

      // Even if after we received the expected number of blocks, we unsubscribe,
      // we might receive more blocks due to race conditions, so we expect at least 10
      // blocks to be received
      expect(messagesReceived.length).toBeGreaterThanOrEqual(10);
      for (const msg of messagesReceived) {
        expect.soft(msg).toBeSuccess();
        expect.soft(msg.data?.blocks).toBeDefined();
      }

      const firstBlockStreamed = messagesReceived[0].data?.blocks;
      expect(firstBlockStreamed).toBeDefined();
      expect(firstBlockStreamed?.hash).toBe(blockOffset.hash);
    });

    /**
     * Subscribing to block updates with a non-existent block hash should
     * not stream any blocks and should return an error response indicating
     * that the block was not found.
     *
     * @given a block hash that does not exist on chain
     * @when we subscribe to block events with that hash as offset
     * @then we should receive an error message indicating the block was not found
     */
    test(`should return an error message, given that hash doesn't exist`, async () => {
      const blockOffset: BlockOffset = {
        hash: '0000000000000000000000000000000000000000000000000000000000000000',
      };

      let completionMessage: GraphQLCompleteMessage | null = null;
      const messagesReceived: BlockSubscriptionResponse[] = [];

      const blockSubscriptionHandler: SubscriptionHandlers<BlockSubscriptionResponse> = {
        next: (payload: BlockSubscriptionResponse) => {
          log.debug(`Received data: ${JSON.stringify(payload)}`);
          messagesReceived.push(payload);
          if (payload.errors !== undefined) {
            log.debug('Received the expected error message');
            eventCoordinator.notify('error');
          }
        },
        complete: (message) => {
          log.debug(`Complete message: ${JSON.stringify(message)}`);
          completionMessage = message;
          eventCoordinator.notify('completion');
        },
      };

      indexerWsClient.subscribeToBlockEvents(blockSubscriptionHandler, blockOffset);

      await eventCoordinator.waitForAll(['error', 'completion']);

      // Validate that we received a complete message
      expect(completionMessage).toBeDefined();
      expect(completionMessage!.type).toBe('complete');
      expect(completionMessage!.id).toBeDefined();

      // ... only one message should be received
      expect(messagesReceived.length).toBe(1);
      expect(messagesReceived[0]).toBeError();
      const errorMessage = messagesReceived[0].errors?.[0].message;
      expect(errorMessage).toContain(`block with hash`);
      expect(errorMessage).toContain(`${blockOffset.hash}`);
      expect(errorMessage).toContain(`not found`);
    });

    /**
     * Subscribing to block updates with an invalid block hash should
     * not stream any blocks and should return an error response indicating
     * that the block hash is invalid.
     *
     * @given an invalid block hash
     * @when we subscribe to block events with that hash as offset
     * @then we should receive an error message indicating the block hash is invalid
     */
    test(`should return an error message, given that hash is invalid`, async () => {
      const blockOffset: BlockOffset = {
        hash: 'TT',
      };

      let completionMessage: GraphQLCompleteMessage | null = null;
      const messagesReceived: BlockSubscriptionResponse[] = [];

      const blockSubscriptionHandler: SubscriptionHandlers<BlockSubscriptionResponse> = {
        next: (payload: BlockSubscriptionResponse) => {
          log.debug(`Received data: ${JSON.stringify(payload)}`);
          messagesReceived.push(payload);
          if (payload.errors !== undefined) {
            log.debug('Received the expected error message');
            eventCoordinator.notify('error');
          }
        },
        complete: (message) => {
          log.debug(`Complete message: ${JSON.stringify(message)}`);
          completionMessage = message;
          eventCoordinator.notify('completion');
        },
      };

      indexerWsClient.subscribeToBlockEvents(blockSubscriptionHandler, blockOffset);

      await eventCoordinator.waitForAll(['error', 'completion']);

      // Validate that we received a complete message
      expect(completionMessage).toBeDefined();
      expect(completionMessage!.type).toBe('complete');
      expect(completionMessage!.id).toBeDefined();

      // ... only one message should be received
      expect(messagesReceived.length).toBe(1);
      expect(messagesReceived[0]).toBeError();
      const errorMessage = messagesReceived[0].errors?.[0].message;
      expect(errorMessage).toContain(`invalid block hash`);
    });
  });

  describe('a subscription to block updates by height', () => {
    /**
     * Subscribing to block updates with a valid block height should stream
     * blocks starting from the specified block height and continue streaming
     * subsequent blocks.
     *
     * @given we get the latest block height from indexer
     * @when we subscribe to block events with that height - 20 as offset
     * @then we should receive blocks starting from the block with that height
     * @and the first block received should have the requested height
     * @and we should receive subsequent blocks as they are produced
     */
    test('should stream blocks from the block with that height, given that height exists', async ({
      skip,
    }: TestContext) => {
      const latestBlockResponse = await indexerHttpClient.getLatestBlock();
      expect(latestBlockResponse).toBeSuccess();
      const latestBlock = latestBlockResponse.data?.block;
      expect(latestBlock).toBeDefined();

      // We need to decide a number of blocks to receive, after which we are
      // happy with the test. Say that number is 20.
      // So we need to make sure there are at least 20 blocks on chain, if not
      // we skip the test becausee the precondition is not met
      if (latestBlock?.height && latestBlock?.height < 20) {
        skip?.(true, 'Skipping as we want at least 20 blocks to be produced');
      }

      const blockMessagesReceived: BlockSubscriptionResponse[] = [];

      const blockHeight = latestBlock?.height;
      expect(blockHeight).toBeDefined();
      const blockOffset: BlockOffset = { height: blockHeight! - 20 };

      const blockSubscriptionHandler: SubscriptionHandlers<BlockSubscriptionResponse> = {
        next: (payload) => {
          blockMessagesReceived.push(payload);
          log.debug(`Received data: ${JSON.stringify(payload)}`);
          if (blockMessagesReceived.length === 20) {
            log.debug('Stop receiving blocks');
            eventCoordinator.notify('expectedBlocksReceived');
          }
        },
      };

      const unsubscribe = indexerWsClient.subscribeToBlockEvents(
        blockSubscriptionHandler,
        blockOffset,
      );

      await eventCoordinator.waitFor('expectedBlocksReceived');

      unsubscribe();

      // We ask for 20 blocks but due to race conditions we might receive more depending on who is faster...
      // ... the test unscribing or the indexer sending blocks
      expect(blockMessagesReceived.length).toBeGreaterThanOrEqual(20);
      expect(blockMessagesReceived[0]).toBeSuccess();
      expect((blockMessagesReceived[0].data as { blocks: Block }).blocks.height).toBe(
        blockOffset.height,
      );
    });

    /**
     * Subscribing to block updates with a block height higher than the latest
     * block height should not stream any blocks and should return an error
     * response indicating that the block was not found.
     *
     * @given a block height that is higher than the latest block height
     * @when we subscribe to block events with that height as offset
     * @then we should not receive any blocks
     * @and we should receive an error indicating that the block was not found
     */
    test('should return an error message, given that height is higher than the latest block height', async () => {
      const latestBlockResponse = await indexerHttpClient.getLatestBlock();

      expect(latestBlockResponse).toBeSuccess();
      const latestBlock = latestBlockResponse.data?.block;
      expect(latestBlock).toBeDefined();
      expect(latestBlock?.height).toBeDefined();
      const blockHeight = latestBlock?.height;
      expect(blockHeight).toBeDefined();

      // We need to make sure that the block height is higher than the latest block height
      // so we add 10 to the latest block height
      const blockOffset: BlockOffset = { height: blockHeight! + 10 };

      const blockMessagesReceived: BlockSubscriptionResponse[] = [];

      const blockSubscriptionHandler: SubscriptionHandlers<BlockSubscriptionResponse> = {
        next: (payload) => {
          blockMessagesReceived.push(payload);
          log.debug(`Received data: ${JSON.stringify(payload)}`);
          if (payload.errors !== undefined) {
            log.debug('Received the expected error message');
            eventCoordinator.notify('error');
          }
        },
      };

      indexerWsClient.subscribeToBlockEvents(blockSubscriptionHandler, blockOffset);

      await eventCoordinator.waitFor('error');

      expect(blockMessagesReceived.length).toBe(1);
      expect(blockMessagesReceived[0]).toBeError();
      const errorMessage = blockMessagesReceived[0].errors?.[0].message;
      expect(errorMessage).toContain(`block with height`);
      expect(errorMessage).toContain(`${blockOffset.height}`);
      expect(errorMessage).toContain(`not found`);
    });

    /**
     * Subscribing to block updates with an invalid block height should
     * not stream any blocks and should return an error response indicating
     * that the block height is invalid.
     *
     * @given an invalid block height
     * @when we subscribe to block events with that height as offset
     * @then we should not receive any blocks
     * @and we should receive an error indicating that the block height is invalid
     */
    test('should return an error message, given that height is invalid', async () => {
      const blockOffset: BlockOffset = { height: 2 ** 32 };
      const blockMessagesReceived: BlockSubscriptionResponse[] = [];

      let completionMessage: GraphQLCompleteMessage | null = null;

      const blockSubscriptionHandler: SubscriptionHandlers<BlockSubscriptionResponse> = {
        next: (payload) => {
          blockMessagesReceived.push(payload);
          log.debug(`Received data: ${JSON.stringify(payload)}`);
          if (payload.errors !== undefined) {
            log.debug('Received the expected error message');
            eventCoordinator.notify('error');
          }
        },
        complete: (message) => {
          log.debug(`Complete message: ${JSON.stringify(message)}`);
          eventCoordinator.notify('completion');
          completionMessage = message;
        },
      };

      indexerWsClient.subscribeToBlockEvents(blockSubscriptionHandler, blockOffset);

      await eventCoordinator.waitForAll(['error', 'completion']);

      expect(completionMessage).toBeDefined();
      expect(completionMessage!.type).toBe('complete');
      expect(completionMessage!.id).toBeDefined();

      expect(blockMessagesReceived.length).toBe(1);
      expect(blockMessagesReceived[0]).toBeError();
      const errorMessage = blockMessagesReceived[0].errors?.[0].message;
      expect(errorMessage).toContain(`Failed to parse "Int"`);
      expect(errorMessage).toContain(`Only integers from 0 to 4294967295 are accepted`);
    });
  });

  describe('a subscription to block updates by height and hash', () => {
    /**
     * Subscribing to block updates with a valid block height and hash should
     * return an error message, as only one parameter at a time can be used
     *
     * @given a valid block height and hash
     * @when we subscribe to block events with that height and hash as offset
     * @then we should receive an error message indicating that only one parameter at a time can be used
     */
    test('should return an error message, as only one parameter at a time can be used', async () => {
      const blockOffset: BlockOffset = {
        height: 1,
        hash: '0'.repeat(64),
      };

      const blockMessagesReceived: BlockSubscriptionResponse[] = [];

      let completionMessage: GraphQLCompleteMessage | null = null;

      const blockSubscriptionHandler: SubscriptionHandlers<BlockSubscriptionResponse> = {
        next: (payload) => {
          blockMessagesReceived.push(payload);
          log.debug(`Received data: ${JSON.stringify(payload)}`);
          if (payload.errors !== undefined) {
            log.debug('Received the expected error message');
            eventCoordinator.notify('error');
          }
        },
        complete: (message) => {
          log.debug(`Complete message: ${JSON.stringify(message)}`);
          eventCoordinator.notify('completion');
          completionMessage = message;
        },
      };

      indexerWsClient.subscribeToBlockEvents(blockSubscriptionHandler, blockOffset);

      await eventCoordinator.waitForAll(['error', 'completion']);

      expect(completionMessage).toBeDefined();
      expect(completionMessage!.type).toBe('complete');
      expect(completionMessage!.id).toBeDefined();

      expect(blockMessagesReceived.length).toBe(1);
      expect(blockMessagesReceived[0]).toBeError();
      const errorMessage = blockMessagesReceived[0].errors?.[0].message;
      expect(errorMessage).toContain(`Invalid value for argument`);
      expect(errorMessage).toContain(`Oneof input objects requires have exactly one field`);
    });
  });
});
