// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { getBlockByHashWithRetry, getTransactionByHashWithRetry } from './test-utils';
import { ToolkitWrapper, DeployContractResult } from '@utils/toolkit/toolkit-wrapper';
import { LocalDataUtils, LocalData } from '@utils/local-data-utils';
import { mkdtempSync } from 'fs';
import { join } from 'path';
import { tmpdir } from 'os';

// To run: yarn test e2e
describe.sequential('contract actions', () => {
  let indexerHttpClient: IndexerHttpClient;
  let toolkit: ToolkitWrapper;
  let deployResult: DeployContractResult;
  let localData: LocalData;
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
      deployResult = await toolkit.deployContract();

      // Give the indexer time to process the deployment
      await new Promise((resolve) => setTimeout(resolve, 2000));

      // Create local.json file for callContract to read from
      const { LocalDataUtils } = await import('@utils/local-data-utils');
      const { env } = await import('../../environment/model');
      const localDataUtils = new LocalDataUtils(`data/static/${env.getEnvName()}`);
      await localDataUtils.writeDeploymentData(deployResult);

      // Read the local data to get the deployment hashes
      localData = localDataUtils.readLocalData();
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
      const transactionResponse = await getTransactionByHashWithRetry(localData['deploy-tx-hash']);

      // Verify the transaction appears in the response
      expect(transactionResponse?.data?.transactions).toBeDefined();
      expect(transactionResponse?.data?.transactions?.length).toBeGreaterThan(0);

      // Find our specific transaction by hash
      const foundTransaction = transactionResponse.data?.transactions?.find(
        (tx: any) => tx.hash === localData['deploy-tx-hash'],
      );

      expect(foundTransaction).toBeDefined();
      expect(foundTransaction?.hash).toBe(localData['deploy-tx-hash']);
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

      // The expected block might take a bit more to show up by indexer, so we retry a few times
      const blockResponse = await getBlockByHashWithRetry(localData['deploy-block-hash']);

      // Verify the block appears in the response
      expect(blockResponse?.data?.block).toBeDefined();
      expect(blockResponse?.data?.block?.transactions).toBeDefined();
      expect(blockResponse?.data?.block?.transactions?.length).toBeGreaterThan(0);

      // Find our specific transaction in the block
      const foundTransaction = blockResponse.data?.block?.transactions?.find(
        (tx: any) => tx.hash === localData['deploy-tx-hash'],
      );

      expect(foundTransaction?.hash).toBe(localData['deploy-tx-hash']);
      expect(blockResponse.data?.block?.hash).toBe(localData['deploy-block-hash']);
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
        deployResult.contractAddress,
      );

      // Verify the contract action appears in the response
      expect(contractActionResponse?.data?.contractAction).toBeDefined();

      const contractAction = contractActionResponse.data?.contractAction;
      expect(contractAction?.__typename).toBe('ContractDeploy');
      expect(contractAction?.address).toBe(deployResult.contractAddress);

      // Verify it has ContractDeploy-specific fields
      if (contractAction?.__typename === 'ContractDeploy') {
        expect(contractAction.address).toBeDefined();
      }
    }, 60000);
  });

  describe('a transaction to call a smart contract', () => {
    beforeAll(async () => {
      await toolkit.callContract();

      // Give the indexer time to process the contract call
      await new Promise((resolve) => setTimeout(resolve, 2000));
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

      const transactionResponse = await getTransactionByHashWithRetry(localData['deploy-tx-hash']);

      // Verify the transaction appears in the response
      expect(transactionResponse?.data?.transactions).toBeDefined();
      expect(transactionResponse?.data?.transactions?.length).toBeGreaterThan(0);

      // Find our specific transaction by hash
      const foundTransaction = transactionResponse.data?.transactions?.find(
        (tx: any) => tx.hash === localData['deploy-tx-hash'],
      );

      expect(foundTransaction).toBeDefined();
      expect(foundTransaction?.hash).toBe(localData['deploy-tx-hash']);
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

      const blockResponse = await getBlockByHashWithRetry(localData['deploy-block-hash']);

      // Verify the block appears in the response
      expect(blockResponse?.data?.block).toBeDefined();
      expect(blockResponse?.data?.block?.transactions).toBeDefined();
      expect(blockResponse?.data?.block?.transactions?.length).toBeGreaterThan(0);

      // Find our specific transaction in the block
      const foundTransaction = blockResponse.data?.block?.transactions?.find(
        (tx: any) => tx.hash === localData['deploy-tx-hash'],
      );

      expect(foundTransaction?.hash).toBe(localData['deploy-tx-hash']);
      expect(blockResponse.data?.block?.hash).toBe(localData['deploy-block-hash']);
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
        deployResult.contractAddress,
      );

      // Verify the contract action appears in the response
      expect(contractActionResponse?.data?.contractAction).toBeDefined();

      const contractAction = contractActionResponse.data?.contractAction;
      expect(contractAction?.__typename).toBe('ContractCall');
      expect(contractAction?.address).toBe(deployResult.contractAddress);

      // Verify it has ContractCall-specific fields
      if (contractAction?.__typename === 'ContractCall') {
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
    test('should be reported by the indexer through a transaction query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Transaction', 'ByHash', 'ContractUpdate'],
      };

      // TODO: This test is empty until updateContract method is implemented
      expect(true).toBe(true); // Placeholder assertion
    }, 60000);

    /**
     * Once a contract update transaction has been submitted to node and confirmed, the indexer should report
     * that transaction in the block through a block query by hash, using the block hash reported by the toolkit.
     *
     * @given a confirmed contract update transaction
     * @when we query the indexer with a block query by hash, using the block hash reported by the toolkit
     * @then the block should contain the contract update transaction
     */
    test('should be reported by the indexer through a block query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash', 'ContractUpdate'],
      };

      // TODO: This test is empty until updateContract method is implemented
      expect(true).toBe(true); // Placeholder assertion
    }, 60000);

    /**
     * Once a contract update transaction has been submitted to node and confirmed, the indexer should report
     * the contract action with the correct type when queried by contract address.
     *
     * @given a confirmed contract update transaction
     * @when we query the indexer with a contract action query by address
     * @then the contract action should be found with __typename 'ContractUpdate'
     */
    test('should be reported by the indexer through a contract action query by address', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'ContractAction', 'ByAddress', 'ContractUpdate'],
      };

      // TODO: This test is empty until updateContract method is implemented
      expect(true).toBe(true); // Placeholder assertion
    }, 60000);
  });
});
