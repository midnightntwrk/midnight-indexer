// This file is part of midnightntwrk/midnight-indexer
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

// global-setup.ts
import { ToolkitWrapper } from '../utils/toolkit/toolkit-wrapper';

let warmupToolkit: ToolkitWrapper | undefined;

export async function setup() {
  console.log('[SETUP] Warming up toolkit cache (this may take several minutes)...');

  try {
    const startTime = Date.now();

    console.log('[SETUP] Creating warmup toolkit instance...');
    warmupToolkit = new ToolkitWrapper({
      warmupCache: true, // This creates/uses the golden cache
    });

    console.log('[SETUP] Starting toolkit container...');
    await warmupToolkit.start();

    console.log('[SETUP] Syncing cache (please wait, this will take time)...');
    await warmupToolkit.warmupCache();

    const duration = ((Date.now() - startTime) / 1000).toFixed(2);
    console.log(`[SETUP] Toolkit cache warmup complete (${duration}s)`);
  } catch (error) {
    console.error('[SETUP] Failed to warmup toolkit cache:', error);
    throw error;
  } finally {
    if (warmupToolkit) {
      await warmupToolkit.stop();
    }
  }
}

export async function teardown() {
  // no-op
}
