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
// See the specific language governing permissions and
// limitations under the License.

import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import {
  ToolkitWrapper,
  FundWalletResult,
  WalletState,
} from '@utils/toolkit/toolkit-wrapper';

const TOOLKIT_WRAPPER_TIMEOUT = 60_000; // 1 minute
const TRANSFER_TIMEOUT = 150_000; // 2.5 minutes
const TEST_TIMEOUT = 10_000; // 10 seconds

describe.sequential('DUST transfer test using working pattern', () => {
  let toolkit: ToolkitWrapper;

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  }, TOOLKIT_WRAPPER_TIMEOUT);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('DUST transfer using proven working seeds', () => {
    // Use the same seed that works in dust-generation.test.ts
    const walletASeed = '0000000000000000000000000000000000000000000000000000000000000009';
    const walletBSeed = '000000000000000000000000000000000000000000000000000000000000000A';
    const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';
    const amount = 10000000; // 10 million Night tokens (same as dust-generation test)

    // Variables to pass between test steps
    let fundingResultA: FundWalletResult;
    let walletAState: WalletState;
    let transferResult: FundWalletResult | null = null;
    let transferError: Error | null = null;
    let walletAAfterTransfer: WalletState;
    let walletBAfterTransfer: WalletState;

    /**
     * Step 1: Fund Wallet A (using the working seed)
     */
    beforeAll(async () => {
      log.info(`Step 1: Funding Wallet A with ${amount} Night tokens...`);
      fundingResultA = await toolkit.fundWallet(walletASeed, amount, sourceSeed);
      log.info(`Wallet A funded: ${fundingResultA.txHash}`);
    }, TRANSFER_TIMEOUT);

    test(
      'should fund Wallet A with Night tokens',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step1'],
        };

        expect(fundingResultA.txHash).toBeDefined();
        expect(fundingResultA.amount).toBe(amount);
        expect(fundingResultA.status).toBe('confirmed');

        log.info(`Wallet A funded successfully`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 2: Check Wallet A state
     */
    beforeAll(async () => {
      log.info(`Step 2: Checking Wallet A state...`);
      walletAState = await toolkit.showWallet(walletASeed);
      log.info(`Wallet A state: ${walletAState.utxos.length} Night UTXOs, ${walletAState.dust_utxos.length} DUST UTXOs`);
    }, TEST_TIMEOUT);

    test(
      'should show Wallet A state after funding',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step2'],
        };

        expect(walletAState).toBeDefined();
        expect(walletAState.utxos.length).toBeGreaterThan(0);

        log.info(`Wallet A state verification complete`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 3: Attempt transfer from Wallet A to Wallet B (expecting insufficient DUST error)
     */
    beforeAll(async () => {
      log.info(`Step 3: Attempting transfer of ${amount / 2} tokens from Wallet A to Wallet B...`);
      try {
        transferResult = await toolkit.fundWallet(walletBSeed, amount / 2, walletASeed);
        log.info(`Transfer completed: ${transferResult.txHash}`);
      } catch (error) {
        transferError = error as Error;
        log.info(`Transfer failed as expected: ${transferError.message}`);
      }
    }, TRANSFER_TIMEOUT);

    test(
      'should fail transfer due to insufficient DUST',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step3'],
        };

        expect(transferError).not.toBeNull();
        expect(transferError!.message).toContain('Insufficient DUST');
        expect(transferResult).toBeNull();

        log.info(`✓ Transfer correctly failed due to insufficient DUST`);
        log.info(`Error message: ${transferError!.message}`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 4: Check wallet states after failed transfer attempt
     */
    beforeAll(async () => {
      log.info(`Step 4: Checking wallet states after failed transfer attempt...`);
      walletAAfterTransfer = await toolkit.showWallet(walletASeed);
      walletBAfterTransfer = await toolkit.showWallet(walletBSeed);

      log.info(`Wallet A after: ${walletAAfterTransfer.utxos.length} Night UTXOs, ${walletAAfterTransfer.dust_utxos.length} DUST UTXOs`);
      log.info(`Wallet B after: ${walletBAfterTransfer.utxos.length} Night UTXOs, ${walletBAfterTransfer.dust_utxos.length} DUST UTXOs`);
    }, TEST_TIMEOUT);

    test(
      'should show no DUST changes after failed transfer',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step4'],
        };

        const dustABefore = walletAState.dust_utxos.length;
        const dustAAfter = walletAAfterTransfer.dust_utxos.length;
        const dustBAfter = walletBAfterTransfer.dust_utxos.length;

        log.info(`Wallet A DUST: ${dustABefore} → ${dustAAfter} (change: ${dustAAfter - dustABefore})`);
        log.info(`Wallet B DUST: 0 → ${dustBAfter} (no transfer occurred)`);

        // Since transfer failed, Wallet A should have same DUST
        expect(dustAAfter).toBe(dustABefore);

        // Wallet B should have no UTXOs since transfer failed
        expect(walletBAfterTransfer.utxos.length).toBe(0);
        expect(dustBAfter).toBe(0);

        log.info(`✓ DUST state unchanged after failed transfer (as expected)`);
      },
      TEST_TIMEOUT,
    );

    test(
      'should show complete test summary',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Summary'],
        };

        log.info(`\n=== DUST Transfer Test Summary ===`);
        log.info(`Test Scenario: Insufficient DUST prevents transfer`);
        log.info(`Attempted Transfer Amount: ${amount / 2} tokens`);
        log.info(`Transfer Result: ${transferError ? 'FAILED (as expected)' : 'SUCCESS (unexpected)'}`);
        log.info(`Error: ${transferError?.message || 'No error'}`);
        log.info(`\nWallet A (Sender):`);
        log.info(`  - Night UTXOs: ${walletAState.utxos.length} → ${walletAAfterTransfer.utxos.length}`);
        log.info(`  - DUST UTXOs: ${walletAState.dust_utxos.length} → ${walletAAfterTransfer.dust_utxos.length}`);
        log.info(`  - Status: Unchanged (transfer failed)`);
        log.info(`\nWallet B (Receiver):`);
        log.info(`  - Night UTXOs: 0 → ${walletBAfterTransfer.utxos.length}`);
        log.info(`  - DUST UTXOs: 0 → ${walletBAfterTransfer.dust_utxos.length}`);
        log.info(`  - Status: Empty (no transfer occurred)`);
        log.info(`\n✓ Test demonstrates DUST requirement enforcement`);

        expect(transferError).not.toBeNull();
      },
      TEST_TIMEOUT,
    );
  });
});
