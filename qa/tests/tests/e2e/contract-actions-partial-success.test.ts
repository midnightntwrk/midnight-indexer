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

import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import dataProvider from '@utils/testdata-provider';
import {
  COMPACT_COMPILER_VERSION,
  compileCompactContract,
  compiledRuntimeVersion,
} from '@utils/compact/compact-compiler';
import { env } from 'environment/model';
import { getTransactionByHashWithRetry } from './test-utils';
import {
  ToolkitWrapper,
  type CustomContractCall,
  type CustomContractSpec,
  type DeployContractResult,
} from '@utils/toolkit/toolkit-wrapper';
import type { RegularTransaction } from '@utils/indexer/indexer-types';

// First use also builds the Compact toolchain image and pulls the toolkit image.
const SETUP_TIMEOUT = 900_000; // 15 minutes
const CONTRACT_ACTION_TIMEOUT = 600_000; // 10 minutes — deploy plus four proven calls
const TEST_TIMEOUT = 60_000; // 1 minute

/**
 * Compact fixture source; see its README for why the contract is shaped the way
 * it is. Only the `.compact` source and its toolkit-js config are committed —
 * the compiled output is produced on the fly by {@link compileCompactContract}.
 */
const SEGMENT_SPLIT_DIR = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../data/contracts/segment-split',
);
const SEGMENT_SPLIT_SOURCE = 'segment-split.compact';
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
  /** The stale call decoded by the toolkit itself, independently of the indexer. */
  staleDecoded: string;
  /**
   * The stale transaction as the indexer reports it, fetched once up front.
   * Keeping the fetch out of the tests means an indexing failure surfaces as a
   * broken fixture rather than as an assertion outcome, so a red assertion
   * always means the indexer reported the wrong thing.
   */
  indexed: RegularTransaction;
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

      const compiledDir = await compileCompactContract({
        sourceDir: SEGMENT_SPLIT_DIR,
        sourceFile: SEGMENT_SPLIT_SOURCE,
        stage: [SEGMENT_SPLIT.configFile],
      });
      const requiredRuntime = compiledRuntimeVersion(compiledDir);
      log.info(
        `segment-split compiled by compactc ${COMPACT_COMPILER_VERSION} ` +
          `for compact-runtime ${requiredRuntime ?? 'unknown'}`,
      );

      toolkit = new ToolkitWrapper({
        customContractDir: compiledDir,
        compactcVersion: COMPACT_COMPILER_VERSION,
      });
      await toolkit.start();
      if (requiredRuntime) {
        await toolkit.assertCompactRuntimeSupported(requiredRuntime);
      }
    }, SETUP_TIMEOUT);

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
      // Without this first call landing, the second one is not stale: it
      // succeeds outright and the scenario under test never happens. Checked
      // here so that shows up as a broken fixture, not as a puzzling
      // assertion failure pointing at the indexer.
      if (!applied.blockHash) {
        throw new Error(
          `${label}: the first call (tx ${applied.txHash}) never reached a block, so the ` +
            'second call would not be stale',
        );
      }

      const staleCall = await build('stale');
      const staleDecoded = await toolkit.showTransaction(staleCall);
      const stale = await toolkit.sendCustomContractCall(staleCall);

      return { staleDecoded, indexed: await indexedTransaction(stale.txHash) };
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

          const result = pair.indexed.transactionResult;

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

          const actions = pair.indexed.contractActions ?? [];

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
       * Fixture self-check: the transaction is still partially successful even
       * though this call has no guaranteed transcript at all — its fallible
       * segment fails on its own.
       *
       * @given a stale contract call whose circuit has no pre-checkpoint section
       * @when we query the indexer for its transaction result
       * @then the result is a partial success
       */
      test(
        'the fixture still produces a partially successful transaction',
        async (context: TestContext) => {
          context.task!.meta.custom = { labels: ['Query', 'Transaction', 'PartialSuccess'] };

          expect(pair.indexed.transactionResult?.status).toBe('PARTIAL_SUCCESS');
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
       * This asserts the correct behaviour, so it is red against an indexer
       * that does not filter contract actions on segment success — which
       * includes 4.4.0-rc.5. That red is the signal the fix is missing on the
       * build under test, not a broken test: the same assertion passes against
       * 4.3.302, where the filter is present.
       */
      test(
        'is not reported by the indexer',
        async (context: TestContext) => {
          context.task!.meta.custom = {
            labels: ['Query', 'Transaction', 'ContractCall', 'PartialSuccess'],
          };

          const actions = pair.indexed.contractActions ?? [];
          expect(
            actions.some((action) => sameHash(action.address, contractAddress)),
            'no execution phase of this Call applied, so the indexer must not report it',
          ).toBe(false);
        },
        TEST_TIMEOUT,
      );
    });
  });
