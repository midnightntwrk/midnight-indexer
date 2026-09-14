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

import { resolve } from 'node:path';
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import dataProvider from '@utils/testdata-provider';
import { env } from 'environment/model';
import { getTransactionByHashWithRetry } from './test-utils';
import {
  ToolkitWrapper,
  type CustomContractCall,
  type CustomContractSpec,
  type DeployContractResult,
  type ToolkitTransactionResult,
} from '@utils/toolkit/toolkit-wrapper';
import type { RegularTransaction } from '@utils/indexer/indexer-types';

const TOOLKIT_WRAPPER_TIMEOUT = 120_000; // 2 minutes
const CONTRACT_ACTION_TIMEOUT = 600_000; // 10 minutes — deploy plus four proven calls
const TEST_TIMEOUT = 60_000; // 1 minute

/** Compiled Compact fixture; see its README for why it is shaped the way it is. */
const SEGMENT_SPLIT_DIR = resolve('data/contracts/segment-split');
const SEGMENT_SPLIT: CustomContractSpec = { configFile: 'segment-split.config.ts' };

/** Normalize hash for comparison (indexer may return with or without 0x prefix). */
function sameHash(a: string | undefined, b: string | undefined): boolean {
  const n = (h: string | undefined) => (h ?? '').trim().toLowerCase().replace(/^0x/, '');
  return n(a) === n(b);
}

/**
 * The outcome of submitting one circuit twice against the same on-chain state
 * snapshot: the first call applies, the second is stale by the time the ledger
 * runs it.
 */
interface StalePair {
  applied: ToolkitTransactionResult;
  stale: ToolkitTransactionResult;
  /** The stale call decoded by the toolkit itself, independently of the indexer. */
  staleDecoded: string;
}

// Undeployed only: the toolkit rebuilds ledger state from genesis on every call, which on a
// long-lived deployed chain takes hours per step (see Env gating in the test conventions).
describe
  .skipIf(!env.isUndeployedEnv())
  .sequential('contract actions in a guaranteed/fallible segment split', () => {
    let toolkit: ToolkitWrapper;
    let fundingSeed: string;
    let deployment: DeployContractResult;
    let contractAddress: string;

    beforeAll(async () => {
      fundingSeed = dataProvider.getFundingSeed();
      toolkit = new ToolkitWrapper({ customContractDir: SEGMENT_SPLIT_DIR });
      await toolkit.start();
    }, TOOLKIT_WRAPPER_TIMEOUT);

    afterAll(async () => {
      await toolkit.stop();
    });

    /**
     * Deploy once and reuse the contract for both scenarios: each scenario burns
     * its own fuse, so they do not interfere.
     */
    beforeAll(async () => {
      deployment = await toolkit.deployCustomContract(SEGMENT_SPLIT, [], fundingSeed);
      contractAddress = deployment['contract-address-untagged'];
      log.info(`segment-split contract deployed at ${contractAddress}`);
    }, CONTRACT_ACTION_TIMEOUT);

    /**
     * Submit `circuitId` twice, both proven against the same state snapshot taken
     * beforehand. The first application moves the state on, so the second call's
     * fallible transcript underflows when the ledger re-runs it at apply time.
     */
    async function submitStalePair(circuitId: string, label: string): Promise<StalePair> {
      const stateFile = await toolkit.snapshotContractState(contractAddress, `${label}_state.bin`);

      // Each call is built and submitted before the next is built. The staleness
      // that makes the second call fail comes from `stateFile` — a snapshot taken
      // before either was applied — not from the build order, so interleaving is
      // safe. It is also necessary: building both up front makes them spend the
      // same DUST fee input, and the node rejects the second as a double-spend
      // long before any of the segment logic under test is reached.
      const build = (suffix: string): Promise<CustomContractCall> =>
        toolkit.generateCustomContractCall({
          circuitId,
          deploymentResult: deployment,
          contract: SEGMENT_SPLIT,
          onchainStateFile: stateFile,
          label: `${label}_${suffix}`,
          fundingSeed,
        });

      const applied = await toolkit.sendCustomContractCall(await build('applied'));

      const staleCall = await build('stale');
      const staleDecoded = await toolkit.showTransaction(staleCall);
      const stale = await toolkit.sendCustomContractCall(staleCall);

      return { applied, stale, staleDecoded };
    }

    /** Fetch the indexed transaction for a hash, failing loudly if it never appears. */
    async function indexedTransaction(hash: string): Promise<RegularTransaction> {
      const response = await getTransactionByHashWithRetry(hash);
      expect(response).toBeSuccess();
      const transactions = response?.data?.transactions;
      expect(transactions, `transaction ${hash} was never indexed`).toBeDefined();
      expect(transactions!.length).toBeGreaterThan(0);
      return transactions![0] as RegularTransaction;
    }

    describe('a Call whose guaranteed transcript applied but whose fallible segment failed', () => {
      let pair: StalePair;

      beforeAll(async () => {
        pair = await submitStalePair('burnWithGuaranteed', 'with_guaranteed');
      }, CONTRACT_ACTION_TIMEOUT);

      /**
       * Fixture self-check. The ledger's partition algorithm decides how much of a
       * circuit lands in the guaranteed phase, and it can silently put all of it in
       * the fallible phase. Confirming the guaranteed transcript is present — using
       * the toolkit's own deserializer, not the indexer — is what makes the
       * assertion below meaningful: without it, "the indexer reported the call"
       * cannot be told apart from "the fixture never produced a guaranteed
       * transcript in the first place".
       *
       * @given a contract call built against a now-stale on-chain state
       * @when we decode the generated transaction with the toolkit
       * @then it carries a non-empty guaranteed transcript
       */
      test(
        'the fixture produces a call with a guaranteed transcript',
        async (context: TestContext) => {
          context.task!.meta.custom = {
            labels: ['Fixture', 'ContractCall', 'GuaranteedTranscript'],
          };

          expect(ToolkitWrapper.hasGuaranteedTranscript(pair.staleDecoded)).toBe(true);
        },
        TEST_TIMEOUT,
      );

      /**
       * Fixture self-check: the transaction really is partially successful, with
       * the guaranteed segment applied and the fallible segment rolled back.
       *
       * @given a stale contract call that was included in a block
       * @when we query the indexer for its transaction result
       * @then the result is a partial success where segment 0 succeeded and a
       *       fallible segment failed
       */
      test(
        'the fixture produces a partially successful transaction',
        async (context: TestContext) => {
          context.task!.meta.custom = { labels: ['Query', 'Transaction', 'PartialSuccess'] };

          const transaction = await indexedTransaction(pair.stale.txHash);
          const result = transaction.transactionResult;

          expect(result?.status).toBe('PARTIAL_SUCCESS');
          expect(result?.segments?.find((segment) => segment.id === 0)?.success).toBe(true);
          expect(
            result?.segments?.some((segment) => segment.id !== 0 && !segment.success),
            'expected at least one failed fallible segment',
          ).toBe(true);
        },
        TEST_TIMEOUT,
      );

      /**
       * The regression itself. The call's guaranteed transcript executed in segment
       * 0 and was committed, so the call did affect chain state and must stay
       * visible — even though the segment physically containing it failed. An
       * indexer that filters contract actions on the physical segment alone drops
       * it, which is the defect this test exists to catch.
       *
       * @given a partially successful transaction whose Call applied its guaranteed
       *        transcript but whose fallible segment failed
       * @when we query the indexer for that transaction
       * @then the contract call is still reported
       */
      test(
        'is still reported by the indexer',
        async (context: TestContext) => {
          context.task!.meta.custom = {
            labels: ['Query', 'Transaction', 'ContractCall', 'PartialSuccess', 'Regression'],
          };

          const transaction = await indexedTransaction(pair.stale.txHash);
          const actions = transaction.contractActions ?? [];

          expect(
            actions.some((action) => sameHash(action.address, contractAddress)),
            'the Call applied its guaranteed transcript, so the indexer must still report it',
          ).toBe(true);
        },
        TEST_TIMEOUT,
      );
    });

    describe('a Call where neither execution phase applied', () => {
      let pair: StalePair;

      beforeAll(async () => {
        pair = await submitStalePair('burnWithoutGuaranteed', 'without_guaranteed');
      }, CONTRACT_ACTION_TIMEOUT);

      /**
       * Fixture self-check, and the converse of the case above: this circuit does
       * no work before its checkpoint, so the partition leaves it with no
       * guaranteed transcript at all.
       *
       * @given a contract call whose circuit has no pre-checkpoint section
       * @when we decode the generated transaction with the toolkit
       * @then it carries no guaranteed transcript
       */
      test(
        'the fixture produces a call without a guaranteed transcript',
        async (context: TestContext) => {
          context.task!.meta.custom = { labels: ['Fixture', 'ContractCall', 'FallibleOnly'] };

          expect(ToolkitWrapper.hasGuaranteedTranscript(pair.staleDecoded)).toBe(false);
        },
        TEST_TIMEOUT,
      );

      /**
       * Counterpart to the assertion above, and the reason both are needed: an
       * indexer that simply never filters anything would pass the "still reported"
       * test for the wrong reason. Nothing about this call reached the ledger
       * state, so it must not be reported.
       *
       * @given a partially successful transaction whose Call had no guaranteed
       *        transcript and whose fallible segment failed
       * @when we query the indexer for that transaction
       * @then the contract call is not reported
       *
       * Marked `test.fails` until the contract-action segment filter lands on
       * main: the current indexer still reports this call. Vitest fails a
       * `test.fails` test that passes, so the marker must be removed the moment
       * the filter ships — it cannot be forgotten.
       */
      test.fails(
        'is not reported by the indexer',
        async (context: TestContext) => {
          context.task!.meta.custom = {
            labels: ['Query', 'Transaction', 'ContractCall', 'PartialSuccess'],
          };

          const transaction = await indexedTransaction(pair.stale.txHash);
          expect(transaction.transactionResult?.status).toBe('PARTIAL_SUCCESS');

          const actions = transaction.contractActions ?? [];
          expect(
            actions.some((action) => sameHash(action.address, contractAddress)),
            'no execution phase of this Call applied, so the indexer must not report it',
          ).toBe(false);
        },
        TEST_TIMEOUT,
      );
    });
  });
