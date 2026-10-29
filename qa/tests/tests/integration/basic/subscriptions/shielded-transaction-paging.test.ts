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
import { randomBytes } from 'crypto';
import { env } from 'environment/model';
import type { TestContext } from 'vitest';
import dataProvider from '@utils/testdata-provider';
import { IndexerWsClient } from '@utils/indexer/websocket-client';
import { ToolkitWrapper, type ToolkitTransactionResult } from '@utils/toolkit/toolkit-wrapper';
import { deriveAddresses } from '@utils/moth/moth-addresses';
import {
  allocateSeededTransactionIds,
  deleteSeededTransactions,
  getWalletIdBySessionId,
  seedRelevantTransactions,
  type SeededRelevantTransaction,
} from '@utils/undeployed/indexer-postgres';
import {
  RelevantTransactionCollector,
  awaitFirstProgress,
  collectRelevantTransactions,
} from 'tests/shared/shielded-transactions-utils';

// The shielded transactions subscription reads relevant transactions in batches
// of this size (indexer-api/config.yaml, subscription.shielded_transactions.batch_size).
const BATCH_SIZE = 20;

// Enough relevant transactions without commitments to cross one batch boundary.
const WITHOUT_COMMITMENTS = BATCH_SIZE + 4;

const TOOLKIT_STARTUP_TIMEOUT = 60_000;
const IDLE_MS = 5_000;

// The live phase needs a real transaction to the wallet. Without the e2e cache
// warmup the toolkit first replays the chain, which dominates this budget.
const LIVE_SETUP_TIMEOUT = 900_000;
const LIVE_WAIT_TIMEOUT = 600_000;

/**
 * Regression coverage for midnight-indexer#1681.
 *
 * A relevant transaction that adds no zswap commitment, such as one whose
 * output to the wallet sits in a fallible segment that failed, has equal zswap
 * start and end indices. The subscription used to page relevant transactions by
 * zswap index: each batch selected `zswapStartIndex >= index` and the next
 * started at the last row's `zswapEndIndex`. When such a transaction was the
 * last of a batch the next batch returned it again, forever, and the live
 * re-query after each WalletIndexed event repeated the last streamed one. The
 * fix pages by transaction ID and keeps the zswap index only as the filter.
 *
 * No toolkit or moth path produces such a transaction on a real chain, so this
 * suite seeds them straight into the undeployed stack's Postgres for a fresh
 * wallet, see `seedRelevantTransactions` for how that stays invisible to the
 * indexers, and asserts over the real WebSocket API against the real Postgres
 * query. The in-process tests that came with the fix cover SQLite only.
 *
 * The seeded rows are visible to every API query by their block for as long as
 * they exist, so they are deleted in `afterAll`, and leftovers of an aborted
 * earlier run are deleted up front.
 */
describe
  .skipIf(!env.isUndeployedEnv())
  .sequential(
    'shielded transactions subscription paging over transactions without commitments',
    () => {
      let toolkit: ToolkitWrapper;

      beforeAll(async () => {
        await deleteSeededTransactions();
        toolkit = new ToolkitWrapper({});
        await toolkit.start();
      }, TOOLKIT_STARTUP_TIMEOUT);

      afterAll(async () => {
        await deleteSeededTransactions();
        await toolkit?.stop();
      });

      /** A fresh wallet: random seed, open session, and its wallet ID for seeding. */
      async function openFreshWallet(ws: IndexerWsClient): Promise<{
        seed: string;
        sessionId: string;
        walletId: string;
        nextZswapIndex: number;
      }> {
        const seed = randomBytes(32).toString('hex');
        const viewingKey = await toolkit.showViewingKey(seed);
        const sessionId = await ws.openWalletSession(viewingKey);
        const walletId = await getWalletIdBySessionId(sessionId);
        if (!walletId) {
          throw new Error(`no wallet row for session ${sessionId}`);
        }
        // The chain's next free zswap index. Seeding at it keeps the seeded rows
        // consistent with the real Merkle tree, so no collapsed update is built
        // over indices the tree does not have.
        const { highestZswapEndIndex } = await awaitFirstProgress(ws, sessionId);
        if (highestZswapEndIndex < 2) {
          throw new Error(
            `expected shielded commitments on the chain (genesis pre-funds), ` +
              `highestZswapEndIndex is ${highestZswapEndIndex}`,
          );
        }
        log.info(
          `fresh wallet ${walletId}, session ${sessionId}, next zswap index ${highestZswapEndIndex}`,
        );
        return { seed, sessionId, walletId, nextZswapIndex: highestZswapEndIndex };
      }

      describe('existing relevant transactions across a batch boundary', () => {
        let ws: IndexerWsClient;
        let sessionId: string;
        let nextZswapIndex: number;
        /** One below the subscription index, then WITHOUT_COMMITMENTS equal-index rows, then one with commitments. */
        let seeded: SeededRelevantTransaction[];

        beforeAll(async () => {
          ws = new IndexerWsClient();
          await ws.connectionInit();
          const wallet = await openFreshWallet(ws);
          ({ sessionId, nextZswapIndex } = wallet);

          const ids = allocateSeededTransactionIds(WITHOUT_COMMITMENTS + 2);
          seeded = await seedRelevantTransactions(wallet.walletId, [
            // Starts below the subscription index: must be filtered out.
            { id: ids[0], zswapStartIndex: nextZswapIndex - 1, zswapEndIndex: nextZswapIndex },
            // Add no commitment: the trigger of #1681, one of them last in the first batch.
            ...ids.slice(1, WITHOUT_COMMITMENTS + 1).map((id) => ({
              id,
              zswapStartIndex: nextZswapIndex,
              zswapEndIndex: nextZswapIndex,
            })),
            // Adds commitments again after the run of equal indices.
            {
              id: ids[WITHOUT_COMMITMENTS + 1],
              zswapStartIndex: nextZswapIndex,
              zswapEndIndex: nextZswapIndex + 2,
            },
          ]);
        }, 60_000);

        afterAll(async () => {
          await ws?.connectionClose();
        });

        /**
         * @given a wallet with WITHOUT_COMMITMENTS + 1 relevant transactions from zswap index N,
         *        of which WITHOUT_COMMITMENTS add no commitment, more than one batch
         * @when we subscribe to shielded transactions from index N
         * @then each of them is streamed exactly once, in ascending transaction ID order
         * @and the stream then goes quiet
         */
        test('streams each relevant transaction once in transaction ID order, then goes quiet', async (ctx: TestContext) => {
          ctx.task!.meta.custom = {
            labels: ['Subscription', 'ShieldedTransaction', 'Paging', 'Regression'],
          };

          const expectedIds = seeded.slice(1).map((row) => row.id);

          const { transactions, settled } = await collectRelevantTransactions(ws, sessionId, {
            index: nextZswapIndex,
            idleMs: IDLE_MS,
            maxMs: 45_000,
            // One more than expected so a repeating stream is cut off, not collected forever.
            maxTransactions: expectedIds.length + 1,
          });

          const ids = transactions.map((t) => t.id);
          expect(ids, 'each relevant transaction from the index once, in ID order').toEqual(
            expectedIds,
          );
          expect(new Set(transactions.map((t) => t.hash)).size).toBe(expectedIds.length);
          expect(settled, `the stream kept going: ${ids.length} transactions`).toBe(true);
        }, 60_000);

        /**
         * The zswap index still filters: `index` selects transactions by their zswap
         * start index, and the transaction ID only pages within that selection.
         *
         * @given the same wallet
         * @when we subscribe from index 0
         * @then the transaction starting below N is streamed as well, first and once
         * @and it alone carries a collapsed Merkle tree update, closing the gap from 0
         */
        test('streams the transaction below the index too when subscribing from zero', async (ctx: TestContext) => {
          ctx.task!.meta.custom = { labels: ['Subscription', 'ShieldedTransaction', 'Paging'] };

          const expectedIds = seeded.map((row) => row.id);

          const { transactions, settled } = await collectRelevantTransactions(ws, sessionId, {
            index: 0,
            idleMs: IDLE_MS,
            maxMs: 45_000,
            maxTransactions: expectedIds.length + 1,
          });

          expect(transactions.map((t) => t.id)).toEqual(expectedIds);
          expect(settled).toBe(true);

          // The first row starts at N - 1 > 0, so the subscription driven from 0 owes
          // the client the tree up to N - 2; from there on the rows are contiguous.
          expect(transactions.map((t) => t.hasCollapsedUpdate)).toEqual(
            expectedIds.map((_, i) => i === 0),
          );
        }, 60_000);
      });

      describe('a live re-query after a relevant transaction without commitments', () => {
        let ws: IndexerWsClient;
        let collector: RelevantTransactionCollector;
        let first: SeededRelevantTransaction;
        let second: SeededRelevantTransaction;
        let transfer: ToolkitTransactionResult | undefined;
        /** Why the trigger transfer did not land, when it did not; the test then skips. */
        let triggerFailure: string | undefined;

        beforeAll(async () => {
          ws = new IndexerWsClient();
          await ws.connectionInit();
          const { seed, sessionId, walletId, nextZswapIndex } = await openFreshWallet(ws);
          const [firstId, secondId] = allocateSeededTransactionIds(2);

          // One existing relevant transaction without commitments.
          [first] = await seedRelevantTransactions(walletId, [
            { id: firstId, zswapStartIndex: nextZswapIndex, zswapEndIndex: nextZswapIndex },
          ]);

          collector = new RelevantTransactionCollector(ws, sessionId, nextZswapIndex).start();
          await collector.waitFor(
            () => collector.transactions.length >= 1,
            30_000,
            'the existing seeded transaction',
          );
          // Let the server's batch loop run its empty follow-up query and enter the
          // live phase before the next row exists.
          await collector.waitUntilIdle({ idleMs: 2_000, maxMs: 10_000 });

          // A second one, indexed after the first was streamed, then a real
          // transaction to this wallet: the wallet-indexer finds it relevant and
          // publishes the WalletIndexed event that makes the subscription re-query
          // from its cursor. The real transaction's ID is below the seeded ones, so
          // the re-query, which resumes after the last streamed ID, does not stream
          // it; what it must stream is the second seeded row, and only that.
          [second] = await seedRelevantTransactions(walletId, [
            { id: secondId, zswapStartIndex: nextZswapIndex, zswapEndIndex: nextZswapIndex },
          ]);
          const shieldedAddress = (await deriveAddresses(seed, toolkit)).shielded;
          // The transfer is the test's trigger, not its subject: when the node or
          // toolkit cannot land it, the test skips with the reason rather than
          // failing on an environment it cannot assert anything about.
          try {
            transfer = await toolkit.generateSingleTx(
              dataProvider.getTransferFundingSeed(),
              'shielded',
              shieldedAddress,
              1,
            );
            log.info(`transfer to the fresh wallet: ${JSON.stringify(transfer)}`);
            if (transfer.status !== 'confirmed') {
              triggerFailure = `the trigger transfer was not confirmed: ${transfer.status}`;
            }
          } catch (error) {
            triggerFailure = `the trigger transfer failed: ${String(error).split('\n')[0]}`;
            log.warn(triggerFailure);
          }
          if (triggerFailure) {
            return;
          }

          await collector.waitFor(
            () => collector.transactions.some((t) => t.id === second.id),
            LIVE_WAIT_TIMEOUT,
            'the seeded transaction indexed after the stream caught up',
          );
          await collector.waitUntilIdle({ idleMs: IDLE_MS, maxMs: 30_000, maxTransactions: 3 });
        }, LIVE_SETUP_TIMEOUT);

        afterAll(async () => {
          collector?.stop();
          await ws?.connectionClose();
        });

        /**
         * @given a subscription that streamed a relevant transaction without commitments
         *        and caught up
         * @when another relevant transaction is indexed and the wallet is indexed again
         * @then only the new transaction is streamed; the one already streamed is not repeated
         */
        test('resumes after the last streamed transaction instead of repeating it', async (ctx: TestContext) => {
          ctx.task!.meta.custom = {
            labels: ['Subscription', 'ShieldedTransaction', 'Paging', 'Live', 'Regression'],
          };
          ctx.skip?.(triggerFailure !== undefined, triggerFailure);

          expect(collector.errors).toEqual([]);
          expect(collector.transactions.map((t) => t.id)).toEqual([first.id, second.id]);
        });
      });
    },
  );
