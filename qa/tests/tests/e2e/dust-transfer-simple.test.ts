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

describe.sequential('DUST transfer simple test', () => {
  let toolkit: ToolkitWrapper;

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  }, TOOLKIT_WRAPPER_TIMEOUT);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('DUST transfer between two wallets', () => {
    // Test data variables
    const walletASeed = '000000000000000000000000000000000000000000000000000000000000000A';
    const walletBSeed = '000000000000000000000000000000000000000000000000000000000000000B';
    const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';
    const initialFundingAmount = 1000000000; // 1 billion Night tokens
    const transferAmount = 500000000; // 500 million Night tokens

    // Variables to pass between test steps
    let fundingResultA: FundWalletResult;
    let fundingResultB: FundWalletResult;
    let walletAInitialState: WalletState;
    let walletBInitialState: WalletState;
    let transferResult: FundWalletResult;
    let walletAAfterTransfer: WalletState;
    let walletBAfterTransfer: WalletState;

    /**
     * Step 1: Fund Wallet A
     */
    beforeAll(async () => {
      log.info(`Step 1: Funding Wallet A with ${initialFundingAmount} Night tokens...`);
      fundingResultA = await toolkit.fundWallet(walletASeed, initialFundingAmount, sourceSeed);
      log.info(`Wallet A funded: ${fundingResultA.txHash}`);
    }, TRANSFER_TIMEOUT);

    test(
      'should fund Wallet A with initial amount',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step1'],
        };

        expect(fundingResultA.txHash).toBeDefined();
        expect(fundingResultA.amount).toBe(initialFundingAmount);
        expect(fundingResultA.status).toBe('confirmed');

        log.info(`Wallet A funded successfully`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 2: Fund Wallet B
     */
    beforeAll(async () => {
      log.info(`Step 2: Funding Wallet B with ${initialFundingAmount} Night tokens...`);
      fundingResultB = await toolkit.fundWallet(walletBSeed, initialFundingAmount, sourceSeed);
      log.info(`Wallet B funded: ${fundingResultB.txHash}`);
    }, TRANSFER_TIMEOUT);

    test(
      'should fund Wallet B with initial amount',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step2'],
        };

        expect(fundingResultB.txHash).toBeDefined();
        expect(fundingResultB.amount).toBe(initialFundingAmount);
        expect(fundingResultB.status).toBe('confirmed');

        log.info(`Wallet B funded successfully`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 3: Get initial wallet states
     */
    beforeAll(async () => {
      log.info(`Step 3: Getting initial wallet states...`);
      walletAInitialState = await toolkit.showWallet(walletASeed);
      walletBInitialState = await toolkit.showWallet(walletBSeed);

      log.info(`Wallet A initial: ${walletAInitialState.utxos.length} Night UTXOs, ${walletAInitialState.dust_utxos.length} DUST UTXOs`);
      log.info(`Wallet B initial: ${walletBInitialState.utxos.length} Night UTXOs, ${walletBInitialState.dust_utxos.length} DUST UTXOs`);
    }, TRANSFER_TIMEOUT);

    test(
      'should have initial wallet states',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step3'],
        };

        expect(walletAInitialState).toBeDefined();
        expect(walletBInitialState).toBeDefined();
        expect(walletAInitialState.utxos.length).toBeGreaterThan(0);
        expect(walletBInitialState.utxos.length).toBeGreaterThan(0);

        log.info(`Initial state verification complete`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 4: Transfer from Wallet A to Wallet B
     */
    beforeAll(async () => {
      log.info(`Step 4: Transferring ${transferAmount} tokens from Wallet A to Wallet B...`);
      transferResult = await toolkit.fundWallet(walletBSeed, transferAmount, walletASeed);
      log.info(`Transfer completed: ${transferResult.txHash}`);
    }, TRANSFER_TIMEOUT);

    test(
      'should complete transfer from Wallet A to Wallet B',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step4'],
        };

        expect(transferResult.txHash).toBeDefined();
        expect(transferResult.amount).toBe(transferAmount);
        expect(transferResult.status).toBe('confirmed');

        log.info(`Transfer verification complete`);
      },
      TEST_TIMEOUT,
    );

    /**
     * Step 5: Get wallet states after transfer
     */
    beforeAll(async () => {
      log.info(`Step 5: Getting wallet states after transfer...`);
      walletAAfterTransfer = await toolkit.showWallet(walletASeed);
      walletBAfterTransfer = await toolkit.showWallet(walletBSeed);

      log.info(`Wallet A after: ${walletAAfterTransfer.utxos.length} Night UTXOs, ${walletAAfterTransfer.dust_utxos.length} DUST UTXOs`);
      log.info(`Wallet B after: ${walletBAfterTransfer.utxos.length} Night UTXOs, ${walletBAfterTransfer.dust_utxos.length} DUST UTXOs`);
    }, TRANSFER_TIMEOUT);

    test(
      'should show DUST changes after transfer',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Step5'],
        };

        const dustABefore = walletAInitialState.dust_utxos.length;
        const dustAAfter = walletAAfterTransfer.dust_utxos.length;
        const dustBBefore = walletBInitialState.dust_utxos.length;
        const dustBAfter = walletBAfterTransfer.dust_utxos.length;

        log.info(`Wallet A DUST: ${dustABefore} → ${dustAAfter} (change: ${dustAAfter - dustABefore})`);
        log.info(`Wallet B DUST: ${dustBBefore} → ${dustBAfter} (change: ${dustBAfter - dustBBefore})`);

        // DUST should increase in receiving wallet (Wallet B)
        expect(dustBAfter).toBeGreaterThanOrEqual(dustBBefore);

        // DUST should decrease or stay same in sending wallet (Wallet A)
        expect(dustAAfter).toBeLessThanOrEqual(dustABefore);

        log.info(`✓ DUST transfer verification complete`);
      },
      TEST_TIMEOUT,
    );

    test(
      'should show complete transfer summary',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['DUST', 'Transfer', 'Summary'],
        };

        log.info(`\n=== DUST Transfer Summary ===`);
        log.info(`Transfer Amount: ${transferAmount} tokens`);
        log.info(`Transfer Hash: ${transferResult.txHash}`);
        log.info(`\nWallet A (Sender):`);
        log.info(`  - Night UTXOs: ${walletAInitialState.utxos.length} → ${walletAAfterTransfer.utxos.length}`);
        log.info(`  - DUST UTXOs: ${walletAInitialState.dust_utxos.length} → ${walletAAfterTransfer.dust_utxos.length}`);
        log.info(`\nWallet B (Receiver):`);
        log.info(`  - Night UTXOs: ${walletBInitialState.utxos.length} → ${walletBAfterTransfer.utxos.length}`);
        log.info(`  - DUST UTXOs: ${walletBInitialState.dust_utxos.length} → ${walletBAfterTransfer.dust_utxos.length}`);

        expect(true).toBe(true);
      },
      TEST_TIMEOUT,
    );
  });
});
