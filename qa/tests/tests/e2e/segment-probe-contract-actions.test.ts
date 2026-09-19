// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
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

// The same contract-action assertions as contract-actions-sequential.test.ts,
// but against the segment-split contract instead of the counter.
//
// WHY A SECOND CONTRACT. The counter is the simplest thing that deploys and
// calls, which is what makes it a good default — and also what makes it a weak
// test of the artifact plumbing. segment-split is a materially different
// artifact: it has a constructor, three ledger fields including a Map, and
// circuits heavy enough to be split across the guaranteed/fallible boundary.
// Deploying and calling it proves the moth/midnight-js path handles a real
// contract rather than the one it was written against.
//
// The contract itself is the fixture from the guaranteed/fallible segment
// investigation (segment-probe, "pink tawny owl"), recompiled here with
// compactc 0.31.1. This suite deliberately exercises only its FIRST call, which
// applies normally — the stale-call behaviour it was built for belongs in that
// probe, not in indexer query assertions.
//
// MOTH ONLY. The toolkit can only deploy its own built-in contracts, so there
// is no toolkit path for this fixture and the suite skips itself unless
// TX_BACKEND=moth.

import { resolve } from 'path';
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import dataProvider from '@utils/testdata-provider';
import { env } from '../../environment/model';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { callCircuitViaMoth, deployContractViaMoth } from '@utils/moth/moth-contracts';
import {
  getBlockByHashWithRetry,
  getContractDeploymentHashes,
  getTransactionByHashWithRetry,
  resolveBlockHash,
} from './test-utils';
import type { Transaction } from '@utils/indexer/indexer-types';

const CONTRACT_ACTION_TIMEOUT = 150_000; // 2.5 minutes
const TEST_TIMEOUT = 10_000; // 10 seconds

/** The compiled segment-split artifact committed alongside the tests. */
const ARTIFACT_DIR = resolve(__dirname, '../../contracts/segment-split/managed');

/**
 * Applies a counter increment before `kernel.checkpoint()`, so a first call
 * lands normally. Chosen over `burnWithoutGuaranteed` only because it is the
 * one that produces a guaranteed transcript, making it the closer analogue of
 * an ordinary contract call.
 */
const CIRCUIT = 'burnWithGuaranteed';

/** Normalize hash for comparison (indexer may return with or without 0x prefix). */
function sameHash(a: string | undefined, b: string | undefined): boolean {
  const n = (h: string | undefined) => (h ?? '').trim().toLowerCase().replace(/^0x/, '');
  return n(a) === n(b);
}

const isMoth = env.getTxBackend() === 'moth';

describe.skipIf(!isMoth).sequential('segment-split contract actions', () => {
  let indexerHttpClient: IndexerHttpClient;
  let fundingSeed: string;
  let contractAddress: string;

  let deployTxHash: string;
  let deployBlockHash: string;

  beforeAll(async () => {
    indexerHttpClient = new IndexerHttpClient();
    fundingSeed = dataProvider.getFundingSeed();
  });

  describe('a transaction to deploy a segment-split smart contract', () => {
    beforeAll(async () => {
      const deployed = await deployContractViaMoth(fundingSeed, ARTIFACT_DIR);
      contractAddress = deployed.contractAddress;
      // The deploy's own transaction and block hashes come from the indexer
      // rather than from the submit call: midnight-js reports the transaction
      // it sent, not the block it landed in.
      const hashes = await getContractDeploymentHashes(contractAddress);
      deployTxHash = hashes.txHash;
      deployBlockHash = hashes.blockHash;
      log.debug(`segment-split deployed at ${contractAddress}, tx ${deployTxHash}`);
    }, CONTRACT_ACTION_TIMEOUT);

    /**
     * @given a confirmed deployment of the segment-split contract
     * @when we query the indexer with a transaction query by hash
     * @then the transaction should be found and reported correctly
     */
    test(
      'should be reported by the indexer through a transaction query by hash',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['Query', 'Transaction', 'ByHash', 'ContractDeploy'],
        };

        const transactionResponse = await getTransactionByHashWithRetry(deployTxHash);

        expect(transactionResponse).toBeSuccess();
        expect(transactionResponse?.data?.transactions).toBeDefined();

        const foundTransaction = transactionResponse.data?.transactions?.find((tx: Transaction) =>
          sameHash(tx.hash, deployTxHash),
        );
        expect(foundTransaction).toBeDefined();
      },
      TEST_TIMEOUT,
    );

    /**
     * @given a confirmed deployment of the segment-split contract
     * @when we query the indexer with a block query by hash
     * @then the block should contain the deployment transaction
     */
    test(
      'should be reported by the indexer through a block query by hash',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['Query', 'Block', 'ByHash', 'ContractDeploy'],
        };

        const blockResponse = await getBlockByHashWithRetry(deployBlockHash);

        expect(blockResponse).toBeSuccess();
        const found = blockResponse.data?.block?.transactions?.find((tx: Transaction) =>
          sameHash(tx.hash, deployTxHash),
        );
        expect(found).toBeDefined();
      },
      TEST_TIMEOUT,
    );

    /**
     * @given a confirmed deployment of the segment-split contract
     * @when we query the indexer with a contract action query by address
     * @then the contract action should be found with __typename 'ContractDeploy'
     */
    test(
      'should be reported by the indexer through a contract action query by address',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['Query', 'ContractAction', 'ByAddress', 'ContractDeploy'],
        };

        const contractActionResponse = await indexerHttpClient.getContractAction(contractAddress);
        expect(contractActionResponse?.data?.contractAction).toBeDefined();

        const contractAction = contractActionResponse.data?.contractAction;
        expect(contractAction?.__typename).toBe('ContractDeploy');

        if (contractAction?.__typename === 'ContractDeploy') {
          expect(sameHash(contractAction.address, contractAddress)).toBe(true);
        }
      },
      TEST_TIMEOUT,
    );
  });

  describe(`a transaction to call the ${CIRCUIT} circuit on the deployed segment-split smart contract`, () => {
    let callTxHash: string;
    let callBlockHash: string;

    beforeAll(async () => {
      callTxHash = await callCircuitViaMoth(fundingSeed, contractAddress, CIRCUIT, ARTIFACT_DIR);
      const result = { txHash: callTxHash, blockHash: '', status: 'sent' as const, rawOutput: '' };
      await resolveBlockHash(result);
      callBlockHash = result.blockHash;
      log.debug(`segment-split ${CIRCUIT} call tx ${callTxHash}, block ${callBlockHash}`);
    }, CONTRACT_ACTION_TIMEOUT);

    /**
     * @given a confirmed call to the segment-split contract
     * @when we query the indexer with a transaction query by hash
     * @then the transaction should be found and reported correctly
     */
    test(
      'should be reported by the indexer through a transaction query by hash',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['Query', 'Transaction', 'ByHash', 'ContractCall'],
        };

        const transactionResponse = await getTransactionByHashWithRetry(callTxHash);

        expect(transactionResponse).toBeSuccess();
        const foundTransaction = transactionResponse.data?.transactions?.find((tx: Transaction) =>
          sameHash(tx.hash, callTxHash),
        );
        expect(foundTransaction).toBeDefined();
      },
      TEST_TIMEOUT,
    );

    /**
     * @given a confirmed call to the segment-split contract
     * @when we query the indexer with a block query by hash
     * @then the block should contain the call transaction
     */
    test(
      'should be reported by the indexer through a block query by hash',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['Query', 'Block', 'ByHash', 'ContractCall'],
        };

        const blockResponse = await getBlockByHashWithRetry(callBlockHash);

        expect(blockResponse).toBeSuccess();
        const found = blockResponse.data?.block?.transactions?.find((tx: Transaction) =>
          sameHash(tx.hash, callTxHash),
        );
        expect(found).toBeDefined();
      },
      TEST_TIMEOUT,
    );

    /**
     * @given a confirmed call to the segment-split contract
     * @when we query the indexer with a contract action query by address
     * @then the latest contract action should be a 'ContractCall'
     */
    test(
      'should be reported by the indexer through a contract action query by address',
      async (context: TestContext) => {
        context.task!.meta.custom = {
          labels: ['Query', 'ContractAction', 'ByAddress', 'ContractCall'],
        };

        const contractActionResponse = await indexerHttpClient.getContractAction(contractAddress);
        expect(contractActionResponse?.data?.contractAction).toBeDefined();

        const contractAction = contractActionResponse.data?.contractAction;
        expect(contractAction?.__typename).toBe('ContractCall');

        if (contractAction?.__typename === 'ContractCall') {
          expect(sameHash(contractAction.address, contractAddress)).toBe(true);
        }
      },
      TEST_TIMEOUT,
    );
  });
});
