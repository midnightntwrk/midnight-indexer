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

import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import { env } from '../../environment/model';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { ToolkitWrapper, type ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';

import type { Transaction } from '@utils/indexer/indexer-types';
import { getBlockByHashWithRetry } from './test-utils';
import { TestContext } from 'vitest';

describe('shielded transactions', () => {
  let toolkit: ToolkitWrapper;
  let transactionResult: ToolkitTransactionResult;

  // Deterministic seeds (hex) that work with the toolkit
  const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';
  const destinationSeed = '0000000000000000000000000000000000000000000000000000000987654321';

  let sourceAddress: string;
  let destinationAddress: string;

  beforeAll(async () => {
    // Start a one-off toolkit container
    toolkit = new ToolkitWrapper({});

    await toolkit.start();

    // Derive shielded addresses from seeds
    sourceAddress = await toolkit.showAddress(sourceSeed, 'shielded');
    destinationAddress = await toolkit.showAddress(destinationSeed, 'shielded');

    // Submit one shielded->shielded transfer (1 NIGHT)
    transactionResult = await toolkit.generateSingleTx(
      sourceSeed,
      'shielded',
      destinationAddress,
      1,
    );

    // Print the TX hashes from toolkit
    const summary = {
      txHash: transactionResult.txHash,
      blockHash: transactionResult.blockHash,
      status: transactionResult.status,
    };
    log.info(`\nTX hashes from toolkit: ${JSON.stringify(summary, null, 2)} \n`);
  }, 120_000);

  afterAll(async () => {
    await Promise.all([toolkit.stop()]);
  });

  describe('a successful shielded transaction transferring 1 Token between two wallets', async () => {
    /**
     * Once a shielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction in the block through a block query by hash, using the block hash reported by the toolkit.
     *
     * @given a confirmed shielded transaction between two wallets
     * @when we query the indexer with a block query by hash, using the block hash reported by the toolkit
     * @then the block should contain the expected transaction
     */
    test('should be reported by the indexer through a block query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash', 'ShieldedTokens'],
        testKey: 'PM-17709',
      };

      context.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // The expected block might take a bit more to show up by indexer, so we retry a few times
      const blockResponse = await getBlockByHashWithRetry(transactionResult.blockHash!);

      // Verify the transaction appears in the block but as it's shielded, we can't see the details
      expect(blockResponse).toBeSuccess();
      expect(blockResponse?.data?.block?.transactions).toBeDefined();
      expect(blockResponse?.data?.block?.transactions?.length).toBeGreaterThan(0);
    });

    /**
     * Once a shielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through a query by transaction hash, using the transaction hash reported by the toolkit.
     *
     * @given a confirmed shielded transaction between two wallets
     * @when we query transactions by the transaction hash
     * @then the indexer should return the expected transaction
     */
    test('should be reported by the indexer through a transaction query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Transaction', 'ByHash', 'ShieldedTokens'],
        testKey: 'PM-17710',
      };

      context.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // The expected transaction might take a bit more to show up by indexer, so we retry a few times
      const transactionResponse = await new IndexerHttpClient().getShieldedTransaction({
        hash: transactionResult.txHash,
      });

      expect(transactionResponse).toBeSuccess();
      expect(transactionResponse?.data?.transactions).toBeDefined();
      expect(transactionResponse?.data?.transactions?.length).toBeGreaterThan(0);
      expect(transactionResponse?.data?.transactions?.map((tx: Transaction) => tx.hash)).toContain(
        transactionResult.txHash,
      );
    });
  });
});
