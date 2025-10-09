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
import '@utils/logging/test-logging-hooks';
import { TestContext } from 'vitest';

import { ToolkitWrapper, ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { Transaction, UnshieldedTransaction, UnshieldedUtxo } from '@utils/indexer/indexer-types';
import {
  IndexerWsClient,
  UnshieldedTransactionSubscriptionParams,
  UnshieldedTxSubscriptionResponse,
} from '@utils/indexer/websocket-client';
import { getBlockByHashWithRetry } from './test-utils';
import { waitForEventsStabilization } from './test-utils';

// To run: yarn test e2e
describe('unshielded transactions', () => {
  let indexerWsClient: IndexerWsClient;
  let indexerHttpClient: IndexerHttpClient;

  // Toolkit instance for generating and submitting transactions
  let toolkit: ToolkitWrapper;

  // Result of the unshielded transaction submitted to node
  let transactionResult: ToolkitTransactionResult;

  // Addresses for the source and destination wallets, derived from their seeds
  let sourceAddress: string;
  let destinationAddress: string;

  // Events from the indexer websocket for both the source and destination addresses
  let sourceAddressEvents: UnshieldedTxSubscriptionResponse[] = [];
  let destinationAddressEvents: UnshieldedTxSubscriptionResponse[] = [];

  // Functions to unsubscribe from the indexer websocket for both the source and destination addresses
  let sourceAddrUnscribeFromEvents: () => void;
  let destAddrUnscribeFromEvents: () => void;

  beforeAll(async () => {
    indexerHttpClient = new IndexerHttpClient();
    indexerWsClient = new IndexerWsClient();

    // Connecting to the indexer websocket
    await indexerWsClient.connectionInit();

    toolkit = new ToolkitWrapper({});
    await toolkit.start();

    const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';
    const destinationSeed = '0000000000000000000000000000000000000000000000000000000987654321';

    // Getting the addresses from their seeds
    sourceAddress = (await toolkit.showAddress(sourceSeed)).unshielded;
    destinationAddress = (await toolkit.showAddress(destinationSeed)).unshielded;

    // Creating the unshielded transaction subscription parameter for the source address (just the address)
    let unshieldedTransactionParam: UnshieldedTransactionSubscriptionParams = {
      address: sourceAddress,
    };

    // Creating the unshielded transaction subscription handler, we will record all relevant events for the source address
    let sourceAddrUnshieldedTxSubscriptionHandler = {
      next: (event: UnshieldedTxSubscriptionResponse) => {
        sourceAddressEvents.push(event);
      },
    };

    // Subscribe to unshielded transaction events for the source address
    sourceAddrUnscribeFromEvents = indexerWsClient.subscribeToUnshieldedTransactionEvents(
      sourceAddrUnshieldedTxSubscriptionHandler,
      unshieldedTransactionParam,
    );

    // Creating the unshielded transaction subscription parameter for the destination address (just the address)
    unshieldedTransactionParam = {
      address: destinationAddress,
    };

    // Creating the unshielded transaction subscription handler, we will record all relevant events for the source address
    let destAddrUnshieldedTxSubscriptionHandler = {
      next: (event: UnshieldedTxSubscriptionResponse) => {
        destinationAddressEvents.push(event);
      },
    };

    // Subscribe to unshielded transaction events for the destination address
    destAddrUnscribeFromEvents = indexerWsClient.subscribeToUnshieldedTransactionEvents(
      destAddrUnshieldedTxSubscriptionHandler,
      unshieldedTransactionParam,
    );

    // Wait until source events count stabilizes, then snapshot to historical array
    let drained = await waitForEventsStabilization(sourceAddressEvents, 500);
    log.info(`Source events count stabilized: ${drained.length}`);

    // Wait until destination events count stabilizes, then snapshot to historical array
    drained = await waitForEventsStabilization(destinationAddressEvents, 1000);
    log.info(`Destination events count stabilized: ${drained.length}`);

    // Generating and submitting the transaction to node
    transactionResult = await toolkit.generateSingleTx(
      sourceSeed,
      'unshielded',
      destinationAddress,
      1,
    );
  }, 200_000);

  afterAll(async () => {
    // Unsubscribe from the unshielded transaction events for the source and destination addresses
    sourceAddrUnscribeFromEvents();
    destAddrUnscribeFromEvents();

    // Let's trigger these operations in parallel
    await Promise.all([toolkit.stop(), indexerWsClient.connectionClose()]);
  });

  describe('a successful unshielded transaction transferring 1 NIGHT between two wallets', async () => {
    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction in the block through a block query by hash, using the block hash reported by the toolkit.
     *
     * @given a confirmed unshielded transaction between two wallets
     * @when we query the indexer with a block query by hash, using the block hash reported by the toolkit
     * @then the block should contain the transaction with outputs for both addresses
     */
    test('should be reported by the indexer through a block query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash', 'UnshieldedToken'],
        testKey: 'PM-17711',
      };

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
      const sourceAddresInTx = blockResponse.data?.block?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find((output: any) => output.owner === sourceAddress),
      );

      const destAddresInTx = blockResponse.data?.block?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find((output: any) => output.owner === destinationAddress),
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
    test('should be reported by the indexer through a transaction query by hash', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Query', 'Transaction', 'ByHash', 'UnshieldedToken'],
        testKey: 'PM-17712',
      };

      context.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // The expected transaction might take a bit more to show up by indexer, so we retry a few times
      const transactionResponse = await indexerHttpClient.getShieldedTransaction({
        hash: transactionResult.txHash,
      });

      // Verify the transaction appears in the block
      expect(transactionResponse?.data?.transactions).toBeDefined();
      expect(
        transactionResponse?.data?.transactions?.length,
        'No transactions found',
      ).toBeGreaterThan(0);

      // Find our specific transaction by hash
      const sourceAddresInTx = transactionResponse.data?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find((output: any) => output.owner === sourceAddress),
      );

      const destAddresInTx = transactionResponse.data?.transactions?.find((tx: Transaction) =>
        tx.unshieldedCreatedOutputs?.find((output: any) => output.owner === destinationAddress),
      );

      expect(sourceAddresInTx).toBeDefined();
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
    test('should be reported by the indexer through an unshielded transaction event for the source address', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Subscription', 'Transaction', 'UnshieldedToken'],
        testKey: 'PM-17713',
      };

      context.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // Wait for the unshielded transaction event for the source address to be reported by the indexer
      // through the unshielded transaction subscription. Note this is an async operation, so we need
      // to try a few times and wait for the event to be reported.
      let sourceAddressEvent: UnshieldedTxSubscriptionResponse | undefined;
      for (let attempt = 0; attempt < 3 && sourceAddressEvent == null; attempt++) {
        sourceAddressEvent = sourceAddressEvents.find((event) => {
          const txEvent = event.data?.unshieldedTransactions as UnshieldedTransaction;
          return (
            txEvent.__typename === 'UnshieldedTransaction' &&
            txEvent.createdUtxos?.some((utxo: UnshieldedUtxo) => utxo.owner === sourceAddress) &&
            txEvent.spentUtxos?.some((utxo: UnshieldedUtxo) => utxo.owner === sourceAddress)
          );
        });
        if (sourceAddressEvent == null && attempt < 2) {
          await new Promise((resolve) => setTimeout(resolve, 1000));
        }
      }
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
    test('should be reported by the indexer through an unshielded transaction event for the destination address', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['Subscription', 'Transaction', 'UnshieldedToken'],
        testKey: 'PM-17714',
      };

      context.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      // Wait for the unshielded transaction event for the destination address to be reported by the indexer
      // through the unshielded transaction subscription. Note this is an async operation, so we need
      // to try a few times and wait for the event to be reported.
      let destinationAddressEvent: UnshieldedTxSubscriptionResponse | undefined;
      for (let attempt = 0; attempt < 3 && destinationAddressEvent == null; attempt++) {
        destinationAddressEvent = destinationAddressEvents.find((event) => {
          const txEvent = event.data?.unshieldedTransactions as UnshieldedTransaction;
          return (
            txEvent.__typename === 'UnshieldedTransaction' &&
            txEvent.createdUtxos?.some((utxo: UnshieldedUtxo) => utxo.owner === destinationAddress)
          );
        });
        if (destinationAddressEvent == null && attempt < 2) {
          await new Promise((resolve) => setTimeout(resolve, 1000));
        }
      }
      expect(destinationAddressEvent).toBeDefined();
    });

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, we should see the transaction
     * giving 1 NIGHT to the destination address.
     *
     * @given a confirmed unshielded transaction between two wallets
     * @when we inspect the containing block for unshielded outputs
     * @then there should be two created outputs and one spent output reflecting the transfer of 1 NIGHT
     */
    test('should have transferred 1 NIGHT from the source to the destination address', async (context: TestContext) => {
      context.task!.meta.custom = {
        labels: ['UnshieldedTokens'],
        testKey: 'PM-17715',
      };

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
        log.info(`Transaction ${tx.hash}: hasCreated=${hasCreated}, hasSpent=${hasSpent}`);
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
  });
});
