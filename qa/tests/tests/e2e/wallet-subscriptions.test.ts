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
import { ToolkitWrapper } from '@utils/toolkit/toolkit-wrapper';
import { UnshieldedTransactionEvent, isUnshieldedTransaction } from '@utils/indexer/indexer-types';
import { IndexerWsClient, UnshieldedTxSubscriptionResponse } from '@utils/indexer/websocket-client';
import {
  waitForEventsStabilization,
  setupWalletSubscriptions,
  getEventsOfType,
  waitForEventType,
} from './test-utils';

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

    // Value & identity
    expect(destTx.createdUtxos[0].value).toBe(expectedValue);
    expect(BigInt(srcTx.createdUtxos[0].value)).toBeGreaterThan(
      BigInt(destTx.createdUtxos[0].value),
    );
    expect(destTx.transaction.hash).toBe(srcTx.transaction.hash);
    expect(destTx.transaction.id).toBe(srcTx.transaction.id);

    // Ownership & indices
    expect(srcTx.createdUtxos[0].owner).toBe(srcAddr);
    expect(destTx.createdUtxos[0].owner).toBe(destAddr);
    expect(destTx.createdUtxos[0].outputIndex).toBe(0);
    expect(srcTx.createdUtxos[0].outputIndex).toBe(1);

    // Parity & dust flags
    // Allow slight delay (indexer events might differ by a few seconds)
    const srcCtime = Number(srcTx.createdUtxos[0].ctime);
    const destCtime = Number(destTx.createdUtxos[0].ctime);
    const delta = Math.abs(srcCtime - destCtime);
    expect(delta).toBeLessThanOrEqual(5);

    expect(destTx.createdUtxos[0].registeredForDustGeneration).toBe(false);
    expect(srcTx.createdUtxos[0].registeredForDustGeneration).toBe(true);

    // Cross-link consistency
    expect(srcTx.createdUtxos[0].createdAtTransaction.hash).toBe(destTx.transaction.hash);
    const spent = srcTx.spentUtxos?.[0];
    if (spent) {
      expect(spent.spentAtTransaction.hash).toBe(destTx.transaction.hash);
      const spentTx = spent.spentAtTransaction as { hash: string; identifiers?: string[] };
      expect(spentTx.identifiers?.[0]).toBe(destTx.transaction.identifiers?.[0]);
      const expectedValue = BigInt(spent.value) - BigInt(srcTx.createdUtxos[0].value);

      expect(BigInt(destTx.createdUtxos[0].value)).toBe(expectedValue);
    }

    log.debug(`Validation complete for ${destAddr} (hash=${destTx.transaction.hash})`);
  });
}

describe.sequential('wallet event subscriptions', () => {
  let indexerWsClient: IndexerWsClient;
  let indexerHttpClient: IndexerHttpClient;

  // Toolkit instance for generating and submitting transactions
  let toolkit: ToolkitWrapper;
  let walletFixture: Awaited<ReturnType<typeof setupWalletSubscriptions>>;
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

      const toolkit = new ToolkitWrapper({});
      await toolkit.start();

      const emptySeed = '000000000000000000000000000000000000000000000000000000000000000E';
      const emptyAddress = (await toolkit.showAddress(emptySeed)).unshielded;
      log.debug(`Empty wallet address: ${emptyAddress}`);

      const ws = new IndexerWsClient();
      await ws.connectionInit();
      const emptyEvents: UnshieldedTxSubscriptionResponse[] = [];

      ws.subscribeToUnshieldedTransactionEvents(
        {
          next: (e) => {
            emptyEvents.push(e);
          },
        },
        { address: emptyAddress },
      );

      const stabilized = await waitForEventsStabilization(emptyEvents, 1000);
      log.debug(`Received ${stabilized.length} events for empty wallet.`);
      const onlyProgressUpdates = stabilized.every((e) => {
        const data = e.data?.unshieldedTransactions;
        return (
          data?.__typename === 'UnshieldedTransactionsProgress' && data.highestTransactionId === 0
        );
      });
      expect(onlyProgressUpdates).toBe(true);

      await Promise.all([toolkit.stop(), ws.connectionClose()]);
    });
  });

  beforeAll(async () => {
    indexerHttpClient = new IndexerHttpClient();
    indexerWsClient = new IndexerWsClient();

    // Connecting to the indexer websocket
    await indexerWsClient.connectionInit();

    toolkit = new ToolkitWrapper({});
    await toolkit.start();

    walletFixture = await setupWalletSubscriptions(toolkit, indexerWsClient, {
      includeSecondDestination: true,
    });

    sourceSeed = walletFixture.sourceSeed;
    sourceAddress = walletFixture.sourceAddress;
    destinationAddress = walletFixture.destinationAddress;
    secondDestinationAddress = walletFixture.secondDestinationAddress;

    sourceAddressEvents = walletFixture.sourceAddressEvents;
    destinationAddressEvents = walletFixture.destinationAddressEvents;
    secondDestinationAddressEvents = walletFixture.secondDestinationAddressEvents;
  }, 200_000);

  afterAll(async () => {
    // Unsubscribe from the unshielded transaction events for the source and destination addresses
    walletFixture.sourceAddrUnscribeFromEvents();
    walletFixture.destAddrUnscribeFromEvents();

    // Let's trigger these operations in parallel
    await Promise.all([toolkit.stop(), indexerWsClient.connectionClose()]);
  });

  describe('multi-destination transaction scenario', () => {
    /**
     * This test verifies correct propagation of event types across multi-destination subscriptions, ensuring that
     * the indexer only emits transaction data to the intended recipient while other wallets observe progress updates.
     *
     * @given a source wallet (A) and two destination wallets (B1, B2) all subscribed to unshielded transaction events
     * @when wallet A performs an unshielded transfer of 3 units to B1
     * @then B1 should receive a single `UnshieldedTransaction` event representing the received funds, while B2 should only receive `UnshieldedTransactionsProgress` events and no actual `UnshieldedTransaction` payloads.
     */
    test('should emit UnshieldedTransaction only for the target wallet (A > B1)', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Wallet', 'Subscription', 'MultiDestination'] };

      sourceAddressEvents.length = 0;
      destinationAddressEvents.length = 0;
      secondDestinationAddressEvents.length = 0;

      // First transaction: A > B1
      await toolkit.generateSingleTx(sourceSeed, 'unshielded', destinationAddress, 3);

      await waitForEventType(sourceAddressEvents, 'UnshieldedTransaction', 'A wallet');
      await waitForEventType(destinationAddressEvents, 'UnshieldedTransaction', 'B1 wallet');
      await waitForEventType(secondDestinationAddressEvents, 'UnshieldedTransactionsProgress', 'B2 wallet');

      // Extracting new events after the transaction
      const destTxs_B1_all = getEventsOfType(destinationAddressEvents, 'UnshieldedTransaction');
      const srcTxs_A = getEventsOfType(sourceAddressEvents, 'UnshieldedTransaction');
      const destProgress_B2 = getEventsOfType(secondDestinationAddressEvents, 'UnshieldedTransactionsProgress');
      const destTxs_B2 = getEventsOfType(secondDestinationAddressEvents, 'UnshieldedTransaction');

      const destTxs_B1 = destTxs_B1_all.filter((destTx) =>
        srcTxs_A.some((srcTx) => srcTx.transaction.hash === destTx.transaction.hash)
      );

      // Validate A > B1 consistency
      validateCrossWalletTransaction(srcTxs_A, destTxs_B1, sourceAddress, destinationAddress, '3');

      // Ensure B2 only got Progress updates
      expect(destProgress_B2.length).toBeGreaterThan(0);
      expect(destTxs_B2.length).toBe(0);
    });

    test('should emit UnshieldedTransaction only for the target wallet (A > B2)', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Wallet', 'Subscription', 'A→B2'] };

      // Generate A > B2 transaction
      await toolkit.generateSingleTx(sourceSeed, 'unshielded', secondDestinationAddress!, 1);

      // Wait for B2 and B1 UnshieldedTransaction
      await waitForEventType(secondDestinationAddressEvents, 'UnshieldedTransaction', 'B2 wallet');
      await waitForEventType(destinationAddressEvents, 'UnshieldedTransaction', 'B1 wallet');


      const destTxs_B2 = getEventsOfType(secondDestinationAddressEvents, 'UnshieldedTransaction');
      const srcTxs_AtoB2 = getEventsOfType(sourceAddressEvents, 'UnshieldedTransaction');
      const destTxs_B1 = getEventsOfType(destinationAddressEvents, 'UnshieldedTransaction');

      // Validate A > B2 transfer consistency
      validateCrossWalletTransaction(srcTxs_AtoB2, destTxs_B2, sourceAddress, secondDestinationAddress!, '1');

      // Ensure B1's previous TXs are unaffected
      const hashes_B2 = destTxs_B2.map((tx) => tx.transaction.hash);
      const hashes_B1 = destTxs_B1.map((tx) => tx.transaction.hash);
      const overlap = hashes_B1.filter((h) => hashes_B2.includes(h));

      expect(hashes_B1.length).toBeGreaterThan(0);
      expect(overlap.length).toBe(0);
    });
  });
});


/**
 * 
 * 
 * Multiple Sequential Transactions Scenario


// Validate that when a wallet with past transactions connects/subscribes, the indexer first streams the historical transactions before live ones.
describe('historical wallet sync scenario', () => {
  test('should replay historical transactions before live updates', async () => {

  });
});

//Ensure no duplicate or missing events when a wallet unsubscribes → reconnects → subscribes again.
describe('re-subscription behavior', () => {
  test('should not duplicate events after resubscription', async () => {
  });
});  

 * Multiple Sequential Transactions Scenario
Purpose:
Check that subscriptions remain consistent over multiple sent transactions (A→B, then B→A, then A→B again).

Test logic:

Use two wallets (A, B).

Send 2–3 transactions in sequence with delays.

Validate correct event propagation each time.

Covers: event ordering, no data leakage, and no missed updates.

4. Mixed Wallets: One Historical + One New

Purpose:
Simulate a real environment where one wallet already has history and the other is new.

Checks:

Historical wallet gets historical + live updates.

New wallet only gets live ProgressUpdate and TransactionUpdate.

5. Shielded vs Unshielded Transactions 

If your ToolkitWrapper can generate shielded txs too:

Verify shielded transactions trigger correct subscription type.

Helps ensure indexer separation logic works.

 */
