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

import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import { ToolkitWrapper, ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';
import { UnshieldedTransactionEvent, isUnshieldedTransaction } from '@utils/indexer/indexer-types';
import { IndexerWsClient, UnshieldedTxSubscriptionResponse } from '@utils/indexer/websocket-client';
import {
  waitForEventsStabilization,
  setupWalletEventSubscriptions,
  getEventsOfType,
  retrySimple,
} from './test-utils';
import dataProvider from '@utils/testdata-provider';
import type { TestContext } from 'vitest';
import { IndexerHttpClient } from '@utils/indexer/http-client';

/**
 * This function validates that unshielded transactions emitted for two wallets are consistent across both event streams.
 *
 * Performs deep consistency checks between matching transactions,
 * ensuring identical hashes, IDs, values, ownership, created and spent UTXOs,
 * and verifying close ctime (creation time) alignment between source and destination.
 *
 * @param {UnshieldedTransactionEvent[]} srcTxs - List of unshielded transaction or progress events emitted for the **source** wallet.
 * @param {UnshieldedTransactionEvent[]} destTxs - List of unshielded transaction or progress events emitted for the **destination** wallet.
 * @param {string} srcAddr - Source wallet address expected to own the created UTXOs with outputIndex=1.
 * @param {string} destAddr - Destination wallet address expected to own the created UTXOs with outputIndex=0.
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

    // Dust Registration Flags
    expect(destUtxo.registeredForDustGeneration).toBe(false);
    expect(srcUtxo.registeredForDustGeneration).toBe(true);

    // Cross-link consistency
    expect(srcUtxo.createdAtTransaction.hash).toBe(destTx.transaction.hash);
    const spent = srcTx.spentUtxos?.[0];
    if (spent) {
      expect(spent.spentAtTransaction.hash).toBe(destTx.transaction.hash);
      const spentTx = spent.spentAtTransaction as { hash: string; identifiers?: string[] };
      expect(spentTx.identifiers?.[0]).toBe(destTx.transaction.identifiers?.[0]);

      const calculatedDestValue = BigInt(spent.value) - BigInt(srcUtxo.value);
      expect(BigInt(destUtxo.value)).toBe(calculatedDestValue);
    }
    log.debug(`Validation complete for ${destAddr} (hash=${destTx.transaction.hash})`);
  });
}

describe.sequential('wallet event subscriptions', () => {
  let indexerWsClient: IndexerWsClient;
  let indexerHttpClient: IndexerHttpClient;

  // Toolkit instance for generating and submitting transactions
  let toolkit: ToolkitWrapper;

  let walletFixture: Awaited<ReturnType<typeof setupWalletEventSubscriptions>>;
  let sourceSeed: string;

  // Addresses for the source and destination wallets, derived from their seeds
  let sourceAddress: string;
  let destinationAddress: string;

  // Events from the indexer websocket for both the source and destination addresses
  let sourceAddressEvents: UnshieldedTxSubscriptionResponse[] = [];
  let destinationAddressEvents: UnshieldedTxSubscriptionResponse[] = [];

  // second wallet
  let secondDestinationAddress: string | undefined;
  let secondDestinationAddressEvents: UnshieldedTxSubscriptionResponse[] = [];

  beforeAll(async () => {
    indexerWsClient = new IndexerWsClient();
    indexerHttpClient = new IndexerHttpClient();
    await indexerWsClient.connectionInit();
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  }, 200_000);

  afterAll(async () => {
    await Promise.all([toolkit.stop(), indexerWsClient.connectionClose()]);
  });

  describe('empty wallet scenario', () => {
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

  describe('multi-destination transaction scenario', () => {
    beforeAll(async () => {
      const sourceSeedLocal = dataProvider.getFundingSeed();
      const destinationSeed = '0000000000000000000000000000000000000000000000000000000987654321';
      const secondDestinationSeed =
        '0000000000000000000000000000000000000000000000000000000123456789';

      walletFixture = await setupWalletEventSubscriptions(
        toolkit,
        indexerWsClient,
        sourceSeedLocal,
        [destinationSeed, secondDestinationSeed],
      );

      // Source
      sourceSeed = walletFixture.source.seed;
      sourceAddress = walletFixture.source.address;
      sourceAddressEvents = walletFixture.source.events;

      // Destinatons
      destinationAddress = walletFixture.destinations[0].destinationAddress;
      secondDestinationAddress = walletFixture.destinations[1].destinationAddress;

      destinationAddressEvents = walletFixture.destinations[0].events;
      secondDestinationAddressEvents = walletFixture.destinations[1].events;
    }, 200_000);

    afterAll(async () => {
      // Unsubscribe from the unshielded transaction events for the source and destination addresses
      walletFixture.source.unsubscribe();
      walletFixture.destinations.forEach((d) => d.unsubscribe());
    });

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

      sourceAddressEvents.length = 0;
      destinationAddressEvents.length = 0;
      secondDestinationAddressEvents.length = 0;

      // First transaction: A > B1
      await toolkit.generateSingleTx(sourceSeed, 'unshielded', destinationAddress, 3);

      // Wait for B1's UnshieldedTransaction
      const latestB1Tx = await retrySimple(async () => {
        const events = getEventsOfType(destinationAddressEvents, 'UnshieldedTransaction');
        return events.find((e) => e.createdUtxos[0]?.value === '3') ?? null;
      });

      const expectedHash = latestB1Tx.transaction.hash;

      // Wait for source event
      const latestSourceTx = await retrySimple(async () => {
        const events = getEventsOfType(sourceAddressEvents, 'UnshieldedTransaction');
        return events.find((e) => e.transaction.hash === expectedHash) ?? null;
      });

      // Wait for B2 progress
      const latestB2Tx = await retrySimple(async () => {
        const progressEvents = getEventsOfType(
          secondDestinationAddressEvents,
          'UnshieldedTransactionsProgress',
        );
        return progressEvents.at(-1) ?? null;
      });

      // Validate A > B1 consistency
      validateCrossWalletTransaction(
        [latestSourceTx],
        [latestB1Tx],
        sourceAddress,
        destinationAddress,
        '3',
      );

      // Ensure B2 did not receive a UnshieldedTransaction event
      const b2Tx = getEventsOfType(secondDestinationAddressEvents, 'UnshieldedTransaction');
      expect(b2Tx.length).toBe(0);

      // B2 must at least show progress
      expect(latestB2Tx).toBeDefined();
    }, 30_000);

    /**
     * This test validates correct event propagation when performing an unshielded transfer from wallet A to the second destination wallet (B2) in a multi-destination
     * subscription scenario.
     * @given a source wallet (A) and two destination wallets (B1, B2), all subscribed to unshielded transaction events
     * @when wallet A performs an unshielded transfer of 1 unit to B2
     * @then B2 should receive a single `UnshieldedTransaction` event representing the received funds, while B1 should only observe its own previous transaction history and must not receive the new `UnshieldedTransaction` intended for B2
     */
    test('should emit UnshieldedTransaction only for the target wallet (A > B2)', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Wallet', 'Subscription', 'A→B2'] };

      // Generate A > B2 transaction
      await toolkit.generateSingleTx(sourceSeed, 'unshielded', secondDestinationAddress!, 1);

      // B2 UnshieldedTransaction
      const latestB2Tx = await retrySimple(async () => {
        const b2Events = getEventsOfType(secondDestinationAddressEvents, 'UnshieldedTransaction');
        return b2Events.find((e) => e.createdUtxos[0]?.value === '1') ?? null;
      });

      const expectedHash = latestB2Tx.transaction.hash;

      // B1 UnshieldedTransaction (should NOT match B2)
      const latestB1Tx = await retrySimple(async () => {
        const b1Events = getEventsOfType(destinationAddressEvents, 'UnshieldedTransaction');
        return b1Events.at(-1) ?? null;
      });

      // Source event
      const latestSourceTx = await retrySimple(async () => {
        const srcEvents = getEventsOfType(sourceAddressEvents, 'UnshieldedTransaction');
        return srcEvents.find((e) => e.transaction.hash === expectedHash) ?? null;
      });

      // Validate cross-wallet consistency for A > B2
      validateCrossWalletTransaction(
        [latestSourceTx],
        [latestB2Tx],
        sourceAddress,
        secondDestinationAddress!,
        '1',
      );

      // Ensure B1 did NOT receive the B2 transaction
      expect(latestB1Tx.transaction.hash).not.toBe(latestB2Tx.transaction.hash);
    }, 30_000);
  });

  describe('transaction failure scenario', () => {
    /**
     * Ensures that failed unshielded transactions do NOT produce any UTXOs.
     *
     * @given a wallet submits an invalid unshielded transaction
     * @when the node rejects the transaction with TransactionResult::Failure
     * @then the indexer must ignore the transaction entirely — no UTXOs are created, and GetTransactionByOffset must return an empty result
     */
    test('should NOT create UTXOs for a failed unshielded transaction', async () => {
      const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';
      const destinationSeed = '0000000000000000000000000000000000000000000000000000000000000002';
      const destinationAddress = (await toolkit.showAddress(destinationSeed)).unshielded;

      let failedResult: ToolkitTransactionResult | null = null;

      failedResult = await toolkit.generateSingleTx(
        sourceSeed,
        'unshielded',
        destinationAddress,
        1,
      );

      const failedHash = failedResult?.txHash ?? null;
      const response = await indexerHttpClient.getTransactionByOffset({
        hash: failedHash!,
      });

      log.debug(`Index lookup for failed transaction ${failedHash}: indexer returned ${response.data?.transactions?.length}
         transactions (expected 0).`);

      expect(response.data).not.toBeNull();
      expect(response.data!.transactions).toBeDefined();
      expect(response.data!.transactions.length).toBe(0);
    });
  });

  // Future scenarios planned for coverage
  describe('future coverage', () => {
    /**
     * Ensures that unsubscribing and resubscribing to the same wallet does NOT cause duplicate historical events or missing live updates.
     *
     * @given a wallet subscribed to unshielded transaction events
     * @when it unsubscribes and then subscribes again later
     * @then the indexer must re-send historical events exactly once, and continue streaming live events with no duplicates.
     */
    test.todo('should not duplicate events after resubscription');

    /**
     * Validates correct subscription behavior under multiple sequential unshielded transactions between two wallets (A and B).
     *
     * @given wallets A and B subscribed to unshielded transaction events
     * @when A > B, then B > A, then A > B transactions are submitted
     * @then each wallet must receive only the events relevant to itself,  in the correct order, with no leakage between addresses.
     */
    test.todo('should correctly handle multiple sequential A > B transactions');

    /**
     * Tests mixed historical + live sync behavior across two wallets: one with pre-existing transactions and one new empty wallet.
     *
     * @given wallet A with historical transactions and wallet B with none
     * @when both subscribe to unshielded transaction events
     * @then A receives historical + live events, while B receives only live events.
     */
    test.todo('should correctly handle mixed historical and new wallet subscriptions');

    /**
     * Ensures shielded and unshielded transactions are routed to the correct subscription types with no cross-contamination.
     *
     * @given a wallet performs both shielded and unshielded transactions
     * @when subscriptions are active for both event types
     * @then shielded events must NOT appear in unshielded subscriptions, and unshielded events must NOT appear in shielded subscriptions.
     */
    test.todo('should segregate shielded and unshielded events correctly');
  });
});
