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

import { TestContext } from 'vitest';
import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import { retry } from '@utils/retry-helper';
import dataProvider from '@utils/testdata-provider';
import {
  getBlockByHashWithRetry,
  getEventsOfType,
  retrySimple,
  setupWalletEventSubscriptions,
  resolveBlockHash,
  waitForEventsStabilization,
} from './test-utils';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { ToolkitWrapper, ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';
import {
  RegularTransaction,
  Transaction,
  UnshieldedTransaction,
  UnshieldedTransactionEvent,
  UnshieldedTransactionsProgress,
  UnshieldedUtxo,
  isUnshieldedTransaction,
} from '@utils/indexer/indexer-types';
import { IndexerWsClient, UnshieldedTxSubscriptionResponse } from '@utils/indexer/websocket-client';
import { collectValidDustLedgerEvents } from 'tests/shared/dust-ledger-utils';
import { EventCoordinator } from '@utils/event-coordinator';
import { DustLedgerEventsUnionSchema } from '@utils/indexer/graphql/schema';

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
      const progressUpdate = txEvent;
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

/**
 * Validates that an unshielded transaction is reported consistently across the event streams of
 * the source and the destination wallet.
 *
 * Each destination transaction is paired with the source transaction carrying the same hash, then
 * deep-checked for identical transaction identity, UTXO ownership and output indices, creation
 * time, dust registration flags, spent-UTXO cross-links and value conservation.
 *
 * @param srcTxs - Events emitted for the **source** wallet.
 * @param destTxs - Events emitted for the **destination** wallet.
 * @param srcAddr - Source wallet address, expected to own the change UTXO at outputIndex 1.
 * @param destAddr - Destination wallet address, expected to own the created UTXO at outputIndex 0.
 * @param expectedValue - Value the destination is expected to receive.
 *
 * - Uses `isUnshieldedTransaction()` to filter out `UnshieldedTransactionsProgress` events.
 */
function validateCrossWalletTransaction(
  srcTxs: UnshieldedTransactionEvent[],
  destTxs: UnshieldedTransactionEvent[],
  srcAddr: string,
  destAddr: string,
  expectedValue: string,
) {
  const validSrcTxs = srcTxs.filter(isUnshieldedTransaction);
  const validDestTxs = destTxs.filter(isUnshieldedTransaction);

  log.debug(validSrcTxs, `Source transactions for ${srcAddr}`);
  log.debug(validDestTxs, `Destination transactions for ${destAddr}`);

  if (!validDestTxs.length) {
    throw new Error(`No UnshieldedTransaction events for ${destAddr} — expected at least one.`);
  }

  validDestTxs.forEach((destTx) => {
    const srcTx = validSrcTxs.find((s) => s.transaction.hash === destTx.transaction.hash);
    if (!srcTx) {
      throw new Error(`No matching source transaction found for hash ${destTx.transaction.hash}`);
    }

    const srcUtxo = srcTx.createdUtxos[0];
    const destUtxo = destTx.createdUtxos[0];

    // Value & identity
    expect(destUtxo.value).toBe(expectedValue);
    expect(BigInt(srcUtxo.value)).toBeGreaterThan(BigInt(destUtxo.value));
    expect(destTx.transaction.hash).toBe(srcTx.transaction.hash);
    expect(destTx.transaction.id).toBe(srcTx.transaction.id);

    // Ownership & indices
    expect(srcUtxo.owner).toBe(srcAddr);
    expect(destUtxo.owner).toBe(destAddr);
    expect(destUtxo.outputIndex).toBe(0);
    expect(srcUtxo.outputIndex).toBe(1);

    // Creation time alignment
    expect(srcUtxo.ctime).toBe(destUtxo.ctime);

    // Dust registration flags. This asymmetry holds only because the source is the funding wallet,
    // which is registered for dust generation, while the destinations are fresh, unregistered ones.
    expect(destUtxo.registeredForDustGeneration).toBe(false);
    expect(srcUtxo.registeredForDustGeneration).toBe(true);

    // Cross-link consistency
    expect(srcUtxo.createdAtTransaction.hash).toBe(destTx.transaction.hash);

    // The source funds the transfer by spending an input, so its stream must carry a spent UTXO.
    // Assert that rather than guarding on it, or the checks below silently disappear.
    expect(
      srcTx.spentUtxos,
      `No spent UTXO in the source stream for ${srcAddr} on hash ${destTx.transaction.hash}`,
    ).not.toHaveLength(0);

    const spent = srcTx.spentUtxos[0];
    expect(spent.spentAtTransaction.hash).toBe(destTx.transaction.hash);
    const spentTx = spent.spentAtTransaction as { hash: string; identifiers?: string[] };
    expect(spentTx.identifiers?.[0]).toBe(destTx.transaction.identifiers?.[0]);

    // Value conservation: the destination receives the spent input minus the change kept by source.
    expect(BigInt(destUtxo.value)).toBe(BigInt(spent.value) - BigInt(srcUtxo.value));

    log.debug(`Validation complete for hash=${destTx.transaction.hash}`);
  });
}

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
  let secondDestinationAddress: string;

  let indexerEventCoordinator: EventCoordinator;
  indexerEventCoordinator = new EventCoordinator();
  let previousMaxDustId: number;
  let dustCommitmentEndIndexBeforeTx: number;

  beforeAll(async () => {
    indexerHttpClient = new IndexerHttpClient();
    indexerWsClient = new IndexerWsClient();

    // Connecting to the indexer websocket
    await indexerWsClient.connectionInit();

    toolkit = new ToolkitWrapper({});
    await toolkit.start();

    const seedA = dataProvider.getFundingSeed();
    const seedB1 = '0000000000000000000000000000000000000000000000000000000987654321';
    // A second destination, subscribed on the same WS connection, so the multi-destination tests
    // below can assert the indexer routes each transfer to the intended recipient only.
    const seedB2 = '0000000000000000000000000000000000000000000000000000000123456789';

    walletFixture = await setupWalletEventSubscriptions(toolkit, indexerWsClient, seedA, [
      seedB1,
      seedB2,
    ]);

    // Extract for convenience
    sourceSeed = walletFixture.source.seed;

    destinationAddress = walletFixture.destinations[0].destinationAddress;
    secondDestinationAddress = walletFixture.destinations[1].destinationAddress;

    const beforeEvents = await collectValidDustLedgerEvents(
      indexerWsClient,
      indexerEventCoordinator,
      1,
    );
    previousMaxDustId = beforeEvents[0].data!.dustLedgerEvents.maxId;
    log.debug(`Previous max dust ID before tx = ${previousMaxDustId}`);

    // Capture the highest dustCommitmentEndIndex before the transaction from genesis block.
    // Guard against null data: older indexer deployments return a GraphQL validation error when
    // the query includes schema fields not yet in that version, which sets data to null.
    const genesisResponse = await indexerHttpClient.getBlockByOffset({ height: 0 });
    const genesisTxs = genesisResponse.data?.block?.transactions ?? [];
    dustCommitmentEndIndexBeforeTx = genesisTxs.reduce((max, tx) => {
      const regularTx = tx as RegularTransaction;
      return regularTx.dustCommitmentEndIndex != null && regularTx.dustCommitmentEndIndex > max
        ? regularTx.dustCommitmentEndIndex
        : max;
    }, 0);
    log.debug(`Highest dustCommitmentEndIndex from genesis = ${dustCommitmentEndIndexBeforeTx}`);

    // Submit a single unshielded transaction (1 STAR) from source → destination
    transactionResult = await toolkit.generateSingleTx(
      sourceSeed,
      'unshielded',
      destinationAddress,
      1,
    );

    await resolveBlockHash(transactionResult);
  }, 200_000);

  afterAll(async () => {
    // Unsubscribe from the unshielded transaction events for the source and destination addresses
    walletFixture.source.unsubscribe();
    walletFixture.destinations.forEach((d) => d.unsubscribe());

    // Let's trigger these operations in parallel
    await Promise.all([toolkit.stop(), indexerWsClient.connectionClose()]);
  });

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
      const blockResponse = await getBlockByHashWithRetry(transactionResult.blockHash);

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
          (output: UnshieldedUtxo) => output.owner === destinationAddress,
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
          (output: UnshieldedUtxo) => output.owner === destinationAddress,
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
      // The event is matched on the submitted transaction hash, not just on the owner: every e2e
      // file spends from the same funding wallet into the same destination seed, and the files run
      // in parallel workers, so an owner-only match can be satisfied by another file's transfer.
      const sourceAddressEvent = await retry(
        async () => {
          const event = walletFixture.source.events.find((event) => {
            const txEvent = event.data?.unshieldedTransactions as UnshieldedTransaction;
            return (
              txEvent.__typename === 'UnshieldedTransaction' &&
              txEvent.transaction.hash === transactionResult.txHash &&
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
              txEvent.transaction.hash === transactionResult.txHash &&
              txEvent.createdUtxos?.some(
                (utxo: UnshieldedUtxo) => utxo.owner === destinationAddress,
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
      const blockResponse = await getBlockByHashWithRetry(transactionResult.blockHash);

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
        (output: UnshieldedUtxo) => output.owner === destinationAddress,
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
    test(
      'should be reported by the indexer through a progress update event for the source address',
      { timeout: 400_000 },
      async () => {
        const progressUpdatesBeforeTransaction = walletFixture.source.historicalEvents.filter(
          (event) => {
            return (
              event.data?.unshieldedTransactions.__typename === 'UnshieldedTransactionsProgress'
            );
          },
        );

        log.debug('Progress updates before transaction:');
        progressUpdatesBeforeTransaction.forEach((update) => {
          log.debug(`${JSON.stringify(update, null, 2)}`);
        });

        const highestTransactionIdBeforeTransaction = (
          progressUpdatesBeforeTransaction.at(-1)?.data
            ?.unshieldedTransactions as UnshieldedTransactionsProgress
        ).highestTransactionId;
        log.info(
          `Highest transaction ID before transaction: ${highestTransactionIdBeforeTransaction}`,
        );

        const progressUpdatesAfterTransaction = walletFixture.source.events.filter((event) => {
          return event.data?.unshieldedTransactions.__typename === 'UnshieldedTransactionsProgress';
        });

        log.debug('Progress updates after transaction:');
        progressUpdatesAfterTransaction.forEach((update) => {
          log.debug(`${JSON.stringify(update, null, 2)}`);
        });

        // Wait for the progress update event for the source address to be reported by the indexer
        // through the unshielded transaction subscription. Since indexer 4.4.0 progress polling
        // backs off while a subscription is idle (30s doubling up to 240s, ±20% jitter), a
        // subscription opened well before the transaction may only report the change after the full
        // backed-off gap (~5 minutes worst case) — hence the wide retry window.
        const sourceAddressEvent = await retry(
          async () =>
            findProgressUpdateEvent(
              walletFixture.source.events,
              highestTransactionIdBeforeTransaction,
              'source',
            ),
          {
            maxRetries: 60,
            delayMs: 5000,
            retryLabel: 'find source address progress update event',
          },
        );

        expect(sourceAddressEvent).toBeDefined();
        const highestTransactionIdAfterTransaction = (
          sourceAddressEvent.data?.unshieldedTransactions as UnshieldedTransactionsProgress
        ).highestTransactionId;
        log.info(
          `Highest transaction ID after transaction: ${highestTransactionIdAfterTransaction}`,
        );
        expect(highestTransactionIdAfterTransaction).toBeGreaterThan(
          highestTransactionIdBeforeTransaction,
        );
      },
    );

    /**
     * Once an unshielded transaction has been submitted to node and confirmed, the indexer should report
     * that transaction through a progress update event for the destination address.
     *
     * @given we subscribe to unshielded transaction events for the destination address
     * @when we submit an unshielded transaction to node
     * @then we should receive a progress update event from indexer
     * @and the progress count should be incremented by 1
     */
    test(
      'should be reported by the indexer through a progress update event for the destination address',
      { timeout: 400_000 },
      async () => {
        const progressUpdatesBeforeTransaction =
          walletFixture.destinations[0].historicalDestinationEvents.filter((event) => {
            return (
              event.data?.unshieldedTransactions.__typename === 'UnshieldedTransactionsProgress'
            );
          });

        log.debug('Progress updates before transaction:');
        progressUpdatesBeforeTransaction.forEach((update) => {
          log.debug(`${JSON.stringify(update, null, 2)}`);
        });

        const highestTransactionIdBeforeTransaction = (
          progressUpdatesBeforeTransaction.at(-1)?.data
            ?.unshieldedTransactions as UnshieldedTransactionsProgress
        ).highestTransactionId;
        log.info(
          `Highest transaction ID before transaction: ${highestTransactionIdBeforeTransaction}`,
        );

        const progressUpdatesAfterTransaction = walletFixture.destinations[0].events.filter(
          (event) => {
            return (
              event.data?.unshieldedTransactions.__typename === 'UnshieldedTransactionsProgress'
            );
          },
        );

        log.debug('Progress updates after transaction:');
        progressUpdatesAfterTransaction.forEach((update) => {
          log.debug(`${JSON.stringify(update, null, 2)}`);
        });

        // Wait for the progress update event for the destination address to be reported by the
        // indexer through the unshielded transaction subscription. Same wide retry window as the
        // source-address test above: idle backoff (since indexer 4.4.0) can delay the change-
        // reporting progress update by up to the full backed-off gap (~5 minutes worst case).
        const destinationAddressEvent = await retry(
          async () =>
            findProgressUpdateEvent(
              walletFixture.destinations[0].events,
              highestTransactionIdBeforeTransaction,
              'destination',
            ),
          {
            maxRetries: 60,
            delayMs: 5000,
            retryLabel: 'find destination address progress update event',
          },
        );

        expect(destinationAddressEvent).toBeDefined();
        const highestTransactionIdAfterTransaction = (
          destinationAddressEvent.data?.unshieldedTransactions as UnshieldedTransactionsProgress
        ).highestTransactionId;
        log.info(
          `Highest transaction ID after transaction: ${highestTransactionIdAfterTransaction}`,
        );
        expect(highestTransactionIdAfterTransaction).toBeGreaterThan(
          highestTransactionIdBeforeTransaction,
        );
      },
    );

    /**
     * After an unshielded transaction is confirmed, the dust commitment Merkle tree should grow.
     * The dustCommitmentEndIndex of the transaction should be higher than the previous maximum.
     *
     * @given a confirmed unshielded transaction
     * @when we query the transaction from the indexer
     * @then the transaction's dustCommitmentEndIndex should be greater than the dustCommitmentEndIndex before the transaction
     */
    test('should increase the dust commitment Merkle tree end index', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Transaction', 'Dust', 'CommitmentMerkleTree', 'UnshieldedTokens'],
      };

      ctx.skip?.(
        transactionResult.status !== 'confirmed',
        "Toolkit transaction hasn't been confirmed",
      );

      const transactionResponse = await indexerHttpClient.getTransactionByOffset({
        hash: transactionResult.txHash,
      });
      expect(transactionResponse).toBeSuccess();

      const transactions = transactionResponse.data!.transactions;
      const tx = transactions.find((t: Transaction) => t.hash === transactionResult.txHash);
      expect(tx).toBeDefined();

      const regularTx = tx as RegularTransaction;
      expect(regularTx.dustCommitmentEndIndex).toBeDefined();
      expect(regularTx.dustCommitmentEndIndex!).toBeGreaterThan(dustCommitmentEndIndexBeforeTx);

      log.debug(
        `dustCommitmentEndIndex before tx: ${dustCommitmentEndIndexBeforeTx}, after tx: ${regularTx.dustCommitmentEndIndex}`,
      );
    });

    /**
     * Once an unshielded transaction has been confirmed, the indexer should stream the full sequence of DUST events associated with that transaction
     *
     * @given a confirmed unshielded transaction that produces DUST activity
     * @when we subscribe to dustLedgerEvents starting from (previousMaxId + 1) to ensure we only receive new dust events produced by this transaction
     * @then the indexer should deliver exactly three events in the order:
     * DustGenerationDtimeUpdate, DustInitialUtxo, DustSpendProcessed
     */
    test('should deliver dust events in correct sequence after unshielded transaction', async () => {
      const received = await collectValidDustLedgerEvents(
        indexerWsClient,
        indexerEventCoordinator,
        3,
        previousMaxDustId + 1,
      );
      expect(received).toHaveLength(3);

      received.forEach((msg) => {
        const event = msg.data!.dustLedgerEvents;
        const parsed = DustLedgerEventsUnionSchema.safeParse(event);
        expect(
          parsed.success,
          `Schema error: ${JSON.stringify(parsed.error?.format(), null, 2)}`,
        ).toBe(true);
      });

      const eventTypes = received.map((msg) => msg.data!.dustLedgerEvents.__typename);
      expect(eventTypes).toEqual([
        'DustGenerationDtimeUpdate',
        'DustInitialUtxo',
        'DustSpendProcessed',
      ]);
    });
  });

  // `.sequential` documents that the A > B2 transfer depends on A > B1 having run first.
  // These tests run after the block above and deliberately do not clear the event buffers: the
  // block above compares them against baselines captured in beforeAll, and matching on the
  // submitted transaction hash already disambiguates the streams.
  describe.sequential('a confirmed unshielded transfer streamed to address subscriptions', () => {
    /**
     * This test verifies correct propagation of event types across multi-destination subscriptions, ensuring that
     * the indexer only emits transaction data to the intended recipient while other wallets observe progress updates.
     *
     * @given a source wallet (A) and two destination wallets (B1, B2) all subscribed to unshielded transaction events
     * @when wallet A performs an unshielded transfer of 3 units to B1
     * @then B1 should receive a single `UnshieldedTransaction` event representing the received funds, while B2 should only
     * receive `UnshieldedTransactionsProgress` events and no actual `UnshieldedTransaction` payloads.
     */
    test('should emit UnshieldedTransaction only for the target wallet (A > B1)', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Wallet', 'Subscription', 'MultiDestination'] };

      const b1TxResult = await toolkit.generateSingleTx(
        sourceSeed,
        'unshielded',
        destinationAddress,
        3,
      );

      // Wait for B1's UnshieldedTransaction matching the submitted tx hash
      const latestB1Tx = await retrySimple(async () => {
        const events = getEventsOfType(
          walletFixture.destinations[0].events,
          'UnshieldedTransaction',
        );
        return events.find((e) => e.transaction.hash === b1TxResult.txHash) ?? null;
      });

      // Wait for source event matching the same tx hash
      const latestSourceTx = await retrySimple(async () => {
        const events = getEventsOfType(walletFixture.source.events, 'UnshieldedTransaction');
        return events.find((e) => e.transaction.hash === b1TxResult.txHash) ?? null;
      });

      // Wait for B2 progress
      const latestB2Tx = await retrySimple(async () => {
        const progressEvents = getEventsOfType(
          walletFixture.destinations[1].events,
          'UnshieldedTransactionsProgress',
        );
        return progressEvents.at(-1) ?? null;
      });

      validateCrossWalletTransaction(
        [latestSourceTx],
        [latestB1Tx],
        walletFixture.source.address,
        destinationAddress,
        '3',
      );

      // Ensure B2 did not receive a UnshieldedTransaction event
      const b2Tx = getEventsOfType(walletFixture.destinations[1].events, 'UnshieldedTransaction');
      expect(b2Tx.length).toBe(0);

      // B2 must at least show progress
      expect(latestB2Tx).toBeDefined();
    });

    /**
     * This test validates correct event propagation when performing an unshielded transfer from wallet A to the second destination wallet (B2) in a multi-destination
     * subscription scenario.
     * @given a source wallet (A) and two destination wallets (B1, B2), all subscribed to unshielded transaction events
     * @when wallet A performs an unshielded transfer of 1 unit to B2
     * @then B2 should receive a single `UnshieldedTransaction` event representing the received funds, while B1 should only observe its own previous transaction history and must not receive the new `UnshieldedTransaction` intended for B2
     */
    test('should emit UnshieldedTransaction only for the target wallet (A > B2)', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Wallet', 'Subscription', 'MultiDestination'] };

      const b2TxResult = await toolkit.generateSingleTx(
        sourceSeed,
        'unshielded',
        secondDestinationAddress,
        1,
      );

      // Wait for B2's UnshieldedTransaction matching the submitted tx hash
      const latestB2Tx = await retrySimple(async () => {
        const b2Events = getEventsOfType(
          walletFixture.destinations[1].events,
          'UnshieldedTransaction',
        );
        return b2Events.find((e) => e.transaction.hash === b2TxResult.txHash) ?? null;
      });

      // B1 UnshieldedTransaction (should NOT match B2)
      const latestB1Tx = await retrySimple(async () => {
        const b1Events = getEventsOfType(
          walletFixture.destinations[0].events,
          'UnshieldedTransaction',
        );
        return b1Events.at(-1) ?? null;
      });

      // Source event matching the same tx hash
      const latestSourceTx = await retrySimple(async () => {
        const srcEvents = getEventsOfType(walletFixture.source.events, 'UnshieldedTransaction');
        return srcEvents.find((e) => e.transaction.hash === b2TxResult.txHash) ?? null;
      });

      validateCrossWalletTransaction(
        [latestSourceTx],
        [latestB2Tx],
        walletFixture.source.address,
        secondDestinationAddress,
        '1',
      );

      // Ensure B1 did NOT receive the B2 transaction
      expect(latestB1Tx.transaction.hash).not.toBe(latestB2Tx.transaction.hash);
    });
  });

  describe('an address with no transaction history', () => {
    /**
     * Validates event subscription behavior for an empty wallet.
     *
     * @given an empty wallet subscribed to unshielded transaction events
     * @when no transactions are performed
     * @then only ProgressUpdate events should be emitted by the indexer
     */
    test('should emit only ProgressUpdate for empty wallet', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Wallet', 'Subscription', 'EmptyWallet'] };

      const emptySeed = '000000000000000000000000000000000000000000000000000000000000000E';
      const emptyAddress = (await toolkit.showAddress(emptySeed)).unshielded;
      log.debug(`Empty wallet address: ${emptyAddress}`);

      const ws = new IndexerWsClient();
      await ws.connectionInit();
      const emptyEvents: UnshieldedTxSubscriptionResponse[] = [];

      const unsubscribe = ws.subscribeToUnshieldedTransactionEvents(
        {
          next: (e) => {
            emptyEvents.push(e);
          },
        },
        { address: emptyAddress },
      );

      try {
        const stabilized = await waitForEventsStabilization(emptyEvents, 1000);
        log.debug(`Received ${stabilized.length} events for empty wallet.`);

        // The stream must be alive, not merely silent: `every` on an empty array is vacuously true,
        // so without this an indexer emitting nothing at all would pass.
        expect(stabilized.length).toBeGreaterThan(0);

        const onlyProgressUpdates = stabilized.every((e) => {
          const data = e.data?.unshieldedTransactions;
          return (
            data?.__typename === 'UnshieldedTransactionsProgress' && data.highestTransactionId === 0
          );
        });

        expect(onlyProgressUpdates).toBe(true);
      } finally {
        unsubscribe();
        await ws.connectionClose();
      }
    });
  });
});
