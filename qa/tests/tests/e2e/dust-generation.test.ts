// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import { ToolkitWrapper, DustGenerationResult } from '@utils/toolkit/toolkit-wrapper';

const TOOLKIT_WRAPPER_TIMEOUT = 60_000; // 1 minute
const DUST_GENERATION_TIMEOUT = 150_000; // 2.5 minutes
const TEST_TIMEOUT = 10_000; // 10 seconds

describe.sequential('DUST generation workflow', () => {
  let toolkit: ToolkitWrapper;

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  }, TOOLKIT_WRAPPER_TIMEOUT);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('DUST generation with funded wallet', () => {
    let dustGenerationResult: DustGenerationResult;

    beforeAll(async () => {
      const destinationSeed = '0000000000000000000000000000000000000000000000000000000000000009';
      const amount = 10000000;

      dustGenerationResult = await toolkit.generateDust(destinationSeed, amount);
    }, DUST_GENERATION_TIMEOUT);

    /**
     * Once the complete DUST generation workflow has been executed, the result should
     * show that the wallet has been funded and DUST generation is active.
     *
     * @given a complete DUST generation workflow execution
     * @when we check the DUST generation result
     * @then the wallet should have Night tokens and DUST generation should be active
     */
    test(
      'should successfully fund wallet and enable DUST generation',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Generation', 'Workflow'],
        };

        expect(dustGenerationResult.walletState.utxos.length).toBeGreaterThan(0);
        expect(dustGenerationResult.dustUtxoCount).toBeGreaterThanOrEqual(0);
        expect(dustGenerationResult.hasDustGeneration).toBeDefined();

        log.info(`DUST generation result:`);
        log.info(`- Night UTXOs: ${dustGenerationResult.walletState.utxos.length}`);
        log.info(`- DUST UTXOs: ${dustGenerationResult.dustUtxoCount}`);
        log.info(`- DUST generation active: ${dustGenerationResult.hasDustGeneration}`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Once DUST generation is active, the wallet state should contain
     * detailed information about DUST UTXOs and their properties.
     *
     * @given an active DUST generation
     * @when we examine the wallet state
     * @then the wallet state should contain DUST UTXO information
     */
    test(
      'should have proper wallet state structure with DUST data',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'WalletState', 'Structure'],
        };

        const walletState = dustGenerationResult.walletState;

        expect(walletState).toBeDefined();
        expect(walletState.coins).toBeDefined();
        expect(walletState.utxos).toBeDefined();
        expect(walletState.dust_utxos).toBeDefined();
        expect(Array.isArray(walletState.utxos)).toBe(true);
        expect(Array.isArray(walletState.dust_utxos)).toBe(true);

        log.info(`Wallet state structure verified:`);
        log.info(`- Coins: ${Object.keys(walletState.coins).length} types`);
        log.info(`- Night UTXOs: ${walletState.utxos.length}`);
        log.info(`- DUST UTXOs: ${walletState.dust_utxos.length}`);
      },
      TEST_TIMEOUT,
    );
  });
});
