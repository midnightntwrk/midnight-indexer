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
import XRayJsonReporter from './utils/reporters/custom-xray-json/xray-json-reporter';
import CustomJUnitReporter from './utils/reporters/custom-junit/custom-junit-reporter';

export default defineConfig({
  test: {
    globals: true,
    environment: 'node',
    globalSetup: [path.resolve(__dirname, './utils/logging/setup.ts')],
    setupFiles: [path.resolve(__dirname, './utils/custom-matchers.ts')],
    coverage: {
      reporter: ['text', 'json', 'html'],
    },
    testTimeout: 15000,
    slowTestThreshold: 800,
    retry: 1, // Retry failed tests one extra time just for random glitches
    reporters: [
      'verbose',
      new XRayJsonReporter(),
      new CustomJUnitReporter(),
      [
        'junit',
        {
          outputFile: './reports/junit/test-results.xml',
        },
      ],
      [
        'json',
        {
          outputFile: './reports/json/test-results.json',
        },
      ],
    ],
  },
  resolve: {
    alias: {
      graphql: path.resolve(__dirname, 'node_modules/graphql'),
      '@utils': path.resolve(__dirname, './utils'),
    },
    // This ensures ESM loading doesn't split contexts
    conditions: ['node'],
    mainFields: ['module', 'main'],
  },
  optimizeDeps: {
    include: ['graphql'], // force deduped version
  },
});
