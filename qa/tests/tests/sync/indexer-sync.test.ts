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
  createLineAssembler,
  IndexerSyncHarness,
  isBailOutLine,
  parseIndexerMetrics,
  parseStateRootMismatch,
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
if (prerequisiteGap !== undefined) {
  // Without this the only compatibility case disappears from a green run with no
  // explanation, which is the worst possible default for a gate.
  console.log(`[SYNC] Skipping the sync compatibility test: ${prerequisiteGap}.`);
}

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
     * One observation gives no interval to average over. Reporting 0 blocks/s there
     * reads as a stall on the first line of every run, so the field is absent instead.
     *
     * @given a single height observation
     * @when progress is derived from it
     * @then no 30-second rate is reported and the line renders it as unavailable
     */
    test('should not report a 30-second rate from a single observation', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Progress'] };

      const stats = computeProgress(SAMPLES.slice(0, 1), 10_000, {
        startHeight: 0,
        maxBlocks: 1000,
        startedAtMs: START_MS,
      });

      expect(stats.windowRate).toBeUndefined();
      expect(formatProgressLine(stats)).toContain('-- blocks/s (30s)');
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
     * Height must not be invented before the indexer reports one: treating an unknown
     * height as 0 would make a stalled indexer look like a healthy one at the start of
     * a chain.
     *
     * @given no height observations yet
     * @when progress is derived
     * @then no height is reported, the overall rate stays at zero, and the line says it
     *       is still waiting rather than claiming 0%
     */
    test('should report no height before the indexer reports one', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Progress'] };

      const stats = computeProgress([], undefined, {
        startHeight: 0,
        maxBlocks: 1000,
        startedAtMs: START_MS,
      });

      expect(stats.height).toBeUndefined();
      expect(stats.synced).toBe(0);
      expect(stats.overallRate).toBe(0);
      expect(stats.windowRate).toBeUndefined();
      expect(formatProgressLine(stats)).toBe('waiting for the indexer to report a height...');
    });
  });

  describe('reading the indexer progress metrics', () => {
    /**
     * None of the height metrics are published until the first block batch is
     * processed, so a freshly started container serves a body without them.
     *
     * @given a metrics body carrying only unrelated counters
     * @when the progress metrics are read from it
     * @then no height, node height or caught-up state is reported
     */
    test('should report nothing from a body without the height metrics', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Progress'] };

      const metrics = parseIndexerMetrics(
        ['# TYPE indexer_transaction_count counter', 'indexer_transaction_count 0'].join('\n'),
      );

      expect(metrics.blockHeight).toBeUndefined();
      expect(metrics.nodeBlockHeight).toBeUndefined();
      expect(metrics.caughtUp).toBeUndefined();
    });

    /**
     * @given a metrics body reporting height 512, node height 4096 and caught up
     * @when the progress metrics are read from it
     * @then all three values are reported, with the caught-up gauge read as a boolean
     */
    test('should report height, node height and caught-up state when published', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Progress'] };

      const metrics = parseIndexerMetrics(
        ['indexer_block_height 512', 'indexer_node_block_height 4096', 'indexer_caught_up 1'].join(
          '\n',
        ),
      );

      expect(metrics).toEqual({ blockHeight: 512, nodeBlockHeight: 4096, caughtUp: true });
    });
  });

  describe('recognising a bail-out in the container logs', () => {
    /**
     * Naming the offending block is the point of the suite, so the height has to come
     * out of the log line rather than being left for a reader to find.
     *
     * @given a zswap state root mismatch logged for block 9df3eda7 at height 15616
     * @when the line is read
     * @then it counts as a bail-out and the offending block and height are reported
     */
    test('should name the offending block of a state root mismatch', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Negative'] };

      // The shape the binary actually emits: the mismatch is an `anyhow` bail that
      // reaches the log only inside the `error` field of the fatal exit line, rendered
      // as a single JSON object by the logging layer.
      const line =
        'chain-indexer-1  | {"timestamp":"2026-08-19T17:17:34.390320+00:00[Etc/UTC]",' +
        '"level":"ERROR","target":"chain_indexer","file":"chain-indexer/src/main.rs",' +
        '"line":33,"message":"process exited with ERROR","kvs":{"backtrace":' +
        '"disabled backtrace","error":"index_blocks_task failed: zswap state root ' +
        'mismatch for block b7840d11 at height 15616: node=Some(00df7a18), ' +
        'indexer=Some(009e6bae)"}}';

      expect(isBailOutLine(line)).toBe(true);
      expect(parseStateRootMismatch(line)).toEqual({
        kind: 'zswap state root',
        block: 'b7840d11',
        height: 15616,
        line,
      });
    });

    /**
     * An incompatible image can bail out for reasons no phrase list enumerates, so the
     * fatal line every binary logs on its way out has to count too. This is a real line
     * captured from chain-indexer 4.3.7 pointed at a chain with a mismatched network id.
     *
     * @given the fatal exit line of an indexer rejecting a transaction's network id
     * @when the line is read
     * @then it counts as a bail-out even though it names no state root
     */
    test('should recognise a fatal exit that names no state root', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Negative'] };

      const line =
        'chain-indexer-1  | {"level":"ERROR","target":"chain_indexer",' +
        '"message":"process exited with ERROR","kvs":{"error":"index_blocks_task failed: ' +
        'apply transactions to ledger state: malformed transaction: invalid network ID - ' +
        "expect 'devnet' found 'qanet'\"}}";

      expect(isBailOutLine(line)).toBe(true);
      expect(parseStateRootMismatch(line)).toBeUndefined();
    });

    /**
     * A main-thread panic exits through the panic hook every binary installs, not
     * through the fatal-exit path, so it needs matching in its own right.
     *
     * @given the line a panicking indexer logs from its panic hook
     * @when the line is read
     * @then it counts as a bail-out
     */
    test('should recognise a panic as a bail-out', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Negative'] };

      const line =
        'chain-indexer-1  | {"level":"ERROR","target":"chain_indexer",' +
        '"message":"process panicked","kvs":{"panic":"called `Option::unwrap()` on a ' +
        '`None` value"}}';

      expect(isBailOutLine(line)).toBe(true);
      expect(parseStateRootMismatch(line)).toBeUndefined();
    });

    /**
     * @given an ordinary block-indexed line
     * @when the line is read
     * @then it is not treated as a bail-out
     */
    test('should not treat a routine log line as a bail-out', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Negative'] };

      const line = 'chain-indexer-1  | {"level":"INFO","message":"block indexed","height":58}';

      expect(isBailOutLine(line)).toBe(false);
      expect(parseStateRootMismatch(line)).toBeUndefined();
    });
  });

  describe('assembling log lines from a chunked stream', () => {
    /**
     * Container output arrives as byte chunks, not lines. If a chunk boundary falls
     * inside a bail-out phrase and each half is treated as a line, neither half matches
     * and the failure report loses the line it exists to surface.
     *
     * @given a fatal log line delivered as two chunks split inside the word "ERROR"
     * @when the chunks are assembled
     * @then one whole line is produced and it is recognised as a bail-out
     */
    test('should rejoin a line split across two chunks', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Negative'] };

      const lines: string[] = [];
      const assembler = createLineAssembler((line) => lines.push(line));
      const full = 'chain-indexer-1  | {"message":"process exited with ERROR"}\n';
      const split = full.indexOf('ERROR') + 2;

      assembler.push(Buffer.from(full.slice(0, split)));
      assembler.push(Buffer.from(full.slice(split)));

      expect(lines).toEqual([full.trimEnd()]);
      expect(isBailOutLine(lines[0])).toBe(true);
    });

    /**
     * @given a chunk boundary falling inside a multi-byte character, and a final line
     *        with no trailing newline
     * @when the chunks are assembled and the remainder flushed
     * @then the character survives intact and the unterminated line is still reported
     */
    test('should preserve a split multi-byte character and flush a trailing line', (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Sync', 'Negative'] };

      const lines: string[] = [];
      const assembler = createLineAssembler((line) => lines.push(line));
      const bytes = Buffer.from('caught up \u2713 done\nno newline here');

      // Split inside the three-byte check mark.
      assembler.push(bytes.subarray(0, 11));
      assembler.push(bytes.subarray(11));
      expect(lines).toEqual(['caught up \u2713 done']);

      assembler.flush();
      expect(lines).toEqual(['caught up \u2713 done', 'no newline here']);
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
          result.failure && `bail-out line:\n${result.failure}`,
          result.logTail.length > 0 && `log tail:\n${result.logTail.join('\n')}`,
        ]
          .filter(Boolean)
          .join('\n');

        // Most specific first, so the message names the divergence and not its symptom.
        // `failure` covers the bail-outs carrying no block height of their own, such as
        // a failed ledger state translation.
        expect(result.mismatch, `state root divergence\n${detail}`).toBeUndefined();
        expect(result.failure, `indexer bailed out\n${detail}`).toBeUndefined();
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
