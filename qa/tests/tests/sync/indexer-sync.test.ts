// This file is part of midnightntwrk/midnight-indexer.
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
import {
  IndexerSyncHarness,
  parseIndexerMetrics,
  readSyncOptions,
  syncPrerequisiteGap,
} from '@utils/sync/harness';
import {
  computeProgress,
  formatDuration,
  formatProgressLine,
  UNBOUNDED_MAX_BLOCKS,
} from '@utils/sync/progress-reporter';

const prerequisiteGap = syncPrerequisiteGap();

// Fixed clock so the rate maths is deterministic.
const START_MS = 1_000_000;
const SAMPLES = [
  { atMs: START_MS, height: 0 },
  { atMs: START_MS + 30_000, height: 60 },
  { atMs: START_MS + 60_000, height: 240 },
];

describe('indexer sync compatibility', () => {
  describe('progress reporting for a sync run', () => {
    /**
     * The long phase of a sync is only legible if the reported rates are right.
     *
     * @given height observations of 0, 60 and 240 blocks taken 30 seconds apart
     * @when progress is derived against a budget of 1000 blocks
     * @then the overall rate is 4 blocks/s, the 30-second rate is the steeper 6
     *       blocks/s, and the line reads `blocks synced: 240/1000 (24%)`
     * @and a run that overshoots its budget reports the true block count with the
     *      percentage capped at 100 rather than exceeding it
     */
    test('should report overall and 30-second-window block rates', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Progress'] };

      const stats = computeProgress(SAMPLES, 10_000, {
        startHeight: 0,
        maxBlocks: 1000,
        startedAtMs: START_MS,
      });

      expect(stats.overallRate).toBeCloseTo(4, 5);
      expect(stats.windowRate).toBeCloseTo(6, 5);
      expect(stats.synced).toBe(240);
      expect(stats.total).toBe(1000);
      expect(stats.percent).toBeCloseTo(24, 5);
      // 760 blocks left at the overall 4 blocks/s.
      expect(formatDuration(stats.etaMs!)).toBe('3m 10s');
      expect(formatProgressLine(stats)).toContain('blocks synced: 240/1000 (24%)');

      // A single poll can carry a run past its budget. The overshoot stays visible in
      // the block count, but the percentage is capped rather than reading e.g. 105%.
      const overshot = computeProgress(
        [...SAMPLES, { atMs: START_MS + 90_000, height: 1100 }],
        10_000,
        {
          startHeight: 0,
          maxBlocks: 1000,
          startedAtMs: START_MS,
        },
      );
      expect(overshot.synced).toBe(1100);
      expect(overshot.percent).toBe(100);
      expect(overshot.etaMs).toBe(0);
    });

    /**
     * `MAX_BLOCKS=0` is the opt-in to an unbounded run. It must be read as "no bound,
     * sync to the chain tip", never as a budget of zero blocks.
     *
     * @given the same observations, with the block budget set to the unbounded sentinel
     * @when progress is derived against a chain tip of 2,000,000 blocks
     * @then the total is the chain tip rather than 0, and both the percentage and the
     *       remaining-time estimate are measured against it
     */
    test('should measure progress against the chain tip when the block budget is unbounded', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Progress'] };

      const tip = 2_000_000;
      const stats = computeProgress(SAMPLES, tip, {
        startHeight: 0,
        maxBlocks: UNBOUNDED_MAX_BLOCKS,
        startedAtMs: START_MS,
      });

      expect(stats.total).toBe(tip);
      expect(stats.total).not.toBe(UNBOUNDED_MAX_BLOCKS);
      expect(stats.percent).toBeCloseTo((240 / tip) * 100, 5);
      expect(formatProgressLine(stats)).toContain(`blocks synced: 240/${tip}`);
      // ~500,000 seconds left at 4 blocks/s, so the estimate is reported in hours.
      expect(formatDuration(stats.etaMs!)).toMatch(/^\d+h \d{2}m$/);
    });

    /**
     * The indexer publishes none of its height metrics until the first block batch is
     * processed, so a freshly started container serves a body without them. An absent
     * metric means "not known yet" — reading it as height 0 would make a stalled
     * indexer look like a healthy one at the start of a chain.
     *
     * @given a metrics body carrying only unrelated counters
     * @when the height metrics are read and progress is derived from no observations
     * @then no height, tip or caught-up state is reported, the rates stay at zero, and
     *       the line says it is still waiting rather than claiming 0%
     */
    test('should report no height until the indexer publishes its first metrics', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Progress'] };

      const silent = ['# TYPE indexer_transaction_count counter', 'indexer_transaction_count 0'];
      const empty = parseIndexerMetrics(silent.join('\n'));
      expect(empty.blockHeight).toBeUndefined();
      expect(empty.nodeBlockHeight).toBeUndefined();
      expect(empty.caughtUp).toBeUndefined();

      const stats = computeProgress([], undefined, {
        startHeight: 0,
        maxBlocks: 1000,
        startedAtMs: START_MS,
      });
      expect(stats.height).toBeUndefined();
      expect(stats.synced).toBe(0);
      expect(stats.overallRate).toBe(0);
      expect(stats.windowRate).toBe(0);
      expect(formatProgressLine(stats)).toBe('waiting for the indexer to report a height...');

      const reporting = parseIndexerMetrics(
        ['indexer_block_height 512', 'indexer_node_block_height 4096', 'indexer_caught_up 1'].join(
          '\n',
        ),
      );
      expect(reporting).toEqual({ blockHeight: 512, nodeBlockHeight: 4096, caughtUp: true });
    });
  });

  // Sequencing is enforced by the project config (`fileParallelism: false`,
  // `maxWorkers: 1`): one indexer stack on one set of host ports at a time.
  describe.skipIf(prerequisiteGap !== undefined)(
    'a stock indexer image syncing a deployed chain',
    () => {
      let harness: IndexerSyncHarness | undefined;

      afterAll(async () => {
        await harness?.stop();
      });

      /**
       * The indexer re-executes every block through the ledger library, so a version
       * that disagrees with the node derives a different state and bails out. Pointing
       * a released image at a live chain and watching it index is what catches that.
       *
       * The run stops at `MAX_BLOCKS` blocks (2000 by default). `MAX_BLOCKS=0` removes
       * the block bound and syncs to the chain tip, leaving `MAX_DURATION_MS` as the
       * only limit — it does not mean "index zero blocks".
       *
       * @given a released indexer image and a deployed chain selected by TARGET_ENV
       * @when the stack indexes that chain from genesis up to its block budget
       * @then no container exits, no ledger or zswap state root mismatch is logged, and
       *       the run reaches either its budget or the chain tip
       */
      test('should index blocks without a state-root mismatch or a crash', async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Sync', 'ChainIndexer', 'Compatibility'] };

        const options = readSyncOptions();
        log.info(
          `Syncing ${options.topology} stack on tag ${options.indexerTag} against ` +
            `${env.getCurrentEnvironmentName()}, budget ${options.maxBlocks} blocks`,
        );

        harness = new IndexerSyncHarness(options);
        await harness.start();
        const result = await harness.run();

        const detail = [
          `outcome: ${result.outcome}`,
          `indexed height: ${result.height ?? 'none reported'}`,
          `node tip: ${result.nodeTip ?? 'unknown'}`,
          result.exit && `service ${result.exit.service} exited with ${result.exit.exitCode}`,
          result.mismatch &&
            `${result.mismatch.kind} mismatch at height ${result.mismatch.height} ` +
              `(block ${result.mismatch.block})`,
          result.logTail.length > 0 && `log tail:\n${result.logTail.join('\n')}`,
        ]
          .filter(Boolean)
          .join('\n');

        expect(result.mismatch, `state root divergence\n${detail}`).toBeUndefined();
        expect(
          result.exit,
          `indexer stopped before reaching its target\n${detail}`,
        ).toBeUndefined();
        expect(
          ['caught-up', 'budget-reached'],
          `sync did not reach its target\n${detail}`,
        ).toContain(result.outcome);
      });
    },
  );
});
