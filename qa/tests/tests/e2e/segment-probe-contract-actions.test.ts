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
// compactc 0.36.0. This suite deliberately exercises only its FIRST call, which
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
import { closeMothWallets } from '@utils/moth/moth-backend';
import {
  callCircuitViaMoth,
  closeMothContractContexts,
  deployContractViaMoth,
} from '@utils/moth/moth-contracts';
import { mothHandlesContracts } from '@utils/moth/ledger';
import {
  awaitIndexedTx,
  getBlockByHashWithRetry,
  getContractDeploymentHashes,
  getTransactionByHashWithRetry,
} from './test-utils';
import type { Transaction } from '@utils/indexer/indexer-types';

// 10 minutes, as in the other moth contract suites: a deploy or call first waits
// up to 3 minutes for the dust trees to reach the tip (`awaitDustRootsCurrent`),
// then proves and submits. A shorter hook masks that wait's own error with
// a generic hook timeout.
const CONTRACT_ACTION_TIMEOUT = 600_000;
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

/**
 * Non-empty when moth cannot build contract transactions on this chain: its
 * contract path is midnight-js 5, whose current era is ledger 9, and this suite
 * has no toolkit fallback (the segment-split contract is not one the toolkit
 * ships). Every test skips with it rather than failing.
 */
let skipReason = '';

describe.skipIf(!isMoth).sequential('segment-split contract actions', () => {
  let indexerHttpClient: IndexerHttpClient;
  let fundingSeed: string;
  let contractAddress: string;

  let deployTxHash: string;
  let deployBlockHash: string;

  beforeAll(async () => {
    indexerHttpClient = new IndexerHttpClient();
    fundingSeed = dataProvider.getFundingSeed();
    if (!(await mothHandlesContracts())) {
      skipReason =
        'ledger v8: moth builds contract transactions through midnight-js 5 ' +
        '(ledger 9 is its current era), and the segment-split contract has no toolkit path';
      log.warn(`Skipping the segment-split suite: ${skipReason}`);
    }
  });

  // No toolkit here, so nothing else releases the wallet this suite opened.
  afterAll(async () => {
    closeMothContractContexts();
    await closeMothWallets();
  });

  describe('a transaction to deploy a segment-split smart contract', () => {
    beforeAll(async () => {
      if (skipReason) return;
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
        context.skip?.(skipReason !== '', skipReason);

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
        context.skip?.(skipReason !== '', skipReason);

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
        context.skip?.(skipReason !== '', skipReason);

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
      if (skipReason) return;
      if (!contractAddress) {
        throw new Error(
          'The segment-split deploy did not succeed (see the deploy hook), so there is nothing to call.',
        );
      }
      callTxHash = await callCircuitViaMoth(fundingSeed, contractAddress, CIRCUIT, ARTIFACT_DIR);
      callBlockHash = (await awaitIndexedTx(callTxHash, `the ${CIRCUIT} call`)).blockHash;
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
        context.skip?.(skipReason !== '', skipReason);

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
        context.skip?.(skipReason !== '', skipReason);

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
        context.skip?.(skipReason !== '', skipReason);

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
