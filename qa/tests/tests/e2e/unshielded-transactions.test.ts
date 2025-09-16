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

import log from '@utils/logging/logger';
import { env } from '../../environment/model';
import '@utils/logging/test-logging-hooks';
import { TestContext } from 'vitest';
import { hash, randomBytes } from 'crypto';

import { ToolkitWrapper, ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { BlockResponse } from '@utils/indexer/indexer-types';

function retry<T>(
  fn: () => Promise<T>,
  condition: (result: T) => boolean,
  maxAttempts: number,
  delay: number,
): Promise<T> {
  return new Promise((resolve, reject) => {
    let attempts = 0;
    const execute = () => {
      attempts++;
      fn()
        .then((result) => {
          if (condition(result)) {
            resolve(result);
          } else if (attempts < maxAttempts) {
            setTimeout(execute, delay);
          } else {
            reject(new Error(`Condition not met after ${maxAttempts} attempts`));
          }
        })
        .catch((error) => {
          if (attempts < maxAttempts) {
            setTimeout(execute, delay);
          } else {
            reject(error);
          }
        });
    };
    execute();
  });
}

/**
 * Simple retry mechanism: try every 500ms for up to 6 seconds
 * Retry getting a block by hash until the block is found or the maximum number of attempts is reached.
 * @param hash - The hash of the block to get.
 * @returns The block response.
 */
function getBlockByHashWithRetry(hash: string): Promise<BlockResponse> {
  return retry(
    () => new IndexerHttpClient().getBlockByOffset({ hash }),
    (response) => response.data?.block != null,
    12,
    500,
  );
}

// To run: yarn test e2e
describe('unshielded transactions', () => {
  let toolkit: ToolkitWrapper;
  let sourceAddress: string;
  let destinationAddress: string;
  let transactionResult: ToolkitTransactionResult;

  beforeAll(async () => {
    const randomId = Math.random().toString(36).substring(2, 12);
    toolkit = new ToolkitWrapper({
      containerName: `mn-toolkit-${env.getEnvName()}-${randomId}`,
      targetDir: '/tmp/toolkit/',
      chain: `${env.getEnvName()}`,
      nodeTag: '0.16.2-71d3d861',
    });
    await toolkit.start();
    const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';
    const destinationSeed = '0000000000000000000000000000000000000000000000000000000987654321';
    sourceAddress = await toolkit.showAddress(sourceSeed, 'unshielded');
    destinationAddress = await toolkit.showAddress(destinationSeed, 'unshielded');
    transactionResult = await toolkit.generateSingleTx(
      sourceSeed,
      'unshielded',
      destinationAddress,
      1,
    );
  }, 60000);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('a successful unshielded transaction transferring 1 NIGHT between two wallets', async () => {
    const indexerHttpClient = new IndexerHttpClient();

    test('should be reported by the indexer through a block query by hash', async (context: TestContext) => {
      context.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // The expected block might take a bit more to show up by indexer, so we retry a few times
      const blockResponse = await getBlockByHashWithRetry(transactionResult.blockHash!);

      // Verify the transaction appears in the block
      expect(blockResponse?.data?.block?.transactions).toBeDefined();
      expect(blockResponse?.data?.block?.transactions?.length).toBeGreaterThan(0);

      // Find our specific transaction by hash
      const sourceAddresInTx = blockResponse.data?.block?.transactions?.find((tx: any) =>
        tx.unshieldedCreatedOutputs.find((output: any) => output.owner === sourceAddress),
      );

      const destAddresInTx = blockResponse.data?.block?.transactions?.find((tx: any) =>
        tx.unshieldedCreatedOutputs.find((output: any) => output.owner === destinationAddress),
      );

      expect(sourceAddresInTx).toBeDefined();
      expect(destAddresInTx).toBeDefined();
    });

    test('should transfer 1 NIGHT from the source wallet to the destination wallet', async (context: TestContext) => {
      context.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // The expected block might take a bit more to show up by indexer, so we retry a few times
      const blockResponse = await getBlockByHashWithRetry(transactionResult.blockHash!);

      // Find the transaction with unshielded outputs
      const unshieldedTx = blockResponse.data?.block?.transactions?.find((tx: any) => {
        const hasCreated = tx.unshieldedCreatedOutputs && tx.unshieldedCreatedOutputs.length > 0;
        const hasSpent = tx.unshieldedSpentOutputs && tx.unshieldedSpentOutputs.length > 0;
        console.log(`Transaction ${tx.hash}: hasCreated=${hasCreated}, hasSpent=${hasSpent}`);
        return hasCreated || hasSpent;
      });

      expect(unshieldedTx).toBeDefined();

      // Validate unshieldedCreatedOutputs - should have 2 entries
      expect(unshieldedTx?.unshieldedCreatedOutputs).toHaveLength(2);

      const createdOutputs = unshieldedTx?.unshieldedCreatedOutputs;
      const sourceOutput = createdOutputs?.find((output: any) => output.owner === sourceAddress);
      const destOutput = createdOutputs?.find((output: any) => output.owner === destinationAddress);

      expect(sourceOutput).toBeDefined();
      expect(destOutput).toBeDefined();
      expect(destOutput?.value).toBe('1');

      // Validate unshieldedSpentOutputs - should have 1 entry
      expect(unshieldedTx?.unshieldedSpentOutputs).toHaveLength(1);

      const spentOutput = unshieldedTx?.unshieldedSpentOutputs?.[0];
      expect(spentOutput?.owner).toBe(sourceAddress);
    });

    // test('mn-toolkit submit single shielded tx test', async () => {
    //     // const sourceSeed = '1113354e9a4fb7bff5e049929197acfcf6dcb4fc1ab3205d92ba9c21813c8906';
    //     const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';
    //     const destinationSeed = '0000000000000000000000000000000000000000000000000000000987654321';

    //     const shieldedAddress = await toolkit.showAddress(destinationSeed, 'shielded');

    //     console.log('Destination shielded address:', shieldedAddress);

    //     const transactionResult: ToolkitTransactionResult = await toolkit.generateSingleTx(sourceSeed, 'shielded', shieldedAddress, 1);

    //     console.log('Block hash      :', transactionResult.blockHash);
    //     console.log('Transaction hash:', transactionResult.txHash);
    // }, 60000);
  });
});
