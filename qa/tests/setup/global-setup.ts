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

// global-setup.ts
import fs from 'fs';
import path from 'path';
import { ToolkitWrapper } from '../utils/toolkit/toolkit-wrapper';
import { startCacheProgressReporter, CacheProgressReporter } from '../utils/toolkit/toolkit-cache';
import { env } from '../environment/model';
import dataProvider from '../utils/testdata-provider';
import { warmMothWallet } from '../utils/moth/moth-backend';
import {
  describeProofServer,
  ensureProofServer,
  stopProofServer,
} from '../utils/moth/proof-server';

let warmupToolkit: ToolkitWrapper | undefined;

function cleanupOrphanedToolkitDirs(): void {
  const root = path.resolve('./.tmp/toolkit');
  if (!fs.existsSync(root)) return;
  const skipped: string[] = [];
  for (const entry of fs.readdirSync(root)) {
    const full = path.join(root, entry);
    try {
      fs.rmSync(full, { recursive: true, force: true });
    } catch {
      skipped.push(entry);
    }
  }
  if (skipped.length > 0) {
    console.warn(
      `[SETUP] ${skipped.length} orphaned toolkit dir(s) could not be removed (root-owned files from a previous run): ${skipped.join(', ')}. ` +
        `Run \`sudo rm -rf .tmp/toolkit\` to clear them.`,
    );
  }
}

const PREWARM_RETRY_DELAY_MS = 5_000;
const PREWARM_MAX_ATTEMPTS = 5;

/**
 * Pre-warms the funding seed's wallet state so tests don't replay the full ledger
 * on the first generateSingleTx call.
 *
 * Mirrors the retry logic in ToolkitWrapper.warmupCache: RPC timeouts are transient
 * and worth retrying; all other errors are unexpected and logged prominently but not
 * thrown (pre-warming is an optimisation, not a hard requirement for test execution).
 */
async function prewarmFundingSeed(toolkit: ToolkitWrapper, seed: string): Promise<void> {
  for (let attempt = 1; attempt <= PREWARM_MAX_ATTEMPTS; attempt++) {
    try {
      await toolkit.getDustBalance(seed);
      console.log('[SETUP] Funding seed wallet state cached');
      return;
    } catch (error) {
      const msg = String(error);
      if (msg.toLowerCase().includes('request timeout')) {
        if (attempt < PREWARM_MAX_ATTEMPTS) {
          console.warn(
            `[SETUP] Funding seed pre-warm interrupted by RPC timeout ` +
              `(attempt ${attempt}/${PREWARM_MAX_ATTEMPTS}), retrying in ${PREWARM_RETRY_DELAY_MS / 1_000}s…`,
          );
          await new Promise((res) => setTimeout(res, PREWARM_RETRY_DELAY_MS));
          continue;
        }
      }
      // Non-retriable error or max retries exhausted — warn visibly but don't abort setup.
      // Tests can still run; the first generateSingleTx will just be slower.
      console.warn(
        `[SETUP] Funding seed wallet pre-warm failed after ${attempt} attempt(s) — ` +
          `tests will proceed but first generateSingleTx may be slow.\n` +
          `  Cause: ${msg.slice(0, 300)}`,
      );
      return;
    }
  }
}

/**
 * Report whether the e2e files will run one at a time, and say so loudly when
 * they will not.
 *
 * `vitest.config.e2e.ts` sets `fileParallelism: false`, but an explicit
 * `--file-parallelism` on the command line overrides config. Rather than assume
 * the config won the argument, read back what Vitest actually resolved, so the
 * line printed here is always true.
 */
function reportFileParallelism(project: { config?: { fileParallelism?: boolean } }): void {
  // Undefined means Vitest did not report it; treat that as parallel, which is
  // its default, so an unknown state warns rather than reassures.
  const sequential = project?.config?.fileParallelism === false;

  if (sequential) {
    console.log(
      '[SETUP] The e2e tests will be executed in sequence, back to back! ' +
        'Parallel execution is not supported yet — every suite spends from the same ' +
        'funding wallet, so concurrent runs fight over the same unspent outputs.',
    );
    return;
  }

  console.warn(
    '[SETUP] WARNING: the e2e tests are running with file parallelism ENABLED, which ' +
      'is not supported yet. Suites share one funding wallet, so concurrent transfers ' +
      'can leave one another unconfirmed and whole suites will skip instead of failing. ' +
      'Drop `--file-parallelism` to restore the supported sequential run.',
  );
}

export async function setup(project: { config?: { fileParallelism?: boolean } }) {
  cleanupOrphanedToolkitDirs();
  reportFileParallelism(project);

  // On the moth backend, skip the toolkit cache warm-up entirely.
  //
  // That warm-up fetches every block of the target chain into the shared
  // toolkit Postgres. On preview (~905k blocks) it is hours of work, and the
  // whole reason for the moth backend is that this cost grows with the chain
  // and makes long-lived environments untestable. Paying it anyway would
  // cancel the benefit out.
  //
  // A few suites still reach for the toolkit for things moth cannot do yet
  // (notably `show-viewing-key`). They keep working — they just pay their own
  // fetch cost on first call instead of having it pre-paid here.
  if (env.getTxBackend() === 'moth') {
    console.log(
      '[SETUP] TX_BACKEND=moth — skipping the toolkit cache warm-up. ' +
        'Any test that still calls the toolkit will pay its own fetch cost on first use.',
    );
    await warmMoth();
    return;
  }

  console.log('[SETUP] Warming up toolkit cache (this may take several minutes)...');

  let reporter: CacheProgressReporter | undefined;
  try {
    const startTime = Date.now();

    console.log('[SETUP] Creating warmup toolkit instance...');
    warmupToolkit = new ToolkitWrapper({});

    console.log('[SETUP] Starting toolkit container...');
    await warmupToolkit.start();

    // The node's HTTP RPC URL lets the reporter show a live percentage
    // (e.g. "fetch progress: 39,485/715,051 (5.5%) blocks complete").
    const nodeRpcUrl = env.getNodeHttpBaseURL();
    reporter = startCacheProgressReporter(process.env.TARGET_ENV ?? 'cache', nodeRpcUrl);

    console.log('[SETUP] Syncing cache (please wait, this will take time)...');
    await warmupToolkit.warmupCache();

    const fundingSeed = dataProvider.getFundingSeed();
    if (fundingSeed) {
      console.log('[SETUP] Pre-warming funding seed wallet state...');
      await prewarmFundingSeed(warmupToolkit, fundingSeed);
    }

    const duration = ((Date.now() - startTime) / 1000).toFixed(2);
    console.log(`[SETUP] Toolkit cache warmup complete (${duration}s)`);
  } catch (error) {
    console.error('[SETUP] Failed to warmup toolkit cache:', error);
    throw error;
  } finally {
    reporter?.stop();
    if (warmupToolkit) {
      await warmupToolkit.stop();
    }
  }
}

/**
 * Warm moth's on-disk wallet cache.
 *
 * Test workers are separate processes and cannot inherit a synced facade; what
 * they inherit is this cache, which turns their cold sync into a short restore.
 * Global setup has no test timeout to burn, so a first sync belongs here.
 */
async function warmMoth(): Promise<void> {
  // The proof server is a plain service, so it is started ONCE here and stopped
  // in teardown, rather than being negotiated by every worker.
  //
  // Publishing the URL into the environment is what makes that work: test
  // workers are forked after global setup and inherit `process.env`, so their
  // `ensureProofServer()` takes the PROOF_SERVER_URL short-circuit and neither
  // inspects docker nor starts anything. It also puts ownership in one place —
  // previously `startedByUs` was per-process, so a container started here could
  // never be stopped by a worker, and teardown did nothing: the container
  // leaked after every run.
  //
  // A PROOF_SERVER_URL the caller set is left exactly as it is, and is not
  // stopped in teardown, because we did not start it.
  //
  // moth builds its proving service during startWalletSync, so even a sync-only
  // warm-up needs a reachable server.
  const proofServerUrl = await ensureProofServer();
  // Vitest skips teardown when setup throws, so a failed warm-up (network drop,
  // sync timeout, ...) has to stop the proof server itself or it leaks.
  try {
    process.env.PROOF_SERVER_URL = proofServerUrl;
    console.log(`[SETUP] Proof server: ${await describeProofServer(proofServerUrl)}`);
    const mothSeed = dataProvider.getFundingSeed();
    console.log('[SETUP] Warming moth wallet cache (first sync can take a while)...');
    const mothStart = Date.now();
    await warmMothWallet(mothSeed);
    console.log(
      `[SETUP] moth wallet cache warm (${((Date.now() - mothStart) / 1000).toFixed(2)}s)`,
    );
  } catch (error) {
    await stopProofServer();
    throw error;
  }
}

export async function teardown() {
  // Stops the proof server only if global setup started it; a server supplied
  // through PROOF_SERVER_URL is left running.
  await stopProofServer();
}
