// This file is part of midnightntwrk/midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
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
      path.resolve(__dirname, './utils/logging/setup.ts'),
      path.resolve(__dirname, './setup/global-setup.ts'),
    ],
    coverage: {
      reporter: ['text', 'json', 'html'],
    },
    testTimeout: 15000,
    retry: 1,
    include: ['tests/e2e/**/*.test.ts'],
  },
  resolve: {
    alias: {
      graphql: path.resolve(__dirname, 'node_modules/graphql'),
      '@utils': path.resolve(__dirname, './utils'),
      environment: path.resolve(__dirname, './environment'),
    },
    conditions: ['node'],
    mainFields: ['module', 'main'],
  },
  optimizeDeps: {
    include: ['graphql'],
  },
});
