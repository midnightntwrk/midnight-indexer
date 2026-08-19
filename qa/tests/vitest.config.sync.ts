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

import path from 'path';
import { defineConfig } from 'vitest/config';

// Sync test configuration - starts a local indexer against a remote chain.
//
// NOTHING here may throw. The root `vitest.config.ts` lists this file in `test.projects`,
// and Vitest resolves every project config before it applies `--project` filtering, so a
// module-scope throw fails `test:smoke`, `test:integration` and `test:e2e` too - runs
// that never asked for the sync project. An earlier revision rejected unsupported values
// of TARGET_ENV here and broke every undeployed suite, including CI.
//
// Environments this suite cannot serve are rejected from `readSyncOptions()` instead, so
// the error lands on the sync test and only when it actually runs.

// Resolved here, in the main Vitest process, because a worker's stdout is a pipe:
// `process.stdout.isTTY` inside a test is always falsy and would pin the reporter to
// plain mode even in a terminal. `SYNC_PROGRESS` overrides the detection.
//
// Deliberately not block-scanner's `process.env.CI === "true"`: that misses runners
// which set `CI=1`. Testing truthiness alone has the opposite fault, treating an
// explicit `CI=false` as CI, so both spellings of "not CI" are excluded here.
const ci = process.env.CI !== undefined && !['false', '0', ''].includes(process.env.CI);
const progressMode = process.env.SYNC_PROGRESS ?? (ci || !process.stdout.isTTY ? 'plain' : 'live');

export default defineConfig({
  test: {
    name: 'sync',
    globals: true,
    environment: 'node',
    setupFiles: [path.resolve(__dirname, './utils/custom-matchers.ts')],
    // Logging setup only. The e2e global setup warms the toolkit fetch cache, which
    // costs many minutes and is of no use to a sync run.
    globalSetup: [path.resolve(__dirname, './utils/logging/setup.ts')],
    coverage: {
      reporter: ['text', 'json', 'html'],
    },
    // A run is bounded by MAX_DURATION_MS (30 min default) and MAX_BLOCKS, and
    // `MAX_BLOCKS=0` lifts the block bound entirely. The Vitest budget has to sit
    // outside whatever the harness is allowed to take, so it is deliberately generous.
    testTimeout: 8 * 60 * 60 * 1000,
    hookTimeout: 30 * 60 * 1000,
    // Never re-run a sync: an attempt costs minutes to hours, and a second attempt
    // would hide the very instability the suite exists to catch.
    retry: 0,
    // One indexer stack, one set of host ports: the suite must not run beside itself.
    fileParallelism: false,
    maxWorkers: 1,
    // Let the progress reporter own the terminal. With Vitest's interception in place
    // the carriage-return-rewritten line is buffered and reordered instead of redrawn.
    disableConsoleIntercept: true,
    env: {
      SYNC_PROGRESS: progressMode,
    },
    include: ['tests/sync/**/*.test.ts'],
  },
  resolve: {
    alias: {
      graphql: path.resolve(__dirname, 'node_modules/graphql'),
      '@utils': path.resolve(__dirname, './utils'),
      environment: path.resolve(__dirname, './environment'),
      // Bare, root-relative specifiers (tsconfig `baseUrl: "."`). Vitest 3's
      // bundled Vite resolved these implicitly; Vite 7 (vitest 4) does not,
      // so they must be aliased explicitly.
      utils: path.resolve(__dirname, './utils'),
      tests: path.resolve(__dirname, './tests'),
    },
    conditions: ['node'],
    mainFields: ['module', 'main'],
  },
  optimizeDeps: {
    include: ['graphql'],
  },
});
