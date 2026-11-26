// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Point-in-time contract state after the chain indexer's garbage collection (gc) has run.
//
// Contract states are pinned in the ledger DB, and gc marks and sweeps only unpinned nodes
// (#1799, #1824, #1820). If a state were left unpinned, or pinned only in part, gc would cull
// its nodes, and querying the contract as of an old block would fail or serve the wrong state.
//
// The file checks contract actions the chain already holds, so it costs almost nothing in a
// full e2e run: run-last-sequencer.ts runs it after every other e2e file, whose deploys and
// calls have aged through several gc passes by then. Run alone, or on a chain with no such
// history, it deploys and calls a counter contract itself and waits for the gc passes, which
// takes about 7-8 minutes on undeployed (two transactions, then 60 blocks of about 6 s).
//
// Expected states come from the node, which the indexer under test cannot influence.

import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import dataProvider from '@utils/testdata-provider';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { NodeRpcClient } from '@utils/node/rpc-client';
import { ToolkitWrapper } from '@utils/toolkit/toolkit-wrapper';
import { sleep } from '@utils/retry-helper';
import { env } from '../../environment/model';

// chain-indexer/config.yaml: a gc pass runs every gc_interval (25) blocks once caught up. The
// newest checked state must be followed by two whole intervals, plus a margin for the pass
// that starts right after the state is written and for indexing lag.
const GC_INTERVAL_BLOCKS = 25;
const GC_PASSES = 2;
const MARGIN_BLOCKS = 10;
const BLOCKS_PAST_NEWEST_CHECKED = GC_INTERVAL_BLOCKS * GC_PASSES + MARGIN_BLOCKS;

// How far back from the tip to look for contract history. On undeployed the chain starts at
// genesis for each e2e run, so this covers the whole run; on long-lived environments it bounds
// the scan to recent blocks.
const SCAN_WINDOW_BLOCKS = 1000;
const SCAN_CONCURRENCY = 20;

// Block time is about 6 s on undeployed; wait twice as long as the blocks need.
const BLOCK_TIME_MS = 6_000;
const TIP_POLL_MS = 5_000;
const TOOLKIT_TIMEOUT = 120_000;
const CONTRACT_ACTION_TIMEOUT = 150_000;
const SETUP_TIMEOUT =
  TOOLKIT_TIMEOUT +
  2 * CONTRACT_ACTION_TIMEOUT +
  2 * BLOCKS_PAST_NEWEST_CHECKED * BLOCK_TIME_MS +
  60_000;
const TEST_TIMEOUT = 60_000;

// Only what the scan needs: every contract action of a block, in block order.
const SCAN_BLOCK_QUERY = `
query ScanBlockContractActions($OFFSET: BlockOffset!) {
  block(offset: $OFFSET) {
    height
    hash
    transactions {
      hash
      contractActions {
        __typename
        address
        state
      }
    }
  }
}`;

type ActionType = 'ContractDeploy' | 'ContractCall' | 'ContractUpdate';

/** The state a contract holds as of a block that has at least one action of it. */
interface ContractBlock {
  height: number;
  hash: string;
  /** The type of the block's last action of the contract, which set the state below. */
  lastActionType: ActionType;
  /** The state as the indexer reported it when the history was scanned. */
  scannedState: string;
}

/** A contract with a state that a later call changed, and the blocks of its actions. */
interface Candidate {
  address: string;
  blocks: ContractBlock[];
  /** Index into `blocks` of a state that the next block's call replaced. */
  beforeCallIndex: number;
}

const strip0x = (hex: string) => hex.trim().toLowerCase().replace(/^0x/, '');

describe.sequential('contract state history after gc passes', () => {
  const indexerHttpClient = new IndexerHttpClient();
  const nodeClient = new NodeRpcClient();
  let toolkit: ToolkitWrapper | undefined;

  let candidate: Candidate;
  let tipAtCheck: number;
  // The node's state as of each checked block, keyed by block hash.
  const nodeStates = new Map<string, string>();

  async function indexerTip(): Promise<number> {
    const response = await indexerHttpClient.getLatestBlock();
    const height = response.data?.block?.height;
    if (height === undefined) {
      throw new Error(`indexer returned no latest block: ${JSON.stringify(response.errors)}`);
    }
    return height;
  }

  /**
   * Reads every contract action in the last SCAN_WINDOW_BLOCKS blocks and groups them, per
   * contract, into the state as of each block with an action.
   */
  async function scanContractHistory(tip: number): Promise<Map<string, ContractBlock[]>> {
    const from = Math.max(0, tip - SCAN_WINDOW_BLOCKS);
    const heights = Array.from({ length: tip - from + 1 }, (_, i) => from + i);
    const history = new Map<string, ContractBlock[]>();

    for (let i = 0; i < heights.length; i += SCAN_CONCURRENCY) {
      const responses = await Promise.all(
        heights
          .slice(i, i + SCAN_CONCURRENCY)
          .map((height) => indexerHttpClient.getBlockByOffset({ height }, SCAN_BLOCK_QUERY)),
      );
      for (const response of responses) {
        const block = response.data?.block;
        if (!block) {
          throw new Error(`scan: indexer returned no block: ${JSON.stringify(response.errors)}`);
        }
        // Later actions of a contract in the same block overwrite earlier ones: the state as
        // of a block is the one its last action left.
        const lastInBlock = new Map<string, ContractBlock>();
        for (const tx of block.transactions ?? []) {
          for (const action of tx.contractActions ?? []) {
            lastInBlock.set(strip0x(action.address), {
              height: block.height,
              hash: strip0x(block.hash),
              lastActionType: action.__typename as ActionType,
              scannedState: strip0x(action.state),
            });
          }
        }
        for (const [address, contractBlock] of lastInBlock) {
          history.set(address, [...(history.get(address) ?? []), contractBlock]);
        }
      }
    }
    for (const blocks of history.values()) blocks.sort((a, b) => a.height - b.height);
    return history;
  }

  /**
   * The contract to check: one whose state a later call replaced, so a stale read is told
   * apart from a historical one. Of those, the one with the oldest action, which has been
   * through the most gc passes.
   */
  function pickCandidate(history: Map<string, ContractBlock[]>): Candidate | undefined {
    let best: Candidate | undefined;
    for (const [address, blocks] of history) {
      const beforeCallIndex = blocks.findIndex(
        (block, i) =>
          i + 1 < blocks.length &&
          blocks[i + 1].lastActionType === 'ContractCall' &&
          blocks[i + 1].scannedState !== block.scannedState,
      );
      if (beforeCallIndex < 0) continue;
      if (!best || blocks[0].height < best.blocks[0].height) {
        best = { address, blocks, beforeCallIndex };
      }
    }
    return best;
  }

  /** Deploys a counter contract and calls it once, for a run with no usable history. */
  async function makeHistory(): Promise<void> {
    toolkit = new ToolkitWrapper({});
    // Under TX_BACKEND=moth contract actions go through midnight-js, not the toolkit container.
    if (env.getTxBackend() === 'toolkit') {
      await toolkit.start();
    }
    const fundingSeed = dataProvider.getFundingSeed();
    const deployment = await toolkit.deployContract(fundingSeed);
    log.info(`Deployed counter ${deployment['contract-address-untagged']} for the history check`);
    const call = await toolkit.callContract('store', deployment, undefined, fundingSeed);
    expect(call.status, 'counter store call').toBe('confirmed');
  }

  /** Waits for the indexer tip to reach `height`, polling about once a block. */
  async function waitForIndexerTip(height: number): Promise<number> {
    for (;;) {
      const tip = await indexerTip();
      if (tip >= height) return tip;
      log.info(`Waiting for gc passes: indexer tip ${tip}, need ${height}`);
      await sleep(TIP_POLL_MS);
    }
  }

  beforeAll(async () => {
    const started = Date.now();
    let history = await scanContractHistory(await indexerTip());
    let found = pickCandidate(history);

    if (!found) {
      log.info('No contract history with a state changed by a call; making some');
      await makeHistory();
      // The call may still be on its way to the indexer.
      for (let attempt = 0; attempt < 30 && !found; attempt++) {
        if (attempt > 0) await sleep(TIP_POLL_MS);
        history = await scanContractHistory(await indexerTip());
        found = pickCandidate(history);
      }
      if (!found) {
        throw new Error('The counter deploy and call left no contract history to check');
      }
    }
    candidate = found;

    const newestChecked = candidate.blocks.at(-1)!.height;
    tipAtCheck = await waitForIndexerTip(newestChecked + BLOCKS_PAST_NEWEST_CHECKED);

    for (const block of candidate.blocks) {
      nodeStates.set(block.hash, await nodeClient.getContractState(candidate.address, block.hash));
    }
    log.info(
      `Checking contract ${candidate.address} at blocks ` +
        `${candidate.blocks.map((b) => `${b.height} (${b.lastActionType})`).join(', ')}; ` +
        `indexer tip ${tipAtCheck}; setup took ${Math.round((Date.now() - started) / 1000)} s`,
    );
  }, SETUP_TIMEOUT);

  afterAll(async () => {
    await toolkit?.stop();
  });

  /**
   * The check runs only once enough blocks for two gc passes have been indexed after every
   * checked state was written.
   *
   * @given a contract whose actions are in blocks of the indexed chain
   * @when the indexer tip is read before the states are checked
   * @then it is at least two gc intervals plus a margin past the newest checked block
   */
  test(
    'should check states that at least two gc passes have run past',
    async (context: TestContext) => {
      context.task!.meta.custom = { labels: ['ContractAction', 'GC', 'Precondition'] };

      const newestChecked = candidate.blocks.at(-1)!.height;
      expect(tipAtCheck - newestChecked).toBeGreaterThanOrEqual(BLOCKS_PAST_NEWEST_CHECKED);
      expect(candidate.blocks.length).toBeGreaterThanOrEqual(2);
    },
    TEST_TIMEOUT,
  );

  /**
   * After gc passes, a contract action queried as of an old block height still resolves, to
   * the action of that block, with the state the node holds as of that block.
   *
   * @given a contract with actions in blocks that gc passes have run past
   * @when contractAction(address, offset: { blockOffset: { height } }) is queried for each
   * @then each returns the type and state of that block's last action of the contract
   * @and the state is the node's state as of that block
   */
  test(
    'should return the historical contract action state by block height',
    async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'ContractAction', 'ByBlockHeight', 'History', 'GC'],
      };

      for (const block of candidate.blocks) {
        const label = `contractAction as of height ${block.height}`;
        const response = await indexerHttpClient.getContractAction(candidate.address, {
          blockOffset: { height: block.height },
        });
        expect(response, label).toBeSuccess();
        const action = response.data?.contractAction;
        expect(action, label).toBeDefined();
        expect(action?.__typename, `${label}: type`).toBe(block.lastActionType);
        expect(strip0x(action!.state), `${label}: state`).toBe(nodeStates.get(block.hash));
      }
    },
    TEST_TIMEOUT,
  );

  /**
   * The same as by height, with the block named by its hash.
   *
   * @given a contract with actions in blocks that gc passes have run past
   * @when contractAction(address, offset: { blockOffset: { hash } }) is queried for each
   * @then each returns the type and state of that block's last action of the contract
   * @and the state is the node's state as of that block
   */
  test(
    'should return the historical contract action state by block hash',
    async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'ContractAction', 'ByBlockHash', 'History', 'GC'],
      };

      for (const block of candidate.blocks) {
        const label = `contractAction as of block ${block.hash}`;
        const response = await indexerHttpClient.getContractAction(candidate.address, {
          blockOffset: { hash: block.hash },
        });
        expect(response, label).toBeSuccess();
        const action = response.data?.contractAction;
        expect(action, label).toBeDefined();
        expect(action?.__typename, `${label}: type`).toBe(block.lastActionType);
        expect(strip0x(action!.state), `${label}: state`).toBe(nodeStates.get(block.hash));
      }
    },
    TEST_TIMEOUT,
  );

  /**
   * The contract query reads the same history: as of an old block, by height or by hash, it
   * serves the node's state as of that block.
   *
   * @given a contract with actions in blocks that gc passes have run past
   * @when contract(address, offset: { height }) and contract(address, offset: { hash }) are
   *       queried for each block
   * @then both return the node's state as of that block
   */
  test(
    'should return the historical contract state by block height and hash',
    async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Contract', 'ByHeight', 'ByHash', 'History', 'GC'],
      };

      for (const block of candidate.blocks) {
        for (const offset of [{ height: block.height }, { hash: block.hash }]) {
          const label = `contract as of ${JSON.stringify(offset)}`;
          const response = await indexerHttpClient.getContract(candidate.address, { offset });
          expect(response, label).toBeSuccess();
          expect(response.data?.contract, label).toBeTruthy();
          expect(strip0x(response.data!.contract!.state), `${label}: state`).toBe(
            nodeStates.get(block.hash),
          );
        }
      }
    },
    TEST_TIMEOUT,
  );

  /**
   * A state that a later call replaced is served as it was, not as the call left it: the read
   * is historical, not the current state served for an old block.
   *
   * @given a contract state that a later call replaced, with gc passes run past both
   * @when the contract action is queried as of the block before the call and of the call
   * @then the two states differ and each is the node's state as of its own block
   * @and both equal the states the indexer served when the history was scanned
   */
  test(
    'should return the state from before a later call changed it',
    async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'ContractAction', 'ByBlockHeight', 'History', 'GC', 'ContractCall'],
      };

      const before = candidate.blocks[candidate.beforeCallIndex];
      const call = candidate.blocks[candidate.beforeCallIndex + 1];
      expect(call.lastActionType).toBe('ContractCall');

      const [beforeResponse, callResponse] = await Promise.all(
        [before, call].map((block) =>
          indexerHttpClient.getContractAction(candidate.address, {
            blockOffset: { height: block.height },
          }),
        ),
      );
      expect(beforeResponse).toBeSuccess();
      expect(callResponse).toBeSuccess();
      const beforeState = strip0x(beforeResponse.data!.contractAction!.state);
      const callState = strip0x(callResponse.data!.contractAction!.state);

      expect(nodeStates.get(before.hash), 'node states before and at the call').not.toBe(
        nodeStates.get(call.hash),
      );
      expect(beforeState, `state as of ${before.height}, before the call`).toBe(
        nodeStates.get(before.hash),
      );
      expect(beforeState, `state as of ${before.height}, before the call`).not.toBe(callState);
      expect(callState, `state as of ${call.height}, the call`).toBe(nodeStates.get(call.hash));
      expect(beforeState, 'state before the call, as scanned').toBe(before.scannedState);
      expect(callState, 'state at the call, as scanned').toBe(call.scannedState);
    },
    TEST_TIMEOUT,
  );
});
