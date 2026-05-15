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

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export type Flavour = 'cloud' | 'standalone';

export interface UndeployedEnvironmentOptions {
  withData: boolean;
  flavour?: Flavour;
}

const READY_URL = 'http://localhost:8088/ready';
const HEALTH_CHECK_ATTEMPTS = 30;
const HEALTH_CHECK_INTERVAL_MS = 2000;

/**
 * Provisions the local `undeployed` docker stack from within the test framework.
 *
 * Wraps the existing bash scripts in `qa/scripts/`:
 *   - `withData: true`  → `startup-localenv-with-data.sh` (integration tests)
 *   - `withData: false` → `startup-localenv-from-genesis.sh` (smoke + e2e tests)
 *
 * Clash safety: if the indexer is already reachable on startup, we treat this
 * as a manually-managed environment — we skip provisioning AND we skip teardown.
 * We never stop a stack we did not start.
 */
export class UndeployedEnvironmentManager {
  private readonly withData: boolean;
  private readonly flavour: Flavour;
  private startedByUs = false;

  constructor(options: UndeployedEnvironmentOptions) {
    this.withData = options.withData;
    this.flavour = options.flavour ?? 'cloud';
    if (this.flavour === 'standalone') {
      // TODO: phase 2 — the existing scripts hardcode `--profile cloud`. Wiring
      // standalone requires either a script modification or a Node-side docker
      // compose invocation that accepts the profile inline.
      throw new Error(
        'FLAVOUR=standalone is not yet supported by the test framework provisioner. ' +
          'Use FLAVOUR=cloud (or omit) for now.',
      );
    }
  }

  /**
   * Ensure the undeployed stack is up. If a stack is already running, do not
   * re-provision and remember not to tear down on exit.
   */
  async ensureRunning(): Promise<void> {
    if (await this.isIndexerReady()) {
      console.log(
        '[undeployed] Existing indexer detected on ' +
          READY_URL +
          ' — skipping provisioning and teardown (manually-managed stack).',
      );
      this.startedByUs = false;
      return;
    }

    this.assertRequiredEnvVars();

    const repoRoot = this.resolveRepoRoot();
    const scriptName = this.withData
      ? 'qa/scripts/startup-localenv-with-data.sh'
      : 'qa/scripts/startup-localenv-from-genesis.sh';

    console.log(`[undeployed] Provisioning stack via ${scriptName} (cwd=${repoRoot})`);
    const result = spawnSync('bash', [scriptName], {
      cwd: repoRoot,
      stdio: 'inherit',
      env: process.env,
    });

    // Mark startedByUs eagerly: the script may have created containers even on
    // failure (e.g. partial bring-up due to a port conflict). We want teardown
    // to clean those up rather than leave them orphaned.
    this.startedByUs = true;

    if (result.status !== 0) {
      throw new Error(
        `[undeployed] Provisioning script ${scriptName} exited with status ${result.status}`,
      );
    }

    await this.waitForIndexerReady();
    console.log('[undeployed] Stack is up and reachable.');
  }

  /**
   * Tear down the stack — only if we started it.
   */
  async teardown(): Promise<void> {
    if (!this.startedByUs) {
      console.log('[undeployed] Skipping teardown — stack was not started by this process.');
      return;
    }

    const repoRoot = this.resolveRepoRoot();
    console.log('[undeployed] Tearing down stack (docker compose down)...');
    const result = spawnSync('docker', ['compose', '--profile', this.flavour, 'down'], {
      cwd: repoRoot,
      stdio: 'inherit',
      env: process.env,
    });

    if (result.status !== 0) {
      // Log but do not throw — teardown is best-effort. A failing teardown
      // should not mask test results.
      console.warn(
        `[undeployed] docker compose down exited with status ${result.status}. ` +
          'Inspect docker state manually.',
      );
    }
  }

  private assertRequiredEnvVars(): void {
    const missing: string[] = [];
    if (!process.env.NODE_TAG) missing.push('NODE_TAG');
    if (!process.env.INDEXER_TAG) missing.push('INDEXER_TAG');
    if (missing.length > 0) {
      throw new Error(
        `[undeployed] Cannot provision stack — missing required env vars: ${missing.join(', ')}. ` +
          'Set NODE_TAG and INDEXER_TAG explicitly when TARGET_ENV=undeployed.',
      );
    }
  }

  private resolveRepoRoot(): string {
    // This file lives at qa/tests/utils/undeployed/environment-manager.ts —
    // four levels up reaches the repo root.
    const here = path.dirname(fileURLToPath(import.meta.url));
    return path.resolve(here, '..', '..', '..', '..');
  }

  private async isIndexerReady(): Promise<boolean> {
    try {
      const response = await fetch(READY_URL, {
        signal: AbortSignal.timeout(1500),
      });
      return response.ok;
    } catch {
      return false;
    }
  }

  private async waitForIndexerReady(): Promise<void> {
    for (let attempt = 1; attempt <= HEALTH_CHECK_ATTEMPTS; attempt++) {
      if (await this.isIndexerReady()) return;
      if (attempt < HEALTH_CHECK_ATTEMPTS) {
        await new Promise((resolve) => setTimeout(resolve, HEALTH_CHECK_INTERVAL_MS));
      }
    }
    throw new Error(
      `[undeployed] Indexer did not become ready on ${READY_URL} after ` +
        `${HEALTH_CHECK_ATTEMPTS} attempts (${HEALTH_CHECK_ATTEMPTS * HEALTH_CHECK_INTERVAL_MS}ms).`,
    );
  }
}
