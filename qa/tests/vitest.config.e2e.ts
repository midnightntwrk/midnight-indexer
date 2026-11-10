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

// E2E test configuration - includes toolkit cache warmup
export default defineConfig({
  test: {
    name: 'e2e',
    globals: true,
    environment: 'node',
    setupFiles: [path.resolve(__dirname, './utils/custom-matchers.ts')],
    globalSetup: [
      path.resolve(__dirname, './setup/undeployed-genesis-setup.ts'),
      path.resolve(__dirname, './utils/logging/setup.ts'),
      path.resolve(__dirname, './setup/global-setup.ts'),
    ],
    coverage: {
      reporter: ['text', 'json', 'html'],
    },
    testTimeout: 15000,
    retry: 1,
    // Run the e2e files one after another, never side by side.
    //
    // WHY. The e2e suites all spend from the same funding wallet, and under
    // TX_BACKEND=moth each worker runs its own wallet sync engine over the same
    // on-disk cache. Two workers then pick from the same unspent outputs at the
    // same time, and one of the two transfers is left unconfirmed — which
    // surfaces as a suite that skips every test rather than as a clear failure.
    //
    // This is the config equivalent of `--no-file-parallelism`, set here so that
    // a run is correct by default instead of depending on the caller remembering
    // the flag. It is a stopgap: supporting parallel runs properly means giving
    // each suite its own wallet, not serialising them.
    //
    // An explicit `--file-parallelism` on the command line still overrides this,
    // so global setup reports which mode is actually in effect.
    fileParallelism: false,
    include: ['tests/e2e/**/*.test.ts'],
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
