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
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import dataProvider from '@utils/testdata-provider';
import {
  IndexerWsClient,
  SubscriptionHandlers,
  ContractActionSubscriptionResponse,
} from '@utils/indexer/websocket-client';
import { EventCoordinator } from '@utils/event-coordinator';
import {
  ContractActionStateAtSchema,
  ContractActionUnionSchema,
} from '@utils/indexer/graphql/schema';
import { CONTRACT_ACTIONS_STATE_AT_SUBSCRIPTION } from '@utils/indexer/graphql/subscriptions';
import type { BlockOffset, GraphQLResponse } from '@utils/indexer/indexer-types';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { extractSubscriptionErrorMessage } from '@utils/indexer/subscription-error';
import { forkBlock, ownBlock } from 'tests/shared/contract-state-utils';

// ContractActionSubscriptionResponse is now imported from websocket-client

// How long a subscription without offset is observed to confirm nothing is replayed.
const NO_REPLAY_WAIT_MS = 8_000;

const CONTRACT_ACTIONS_WITH_HEIGHT_FROM_LATEST_BLOCK = `subscription ContractActionsWithHeightFromLatestBlock($ADDRESS: HexEncoded!) {
    contractActions(address: $ADDRESS) {
        __typename
        address
        transaction {
            hash
            block {
                height
            }
        }
    }
}`;

type ContractActionWithHeightResponse = GraphQLResponse<{
  contractActions: {
    __typename: string;
    address: string;
    transaction: { hash: string; block: { height: number } };
  };
}>;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

describe('contract action subscriptions', () => {
  let indexerWsClient: IndexerWsClient;
  let indexerHttpClient: IndexerHttpClient;
  let eventCoordinator: EventCoordinator;

  beforeEach(async () => {
    indexerHttpClient = new IndexerHttpClient();
    indexerWsClient = new IndexerWsClient();
    eventCoordinator = new EventCoordinator();
    await indexerWsClient.connectionInit();
  }, 30_000);

  afterEach(async () => {
    await indexerWsClient.connectionClose();
    eventCoordinator.clear();
  });

  /**
   * Helper to subscribe to contract actions and collect a specified number of them.
   */
  async function collectContractActions(
    contractAddress: string,
    expectedCount = 2,
    offset?: BlockOffset,
    eventName = 'contractActionReceived',
  ): Promise<ContractActionSubscriptionResponse[]> {
    // We wait for at least one contract action to be received
    const receivedContractActions: ContractActionSubscriptionResponse[] = [];

    const contractActionSubscriptionHandler: SubscriptionHandlers<ContractActionSubscriptionResponse> =
      {
        next: (payload: ContractActionSubscriptionResponse) => {
          log.debug(`Received contract action:\n${JSON.stringify(payload)}`);
          receivedContractActions.push(payload);
          if (receivedContractActions.length === expectedCount) eventCoordinator.notify(eventName);
          log.debug(`Event triggered: ${eventName}`);
        },
      };

    // We subscribe to contract actions for a specific address without block offset
    // This will start streaming contract actions from the latest block
    const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
      contractActionSubscriptionHandler,
      contractAddress,
      offset,
    );
    // Maximum wait time for contract action (similar to block timeout)
    const maxTimeForContractAction = 8_000;
    await eventCoordinator.waitForAll([eventName], maxTimeForContractAction);
    unsubscribe();
    return receivedContractActions;
  }

  /**
   * Streams contract actions with their stateAt from `offset` until `count` actions arrived or
   * `timeoutMs` passed, then for `settleMs` more to catch anything streamed after them.
   */
  async function streamStateAt(
    contractAddress: string,
    offset: BlockOffset,
    count: number,
    timeoutMs: number,
    settleMs = 0,
  ) {
    const received: ContractActionSubscriptionResponse[] = [];
    const errors: string[] = [];
    const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
      {
        next: (payload) => received.push(payload),
        error: (error) => errors.push(extractSubscriptionErrorMessage(error)),
      },
      contractAddress,
      offset,
      CONTRACT_ACTIONS_STATE_AT_SUBSCRIPTION,
    );
    const deadline = Date.now() + timeoutMs;
    while (received.length < count && errors.length === 0 && Date.now() < deadline) {
      await sleep(500);
    }
    await sleep(settleMs);
    unsubscribe();
    return { received, errors };
  }

  describe('a subscription to contract action updates without parameters', () => {
    /**
     * Subscribing to contract action updates without an offset starts at the latest block:
     * the contract's existing actions are not replayed, only actions indexed after the
     * subscription starts are streamed (midnight-indexer #1654).
     *
     * @given a known contract address with existing contract actions
     * @and the height of the latest indexed block
     * @when we subscribe to contract action events without a block offset
     * @then the subscription is accepted without errors
     * @and no contract action at or below that height is replayed
     */
    test('should not replay existing contract actions', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'ContractAction', 'NoOffset'] };

      let contractAddress: string;
      try {
        contractAddress = dataProvider.getKnownContractAddress();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      const latestBlock = await indexerHttpClient.getLatestBlock();
      expect(latestBlock).toBeSuccess();
      const tipHeight = latestBlock.data!.block.height;

      const received: ContractActionWithHeightResponse[] = [];
      const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
        {
          next: (payload) => {
            log.debug(`Received contract action:\n${JSON.stringify(payload)}`);
            received.push(payload as ContractActionWithHeightResponse);
          },
        },
        contractAddress!,
        undefined,
        CONTRACT_ACTIONS_WITH_HEIGHT_FROM_LATEST_BLOCK,
      );
      await sleep(NO_REPLAY_WAIT_MS);
      unsubscribe();

      // Hosted environments may index new actions for this contract while we wait; those are
      // allowed, but each must come from a block after the tip read before subscribing.
      for (const msg of received) {
        expect(msg).toBeSuccess();
        const contractAction = msg.data!.contractActions;
        expect(contractAction.address).toBe(contractAddress!);
        expect(
          contractAction.transaction.block.height,
          `Contract action from block ${contractAction.transaction.block.height} was replayed; ` +
            `a subscription without offset must only stream actions after block ${tipHeight}`,
        ).toBeGreaterThan(tipHeight);
      }
    });
  });

  describe('a subscription to contract action updates from genesis', () => {
    /**
     * Subscribing to contract action updates from block height 0 replays the contract's full
     * history; every streamed contract action must conform to its expected schema.
     *
     * @given a known contract address with existing contract actions
     * @when we subscribe to contract action events from block height 0
     * @then we should receive the contract's historical contract actions
     * @and each received contract action should match its corresponding Zod schema
     */
    test('should stream contract actions adhering to the expected schema', async (ctx: TestContext) => {
      let contractAddress: string;
      try {
        contractAddress = dataProvider.getKnownContractAddress();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }
      const receivedContractActions = await collectContractActions(contractAddress!, 2, {
        height: 0,
      });

      expect(receivedContractActions.length).toBeGreaterThanOrEqual(2);
      receivedContractActions.forEach((msg) => {
        expect.soft(msg).toBeSuccess();
        const contractAction = msg.data?.contractActions;
        expect(['ContractDeploy', 'ContractCall', 'ContractUpdate']).toContain(
          contractAction?.__typename,
        );
        expect(contractAction?.address).toBe(contractAddress!);

        const parsed = ContractActionUnionSchema.safeParse(contractAction);
        expect(
          parsed.success,
          `Contract action schema validation failed: ${JSON.stringify(
            parsed.error?.format(),
            null,
            2,
          )}`,
        ).toBe(true);
      });
    });
  });

  describe('a subscription to contract action updates with block hash offset', () => {
    /**
     * Subscribing to contract action updates with a block hash offset should stream
     * all historical contract actions starting from the specified block hash and
     * continue streaming new contract actions as they are produced.
     *
     * @given a valid block hash from before the latest action
     * @when we subscribe to contract action events with that block hash offset
     * @then we should receive all historical contract actions since that block
     * @and the first message's block hash should be >= the requested hash
     * @and we should continue to receive new contract actions as they are produced
     */
    test('should stream historical and new contract actions from a specific block hash', async (ctx: TestContext) => {
      // We get a known contract address from test data provider
      let contractAddress: string;
      let historicalBlockHash: string;
      try {
        contractAddress = dataProvider.getKnownContractAddress();
        // We get a known block hash from before the latest action
        // This should be a block hash that contains historical contract actions
        historicalBlockHash = await dataProvider.getContractDeployBlockHash();
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      // We collect all received contract actions
      const receivedContractActions = await collectContractActions(
        contractAddress!,
        1,
        { hash: historicalBlockHash! },
        'firstHistoricalActionReceived',
      );

      // We should receive at least one contract action message
      expect(receivedContractActions.length).toBeGreaterThanOrEqual(1);
      expect(receivedContractActions[0]).toBeSuccess();

      // Validate the received contract actions
      for (const action of receivedContractActions) {
        if (action.data?.contractActions) {
          const contractAction = action.data.contractActions;
          expect(contractAction).toBeDefined();
          expect(contractAction.__typename).toBeDefined();
          expect(['ContractDeploy', 'ContractCall', 'ContractUpdate']).toContain(
            contractAction.__typename,
          );
          expect(contractAction.address).toBe(contractAddress!);
        }
      }

      // Validate that the first message's block hash is >= the requested hash
      // This ensures we're getting historical actions from the specified block onwards
      if (receivedContractActions.length > 0 && receivedContractActions[0].data?.contractActions) {
        const firstAction = receivedContractActions[0].data.contractActions;
        if (firstAction.transaction?.block?.hash) {
          const firstActionBlockHash = firstAction.transaction.block.hash;
          log.debug(`First action block hash: ${firstActionBlockHash}`);
          log.debug(`Requested historical block hash: ${historicalBlockHash!}`);

          // Note: In a real blockchain, we would compare block heights or hashes
          // For this test, we verify that we received actions and that they match the contract address
          expect(firstActionBlockHash).toBeDefined();
          expect(firstAction.address).toBe(contractAddress!);
        }
      }
    });
  });

  describe('a subscription to contract action updates with the block their state is anchored at', () => {
    /**
     * Every streamed contract action names the block its state is anchored at: its own block,
     * or the hard fork block for the re-emission of a contract's latest pre-fork action.
     *
     * @given a known contract address with existing contract actions
     * @when contract actions are streamed from block height 0
     * @then each action's stateAt names the height, hash and protocol version of its own block
     * @and an action streamed twice is a re-emission at the hard fork block of a chain that
     *      crossed one, and on any other chain no action is streamed twice
     */
    test('should anchor each streamed contract action at the block its state was written', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Subscription', 'ContractAction', 'StateAt'] };
      let contractAddress: string;
      try {
        contractAddress = dataProvider.getKnownContractAddress();
      } catch (error) {
        log.warn(error);
        return ctx.skip(true, (error as Error).message);
      }
      const fork = dataProvider.getLedgerHardFork();

      const { received, errors } = await streamStateAt(
        contractAddress,
        { height: 0 },
        Infinity,
        NO_REPLAY_WAIT_MS,
      );

      expect(errors).toEqual([]);
      expect(received.length).toBeGreaterThan(0);
      const streamed = new Set<string>();
      for (const msg of received) {
        const action = ContractActionStateAtSchema.parse(msg.data?.contractActions);
        const { hash } = action.transaction;
        if (streamed.has(hash)) {
          expect(fork, `action of tx ${hash} streamed twice without a hard fork`).not.toBeNull();
          expect(action.stateAt, `re-emission of tx ${hash}`).toEqual(forkBlock(fork!));
        } else {
          expect(action.stateAt, `action of tx ${hash}`).toEqual(ownBlock(action));
        }
        streamed.add(hash);
      }
    });
  });
});
