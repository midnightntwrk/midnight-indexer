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

import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import dataProvider from '@utils/testdata-provider';
import { env } from 'environment/model';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { NodeRpcClient, type ContractStateResult } from '@utils/node/rpc-client';
import { GET_TRANSACTION_CONTRACT_ACTIONS_BY_OFFSET } from '@utils/indexer/graphql/transaction-queries';
import type { ContractAction } from '@utils/indexer/indexer-types';
import {
  ToolkitWrapper,
  type DeployContractResult,
  type DuplicateDeployResult,
} from '@utils/toolkit/toolkit-wrapper';

const TOOLKIT_STARTUP_TIMEOUT = 300_000;
const CRAFT_TIMEOUT = 900_000;
const ASSERTION_TIMEOUT = 30_000;
const POLLED_ASSERTION_TIMEOUT = 180_000;
const ADVANCE_BUDGET_MS = 120_000;
const INDEXED_BUDGET_MS = 120_000;
const FINALIZATION_BUDGET_MS = 120_000;
const BLOCK_SEARCH_BUDGET_MS = 180_000;
const POLL_MS = 3_000;
// The tip must move on past the transaction's block, not merely reach it: the halting build
// reached it too.
const BLOCKS_PAST_TX_BLOCK = 3;
// How far past the pre-submission height the transaction's block is looked for.
const BLOCK_SEARCH_DEPTH = 40;

function sameHash(a: string | undefined, b: string | undefined): boolean {
  const n = (h: string | undefined) => (h ?? '').trim().toLowerCase().replace(/^0x/, '');
  return n(a) !== '' && n(a) === n(b);
}

function majorVersion(version: string): string {
  return version.trim().replace(/^v/, '').split('.')[0];
}

/**
 * The indexer release line under test, taken from the declared INDEXER_TAG.
 *
 * The line decides which assertions apply: the state-lookup halt needs a node 2.x, which only
 * the 4.4 line can index, while the phantom contract action is a defect on both lines. Read
 * from the declared tag rather than probed from the indexer — a build must not be allowed to
 * tell the suite which of its defects to look for.
 */
function declaredIndexerLine(): { major: number; minor: number } {
  const tag = process.env.INDEXER_TAG;
  if (!tag) {
    throw new Error('INDEXER_TAG must be set so the suite knows which indexer line it is testing');
  }
  const [major, minor] = tag
    .trim()
    .replace(/^v/, '')
    .split('.')
    .map((part) => Number.parseInt(part, 10));
  if (!Number.isInteger(major) || !Number.isInteger(minor)) {
    throw new Error(`cannot read an indexer line from INDEXER_TAG "${tag}"`);
  }
  return { major, minor };
}

const line = env.isUndeployedEnv() ? declaredIndexerLine() : { major: 0, minor: 0 };
// Only 4.4 and later speak NodeVersion V2_x. On 4.3 a node 2.x cannot be indexed at all, so
// the block-advance case has nothing to observe there.
const indexesNodeTwo = line.major > 4 || (line.major === 4 && line.minor >= 4);

/**
 * Pink-tawny-owl vector B (SSE #612, indexer PR #1520, backports #1524 / #1526): a contract
 * Deploy whose fallible segment is rolled back must not reach the API as a contract action,
 * and must not stop the chain-indexer.
 *
 * The oracle throughout is the node, never the indexer: the node says which block the
 * transaction landed in and whether the contract exists, and the indexer has to agree.
 *
 * What this suite can and cannot prove. The phantom contract action is observable on any
 * build — a pre-fix one reports two Deploy rows at an address holding no contract. The
 * crash-loop is not: since #1386 the indexer captures contract state from its own ledger
 * arena and never asks the node for it, so on a fixed build the block-advance case cannot
 * fail. It is kept because it maps to the outage the ticket describes, and it is given teeth
 * by running the same suite against a pre-#1386 build (4.4.0-rc.3 on a node 2.0.x), where it
 * must go red with the indexer's own `ContractNotPresent` fatal. That control run is part of
 * the procedure, not an optional extra.
 */
describe
  .skipIf(!env.isUndeployedEnv())
  .sequential('a contract deploy in a rolled-back fallible segment', () => {
    let indexer: IndexerHttpClient;
    let node: NodeRpcClient;
    let toolkit: ToolkitWrapper;
    let deploy: DuplicateDeployResult;
    let plainDeploy: DeployContractResult;
    let plainDeployState: ContractStateResult;
    let txBlockHeight = Number.NaN;
    let nodeContractState: ContractStateResult;

    beforeAll(async () => {
      indexer = new IndexerHttpClient();
      node = new NodeRpcClient();

      // The halting path only exists against a node 2.x, and NODE_TAG defaults to the 1.0.x
      // entry of NODE_VERSIONS — so a 4.4 run that was not pinned to 2.x exercises nothing.
      // Fail here rather than skip: an unpinned run is a broken run.
      const declaredNodeTag = env.getNodeVersion();
      const reportedVersion = await node.getSystemVersion();
      log.info(
        `indexer line ${line.major}.${line.minor}, NODE_TAG=${declaredNodeTag}, ` +
          `node system_version=${reportedVersion}`,
      );
      expect(
        majorVersion(reportedVersion),
        `the running node reports "${reportedVersion}", which is not the declared NODE_TAG "${declaredNodeTag}"`,
      ).toBe(majorVersion(declaredNodeTag));
      if (indexesNodeTwo) {
        expect(
          majorVersion(declaredNodeTag),
          `indexer line ${line.major}.${line.minor} must be exercised against a node 2.x; NODE_TAG is "${declaredNodeTag}"`,
        ).toBe('2');
      }

      const finalizationDeadline = Date.now() + FINALIZATION_BUDGET_MS;
      toolkit = new ToolkitWrapper({});
      await toolkit.start();

      // The toolkit refuses to build against a chain with only genesis finalized, which a
      // freshly provisioned stack has for the first few seconds.
      while ((await node.getFinalizedHeight()) < 1) {
        if (Date.now() > finalizationDeadline) {
          throw new Error(
            `the node finalized nothing beyond genesis within ${FINALIZATION_BUDGET_MS}ms`,
          );
        }
        await new Promise((resolve) => setTimeout(resolve, POLL_MS));
      }

      const fundingSeed = dataProvider.getFundingSeed();

      // An ordinary deploy, read back through the same oracle: it keeps "the node says the
      // contract is absent" from passing because the oracle itself stopped working.
      plainDeploy = await toolkit.deployContract(fundingSeed);
      plainDeployState = await node.getContractState(plainDeploy['contract-address-untagged']);

      const heightBeforeSubmit = await node.getChainTip();
      deploy = await toolkit.deployDuplicateContractIntent(fundingSeed);
      log.info(
        `duplicate deploy: address=${deploy.contractAddressUntagged} ` +
          `deployActions=${deploy.deployActionsInTx} tx=${deploy.transaction.txHash}`,
      );

      // The toolkit's send reports the transaction hash but no block, so the block is found on
      // the node. This also proves the transaction really landed: every later assertion would
      // hold vacuously for a transaction that was never included.
      const searchDeadline = Date.now() + BLOCK_SEARCH_BUDGET_MS;
      const bareTxHash = deploy.transaction.txHash.toLowerCase().replace(/^0x/, '');
      for (
        let height = heightBeforeSubmit;
        height <= heightBeforeSubmit + BLOCK_SEARCH_DEPTH;
        height += 1
      ) {
        while (height > (await node.getChainTip())) {
          if (Date.now() > searchDeadline) {
            throw new Error(
              `the chain did not reach height ${height} within ${BLOCK_SEARCH_BUDGET_MS}ms while looking for ${deploy.transaction.txHash}`,
            );
          }
          await new Promise((resolve) => setTimeout(resolve, POLL_MS));
        }
        if ((await toolkit.showBlock(height)).toLowerCase().includes(bareTxHash)) {
          txBlockHeight = height;
          break;
        }
      }
      log.info(`duplicate deploy landed at height ${txBlockHeight}`);

      nodeContractState = await node.getContractState(deploy.contractAddressUntagged);
      log.info(
        `node contract state for ${deploy.contractAddressUntagged}: ` +
          `${nodeContractState.present ? 'present' : 'absent'} — ${nodeContractState.nodeAnswer}`,
      );
    }, TOOLKIT_STARTUP_TIMEOUT + CRAFT_TIMEOUT);

    afterAll(async () => {
      await toolkit?.stop();
    });

    describe('the crafted transaction', () => {
      /**
       * @given one deploy intent passed twice to send-intent
       * @when the built transaction is deserialized before submission
       * @then it carries two deploy actions, so a second, unapplicable deploy is really in it
       */
      test(
        'should carry two deploys of the same contract',
        async (context: TestContext) => {
          context.task!.meta.custom = { labels: ['ContractDeploy', 'FailedSegment', 'Toolkit'] };

          expect(deploy.contractAddressUntagged).not.toBe('');
          expect(deploy.deployActionsInTx).toBe(2);
        },
        ASSERTION_TIMEOUT,
      );

      /**
       * @given the duplicate-deploy transaction was submitted
       * @when the node's own blocks are searched for its hash
       * @then it is found, so nothing downstream can pass on a transaction that never landed
       */
      test(
        'should be included in a block the node serves',
        async (context: TestContext) => {
          context.task!.meta.custom = { labels: ['ContractDeploy', 'FailedSegment', 'Node'] };

          expect(
            Number.isNaN(txBlockHeight),
            `the node served no block containing ${deploy.transaction.txHash}`,
          ).toBe(false);
          expect(txBlockHeight).toBeGreaterThan(0);
        },
        ASSERTION_TIMEOUT,
      );
    });

    describe('what the node holds afterwards', () => {
      /**
       * @given an ordinary contract deploy on the same chain
       * @when its state is read through the same node RPC used to judge the rolled-back one
       * @then the node reports it present, so an absent answer below means absence
       *
       * Without this, a renamed method or a changed answer would make every address look
       * absent — which is also the expected result, so the suite would pass blind.
       */
      test(
        'should report an ordinary deploy as present',
        async (context: TestContext) => {
          context.task!.meta.custom = { labels: ['ContractDeploy', 'Node', 'Oracle'] };

          expect(
            plainDeployState.present,
            `the node reports no state for the ordinary contract ${plainDeploy['contract-address-untagged']} (${plainDeployState.nodeAnswer}); the contract-state oracle is not working`,
          ).toBe(true);
        },
        ASSERTION_TIMEOUT,
      );

      /**
       * @given a transaction whose deploys were all in the rolled-back fallible segment
       * @when the node is asked for the contract state at that address
       * @then it holds no contract there — the acceptance criterion of SSE #612
       */
      test(
        'should hold no contract at the rolled-back address',
        async (context: TestContext) => {
          context.task!.meta.custom = { labels: ['ContractDeploy', 'FailedSegment', 'Node'] };

          // Tied to the transaction being on chain: "the node holds no contract" is also true
          // when nothing was ever submitted.
          expect(Number.isNaN(txBlockHeight)).toBe(false);
          expect(
            nodeContractState.present,
            `the craft no longer rolls the deploy back: the node holds ${nodeContractState.nodeAnswer} at ${deploy.contractAddressUntagged}`,
          ).toBe(false);
        },
        ASSERTION_TIMEOUT,
      );
    });

    describe('what the indexer reports', () => {
      /**
       * @given the rolled-back deploy is in a finalized block
       * @when the indexer is polled after that block
       * @then its tip moves on past it instead of re-fetching the same block forever
       */
      test.skipIf(!indexesNodeTwo)(
        'should keep advancing past that block',
        async (context: TestContext) => {
          context.task!.meta.custom = { labels: ['ContractDeploy', 'FailedSegment', 'Liveness'] };

          expect(Number.isNaN(txBlockHeight)).toBe(false);
          const target = txBlockHeight + BLOCKS_PAST_TX_BLOCK;
          const deadline = Date.now() + ADVANCE_BUDGET_MS;
          let height = -1;

          while (Date.now() < deadline && height < target) {
            height = (await indexer.getLatestBlock()).data?.block?.height ?? -1;
            if (height < target) {
              await new Promise((resolve) => setTimeout(resolve, POLL_MS));
            }
          }

          log.info(`indexer height ${height}, needed >= ${target}`);
          expect(
            height,
            `the indexer stalled at ${height}; the transaction's block was ${txBlockHeight}`,
          ).toBeGreaterThanOrEqual(target);
        },
        POLLED_ASSERTION_TIMEOUT,
      );

      /**
       * @given a transaction whose deploys the ledger rolled back
       * @when the indexer is asked for that transaction's contract actions and for the address
       * @then it reports no deploy at all, matching the contract the node does not hold
       */
      test(
        'should report no deploy for the rolled-back address',
        async (context: TestContext) => {
          context.task!.meta.custom = { labels: ['ContractDeploy', 'FailedSegment', 'Query'] };

          expect(Number.isNaN(txBlockHeight)).toBe(false);

          // The transaction is on chain; give the indexer time to reach its block before
          // reading an empty answer as a correct one.
          const deadline = Date.now() + INDEXED_BUDGET_MS;
          let transactions: { contractActions?: ContractAction[] }[] = [];
          while (Date.now() < deadline && transactions.length === 0) {
            const response = await indexer.getTransactionByOffset(
              { hash: deploy.transaction.txHash },
              GET_TRANSACTION_CONTRACT_ACTIONS_BY_OFFSET,
            );
            expect(response).toBeSuccess();
            transactions = response.data?.transactions ?? [];
            if (transactions.length === 0) {
              await new Promise((resolve) => setTimeout(resolve, POLL_MS));
            }
          }
          expect(
            transactions.length,
            `the indexer never reported transaction ${deploy.transaction.txHash}, which the node serves in block ${txBlockHeight}`,
          ).toBeGreaterThan(0);

          const actions = transactions.flatMap((transaction) => transaction.contractActions ?? []);
          const deploys = actions.filter((action) => action.__typename === 'ContractDeploy');
          const deploysForAddress = deploys.filter((action) =>
            sameHash(action.address, deploy.contractAddressUntagged),
          );
          expect(
            deploysForAddress.length,
            `the ledger rolled this deploy back and the node holds no contract for it, but the indexer reports ${deploysForAddress.length} deploy action(s) for the address`,
          ).toBe(0);
          // Both deploys in this transaction were rolled back, so it should carry no deploy at
          // all. Checked without matching the address, so a differently formatted address
          // cannot hide a phantom from the filter above.
          expect(
            deploys.length,
            `the transaction's deploys were all rolled back, but the indexer reports ${deploys.length} deploy action(s): ${deploys.map((action) => action.address).join(', ')}`,
          ).toBe(0);

          const actionResponse = await indexer.getContractAction(deploy.contractAddressUntagged);
          expect(actionResponse).toBeSuccess();
          expect(actionResponse.data?.contractAction ?? null).toBeNull();
        },
        POLLED_ASSERTION_TIMEOUT,
      );

      // A second, independent witness of the same filter: without it the rolled-back action
      // reaches state capture, misses the arena and increments the counter. It cannot be read
      // on this stack — chain-indexer/config.yaml has telemetry.metrics.enabled=false and the
      // chain-indexer service in docker-compose.yaml publishes no port, so :9000/metrics is
      // unreachable. Enabling both is a change to the shared compose profile and is tracked
      // separately from this suite.
      test.todo('should not increment indexer_uncaptured_contract_state_count');
    });
  });
