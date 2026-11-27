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
import { ToolkitWrapper } from '@utils/toolkit/toolkit-wrapper';
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
// It only sizes the seed: with more equal-index rows than one batch the fixed
// server has to page across a batch boundary by transaction ID. Detecting the
// bug does not depend on it, see WITHOUT_COMMITMENTS.
const BATCH_SIZE = 20;

// Relevant transactions without commitments to seed. Every seeded row is one,
// so whichever row ends a batch, for any batch size, makes the pre-fix server
// resume at its own zswap start index and stream the batch again.
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
 * indexers and what else sees the rows, and asserts over the real WebSocket
 * API against the real Postgres query. The in-process tests that came with the
 * fix cover SQLite only.
 *
 * Every seeded row ends at the chain's highest zswap end index N, so the
 * progress other subscriptions report is unchanged while the rows exist. Each
 * describe seeds in its `beforeAll` and deletes its own rows in its
 * `afterAll`, and leftovers of an aborted earlier run are deleted up front.
 *
 * This is an e2e suite, not an integration one, for two reasons. The live
 * re-query needs a real transaction to the wallet, and on the integration
 * stack, the one restored from `.node/<version>/`, no toolkit transaction
 * lands at all: the node proposes every block with `HitBlockWeightLimit` and
 * only its inherents while the transaction waits in the pool. The e2e stack
 * starts from genesis and lands them. And e2e files run one at a time, so the
 * seeded rows are never visible to another file while they exist.
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
        // The chain's next free zswap index, which is also the highest zswap end
        // index of any indexed transaction. Seeding rows that start and end there
        // keeps them consistent with the real Merkle tree, so no collapsed update
        // is built over indices the tree does not have, and leaves the highest
        // end index, which every subscription reports as progress, as it is.
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
        /** One starting below the subscription index, then WITHOUT_COMMITMENTS equal-index rows. */
        let seeded: SeededRelevantTransaction[] = [];

        beforeAll(async () => {
          ws = new IndexerWsClient();
          await ws.connectionInit();
          const wallet = await openFreshWallet(ws);
          ({ sessionId, nextZswapIndex } = wallet);

          const ids = allocateSeededTransactionIds(WITHOUT_COMMITMENTS + 1);
          seeded = await seedRelevantTransactions(wallet.walletId, [
            // Starts below the subscription index: must be filtered out. Its one
            // commitment, at N - 1, is one the real tree has.
            { id: ids[0], zswapStartIndex: nextZswapIndex - 1, zswapEndIndex: nextZswapIndex },
            // Add no commitment: the trigger of #1681. All of them, so the last row
            // of every batch is one, whatever the server's batch size.
            ...ids.slice(1).map((id) => ({
              id,
              zswapStartIndex: nextZswapIndex,
              zswapEndIndex: nextZswapIndex,
            })),
          ]);
        }, 60_000);

        afterAll(async () => {
          await deleteSeededTransactions(seeded.map((row) => row.id));
          await ws?.connectionClose();
        });

        /**
         * @given a wallet with WITHOUT_COMMITMENTS relevant transactions from zswap index N,
         *        none of which adds a commitment, more than one batch
         * @when a shielded transactions subscription is opened from index N
         * @then each of them is streamed exactly once, in ascending transaction ID order
         * @and the stream then goes quiet
         */
        test('should stream each relevant transaction once in transaction ID order, then go quiet', async (ctx: TestContext) => {
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
         * @when a shielded transactions subscription is opened from index 0
         * @then the transaction starting below N is streamed as well, first and once
         * @and it alone carries a collapsed Merkle tree update, closing the gap from 0
         */
        test('should stream the transaction below the index too, given a subscription from zero', async (ctx: TestContext) => {
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
        let seeded: SeededRelevantTransaction[] = [];
        let first: SeededRelevantTransaction;
        let second: SeededRelevantTransaction;

        beforeAll(async () => {
          ws = new IndexerWsClient();
          await ws.connectionInit();
          const { seed, sessionId, walletId, nextZswapIndex } = await openFreshWallet(ws);
          const [firstId, secondId] = allocateSeededTransactionIds(2);

          // One existing relevant transaction without commitments.
          [first] = await seedRelevantTransactions(walletId, [
            { id: firstId, zswapStartIndex: nextZswapIndex, zswapEndIndex: nextZswapIndex },
          ]);
          seeded = [first];

          collector = new RelevantTransactionCollector(ws, sessionId, nextZswapIndex).start();
          await collector.waitFor(
            () => collector.transactions.length >= 1,
            30_000,
            'the existing seeded transaction',
          );
          // Let the server's batch loop run its empty follow-up query and enter the
          // live phase before the next row exists. A stream that does not go quiet
          // here is still in the existing-rows phase, or repeating, and the test
          // below would not be about the live re-query.
          const idle = await collector.waitUntilIdle({ idleMs: 2_000, maxMs: 10_000 });
          expect(
            idle,
            `the stream did not go quiet after the existing transaction: ` +
              `[${collector.transactions.map((t) => t.id).join(', ')}]`,
          ).toBe(true);

          // A second one, indexed after the first was streamed, then a real
          // transaction to this wallet: the wallet-indexer finds it relevant and
          // publishes the WalletIndexed event that makes the subscription re-query
          // from its cursor. The real transaction's ID is below the seeded ones, so
          // the re-query, which resumes after the last streamed ID, does not stream
          // it; what it must stream is the second seeded row, and only that.
          [second] = await seedRelevantTransactions(walletId, [
            { id: secondId, zswapStartIndex: nextZswapIndex, zswapEndIndex: nextZswapIndex },
          ]);
          seeded = [first, second];
          const shieldedAddress = (await deriveAddresses(seed, toolkit)).shielded;
          // The transfer is the test's trigger. On the undeployed stack it has to
          // land, so a failure here fails the test instead of skipping it.
          const transfer = await toolkit.generateSingleTx(
            dataProvider.getTransferFundingSeed(),
            'shielded',
            shieldedAddress,
            1,
          );
          log.info(`transfer to the fresh wallet: ${JSON.stringify(transfer)}`);
          expect(transfer.status, 'the trigger transfer must be confirmed').toBe('confirmed');

          await collector.waitFor(
            () => collector.transactions.some((t) => t.id === second.id),
            LIVE_WAIT_TIMEOUT,
            'the seeded transaction indexed after the stream caught up',
          );
          await collector.waitUntilIdle({ idleMs: IDLE_MS, maxMs: 30_000, maxTransactions: 3 });
        }, LIVE_SETUP_TIMEOUT);

        afterAll(async () => {
          collector?.stop();
          await deleteSeededTransactions(seeded.map((row) => row.id));
          await ws?.connectionClose();
        });

        /**
         * @given a subscription that streamed a relevant transaction without commitments
         *        and caught up
         * @when another relevant transaction is indexed and the wallet is indexed again
         * @then only the new transaction is streamed; the one already streamed is not repeated
         */
        test('should resume after the last streamed transaction instead of repeating it', async (ctx: TestContext) => {
          ctx.task!.meta.custom = {
            labels: ['Subscription', 'ShieldedTransaction', 'Paging', 'Live', 'Regression'],
          };

          expect(collector.errors).toEqual([]);
          expect(collector.transactions.map((t) => t.id)).toEqual([first.id, second.id]);
        });
      });
    },
  );
