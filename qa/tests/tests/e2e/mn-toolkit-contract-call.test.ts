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
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { getBlockByHashWithRetry, getTransactionByHashWithRetry } from './test-utils';
import dataProvider from '@utils/testdata-provider';
import { ToolkitWrapper, ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';
import { readFileSync } from 'fs';
import { join } from 'path';

describe('mn-toolkit contract calls', () => {
  let indexerHttpClient: IndexerHttpClient;
  let toolkit: ToolkitWrapper;
  let contractCallResult: ToolkitTransactionResult;

  const getLocalData = () => {
    const localJsonPath = join(__dirname, '../../data/static/undeployed/local.json');
    return JSON.parse(readFileSync(localJsonPath, 'utf8'));
  };

  beforeAll(async () => {
    await dataProvider.init();

    indexerHttpClient = new IndexerHttpClient();
    toolkit = new ToolkitWrapper({});
    await toolkit.start();

    // Deploy contract with logging and test data writing enabled
    await toolkit.deployContract({
      enableLogging: true,
      writeTestData: true,
      dataDir: 'data/static/undeployed',
    });

    await new Promise((resolve) => setTimeout(resolve, 100));
    await dataProvider.init();

    contractCallResult = await toolkit.callContract();
  }, 300000);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('a successful contract call transaction', () => {
    /**
     * Once a contract call transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through a query by transaction hash, using the transaction hash reported by the toolkit.
     *
     * @given a confirmed contract call transaction
     * @when we query the indexer with a transaction query by hash, using the transaction hash reported by the toolkit
     * @then the transaction should be found and reported correctly
     */
    test('should be reported by the indexer through a transaction query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Transaction', 'ByHash', 'ContractCall'],
      };

      const localData = getLocalData();
      const deployTxHash = localData['deploy-tx-hash'];
      const transactionResponse = await getTransactionByHashWithRetry(deployTxHash);

      expect(transactionResponse?.data?.transactions).toBeDefined();
      expect(transactionResponse?.data?.transactions?.length).toBeGreaterThan(0);

      const foundTransaction = transactionResponse.data?.transactions?.find(
        (tx: any) => tx.hash === deployTxHash,
      );

      expect(foundTransaction).toBeDefined();
      expect(foundTransaction?.hash).toBe(deployTxHash);
    }, 60000);

    /**
     * Once a contract call transaction has been submitted to node and confirmed, the indexer should report
     * that transaction in the block through a block query by hash, using the block hash reported by the toolkit.
     *
     * @given a confirmed contract call transaction
     * @when we query the indexer with a block query by hash, using the block hash reported by the toolkit
     * @then the block should contain the contract call transaction
     */
    test('should be reported by the indexer through a block query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash', 'ContractCall'],
      };

      const localData = getLocalData();
      const deployTxHash = localData['deploy-tx-hash'];
      const deployBlockHash = localData['deploy-block-hash'];
      const blockResponse = await getBlockByHashWithRetry(deployBlockHash);

      expect(blockResponse?.data?.block).toBeDefined();
      expect(blockResponse?.data?.block?.transactions).toBeDefined();
      expect(blockResponse?.data?.block?.transactions?.length).toBeGreaterThan(0);

      const foundTransaction = blockResponse.data?.block?.transactions?.find(
        (tx: any) => tx.hash === deployTxHash,
      );

      expect(foundTransaction).toBeDefined();
      expect(foundTransaction?.hash).toBe(deployTxHash);
      expect(blockResponse.data?.block?.hash).toBe(deployBlockHash);
    }, 60000);

    /**
     * Once a contract call transaction has been submitted to node and confirmed, the indexer should report
     * the contract action with the correct type when queried by contract address.
     *
     * @given a confirmed contract call transaction
     * @when we query the indexer with a contract action query by address
     * @then the contract action should be found with __typename 'ContractCall'
     */
    test('should be reported by the indexer through a contract action query by address', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'ContractAction', 'ByAddress', 'ContractCall'],
      };

      const localData = getLocalData();
      const contractAddress = localData['contract-address'];
      const contractActionResponse = await indexerHttpClient.getContractAction(contractAddress);

      expect(contractActionResponse?.data?.contractAction).toBeDefined();

      const contractAction = contractActionResponse.data?.contractAction;
      expect(contractAction?.__typename).toBe('ContractCall');
      expect(contractAction?.address).toBe(contractAddress);

      if (contractAction?.__typename === 'ContractCall') {
        expect(contractAction.entryPoint).toBeDefined();
        expect(contractAction.deploy).toBeDefined();
        expect(contractAction.deploy?.address).toBe(contractAddress);
      }
    }, 60000);
  });
});
