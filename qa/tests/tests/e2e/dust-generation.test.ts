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
import {
  ToolkitWrapper,
  FundWalletResult,
  DustGenerationResult,
  WalletState,
} from '@utils/toolkit/toolkit-wrapper';

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
    // Test data variables
    const destinationSeed = '0000000000000000000000000000000000000000000000000000000000000009';
    const amount = 10000000; // 10 million Night tokens
    const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';

    // Variables to pass between test steps
    let fundingResult: FundWalletResult;
    let walletStateAfterFunding: WalletState;
    let dustGenerationResult: DustGenerationResult;
    let finalWalletState: WalletState;

    /**
     * Step 1: Fund the wallet with Night tokens
     * This creates the initial funding transaction
     */
    beforeAll(async () => {
      log.info(`Step 1: Funding wallet with ${amount} Night tokens...`);
      fundingResult = await toolkit.fundWallet(destinationSeed, amount, sourceSeed);
      log.info(`Funding successful: ${fundingResult.txHash}`);
    }, DUST_GENERATION_TIMEOUT);

    test(
      'should fund wallet with Night tokens',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Funding', 'Step1'],
        };

        // Verify funding was successful
        expect(fundingResult.txHash).toBeDefined();
        expect(fundingResult.walletAddress).toBeDefined();
        expect(fundingResult.amount).toBe(amount);
        expect(fundingResult.status).toBe('confirmed');

        log.info(`Funding verification: ${fundingResult.txHash}`);
        log.info(`Wallet address: ${fundingResult.walletAddress}`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 2: Check wallet state after funding
     * Uses the funding result from Step 1
     */
    beforeAll(async () => {
      log.info(`Step 2: Checking wallet state for seed: ${destinationSeed.substring(0, 8)}...`);
      walletStateAfterFunding = await toolkit.showWallet(destinationSeed);
      log.info(
        `Wallet state after funding: ${walletStateAfterFunding.utxos.length} Night UTXOs, ${walletStateAfterFunding.dust_utxos.length} DUST UTXOs`,
      );
    }, TEST_TIMEOUT);

    test(
      'should show Night UTXOs in wallet state after funding',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'WalletState', 'Step2'],
        };

        // Verify wallet state structure
        expect(walletStateAfterFunding).toBeDefined();
        expect(walletStateAfterFunding.utxos).toBeDefined();
        expect(walletStateAfterFunding.utxos.length).toBeGreaterThan(0);
        expect(walletStateAfterFunding.dust_utxos).toBeDefined();

        log.info(`Wallet state verification:`);
        log.info(`- Night UTXOs: ${walletStateAfterFunding.utxos.length}`);
        log.info(`- DUST UTXOs: ${walletStateAfterFunding.dust_utxos.length}`);
        log.info(`- DUST generation active: ${walletStateAfterFunding.dust_utxos.length > 0}`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 3: Execute complete DUST generation workflow
     * Uses the funding result and wallet state from previous steps
     */
    beforeAll(async () => {
      log.info(`Step 3: Executing complete DUST generation workflow...`);
      dustGenerationResult = await toolkit.generateDust(destinationSeed, amount, sourceSeed);
      log.info(
        `DUST generation result: ${dustGenerationResult.hasDustGeneration ? 'Active' : 'Inactive'}`,
      );
    }, DUST_GENERATION_TIMEOUT);

    test(
      'should complete DUST generation workflow',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Generation', 'Step3'],
        };

        // Verify DUST generation was successful
        expect(dustGenerationResult.walletState).toBeDefined();
        expect(dustGenerationResult.walletState.utxos.length).toBeGreaterThan(0);
        expect(dustGenerationResult.dustUtxoCount).toBeGreaterThanOrEqual(0);
        expect(dustGenerationResult.hasDustGeneration).toBeDefined();

        log.info(`DUST generation verification:`);
        log.info(`- Night UTXOs: ${dustGenerationResult.walletState.utxos.length}`);
        log.info(`- DUST UTXOs: ${dustGenerationResult.dustUtxoCount}`);
        log.info(`- DUST generation active: ${dustGenerationResult.hasDustGeneration}`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 4: Verify final wallet state with DUST data
     * Uses all previous results to verify the complete workflow
     */
    beforeAll(async () => {
      log.info(`Step 4: Verifying final wallet state structure...`);
      finalWalletState = await toolkit.showWallet(destinationSeed);
      log.info(
        `Final wallet state: ${finalWalletState.utxos.length} Night UTXOs, ${finalWalletState.dust_utxos.length} DUST UTXOs`,
      );
    }, TEST_TIMEOUT);

    test(
      'should have proper wallet state structure with DUST data',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Verification', 'Step4'],
        };

        // Verify final wallet state structure
        expect(finalWalletState).toBeDefined();
        expect(finalWalletState.coins).toBeDefined();
        expect(finalWalletState.utxos).toBeDefined();
        expect(finalWalletState.dust_utxos).toBeDefined();
        expect(Array.isArray(finalWalletState.utxos)).toBe(true);
        expect(Array.isArray(finalWalletState.dust_utxos)).toBe(true);

        // Cross-reference with previous steps
        expect(finalWalletState.utxos.length).toBeGreaterThanOrEqual(
          walletStateAfterFunding.utxos.length,
        );
        expect(finalWalletState.dust_utxos.length).toBeGreaterThanOrEqual(
          walletStateAfterFunding.dust_utxos.length,
        );

        log.info(`Final wallet state verification:`);
        log.info(`- Coins: ${Object.keys(finalWalletState.coins).length} types`);
        log.info(`- Night UTXOs: ${finalWalletState.utxos.length}`);
        log.info(`- DUST UTXOs: ${finalWalletState.dust_utxos.length}`);
        log.info(`- DUST generation active: ${finalWalletState.dust_utxos.length > 0}`);

        // Summary of the complete workflow
        log.info(`\n=== DUST Generation Workflow Summary ===`);
        log.info(`1. Funding: ${fundingResult.txHash} (${fundingResult.amount} tokens)`);
        log.info(
          `2. Initial state: ${walletStateAfterFunding.utxos.length} Night UTXOs, ${walletStateAfterFunding.dust_utxos.length} DUST UTXOs`,
        );
        log.info(
          `3. DUST generation: ${dustGenerationResult.hasDustGeneration ? 'Active' : 'Inactive'}`,
        );
        log.info(
          `4. Final state: ${finalWalletState.utxos.length} Night UTXOs, ${finalWalletState.dust_utxos.length} DUST UTXOs`,
        );
      },
      TEST_TIMEOUT,
    );
  });
});
