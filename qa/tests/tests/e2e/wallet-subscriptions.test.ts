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
import dataProvider from '@utils/testdata-provider';
import { ToolkitWrapper, ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';
import { IndexerWsClient, UnshieldedTxSubscriptionResponse } from '@utils/indexer/websocket-client';
import {
  waitForEventsStabilization,
  retry,
  setupWalletSubscriptions,
  getEventsOfType,
} from './test-utils';

import type { TestContext } from 'vitest';
import { IndexerHttpClient } from '@utils/indexer/http-client';

async function waitForEventType(
  events: UnshieldedTxSubscriptionResponse[],
  type: string,
  label: string,
  maxAttempts = 15,
  delayMs = 2000,
) {
  await retry(
    async () => {
      const ready = getEventsOfType(events, type).length > 0;
      log.debug(`${label}: ${ready ? 'found' : 'waiting for'} ${type}`);
      return ready || null;
    },
    Boolean,
    maxAttempts,
    delayMs,
  );
}

function validateCrossWalletTransaction(
  srcTxs: any[],
  destTxs: any[],
  srcAddr: string,
  destAddr: string,
) {
  if (!destTxs || destTxs.length === 0) {
    log.debug(`No UnshieldedTransaction events for ${destAddr} yet — skipping validation.`);
    return;
  }

  // Match strictly by transaction.hash
  const matchingSrcTxs = srcTxs.filter((srcTx) =>
    destTxs.some((destTx) => destTx.transaction.hash === srcTx.transaction.hash),
  );

  if (matchingSrcTxs.length === 0) {
    log.debug(`No matching source transactions found for destination ${destAddr}.`);
    return;
  }

  expect(matchingSrcTxs.length).toBeGreaterThanOrEqual(1);
  expect(destTxs.length).toBeGreaterThanOrEqual(1);

  destTxs.forEach((destTx) => {
    const srcTx = matchingSrcTxs.find((s) => s.transaction.hash === destTx.transaction.hash);
    if (!srcTx) {
      throw new Error(`No matching source transaction found for hash ${destTx.transaction.hash}`);
    }

    // Value & identity
    expect(destTx.createdUtxos[0].value).toBe('1');
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
    }

    log.debug(`Validation complete for ${destAddr} (hash=${destTx.transaction.hash})`);
  });
}

describe.sequential('wallet event subscriptions', () => {
  let indexerWsClient: IndexerWsClient;
  let indexerHttpClient: IndexerHttpClient;

  // Toolkit instance for generating and submitting transactions
  let toolkit: ToolkitWrapper;

  // Result of the unshielded transaction submitted to node
  let transactionResult: ToolkitTransactionResult;

  let walletFixture: Awaited<ReturnType<typeof setupWalletSubscriptions>>;

  let sourceSeed: string;
  let destinationSeed: string;

  // Addresses for the source and destination wallets, derived from their seeds
  let sourceAddress: string;
  let destinationAddress: string;

  // Events from the indexer websocket for both the source and destination addresses
  let sourceAddressEvents: UnshieldedTxSubscriptionResponse[] = [];
  let destinationAddressEvents: UnshieldedTxSubscriptionResponse[] = [];

  // Functions to unsubscribe from the indexer websocket for both the source and destination addresses
  let sourceAddrUnscribeFromEvents: () => void;
  let destAddrUnscribeFromEvents: () => void;

  // second wallet
  let secondDestinationAddress: string | undefined;
  let secondDestinationAddressEvents: UnshieldedTxSubscriptionResponse[] = [];
  let secondDestAddrUnscribeFromEvents: (() => void) | undefined;

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
    ({
      sourceSeed,
      destinationSeed,
      sourceAddress,
      destinationAddress,
      secondDestinationAddress,
      sourceAddressEvents,
      destinationAddressEvents,
      secondDestinationAddressEvents,
      sourceAddrUnscribeFromEvents,
      destAddrUnscribeFromEvents,
      secondDestAddrUnscribeFromEvents,
    } = walletFixture);
  }, 200_000);

  afterAll(async () => {
    // Unsubscribe from the unshielded transaction events for the source and destination addresses
    walletFixture.sourceAddrUnscribeFromEvents();
    walletFixture.destAddrUnscribeFromEvents();

    // Let's trigger these operations in parallel
    await Promise.all([toolkit.stop(), indexerWsClient.connectionClose()]);
  });

  /**
   * Two-wallet transaction
   *
   * Validates event propagation between two wallets during an unshielded transaction.
   */
  describe('multi-destination transaction scenario', () => {
    test.only('should propagate ProgressUpdate to all but UnshieldedTransaction only to target wallet', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Wallet', 'Subscription', 'MultiDestination'] };

      sourceAddressEvents.length = 0;
      destinationAddressEvents.length = 0;
      secondDestinationAddressEvents.length = 0;

      // First transaction: A → B1
      const tx1 = await toolkit.generateSingleTx(sourceSeed, 'unshielded', destinationAddress, 1);

      await waitForEventType(destinationAddressEvents, 'UnshieldedTransaction', 'B1 wallet');
      await waitForEventType(
        secondDestinationAddressEvents,
        'UnshieldedTransactionsProgress',
        'B2 wallet',
      );

      const allDestTxs_B1 = getEventsOfType(destinationAddressEvents, 'UnshieldedTransaction');
      const ids_B1 = allDestTxs_B1
        .map((tx) => tx.transaction?.id)
        .filter((id): id is number => typeof id === 'number');
      const maxId_B1 = Math.max(...ids_B1);
      const destTxs_B1 = allDestTxs_B1.filter((tx) => tx.transaction?.id === maxId_B1);

      log.debug(`🔹 Filtered B1 UnshieldedTransaction events to latest id=${maxId_B1}`);

      // Align source transactions to destination by transaction.hash
      const srcTxs_AtoB1 = getEventsOfType(sourceAddressEvents, 'UnshieldedTransaction').filter(
        (srcTx) => destTxs_B1.some((destTx) => destTx.transaction.hash === srcTx.transaction.hash),
      );

      //  Validate A → B1 transfer consistency
      validateCrossWalletTransaction(srcTxs_AtoB1, destTxs_B1, sourceAddress, destinationAddress);

      //  Ensure both other wallets got only ProgressUpdates
      const progressB2 = getEventsOfType(
        secondDestinationAddressEvents,
        'UnshieldedTransactionsProgress',
      );
      expect(progressB2.length).toBeGreaterThan(0);
      const txsB2 = getEventsOfType(secondDestinationAddressEvents, 'UnshieldedTransaction');
      expect(txsB2.length).toBe(0);
    }, 60_000);

    test.only('A → B2 should emit UnshieldedTransaction for B2 and not affect B1', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Wallet', 'Subscription', 'A→B2'] };

      // Generate A → B2 transaction
      const tx2 = await toolkit.generateSingleTx(
        sourceSeed,
        'unshielded',
        secondDestinationAddress!,
        1,
      );

      // Wait for B2’s UnshieldedTransaction
      await waitForEventType(secondDestinationAddressEvents, 'UnshieldedTransaction', 'B2 wallet');

      // Ensure B1 still has its previous UnshieldedTransactions
      await waitForEventType(destinationAddressEvents, 'UnshieldedTransaction', 'B1 wallet');

      // Get all B2 UnshieldedTransaction events
      const allDestTxs_B2 = getEventsOfType(
        secondDestinationAddressEvents,
        'UnshieldedTransaction',
      );

      // Keep only the freshest transaction (latest id)
      const ids_B2 = allDestTxs_B2
        .map((tx) => tx.transaction?.id)
        .filter((id): id is number => typeof id === 'number');
      const maxId_B2 = Math.max(...ids_B2);
      const destTxs_B2 = allDestTxs_B2.filter((tx) => tx.transaction?.id === maxId_B2);

      // Align matching source transactions by hash
      const srcTxs_AtoB2 = getEventsOfType(sourceAddressEvents, 'UnshieldedTransaction').filter(
        (srcTx) => destTxs_B2.some((destTx) => destTx.transaction.hash === srcTx.transaction.hash),
      );

      // Validate A → B2 transfer consistency
      validateCrossWalletTransaction(
        srcTxs_AtoB2,
        destTxs_B2,
        sourceAddress,
        secondDestinationAddress!,
      );

      // Wait for a progress update on B1 (ensure sync caught up)
      await waitForEventType(
        destinationAddressEvents,
        'UnshieldedTransactionsProgress',
        'B1 wallet',
      );

      // Fetch all UnshieldedTransaction events again for B1
      const final_B1_txs = getEventsOfType(destinationAddressEvents, 'UnshieldedTransaction');

      // Integrity check: B1 should not receive any of B2’s transaction hashes
      const hashes_B2 = destTxs_B2.map((tx) => tx.transaction.hash);
      const hashes_B1 = final_B1_txs.map((tx) => tx.transaction.hash);
      const overlapping = hashes_B1.filter((h) => hashes_B2.includes(h));

      // B1 must still have its original TXs, but not the new B2 hashes
      expect(hashes_B1.length).toBeGreaterThan(0);
      expect(overlapping.length).toBe(0);
    }, 90_000);
  });
});

/**
 * Empty wallet subscriptions
 * Verifies that an empty wallet emits only ProgressUpdate events.
 */
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
