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

// Unit test configuration - pure helpers under `utils/`, nothing else.
//
// No global setup and no setup files, on purpose: every other project boots
// Docker or reaches a live indexer before the first test runs, and this one
// exists precisely so that the parts of the harness which need neither (the
// retry helper, the websocket client's liveness logic, ...) can be checked on
// every change, in milliseconds, with no TARGET_ENV and no network.
//
// A test in this project that needs the environment is in the wrong project.
// Modules that read the environment at import time (`environment/model`) and
// the logger (which creates a log directory on import) are mocked with
// `vi.mock` from the test file itself.
export default defineConfig({
  test: {
    name: 'unit',
    globals: true,
    environment: 'node',
    testTimeout: 10000,
    // Never re-run: a flaky unit test is a bug, and a retry would hide it.
    retry: 0,
    include: ['tests/unit/**/*.test.ts'],
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
