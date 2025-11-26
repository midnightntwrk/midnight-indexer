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

import { TestContext } from 'vitest';
import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import { retry } from '@utils/retry-helper';
import dataProvider from '@utils/testdata-provider';
import { getBlockByHashWithRetry, setupWalletEventSubscriptions } from './test-utils';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { ToolkitWrapper, ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';
import {
  Transaction,
  UnshieldedTransaction,
  UnshieldedTransactionEvent,
  UnshieldedTransactionsProgress,
  UnshieldedUtxo,
} from '@utils/indexer/indexer-types';
import { IndexerWsClient, UnshieldedTxSubscriptionResponse } from '@utils/indexer/websocket-client';

describe('unshielded transactions', { timeout: 200_000 }, () => {
  let indexerWsClient: IndexerWsClient;
  let indexerHttpClient: IndexerHttpClient;

  // Toolkit instance for generating and submitting transactions
  let toolkit: ToolkitWrapper;

  // Result of the unshielded transaction submitted to node
  let transactionResult: ToolkitTransactionResult;

  let walletFixture: Awaited<ReturnType<typeof setupWalletEventSubscriptions>>;

  let sourceSeed: string;

  // Addresses for the source and destination wallets, derived from their seeds
  let destinationAddress: string;

  beforeAll(async () => {
    indexerHttpClient = new IndexerHttpClient();
    indexerWsClient = new IndexerWsClient();

    // Connecting to the indexer websocket
    await indexerWsClient.connectionInit();

    toolkit = new ToolkitWrapper({});
    await toolkit.start();

    const seedA = dataProvider.getFundingSeed();
    const seedB = '0000000000000000000000000000000000000000000000000000000987654321';

    walletFixture = await setupWalletEventSubscriptions(toolkit, indexerWsClient, seedA, [seedB]);

    // Extract for convenience
    sourceSeed = walletFixture.source.seed;

    destinationAddress = walletFixture.destinations[0].destinationAddress;

    // Submit a single unshielded transaction (1 STAR) from source → destination
    transactionResult = await toolkit.generateSingleTx(
      sourceSeed,
      'unshielded',
      destinationAddress,
      1,
    );
  }, 200_000);

  afterAll(async () => {
    // Unsubscribe from the unshielded transaction events for the source and destination addresses
    walletFixture.source.unsubscribe();
    walletFixture.destinations.forEach((d) => d.unsubscribe());

    // Let's trigger these operations in parallel
    await Promise.all([toolkit.stop(), indexerWsClient.connectionClose()]);
  });

  /**
   * Helper function to find a progress update event with an incremented transaction ID.
   * This is the logic used inside the retry function for both source and destination address tests.
   *
   * @param events - The events array to search
   * @param baselineTransactionId - The transaction ID to compare against
   * @param addressLabel - Label for error messages (e.g., 'source' or 'destination')
   * @returns The found event
   * @throws Error if no matching event is found
   */
  function findProgressUpdateEvent(
    events: UnshieldedTxSubscriptionResponse[],
    baselineTransactionId: number,
    addressLabel: string,
  ): UnshieldedTxSubscriptionResponse {
    const event = events.find((event) => {
      const txEvent = event.data?.unshieldedTransactions as UnshieldedTransactionEvent;

      log.debug(`waiting for UnshieldedTransactionsProgress event`);
      if (txEvent.__typename === 'UnshieldedTransactionsProgress') {
        const progressUpdate = txEvent as UnshieldedTransactionsProgress;
        log.debug(`progressUpdate received: ${JSON.stringify(progressUpdate, null, 2)}`);
        if (progressUpdate.highestTransactionId > baselineTransactionId) {
          return true;
        }
      }
    });
    if (!event) {
      throw new Error(`${addressLabel} address progress update event not found yet`);
    }
    return event;
  }

  describe('a successful unshielded transaction transferring 1 STAR between two addresses', async () => {
    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction in the block through a block query by hash, using the block hash reported by the toolkit.
     *
     * @given a confirmed unshielded transaction between two wallets
     * @when we query the indexer with a block query by hash, using the block hash reported by the toolkit
     * @then the block should contain the transaction with outputs for both addresses
     */
    test('should be reported by the indexer through a block query by hash', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash', 'UnshieldedTokens'],
        testKey: 'PM-17711',
      };

      ctx.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // The expected block might take a bit more to show up by indexer, so we retry a few times
      const blockResponse = await getBlockByHashWithRetry(transactionResult.blockHash!);

      // Verify the transaction appears in the block
      expect(blockResponse?.data?.block?.transactions).toBeDefined();
      expect(blockResponse?.data?.block?.transactions?.length).toBeGreaterThan(0);

      // Find our specific transaction by hash
      const sourceAddresInTx = blockResponse.data?.block?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find(
          (output: UnshieldedUtxo) => output.owner === walletFixture.source.address,
        ),
      );

      const destAddresInTx = blockResponse.data?.block?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find(
          (output: UnshieldedUtxo) =>
            output.owner === walletFixture.destinations[0].destinationAddress,
        ),
      );

      expect(sourceAddresInTx).toBeDefined();
      expect(destAddresInTx).toBeDefined();
    });

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through a query by transaction hash, using the transaction hash reported by the toolkit.
     *
     * @given a confirmed unshielded transaction between two wallets
     * @when we query transactions by the transaction hash
     * @then the returned transactions should include outputs for both addresses involved
     */
    test('should be reported by the indexer through a transaction query by hash', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Transaction', 'ByHash', 'UnshieldedTokens'],
        testKey: 'PM-17712',
      };

      ctx.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // The expected transaction might take a bit more to show up by indexer, so we retry a few times
      const transactionResponse = await indexerHttpClient.getTransactionByOffset({
        hash: transactionResult.txHash,
      });

      // Verify the transaction appears in the block
      expect(transactionResponse?.data?.transactions).toBeDefined();
      expect(
        transactionResponse?.data?.transactions?.length,
        'No transactions found',
      ).toBeGreaterThan(0);

      // Find our specific transaction that contains unshielded created outputs for the source address
      const sourceAddresInTx = transactionResponse.data?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find(
          (output: UnshieldedUtxo) => output.owner === walletFixture.source.address,
        ),
      );
      expect(sourceAddresInTx).toBeDefined();

      // Find our specific transaction that contains unshielded created outputs for the destination address
      const destAddresInTx = transactionResponse.data?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find(
          (output: UnshieldedUtxo) =>
            output.owner === walletFixture.destinations[0].destinationAddress,
        ),
      );
      expect(destAddresInTx).toBeDefined();
    });

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through an unshielded transaction event for the source address.
     *
     * @given we subscribe to unshielded transaction events for the source address
     * @when we submit an unshielded transaction to node
     * @then we should receive a transaction event that includes created and spent UTXOs for the source address
     */
    test('should be reported by the indexer through an unshielded transaction event for the source address', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Subscription', 'Transaction', 'UnshieldedTokens'],
        testKey: 'PM-17713',
      };

      ctx.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // Wait for the unshielded transaction event for the source address to be reported by the indexer
      // through the unshielded transaction subscription. Note this is an async operation, so we need
      // to retry a few times.
      const sourceAddressEvent = await retry(
        async () => {
          const event = walletFixture.source.events.find((event) => {
            const txEvent = event.data?.unshieldedTransactions as UnshieldedTransaction;
            return (
              txEvent.__typename === 'UnshieldedTransaction' &&
              txEvent.createdUtxos?.some(
                (utxo: UnshieldedUtxo) => utxo.owner === walletFixture.source.address,
              ) &&
              txEvent.spentUtxos?.some(
                (utxo: UnshieldedUtxo) => utxo.owner === walletFixture.source.address,
              )
            );
          });
          if (!event) {
            throw new Error('Source address transaction event not found yet');
          }
          return event;
        },
        {
          maxRetries: 10,
          delayMs: 3000,
          retryLabel: 'find source address transaction event',
        },
      );
      expect(sourceAddressEvent).toBeDefined();
    });

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through an unshielded transaction event for the destination address.
     *
     * @given we subscribe to unshielded transaction events for the destination address
     * @when we submit an unshielded transaction to node
     * @then we should receive a transaction event that includes a created UTXO for the destination
     */
    test('should be reported by the indexer through an unshielded transaction event for the destination address', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Subscription', 'Transaction', 'UnshieldedTokens'],
        testKey: 'PM-17714',
      };

      ctx.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // Wait for the unshielded transaction event for the destination address to be reported by the indexer
      // through the unshielded transaction subscription. Note this is an async operation, so we need
      // to retry a few times.
      const destinationAddressEvent = await retry(
        async () => {
          const event = walletFixture.destinations[0].events.find((event) => {
            const txEvent = event.data?.unshieldedTransactions as UnshieldedTransaction;
            return (
              txEvent.__typename === 'UnshieldedTransaction' &&
              txEvent.createdUtxos?.some(
                (utxo: UnshieldedUtxo) =>
                  utxo.owner === walletFixture.destinations[0].destinationAddress,
              )
            );
          });

          if (!event) {
            throw new Error('Destination address transaction event not found yet');
          }
          return event;
        },
        {
          maxRetries: 10,
          delayMs: 3000,
          retryLabel: 'find destination address transaction event',
        },
      );
      expect(destinationAddressEvent).toBeDefined();
    });

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, we should see the transaction
     * giving 1 STAR to the destination address.
     *
     * @given a confirmed unshielded transaction between two wallets
     * @when we inspect the containing block for unshielded outputs
     * @then there should be two created outputs and one spent output reflecting the transfer of 1 STAR
     */
    test('should have transferred 1 STAR from the source to the destination address', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['UnshieldedTokens'],
        testKey: 'PM-17715',
      };

      ctx.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // The expected block might take a bit more to show up by indexer, so we retry a few times
      const blockResponse = await getBlockByHashWithRetry(transactionResult.blockHash!);

      // Find the transaction with unshielded outputs
      const unshieldedTx = blockResponse.data?.block?.transactions?.find((tx: Transaction) => {
        const hasCreated = tx.unshieldedCreatedOutputs && tx.unshieldedCreatedOutputs.length > 0;
        const hasSpent = tx.unshieldedSpentOutputs && tx.unshieldedSpentOutputs.length > 0;
        log.info(`Transaction ${tx.hash}: hasCreated=${hasCreated}, hasSpent=${hasSpent}`);
        return hasCreated || hasSpent;
      });

      expect(unshieldedTx).toBeDefined();

      // Validate unshieldedCreatedOutputs - should have 2 entries
      expect(unshieldedTx?.unshieldedCreatedOutputs).toHaveLength(2);

      const createdOutputs = unshieldedTx?.unshieldedCreatedOutputs;
      const sourceOutput = createdOutputs?.find(
        (output: UnshieldedUtxo) => output.owner === walletFixture.source.address,
      );
      const destOutput = createdOutputs?.find(
        (output: UnshieldedUtxo) =>
          output.owner === walletFixture.destinations[0].destinationAddress,
      );

      expect(sourceOutput).toBeDefined();
      expect(destOutput).toBeDefined();
      expect(destOutput?.value).toBe('1');

      // Validate unshieldedSpentOutputs - should have 1 entry
      expect(unshieldedTx?.unshieldedSpentOutputs).toHaveLength(1);

      const spentOutput = unshieldedTx?.unshieldedSpentOutputs?.[0];
      expect(spentOutput?.owner).toBe(walletFixture.source.address);
    });

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through a progress update event for the source address.
     *
     * @given we subscribe to unshielded transaction events for the source address
     * @when we submit an unshielded transaction to node
     * @then we should receive a progress update event from indexer
     * @and the progress count should be incremented by 1
     */
    test('should be reported by the indexer through a progress update event for the source address', async () => {
      const progressUpdatesBeforeTransaction = walletFixture.source.historicalEvents.filter(
        (event) => {
          return event.data?.unshieldedTransactions.__typename === 'UnshieldedTransactionsProgress';
        },
      );

      log.debug('Progress updates before transaction:');
      progressUpdatesBeforeTransaction!.forEach((update) => {
        log.debug(`${JSON.stringify(update, null, 2)}`);
      });

      const highestTransactionIdBeforeTransaction = (
        progressUpdatesBeforeTransaction![progressUpdatesBeforeTransaction!.length - 1].data
          ?.unshieldedTransactions as UnshieldedTransactionsProgress
      ).highestTransactionId;
      log.info(
        `Highest transaction ID before transaction: ${highestTransactionIdBeforeTransaction}`,
      );

      const progressUpdatesAfterTransaction = walletFixture.source.events.filter((event) => {
        return event.data?.unshieldedTransactions.__typename === 'UnshieldedTransactionsProgress';
      });

      log.debug('Progress updates after transaction:');
      progressUpdatesAfterTransaction!.forEach((update) => {
        log.debug(`${JSON.stringify(update, null, 2)}`);
      });

      // Wait for the progress update event for the source address to be reported by the indexer
      // through the unshielded transaction subscription. Note this is an async operation, so we need
      // to retry a few times.
      const sourceAddressEvent = await retry(
        async () =>
          findProgressUpdateEvent(
            walletFixture.source.events,
            highestTransactionIdBeforeTransaction,
            'source',
          ),
        {
          maxRetries: 10,
          delayMs: 3000,
          retryLabel: 'find source address progress update event',
        },
      );

      expect(sourceAddressEvent).toBeDefined();
      const highestTransactionIdAfterTransaction = (
        sourceAddressEvent.data?.unshieldedTransactions as UnshieldedTransactionsProgress
      ).highestTransactionId;
      log.info(`Highest transaction ID after transaction: ${highestTransactionIdAfterTransaction}`);
      expect(highestTransactionIdAfterTransaction).toBeGreaterThan(
        highestTransactionIdBeforeTransaction,
      );
    });

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through a progress update event for the destination address.
     *
     * @given we subscribe to unshielded transaction events for the destination address
     * @when we submit an unshielded transaction to node
     * @then we should receive a progress update event from indexer
     * @and the progress count should be incremented by 1
     */
    test('should be reported by the indexer through a progress update event for the destination address', async () => {
      const progressUpdatesBeforeTransaction =
        walletFixture.destinations[0].historicalDestinationEvents.filter((event) => {
          return event.data?.unshieldedTransactions.__typename === 'UnshieldedTransactionsProgress';
        });

      log.debug('Progress updates before transaction:');
      progressUpdatesBeforeTransaction!.forEach((update) => {
        log.debug(`${JSON.stringify(update, null, 2)}`);
      });

      const highestTransactionIdBeforeTransaction = (
        progressUpdatesBeforeTransaction![progressUpdatesBeforeTransaction!.length - 1].data
          ?.unshieldedTransactions as UnshieldedTransactionsProgress
      ).highestTransactionId;
      log.info(
        `Highest transaction ID before transaction: ${highestTransactionIdBeforeTransaction}`,
      );

      const progressUpdatesAfterTransaction = walletFixture.destinations[0].events.filter(
        (event) => {
          return event.data?.unshieldedTransactions.__typename === 'UnshieldedTransactionsProgress';
        },
      );

      log.debug('Progress updates after transaction:');
      progressUpdatesAfterTransaction!.forEach((update) => {
        log.debug(`${JSON.stringify(update, null, 2)}`);
      });

      // Wait for the progress update event for the destination address to be reported by the indexer
      // through the unshielded transaction subscription. Note this is an async operation, so we need
      // to retry a few times.
      const destinationAddressEvent = await retry(
        async () =>
          findProgressUpdateEvent(
            walletFixture.destinations[0].events,
            highestTransactionIdBeforeTransaction,
            'destination',
          ),
        {
          maxRetries: 10,
          delayMs: 3000,
          retryLabel: 'find destination address progress update event',
        },
      );

      expect(destinationAddressEvent).toBeDefined();
      const highestTransactionIdAfterTransaction = (
        destinationAddressEvent.data?.unshieldedTransactions as UnshieldedTransactionsProgress
      ).highestTransactionId;
      log.info(`Highest transaction ID after transaction: ${highestTransactionIdAfterTransaction}`);
      expect(highestTransactionIdAfterTransaction).toBeGreaterThan(
        highestTransactionIdBeforeTransaction,
      );
    });
  });
});
