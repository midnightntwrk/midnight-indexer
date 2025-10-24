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
import { env } from '../../environment/model';
import log from '@utils/logging/logger';
import dataProvider from '@utils/testdata-provider';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { getBlockByHashWithRetry, getTransactionByHashWithRetry } from './test-utils';
import {
  ToolkitWrapper,
  DeployContractResult,
  ToolkitTransactionResult,
} from '@utils/toolkit/toolkit-wrapper';

const TOOLKIT_WRAPPER_TIMEOUT = 60_000; // 1 minute
const CONTRACT_ACTION_TIMEOUT = 150_000; // 2.5 minutes
const TEST_TIMEOUT = 10_000; // 10 seconds

describe.sequential('contract actions', () => {
  let indexerHttpClient: IndexerHttpClient;
  let toolkit: ToolkitWrapper;
  let contractDeployResult: DeployContractResult;
  let contractCallResult: ToolkitTransactionResult;

  beforeAll(async () => {
    indexerHttpClient = new IndexerHttpClient();

    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  }, TOOLKIT_WRAPPER_TIMEOUT);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('a transaction to deploy a smart contract', () => {
    beforeAll(async () => {
      contractDeployResult = await toolkit.deployContract({
        writeTestData: true,
        dataDir: `data/static/${env.getEnvName()}`,
      });
    }, CONTRACT_ACTION_TIMEOUT);

    /**
     * Once a contract deployment transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through a transaction query by hash, using the transaction hash reported by the toolkit.
     *
     * @given a confirmed contract deployment transaction
     * @when we query the indexer with a transaction query by hash, using the transaction hash reported by the toolkit
     * @then the transaction should be found and reported correctly
     */
    test(
      'should be reported by the indexer through a transaction query by hash',
      async (context: TestContext) => {
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
      },
      TEST_TIMEOUT,
    );

    /**
     * Once a contract deployment transaction has been submitted to node and confirmed, the indexer should report
     * that transaction in the block through a block query by hash, using the block hash reported by the toolkit.
     *
     * @given a confirmed contract deployment transaction
     * @when we query the indexer with a block query by hash, using the block hash reported by the toolkit
     * @then the block should contain the contract deployment transaction
     */
    test(
      'should be reported by the indexer through a block query by hash',
      async (context: TestContext) => {
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
      },
      TEST_TIMEOUT,
    );

    /**
     * Once a contract deployment transaction has been submitted to node and confirmed, the indexer should report
     * the contract action with the correct type when queried by contract address.
     *
     * @given a confirmed contract deployment transaction
     * @when we query the indexer with a contract action query by address
     * @then the contract action should be found with __typename 'ContractDeploy'
     */
    test(
      'should be reported by the indexer through a contract action query by address',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['Query', 'ContractAction', 'ByAddress', 'ContractDeploy'],
        };

        // Query the contract action by address (using the contract address for GraphQL queries)
        const contractActionResponse = await indexerHttpClient.getContractAction(
          contractDeployResult['contract-address-untagged'],
        );

        // Verify the contract action appears in the response
        expect(contractActionResponse?.data?.contractAction).toBeDefined();

        const contractAction = contractActionResponse.data?.contractAction;
        expect(contractAction?.__typename).toBe('ContractDeploy');

        // Verify it has ContractDeploy-specific fields
        if (contractAction?.__typename === 'ContractDeploy') {
          expect(contractAction.address).toBeDefined();
          expect(contractAction.address).toBe(contractDeployResult['contract-address-untagged']);

          const zswapState = contractAction.zswapState;
          log.debug(`zswapState (Deploy): length ${zswapState?.length ?? 0}`);
          expect.soft(zswapState).toBeDefined();
          expect.soft(typeof zswapState).toBe('string');
          expect.soft(zswapState?.length ?? 0).toBeGreaterThan(0);
        }
      },
      TEST_TIMEOUT,
    );
  });

  describe('a transaction to call a smart contract', () => {
    let contractCallBlockHash: string;
    let contractCallTransactionHash: string;

    beforeAll(async () => {
      contractCallResult = await toolkit.callContract(
        'increment',
        undefined,
        `data/static/${env.getEnvName()}`,
      );

      expect(contractCallResult.status).toBe('confirmed');
      log.debug(`Raw output: ${JSON.stringify(contractCallResult.rawOutput, null, 2)}`);
      log.debug(`Transaction hash: ${contractCallResult.txHash}`);
      log.debug(`Block hash: ${contractCallResult.blockHash}`);

      contractCallBlockHash = contractCallResult.blockHash;
      contractCallTransactionHash = contractCallResult.txHash;
    }, CONTRACT_ACTION_TIMEOUT);

    /**
     * Once a contract call transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through a query by transaction hash, using the transaction hash reported by the toolkit.
     *
     * @given a confirmed contract call transaction
     * @when we query the indexer with a transaction query by hash, using the transaction hash reported by the toolkit
     * @then the transaction should be found and reported correctly
     */
    test(
      'should be reported by the indexer through a transaction query by hash',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['Query', 'Transaction', 'ByHash', 'ContractCall'],
        };

        const transactionResponse = await getTransactionByHashWithRetry(
          contractCallTransactionHash,
        );

        // Verify the transaction appears in the response
        expect(transactionResponse?.data?.transactions).toBeDefined();
        expect(transactionResponse?.data?.transactions?.length).toBeGreaterThan(0);

        // Find our specific transaction by hash
        const foundTransaction = transactionResponse.data?.transactions?.find(
          (tx: any) => tx.hash === contractCallTransactionHash,
        );

        expect(foundTransaction).toBeDefined();
        expect(foundTransaction?.hash).toBe(contractCallTransactionHash);
      },
      TEST_TIMEOUT,
    );

    /**
     * Once a contract call transaction has been submitted to node and confirmed, the indexer should report
     * that transaction in the block through a block query by hash, using the block hash reported by the toolkit.
     *
     * @given a confirmed contract call transaction
     * @when we query the indexer with a block query by hash, using the block hash reported by the toolkit
     * @then the block should contain the contract call transaction
     */
    test(
      'should be reported by the indexer through a block query by hash',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['Query', 'Block', 'ByHash', 'ContractCall'],
        };

        const blockResponse = await getBlockByHashWithRetry(contractCallBlockHash);

        // Verify the block appears in the response
        expect(blockResponse).toBeSuccess();
        expect(blockResponse.data?.block).toBeDefined();
        expect(blockResponse.data?.block?.hash).toBe(contractCallBlockHash);
      },
      TEST_TIMEOUT,
    );

    /**
     * Once a contract call transaction has been submitted to node and confirmed, the indexer should report
     * the contract action with the correct type when queried by contract address.
     *
     * @given a confirmed contract call transaction
     * @when we query the indexer with a contract action query by address
     * @then the contract action should be found with __typename 'ContractCall'
     */
    test(
      'should be reported by the indexer through a contract action query by address',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['Query', 'ContractAction', 'ByAddress', 'ContractCall'],
        };

        // Query the contract action by address (using the contract address for GraphQL queries)
        const contractActionResponse = await indexerHttpClient.getContractAction(
          contractDeployResult['contract-address-untagged'],
        );

        // Verify the contract action appears in the response
        expect(contractActionResponse?.data?.contractAction).toBeDefined();

        const contractAction = contractActionResponse.data?.contractAction;
        expect(contractAction?.__typename).toBe('ContractCall');

        if (contractAction?.__typename === 'ContractCall') {
          expect(contractAction.address).toBeDefined();
          expect(contractAction.address).toBe(contractDeployResult['contract-address-untagged']);
          expect(contractAction.entryPoint).toBeDefined();
          expect(contractAction.deploy).toBeDefined();
          expect(contractAction.deploy?.address).toBeDefined();
        }
      },
      TEST_TIMEOUT,
    );
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
    test.todo(
      'should be reported by the indexer through a transaction query by hash',
      async (context: TestContext) => {},
      TEST_TIMEOUT,
    );

    /**
     * Once a contract update transaction has been submitted to node and confirmed, the indexer should report
     * that transaction in the block through a block query by hash, using the block hash reported by the toolkit.
     *
     * @given a confirmed contract update transaction
     * @when we query the indexer with a block query by hash, using the block hash reported by the toolkit
     * @then the block should contain the contract update transaction
     */
    test.todo(
      'should be reported by the indexer through a block query by hash',
      async (context: TestContext) => {},
      TEST_TIMEOUT,
    );

    /**
     * Once a contract update transaction has been submitted to node and confirmed, the indexer should report
     * the contract action with the correct type when queried by contract address.
     *
     * @given a confirmed contract update transaction
     * @when we query the indexer with a contract action query by address
     * @then the contract action should be found with __typename 'ContractUpdate'
     */
    test.todo(
      'should be reported by the indexer through a contract action query by address',
      async (context: TestContext) => {},
      TEST_TIMEOUT,
    );
  });
});
