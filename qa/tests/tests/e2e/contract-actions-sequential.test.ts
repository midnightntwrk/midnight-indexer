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
import { mkdtempSync } from 'fs';
import { join } from 'path';
import { tmpdir } from 'os';

import '@utils/logging/test-logging-hooks';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { getBlockByHashWithRetry, getTransactionByHashWithRetry } from './test-utils';
import { ToolkitWrapper, DeployContractResult } from '@utils/toolkit/toolkit-wrapper';
import { env } from '../../environment/model';
import dataProvider from '@utils/testdata-provider';

// To run: yarn test e2e
describe.sequential('contract actions', () => {
  let indexerHttpClient: IndexerHttpClient;
  let toolkit: ToolkitWrapper;
  let deployResult: DeployContractResult;
  let outDir: string;

  beforeAll(async () => {
    indexerHttpClient = new IndexerHttpClient();

    // Create a temporary directory for this test run
    outDir = mkdtempSync(join(tmpdir(), 'contract-actions-test-'));

    toolkit = new ToolkitWrapper({ targetDir: outDir });
    await toolkit.start();
  }, 150000);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('a transaction to deploy a smart contract', () => {
    beforeAll(async () => {
      deployResult = await toolkit.deployContract({
        writeTestData: true,
        dataDir: `data/static/${env.getEnvName()}`,
      });

      await new Promise((resolve) => setTimeout(resolve, 2000));
    }, 150000);

    /**
     * Once a contract deployment transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through a query by transaction hash, using the transaction hash reported by the toolkit.
     *
     * @given a confirmed contract deployment transaction
     * @when we query the indexer with a transaction query by hash, using the transaction hash reported by the toolkit
     * @then the transaction should be found and reported correctly
     */
    test('should be reported by the indexer through a transaction query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Transaction', 'ByHash', 'ContractDeploy'],
      };

      // The expected transaction might take a bit more to show up by indexer, so we retry a few times
      const deployTxHash = dataProvider.getLocalDeployTxHash();
      const transactionResponse = await getTransactionByHashWithRetry(deployTxHash);

      // Verify the transaction appears in the response
      expect(transactionResponse).toBeSuccess();
      expect(transactionResponse?.data?.transactions).toBeDefined();
      expect(transactionResponse?.data?.transactions?.length).toBeGreaterThan(0);

      // Find our specific transaction by hash
      const foundTransaction = transactionResponse.data?.transactions?.find(
        (tx: any) => tx.hash === deployTxHash,
      );

      expect(foundTransaction).toBeDefined();
      expect(foundTransaction?.hash).toBe(deployTxHash);
    }, 15000);

    /**
     * Once a contract deployment transaction has been submitted to node and confirmed, the indexer should report
     * that transaction in the block through a block query by hash, using the block hash reported by the toolkit.
     *
     * @given a confirmed contract deployment transaction
     * @when we query the indexer with a block query by hash, using the block hash reported by the toolkit
     * @then the block should contain the contract deployment transaction
     */
    test('should be reported by the indexer through a block query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash', 'ContractDeploy'],
      };

      const deployTxHash = dataProvider.getLocalDeployTxHash();
      const deployBlockHash = dataProvider.getLocalDeployBlockHash();
      const blockResponse = await getBlockByHashWithRetry(deployBlockHash);

      // Verify the block appears in the response
      expect(blockResponse).toBeSuccess();
      expect(blockResponse?.data?.block).toBeDefined();
      expect(blockResponse?.data?.block?.transactions).toBeDefined();
      expect(blockResponse?.data?.block?.transactions?.length).toBeGreaterThan(0);

      // Find our specific transaction in the block
      const foundTransaction = blockResponse.data?.block?.transactions?.find(
        (tx: any) => tx.hash === deployTxHash,
      );

      expect(foundTransaction?.hash).toBe(deployTxHash);
      expect(blockResponse.data?.block?.hash).toBe(deployBlockHash);
    }, 15000);

    /**
     * Once a contract deployment transaction has been submitted to node and confirmed, the indexer should report
     * the contract action with the correct type when queried by contract address.
     *
     * @given a confirmed contract deployment transaction
     * @when we query the indexer with a contract action query by address
     * @then the contract action should be found with __typename 'ContractDeploy'
     */
    test('should be reported by the indexer through a contract action query by address', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'ContractAction', 'ByAddress', 'ContractDeploy'],
      };

      // Query the contract action by address (using the contract address for GraphQL queries)
      const contractActionResponse = await indexerHttpClient.getContractAction(
        deployResult['contract-address-untagged'],
      );

      // Verify the contract action appears in the response
      expect(contractActionResponse?.data?.contractAction).toBeDefined();

      const contractAction = contractActionResponse.data?.contractAction;
      expect(contractAction?.__typename).toBe('ContractDeploy');

      // Verify it has ContractDeploy-specific fields
      if (contractAction?.__typename === 'ContractDeploy') {
        expect(contractAction.address).toBeDefined();
        expect(contractAction.address).toBe(deployResult['contract-address-untagged']);
      }
    }, 60000);
  });

  describe('a transaction to call a smart contract', () => {
    beforeAll(async () => {
      await toolkit.callContract();

      // Give the indexer time to process the contract call
      await new Promise((resolve) => setTimeout(resolve, 3000));
    }, 150000);

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

      const transactionResponse = await getTransactionByHashWithRetry(
        dataProvider.getLocalDeployTxHash(),
      );

      // Verify the transaction appears in the response
      expect(transactionResponse?.data?.transactions).toBeDefined();
      expect(transactionResponse?.data?.transactions?.length).toBeGreaterThan(0);

      // Find our specific transaction by hash
      const foundTransaction = transactionResponse.data?.transactions?.find(
        (tx: any) => tx.hash === dataProvider.getLocalDeployTxHash(),
      );

      expect(foundTransaction).toBeDefined();
      expect(foundTransaction?.hash).toBe(dataProvider.getLocalDeployTxHash());
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

      const blockResponse = await getBlockByHashWithRetry(dataProvider.getLocalDeployBlockHash());

      // Verify the block appears in the response
      expect(blockResponse?.data?.block).toBeDefined();
      expect(blockResponse?.data?.block?.transactions).toBeDefined();
      expect(blockResponse?.data?.block?.transactions?.length).toBeGreaterThan(0);

      // Find our specific transaction in the block
      const foundTransaction = blockResponse.data?.block?.transactions?.find(
        (tx: any) => tx.hash === dataProvider.getLocalDeployTxHash(),
      );

      expect(foundTransaction?.hash).toBe(dataProvider.getLocalDeployTxHash());
      expect(blockResponse.data?.block?.hash).toBe(dataProvider.getLocalDeployBlockHash());
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

      // Query the contract action by address (using the contract address for GraphQL queries)
      const contractActionResponse = await indexerHttpClient.getContractAction(
        deployResult['contract-address-untagged'],
      );

      // Verify the contract action appears in the response
      expect(contractActionResponse?.data?.contractAction).toBeDefined();

      const contractAction = contractActionResponse.data?.contractAction;
      expect(contractAction?.__typename).toBe('ContractCall');

      if (contractAction?.__typename === 'ContractCall') {
        expect(contractAction.address).toBeDefined();
        expect(contractAction.address).toBe(deployResult['contract-address-untagged']);
        expect(contractAction.entryPoint).toBeDefined();
        expect(contractAction.deploy).toBeDefined();
        expect(contractAction.deploy?.address).toBeDefined();
      }
    }, 60000);
  });

  describe('a transaction to update a smart contract', () => {
    beforeAll(async () => {
      // TODO: updateContract method is not yet implemented in ToolkitWrapper
      // This section is empty for now until updateContract is implemented
    });

    /**
     * Once a contract update transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through a query by transaction hash, using the transaction hash reported by the toolkit.
     *
     * @given a confirmed contract update transaction
     * @when we query the indexer with a transaction query by hash, using the transaction hash reported by the toolkit
     * @then the transaction should be found and reported correctly
     */
    test.skip('should be reported by the indexer through a transaction query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Transaction', 'ByHash', 'ContractUpdate'],
      };
    }, 60000);

    /**
     * Once a contract update transaction has been submitted to node and confirmed, the indexer should report
     * that transaction in the block through a block query by hash, using the block hash reported by the toolkit.
     *
     * @given a confirmed contract update transaction
     * @when we query the indexer with a block query by hash, using the block hash reported by the toolkit
     * @then the block should contain the contract update transaction
     */
    test.skip('should be reported by the indexer through a block query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash', 'ContractUpdate'],
      };
    }, 60000);

    /**
     * Once a contract update transaction has been submitted to node and confirmed, the indexer should report
     * the contract action with the correct type when queried by contract address.
     *
     * @given a confirmed contract update transaction
     * @when we query the indexer with a contract action query by address
     * @then the contract action should be found with __typename 'ContractUpdate'
     */
    test.skip('should be reported by the indexer through a contract action query by address', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'ContractAction', 'ByAddress', 'ContractUpdate'],
      };
    }, 60000);
  });
});
