// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) Midnight Foundation
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
import dataProvider from '@utils/testdata-provider';
import { ToolkitWrapper, type ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';

import type { Transaction, RegularTransaction } from '@utils/indexer/indexer-types';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import {
  getBlockByHashWithRetry,
  getTransactionByHashWithRetry,
  resolveBlockHash,
} from './test-utils';
import { TestContext } from 'vitest';
import { collectValidZswapEvents } from 'tests/shared/zswap-events-utils';
import { RegularTransactionSchema, ZswapLedgerEventSchema } from '@utils/indexer/graphql/schema';
import { IndexerWsClient } from '@utils/indexer/websocket-client';
import { EventCoordinator } from '@utils/event-coordinator';
import { collectValidDustLedgerEvents } from 'tests/shared/dust-ledger-utils';

describe('shielded transactions', () => {
  let indexerWsClient: IndexerWsClient;
  let indexerEventCoordinator: EventCoordinator;
  let indexerHttpClient: IndexerHttpClient;
  let previousMaxLedgerId: number;
  let zswapEndIndexBeforeTx: number;
  let dustCommitmentEndIndexBeforeTx: number;
  let toolkit: ToolkitWrapper;
  let transactionResult: ToolkitTransactionResult;

  // Deterministic seeds (hex) that work with the toolkit
  const sourceSeed = dataProvider.getFundingSeed();
  const destinationSeed = '0000000000000000000000000000000000000000000000000000000987654321';

  let destinationAddress: string;

  beforeAll(async () => {
    indexerWsClient = new IndexerWsClient();
    indexerEventCoordinator = new EventCoordinator();
    indexerHttpClient = new IndexerHttpClient();
    await indexerWsClient.connectionInit();
    // Start a one-off toolkit container
    toolkit = new ToolkitWrapper({});

    await toolkit.start();

    // Derive shielded addresses from seeds
    destinationAddress = (await toolkit.showAddress(destinationSeed)).shielded;

    const beforeDustEvents = await collectValidDustLedgerEvents(
      indexerWsClient,
      indexerEventCoordinator,
      1,
    );
    previousMaxLedgerId = beforeDustEvents[0].data!.dustLedgerEvents.maxId;
    log.debug(`Previous max ledger ID before tx = ${previousMaxLedgerId}`);

    // Capture the highest zswapEndIndex before the transaction from genesis block.
    // E2E tests run on a fresh environment, so genesis provides the baseline zswap state.
    const genesisResponse = await indexerHttpClient.getBlockByOffset({ height: 0 });
    const genesisTxs = genesisResponse.data!.block.transactions;
    zswapEndIndexBeforeTx = genesisTxs.reduce((max, tx) => {
      const regularTx = tx as RegularTransaction;
      return regularTx.zswapEndIndex != null && regularTx.zswapEndIndex > max
        ? regularTx.zswapEndIndex
        : max;
    }, 0);
    log.debug(`Highest zswapEndIndex from genesis = ${zswapEndIndexBeforeTx}`);

    dustCommitmentEndIndexBeforeTx = genesisTxs.reduce((max, tx) => {
      const regularTx = tx as RegularTransaction;
      return regularTx.dustCommitmentEndIndex != null && regularTx.dustCommitmentEndIndex > max
        ? regularTx.dustCommitmentEndIndex
        : max;
    }, 0);
    log.debug(`Highest dustCommitmentEndIndex from genesis = ${dustCommitmentEndIndexBeforeTx}`);

    // Submit one shielded->shielded transfer (1 STAR)
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

    await resolveBlockHash(transactionResult);
  }, 200_000);

  afterAll(async () => {
    await Promise.all([toolkit.stop(), indexerWsClient.connectionClose()]);
  });

  describe('a successful shielded transaction transferring 1 Shielded Token between two wallets', async () => {
    /**
     * Once a shielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction in the block through a block query by hash, using the block hash reported by the toolkit.
     *
     * @given a confirmed shielded transaction between two wallets
     * @when we query the indexer with a block query by hash, using the block hash reported by the toolkit
     * @then the block should contain the expected transaction
     */
    test('should be reported by the indexer through a block query by hash', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash', 'ShieldedTokens'],
        testKey: 'PM-17709',
      };

      ctx.skip?.(
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
    test('should be reported by the indexer through a transaction query by hash', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Transaction', 'ByHash', 'ShieldedTokens'],
        testKey: 'PM-17710',
      };

      ctx.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      log.info(
        `Verifying indexer reports a shielded transaction by hash: ${transactionResult.txHash}`,
      );
      // The expected transaction might take a bit more to show up by indexer, so we retry a few times
      const transactionResponse = await getTransactionByHashWithRetry(transactionResult.txHash!);
      expect(transactionResponse).toBeSuccess();
      expect(transactionResponse?.data?.transactions).toBeDefined();
      expect(transactionResponse?.data?.transactions?.length).toBeGreaterThan(0);

      const tx = transactionResponse!.data!.transactions!.find(
        (t: Transaction) => t.hash === transactionResult.txHash,
      );

      expect(tx).toBeDefined();

      // Validate transaction shape and narrow type using schema
      const parsed = RegularTransactionSchema.safeParse(tx);
      expect(parsed.success, JSON.stringify(parsed.error?.format(), null, 2)).toBe(true);

      const regularTx = parsed.data!;

      // Shielded transactions do NOT expose unshielded details
      expect(regularTx.unshieldedCreatedOutputs).toEqual([]);
      expect(regularTx.unshieldedSpentOutputs).toEqual([]);

      // Fees are Substrate weight values (not actual DUST fees) until PM-20972/PM-20973
      expect(BigInt(regularTx.fees.paidFees)).toBeGreaterThanOrEqual(0n);
      expect(BigInt(regularTx.fees.estimatedFees)).toBeGreaterThanOrEqual(0n);
    });

    /**
     * After a shielded transaction is confirmed, the indexer streams the Zswap
     * events in sequence, followed by a DustSpendProcessed event.
     *
     * @given a confirmed shielded transaction
     * @when we subscribe to Zswap events starting from (previousMaxId + 1)
     * @then the Zswap events are delivered in order
     * @and the following event is DustSpendProcessed
     */
    test(
      'should stream Zswap events followed by DustSpendProcessed after a shielded transaction',
      { timeout: 90_000 },
      async () => {
        // Reconnect WS client - the connection may have gone stale during the long toolkit transaction
        await indexerWsClient.connectionClose();
        await indexerWsClient.connectionInit();

        const received = await collectValidZswapEvents(
          indexerWsClient,
          indexerEventCoordinator,
          3,
          previousMaxLedgerId + 1,
          30_000,
        );
        expect(received).toHaveLength(3);

        received.forEach((msg) => {
          const event = msg.data!.zswapLedgerEvents;
          const parsed = ZswapLedgerEventSchema.safeParse(event);
          expect(
            parsed.success,
            `Schema error: ${JSON.stringify(parsed.error?.format(), null, 2)}`,
          ).toBe(true);
        });

        // Validate Zswap event grouping and ordering
        const events = received.map((m) => m.data!.zswapLedgerEvents);
        expect(new Set(events.map((e) => e.maxId)).size).toBe(1);

        events.slice(1).forEach((e, i) => {
          expect(e.id).toBe(events[i].id + 1);
        });

        const lastZswapMaxId = received.at(-1)!.data!.zswapLedgerEvents.maxId;

        // verify the Dust event directly follows the Zswap events
        const dustEvents = await collectValidDustLedgerEvents(
          indexerWsClient,
          indexerEventCoordinator,
          1,
          lastZswapMaxId + 1,
          30_000,
        );
        expect(dustEvents).toHaveLength(1);
        const dust = dustEvents[0].data!.dustLedgerEvents;
        expect(dust.__typename).toBe('DustSpendProcessed');
        expect(dust.id).toBe(lastZswapMaxId + 1);
      },
    );

    /**
     * After a shielded transaction is confirmed, the zswap Merkle tree should grow.
     * The zswapEndIndex of the transaction should be higher than the previous maximum.
     *
     * @given a confirmed shielded transaction
     * @when we query the transaction from the indexer
     * @then the transaction's zswapEndIndex should be greater than the zswapEndIndex before the transaction
     */
    test('should increase the zswap Merkle tree end index', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Transaction', 'Zswap', 'ShieldedTokens'],
      };

      ctx.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      const transactionResponse = await getTransactionByHashWithRetry(transactionResult.txHash);
      expect(transactionResponse).toBeSuccess();

      const transactions = transactionResponse.data!.transactions;
      const tx = transactions.find((t: Transaction) => t.hash === transactionResult.txHash);
      expect(tx).toBeDefined();

      const regularTx = tx as RegularTransaction;
      expect(regularTx.zswapEndIndex).toBeDefined();
      expect(regularTx.zswapEndIndex!).toBeGreaterThan(zswapEndIndexBeforeTx);

      log.debug(
        `zswapEndIndex before tx: ${zswapEndIndexBeforeTx}, after tx: ${regularTx.zswapEndIndex}`,
      );
    });

    /**
     * After a shielded transaction is confirmed, the dust commitment Merkle tree should grow.
     * The dustCommitmentEndIndex of the transaction should be higher than the previous maximum.
     *
     * @given a confirmed shielded transaction
     * @when we query the transaction from the indexer
     * @then the transaction's dustCommitmentEndIndex should be greater than the dustCommitmentEndIndex before the transaction
     */
    test('should increase the dust commitment Merkle tree end index', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Transaction', 'Dust', 'CommitmentMerkleTree', 'ShieldedTokens'],
      };

      ctx.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      const transactionResponse = await getTransactionByHashWithRetry(transactionResult.txHash);
      expect(transactionResponse).toBeSuccess();

      const transactions = transactionResponse.data!.transactions;
      const tx = transactions.find(
        (t: Transaction) => t.hash === transactionResult.txHash,
      );
      expect(tx).toBeDefined();

      const regularTx = tx as RegularTransaction;
      expect(regularTx.dustCommitmentEndIndex).toBeDefined();
      expect(regularTx.dustCommitmentEndIndex!).toBeGreaterThan(dustCommitmentEndIndexBeforeTx);

      log.debug(`dustCommitmentEndIndex before tx: ${dustCommitmentEndIndexBeforeTx}, after tx: ${regularTx.dustCommitmentEndIndex}`);
    });

    /**
     * Once a shielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through an shielded transaction event for the source viewing key.
     *
     * @given we subscribe to shielded transaction events for the source viewing key
     * @when we submit an shielded transaction to node
     * @then we should receive a transaction event that includes transaction details for the source viewing key
     */
    test.todo(
      'should be reported by the indexer through an shielded transaction event for the source address',
      async () => {
        // Implement me
      },
    );

    /**
     * Once a shielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through an shielded transaction event for the destination viewing key.
     *
     * @given we subscribe to shielded transaction events for the destination viewing key
     * @when we submit an shielded transaction to node
     * @then we should receive a transaction event that includes transaction details for the destination viewing key
     */
    test.todo(
      'should be reported by the indexer through an shielded transaction event for the destination address',
      async () => {
        // Implement me
      },
    );

    /**
     * Once an shielded transaction has been submitted to node and confirmed, we should see the transaction
     * giving 1 shielded token to the destination address.
     *
     * @given a confirmed shielded transaction between two wallets
     * @when we inspect the containing block for shielded transaction details
     * @then there should be a balance change that reflects the transfer of 1 shielded token
     */
    test.todo(
      'should have transferred 1 token from the source to the destination address',
      async () => {
        // Implement me but... can we really implement this test? We need to be able to view the transaction details in
        // the block and use the viewing key for that. Does the toolkit offer that level of support?
      },
    );
  });
});
