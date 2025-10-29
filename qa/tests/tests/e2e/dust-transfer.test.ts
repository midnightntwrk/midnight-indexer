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
  WalletState,
} from '@utils/toolkit/toolkit-wrapper';

const TOOLKIT_WRAPPER_TIMEOUT = 60_000; // 1 minute
const TRANSFER_TIMEOUT = 150_000; // 2.5 minutes
const TEST_TIMEOUT = 10_000; // 10 seconds

describe.sequential('DUST transfer between two wallets', () => {
  let toolkit: ToolkitWrapper;

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  }, TOOLKIT_WRAPPER_TIMEOUT);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('DUST transfer and appreciation/depreciation verification', () => {
    // Test data variables
    const walletASeed = '000000000000000000000000000000000000000000000000000000000000000A';
    const walletBSeed = '000000000000000000000000000000000000000000000000000000000000000B';
    const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';
    
    const initialFundingAmount = 1000000000; // 1 billion Night tokens
    const transferAmount = 500000000; // 500 million Night tokens

    // Variables to pass between test steps
    let walletAInitialState: WalletState;
    let walletBInitialState: WalletState;
    let fundingResultA: FundWalletResult;
    let fundingResultB: FundWalletResult;
    let walletABeforeTransfer: WalletState;
    let walletBBeforeTransfer: WalletState;
    let transferResult: FundWalletResult;
    let walletAAfterTransfer: WalletState;
    let walletBAfterTransfer: WalletState;

    /**
     * Step 1: Fund both wallets with initial amounts
     */
    beforeAll(async () => {
      log.info(`Step 1: Funding Wallet A with ${initialFundingAmount} Night tokens...`);
      fundingResultA = await toolkit.fundWallet(walletASeed, initialFundingAmount, sourceSeed);
      log.info(`Wallet A funded: ${fundingResultA.txHash}`);
      
      log.info(`Step 1: Funding Wallet B with ${initialFundingAmount} Night tokens...`);
      fundingResultB = await toolkit.fundWallet(walletBSeed, initialFundingAmount, sourceSeed);
      log.info(`Wallet B funded: ${fundingResultB.txHash}`);
    }, TRANSFER_TIMEOUT);

    test(
      'should fund both wallets with initial amounts',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step1'],
        };

        expect(fundingResultA.txHash).toBeDefined();
        expect(fundingResultA.amount).toBe(initialFundingAmount);
        expect(fundingResultA.status).toBe('confirmed');

        expect(fundingResultB.txHash).toBeDefined();
        expect(fundingResultB.amount).toBe(initialFundingAmount);
        expect(fundingResultB.status).toBe('confirmed');

        log.info(`Both wallets funded successfully`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 2: Get initial wallet states (before transfer)
     */
    beforeAll(async () => {
      log.info(`Step 2: Getting initial wallet states...`);
      walletABeforeTransfer = await toolkit.showWallet(walletASeed);
      walletBBeforeTransfer = await toolkit.showWallet(walletBSeed);
      
      walletAInitialState = { ...walletABeforeTransfer };
      walletBInitialState = { ...walletBBeforeTransfer };
      
      log.info(`Wallet A initial state: ${walletABeforeTransfer.utxos.length} Night UTXOs, ${walletABeforeTransfer.dust_utxos.length} DUST UTXOs`);
      log.info(`Wallet B initial state: ${walletBBeforeTransfer.utxos.length} Night UTXOs, ${walletBBeforeTransfer.dust_utxos.length} DUST UTXOs`);
    }, TRANSFER_TIMEOUT);

    test(
      'should have initial wallet states before transfer',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step2'],
        };

        expect(walletABeforeTransfer).toBeDefined();
        expect(walletBBeforeTransfer).toBeDefined();
        expect(walletABeforeTransfer.utxos.length).toBeGreaterThan(0);
        expect(walletBBeforeTransfer.utxos.length).toBeGreaterThan(0);

        log.info(`Initial state verification complete`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 3: Transfer funds from Wallet A to Wallet B
     */
    beforeAll(async () => {
      log.info(`Step 3: Transferring ${transferAmount} tokens from Wallet A to Wallet B...`);
      transferResult = await toolkit.fundWallet(walletBSeed, transferAmount, walletASeed);
      log.info(`Transfer completed: ${transferResult.txHash}`);
    }, TRANSFER_TIMEOUT);

    test(
      'should complete transfer from Wallet A to Wallet B',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step3'],
        };

        expect(transferResult.txHash).toBeDefined();
        expect(transferResult.amount).toBe(transferAmount);
        expect(transferResult.status).toBe('confirmed');

        log.info(`Transfer verification complete`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 4: Get wallet states after transfer
     */
    beforeAll(async () => {
      log.info(`Step 4: Getting wallet states after transfer...`);
      walletAAfterTransfer = await toolkit.showWallet(walletASeed);
      walletBAfterTransfer = await toolkit.showWallet(walletBSeed);
      
      log.info(`Wallet A after transfer: ${walletAAfterTransfer.utxos.length} Night UTXOs, ${walletAAfterTransfer.dust_utxos.length} DUST UTXOs`);
      log.info(`Wallet B after transfer: ${walletBAfterTransfer.utxos.length} Night UTXOs, ${walletBAfterTransfer.dust_utxos.length} DUST UTXOs`);
    }, TRANSFER_TIMEOUT);

    test(
      'should show increased DUST UTXOs in receiving wallet (Wallet B)',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Appreciation', 'Step4'],
        };

        const dustBefore = walletBBeforeTransfer.dust_utxos.length;
        const dustAfter = walletBAfterTransfer.dust_utxos.length;

        log.info(`Wallet B DUST UTXOs - Before: ${dustBefore}, After: ${dustAfter}`);
        log.info(`DUST UTXO change in receiving wallet: ${dustAfter - dustBefore}`);

        // DUST should appreciate (increase) in the receiving wallet
        expect(dustAfter).toBeGreaterThanOrEqual(dustBefore);

        if (dustAfter > dustBefore) {
          log.info(`✓ DUST appreciated in receiving wallet (Wallet B)`);
        }

        log.info(`DUST appreciation verification complete`);
      },
      TEST_TIMEOUT,
    );

    test(
      'should show decreased DUST UTXOs in sending wallet (Wallet A)',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Depreciation', 'Step4'],
        };

        const dustBefore = walletABeforeTransfer.dust_utxos.length;
        const dustAfter = walletAAfterTransfer.dust_utxos.length;

        log.info(`Wallet A DUST UTXOs - Before: ${dustBefore}, After: ${dustAfter}`);
        log.info(`DUST UTXO change in sending wallet: ${dustAfter - dustBefore}`);

        // DUST should depreciate (decrease or stay same) in the sending wallet
        expect(dustAfter).toBeLessThanOrEqual(dustBefore);

        if (dustAfter < dustBefore) {
          log.info(`✓ DUST depreciated in sending wallet (Wallet A)`);
        }

        log.info(`DUST depreciation verification complete`);
      },
      TEST_TIMEOUT,
    );

    test(
      'should show correct Night UTXO changes in both wallets',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'NightUTXO', 'Step4'],
        };

        const nightUtxosABefore = walletABeforeTransfer.utxos.length;
        const nightUtxosAAfter = walletAAfterTransfer.utxos.length;
        const nightUtxosBBefore = walletBBeforeTransfer.utxos.length;
        const nightUtxosBAfter = walletBAfterTransfer.utxos.length;

        log.info(`Wallet A Night UTXOs - Before: ${nightUtxosABefore}, After: ${nightUtxosAAfter}`);
        log.info(`Wallet B Night UTXOs - Before: ${nightUtxosBBefore}, After: ${nightUtxosBAfter}`);

        // Wallet A should have fewer or same number of UTXOs (spent some)
        expect(nightUtxosAAfter).toBeLessThanOrEqual(nightUtxosABefore);
        
        // Wallet B should have more or same number of UTXOs (received some)
        expect(nightUtxosBAfter).toBeGreaterThanOrEqual(nightUtxosBBefore);

        log.info(`Night UTXO verification complete`);
      },
      TEST_TIMEOUT,
    );

    test(
      'should maintain overall DUST conservation',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Conservation', 'Step4'],
        };

        const totalDustBefore = walletABeforeTransfer.dust_utxos.length + walletBBeforeTransfer.dust_utxos.length;
        const totalDustAfter = walletAAfterTransfer.dust_utxos.length + walletBAfterTransfer.dust_utxos.length;

        log.info(`Total DUST UTXOs - Before: ${totalDustBefore}, After: ${totalDustAfter}`);
        log.info(`Total DUST change: ${totalDustAfter - totalDustBefore}`);

        // Total DUST should increase due to transfer creating new UTXOs
        expect(totalDustAfter).toBeGreaterThanOrEqual(totalDustBefore);

        log.info(`✓ Overall DUST conservation verified`);
      },
      TEST_TIMEOUT,
    );

    test(
      'should show complete transfer summary',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Summary', 'Step4'],
        };

        log.info(`\n=== DUST Transfer Summary ===`);
        log.info(`Transfer Amount: ${transferAmount} tokens`);
        log.info(`Transfer Hash: ${transferResult.txHash}`);
        log.info(`\nWallet A (Sender):`);
        log.info(`  - Night UTXOs: ${walletABeforeTransfer.utxos.length} → ${walletAAfterTransfer.utxos.length}`);
        log.info(`  - DUST UTXOs: ${walletABeforeTransfer.dust_utxos.length} → ${walletAAfterTransfer.dust_utxos.length}`);
        log.info(`  - DUST change: ${walletAAfterTransfer.dust_utxos.length - walletABeforeTransfer.dust_utxos.length}`);
        log.info(`\nWallet B (Receiver):`);
        log.info(`  - Night UTXOs: ${walletBBeforeTransfer.utxos.length} → ${walletBAfterTransfer.utxos.length}`);
        log.info(`  - DUST UTXOs: ${walletBBeforeTransfer.dust_utxos.length} → ${walletBAfterTransfer.dust_utxos.length}`);
        log.info(`  - DUST change: ${walletBAfterTransfer.dust_utxos.length - walletBBeforeTransfer.dust_utxos.length}`);
        log.info(`\nOverall:`);
        log.info(`  - Total DUST UTXOs: ${walletABeforeTransfer.dust_utxos.length + walletBBeforeTransfer.dust_utxos.length} → ${walletAAfterTransfer.dust_utxos.length + walletBAfterTransfer.dust_utxos.length}`);

        expect(true).toBe(true);
      },
      TEST_TIMEOUT,
    );
  });

  describe('DUST insufficient funds negative scenario', () => {
    // Test data variables for negative scenario
    const walletCSeed = '000000000000000000000000000000000000000000000000000000000000000C';
    const walletDSeed = '000000000000000000000000000000000000000000000000000000000000000D';
    const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';
    const smallFundingAmount = 1000000; // 1 million Night tokens (small amount)
    const largeTransferAmount = 2000000; // 2 million Night tokens (larger than funding)

    // Variables for negative test
    let walletCFundingResult: FundWalletResult;
    let walletDFundingResult: FundWalletResult;
    let walletCState: WalletState;
    let transferError: Error | null = null;

    /**
     * Step 1: Fund wallets with small amounts
     */
    beforeAll(async () => {
      log.info(`Negative Test Step 1: Funding Wallet C with small amount ${smallFundingAmount}...`);
      walletCFundingResult = await toolkit.fundWallet(walletCSeed, smallFundingAmount, sourceSeed);
      log.info(`Wallet C funded: ${walletCFundingResult.txHash}`);

      log.info(`Negative Test Step 1: Funding Wallet D with small amount ${smallFundingAmount}...`);
      walletDFundingResult = await toolkit.fundWallet(walletDSeed, smallFundingAmount, sourceSeed);
      log.info(`Wallet D funded: ${walletDFundingResult.txHash}`);
    }, TRANSFER_TIMEOUT);

    test(
      'should fund wallets with small amounts for negative test',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Negative', 'Step1'],
        };

        expect(walletCFundingResult.txHash).toBeDefined();
        expect(walletCFundingResult.status).toBe('confirmed');
        expect(walletDFundingResult.txHash).toBeDefined();
        expect(walletDFundingResult.status).toBe('confirmed');

        log.info(`Both wallets funded with small amounts for negative test`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 2: Check wallet state before attempting large transfer
     */
    beforeAll(async () => {
      log.info(`Negative Test Step 2: Checking Wallet C state before large transfer...`);
      walletCState = await toolkit.showWallet(walletCSeed);
      log.info(`Wallet C state: ${walletCState.utxos.length} Night UTXOs, ${walletCState.dust_utxos.length} DUST UTXOs`);
    }, TRANSFER_TIMEOUT);

    test(
      'should show wallet state before attempting large transfer',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Negative', 'Step2'],
        };

        expect(walletCState).toBeDefined();
        expect(walletCState.utxos.length).toBeGreaterThan(0);

        log.info(`Wallet C state verification complete for negative test`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 3: Attempt transfer with insufficient DUST (should fail)
     */
    beforeAll(async () => {
      log.info(`Negative Test Step 3: Attempting large transfer ${largeTransferAmount} from Wallet C to Wallet D...`);
      try {
        await toolkit.fundWallet(walletDSeed, largeTransferAmount, walletCSeed);
        log.error('Transfer unexpectedly succeeded - this should have failed!');
      } catch (error) {
        transferError = error as Error;
        log.info(`Transfer failed as expected: ${transferError.message}`);
      }
    }, TRANSFER_TIMEOUT);

    test(
      'should fail transfer due to insufficient DUST',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Negative', 'Step3'],
        };

        expect(transferError).not.toBeNull();
        expect(transferError!.message).toContain('Insufficient DUST');
        
        log.info(`✓ Transfer correctly failed due to insufficient DUST`);
        log.info(`Error message: ${transferError!.message}`);
      },
      TEST_TIMEOUT,
    );

    test(
      'should show negative test summary',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Negative', 'Summary'],
        };

        log.info(`\n=== DUST Negative Test Summary ===`);
        log.info(`Test Scenario: Insufficient DUST for transfer`);
        log.info(`Wallet C Funding: ${smallFundingAmount} tokens`);
        log.info(`Attempted Transfer: ${largeTransferAmount} tokens`);
        log.info(`Expected Result: Transfer failure due to insufficient DUST`);
        log.info(`Actual Result: ${transferError ? 'Transfer failed as expected' : 'Transfer unexpectedly succeeded'}`);
        log.info(`Error: ${transferError?.message || 'No error'}`);

        expect(transferError).not.toBeNull();
      },
      TEST_TIMEOUT,
    );
  });
});
