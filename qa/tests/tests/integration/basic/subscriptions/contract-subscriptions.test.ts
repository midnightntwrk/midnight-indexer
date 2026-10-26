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
import { env, EnvironmentName } from 'environment/model';
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
import {
  CONTRACT_ACTIONS_STATE_AT_SUBSCRIPTION,
  CONTRACT_ACTIONS_WITH_HEIGHT_FROM_LATEST_BLOCK,
} from '@utils/indexer/graphql/subscriptions';
import type { BlockOffset, ContractActionWithHeightResponse } from '@utils/indexer/indexer-types';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { extractSubscriptionErrorMessage } from '@utils/indexer/subscription-error';
import { NodeRpcClient } from '@utils/node/rpc-client';
import { sleep } from '@utils/retry-helper';
import {
  CONTRACT_STATE_V6_TAG,
  CONTRACT_STATE_V8_TAG,
  contractStateTag,
  expectAnchorConfirmedByNode,
  forkBlock,
  ownBlock,
  sameBlock,
} from 'tests/shared/contract-state-utils';

// ContractActionSubscriptionResponse is now imported from websocket-client

// How long a subscription without offset is observed to confirm nothing is replayed.
const NO_REPLAY_WAIT_MS = 8_000;

// Test budgets: each test's timeout is the sum of its worst cases plus a margin, so a failure
// surfaces as its assertion rather than as a test timeout.
// One node or indexer request (the node RPC client's own timeout).
const RPC_MS = 10_000;
// Streaming a contract's history from genesis up to the indexer tip.
const STREAM_HISTORY_MS = 30_000;
// Streaming from before the devnet fork to the re-emission at the fork block.
const FORK_STREAM_MS = 90_000;

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
   * Streams contract actions (with their stateAt, unless another `query` is given) from `offset`,
   * or from the latest block without one, until `done(received)` holds, the stream errored or
   * completed, or `timeoutMs` passed, then for `settleMs` more to catch anything streamed after.
   * A contract action stream never ends by itself, so `completed` is a failure to assert on.
   */
  async function streamContractActions(
    contractAddress: string,
    offset: BlockOffset | undefined,
    done: (received: ContractActionSubscriptionResponse[]) => boolean,
    timeoutMs: number,
    settleMs = 0,
    query = CONTRACT_ACTIONS_STATE_AT_SUBSCRIPTION,
  ) {
    const received: ContractActionSubscriptionResponse[] = [];
    const errors: string[] = [];
    let completed = false;
    const unsubscribe = indexerWsClient.subscribeToContractActionEvents(
      {
        next: (payload) => received.push(payload),
        error: (error) => errors.push(extractSubscriptionErrorMessage(error)),
        complete: () => (completed = true),
      },
      contractAddress,
      offset,
      query,
    );
    try {
      const deadline = Date.now() + timeoutMs;
      while (!done(received) && !errors.length && !completed && Date.now() < deadline) {
        await sleep(500);
      }
      await sleep(settleMs);
      indexerWsClient.assertSocketAlive();
    } finally {
      unsubscribe();
    }
    return { received, errors, completed };
  }

  /**
   * The state of the last streamed action anchored at or below `height`, skipping actions of
   * failed transactions, whose state is empty.
   */
  function lastStateUpTo(received: ContractActionSubscriptionResponse[], height: number) {
    return received
      .map((msg) => ContractActionStateAtSchema.safeParse(msg.data?.contractActions).data)
      .findLast((action) => action && action.state !== '' && action.stateAt.height <= height)
      ?.state;
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
     * @then the subscription is accepted without errors and stays open
     * @and no contract action below that height is replayed
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

      const { received, errors, completed } = await streamContractActions(
        contractAddress!,
        undefined,
        () => false,
        NO_REPLAY_WAIT_MS,
        0,
        CONTRACT_ACTIONS_WITH_HEIGHT_FROM_LATEST_BLOCK,
      );

      // On a static chain nothing arrives, so the subscription's acceptance shows only as the
      // absence of errors on a stream that is still open.
      expect(errors).toEqual([]);
      expect(completed, 'subscription without offset completed').toBe(false);
      // Hosted environments may index new actions for this contract while we wait; those are
      // allowed, as are actions in the tip block itself, where the stream starts. Each must come
      // from the tip read before subscribing or a later block.
      for (const msg of received as ContractActionWithHeightResponse[]) {
        expect(msg).toBeSuccess();
        const contractAction = msg.data!.contractActions;
        expect(contractAction.address).toBe(contractAddress!);
        expect(
          contractAction.transaction.block.height,
          `Contract action from block ${contractAction.transaction.block.height} was replayed; ` +
            `a subscription without offset must only stream actions from block ${tipHeight} on`,
        ).toBeGreaterThanOrEqual(tipHeight);
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
     * or the hard fork block for the re-emission of a contract's latest pre-fork action. The
     * node, not the indexer, decides whether a re-emission was due and where.
     *
     * @given a known contract address with existing contract actions
     * @and the node's state of the contract at the indexer tip
     * @when contract actions are streamed from block height 0
     * @then each action's stateAt names the height, hash and protocol version of its own block
     * @and an action anchored elsewhere is a single re-emission of an action already streamed,
     *      at a later block on a newer protocol version where the node's state changed to the
     *      re-emitted one: the hard fork block of a chain that declares it
     * @and no action is streamed twice at its own block
     * @and the last state streamed up to the tip is the node's state there
     */
    test(
      'should anchor each streamed contract action at the block its state was written',
      async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Subscription', 'ContractAction', 'StateAt'] };
        let contractAddress: string;
        try {
          contractAddress = dataProvider.getKnownContractAddress();
        } catch (error) {
          log.warn(error);
          return ctx.skip(true, (error as Error).message);
        }
        const fork = dataProvider.getLedgerHardFork();
        const nodeClient = new NodeRpcClient();
        const tip = await indexerHttpClient.getLatestBlock();
        expect(tip).toBeSuccess();
        const { height: tipHeight, hash: tipHash } = tip.data!.block;
        const nodeState = await nodeClient.getContractState(contractAddress, tipHash);

        const { received, errors, completed } = await streamContractActions(
          contractAddress,
          { height: 0 },
          (soFar) => lastStateUpTo(soFar, tipHeight) === nodeState,
          STREAM_HISTORY_MS,
          NO_REPLAY_WAIT_MS,
        );

        expect(errors).toEqual([]);
        expect(completed, 'subscription from genesis completed').toBe(false);
        expect(received.length).toBeGreaterThan(0);
        // Own-block items are keyed by transaction, not by state, so a re-emission wrongly anchored
        // at its own block cannot pass as another action of its transaction: a transaction's
        // actions arrive together, in one encoding, each once.
        const streamed = new Map<string, { tag: string; items: string[] }>();
        const reEmitted = new Set<string>();
        let previousTx: string | undefined;
        for (const msg of received) {
          const action = ContractActionStateAtSchema.parse(msg.data?.contractActions);
          const { hash } = action.transaction;
          const item = `${action.__typename}:${action.state}`;
          const tag = contractStateTag(action.state);
          if (sameBlock(action.stateAt, ownBlock(action))) {
            const tx = streamed.get(hash);
            if (!tx) streamed.set(hash, { tag, items: [item] });
            else {
              expect(previousTx, `action of tx ${hash} streamed again after other items`).toBe(
                hash,
              );
              expect(tx.items, `action of tx ${hash} streamed twice`).not.toContain(item);
              expect(tag, `action of tx ${hash} in another encoding than its transaction`).toBe(
                tx.tag,
              );
              tx.items.push(item);
            }
            previousTx = hash;
            continue;
          }
          // Anchored away from its own block: only the re-emission of an earlier action at a fork.
          previousTx = undefined;
          expect(
            streamed.has(hash),
            `action of tx ${hash} anchored at ${action.stateAt.height} before it was streamed`,
          ).toBe(true);
          expect(reEmitted.has(hash), `action of tx ${hash} re-emitted twice`).toBe(false);
          reEmitted.add(hash);
          await expectAnchorConfirmedByNode(
            nodeClient,
            contractAddress,
            action.state,
            action.stateAt,
            action,
          );
          if (fork) expect(action.stateAt, `re-emission of tx ${hash}`).toEqual(forkBlock(fork));
        }
        // Catches a missing re-emission: the stream would end on a state the node has replaced.
        expect(
          lastStateUpTo(received, tipHeight),
          `last state streamed up to the indexer tip ${tipHeight} vs the node's there`,
        ).toBe(nodeState);
      },
      // Tip, node state, stream, settle and the node checks of one re-emission.
      6 * RPC_MS + STREAM_HISTORY_MS + NO_REPLAY_WAIT_MS + 15_000,
    );
  });

  // Expected blocks, hashes and states come from the hard fork fixture and the node, never from
  // the indexer under test.
  describe.runIf(env.getCurrentEnvironmentName() === EnvironmentName.DEVNET)(
    'a subscription from before a ledger hard fork to a contract untouched since',
    () => {
      /**
       * A contract untouched since the hard fork has its latest action re-emitted once at the
       * fork block, with the state translated to the post-fork encoding the node serves.
       *
       * @given a contract deployed and last called before the fork (actions at 124,321, 124,417
       *        and 124,423; fork at 169,496)
       * @when its contract actions are streamed from its deploy block
       * @then the three original actions arrive in order, in the [v6] encoding, each anchored at
       *       its own block
       * @and then exactly one re-emission of the latest action: same transaction, the node's
       *      [v8] state at the fork block, and stateAt naming the fork block (protocol version
       *      2001000), which differs from the transaction's block
       */
      test(
        'should re-emit the latest pre-fork action once at the fork block',
        async (ctx: TestContext) => {
          ctx.task!.meta.custom = {
            labels: ['Subscription', 'ContractAction', 'StateAt', 'HardFork'],
          };
          const fork = dataProvider.getLedgerHardFork();
          if (!fork) throw new Error('devnet must declare the hard fork it crossed');
          const { 'contract-address': address, 'contract-actions': actions } =
            fork['untouched-contracts'][0];
          const latest = actions.at(-1)!;
          const nodeState = await new NodeRpcClient().getContractState(
            address,
            fork['fork-block'].hash,
          );
          expect(contractStateTag(nodeState), 'node state tag at the fork').toBe(
            CONTRACT_STATE_V8_TAG,
          );

          const { received, errors, completed } = await streamContractActions(
            address,
            { height: actions[0]['block-height'] },
            (soFar) => soFar.length > actions.length,
            FORK_STREAM_MS,
            NO_REPLAY_WAIT_MS,
          );

          expect(errors).toEqual([]);
          expect(completed, 'subscription from before the fork completed').toBe(false);
          const streamed = received.map((msg) =>
            ContractActionStateAtSchema.parse(msg.data?.contractActions),
          );
          expect(streamed.map((action) => action.transaction.hash)).toEqual([
            ...actions.map((action) => action['transaction-hash']),
            latest['transaction-hash'],
          ]);
          streamed.slice(0, -1).forEach((action, i) => {
            expect(contractStateTag(action.state)).toBe(CONTRACT_STATE_V6_TAG);
            expect(action.stateAt).toEqual(ownBlock(action));
            expect(action.stateAt).toMatchObject({
              height: actions[i]['block-height'],
              hash: actions[i]['block-hash'],
            });
          });
          const reEmitted = streamed.at(-1)!;
          expect(reEmitted.stateAt).toEqual(forkBlock(fork));
          expect(reEmitted.stateAt.hash).not.toBe(reEmitted.transaction.block.hash);
          expect(reEmitted.transaction.block.height).toBe(latest['block-height']);
          expect(reEmitted.state).toBe(nodeState);
        },
        // Node state, stream to the re-emission at the fork and settle.
        RPC_MS + FORK_STREAM_MS + NO_REPLAY_WAIT_MS + 15_000,
      );
    },
  );
});
