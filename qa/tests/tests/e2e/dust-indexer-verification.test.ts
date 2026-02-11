// This file is part of midnightntwrk/midnight-indexer
// Copyright (C) 2025 Midnight Foundation
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

import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import log from '@utils/logging/logger';
import { ToolkitWrapper, FundWalletResult } from '@utils/toolkit/toolkit-wrapper';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import type { Transaction, Block, RegularTransaction } from '@utils/indexer/indexer-types';

const TOOLKIT_WRAPPER_TIMEOUT = 60_000; // 1 minute
const DUST_GENERATION_TIMEOUT = 150_000; // 2.5 minutes
const TEST_TIMEOUT = 10_000; // 10 seconds
const INDEXER_SYNC_WAIT = 30_000; // 30 seconds for indexer to sync

describe.sequential('DUST indexer verification', () => {
  let toolkit: ToolkitWrapper;
  let indexerClient: IndexerHttpClient;

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
    indexerClient = new IndexerHttpClient();
  }, TOOLKIT_WRAPPER_TIMEOUT);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('DUST records verification for funded wallet transaction', () => {
    let fundingResult: FundWalletResult;
    let transactionFromHash: Transaction;
    let blockFromHash: Block;
    let blockFromHeight: Block;

    beforeAll(async () => {
      // Step 1: Fund wallet using toolkit (this triggers DUST generation)
      const destinationSeed = '000000000000000000000000000000000000000000000000000000000000000a';
      const amount = 10000000; // 10 million Night tokens
      const sourceSeed = '0000000000000000000000000000000000000000000000000000000000000001';

      log.info(`Funding wallet with ${amount} Night tokens using toolkit...`);
      fundingResult = await toolkit.fundWallet(destinationSeed, amount, sourceSeed);

      expect(fundingResult.txHash).toBeDefined();
      expect(fundingResult.blockHash).toBeDefined();
      expect(fundingResult.status).toBe('confirmed');

      log.info(
        `Funding successful. Transaction hash: ${fundingResult.txHash}, Block hash: ${fundingResult.blockHash}`,
      );

      // Wait for indexer to sync the transaction
      log.info(`Waiting ${INDEXER_SYNC_WAIT / 1000}s for indexer to sync transaction...`);
      await new Promise((resolve) => setTimeout(resolve, INDEXER_SYNC_WAIT));

      // Pre-fetch transaction and block data for verification
      const transactionResponse = await indexerClient.getTransactionByOffset({
        hash: fundingResult.txHash,
      });
      expect(transactionResponse.data?.transactions).toBeDefined();
      expect(transactionResponse.data?.transactions.length).toBeGreaterThan(0);
      transactionFromHash = transactionResponse.data?.transactions[0]!;

      const blockFromHashResponse = await indexerClient.getBlockByOffset({
        hash: fundingResult.blockHash,
      });
      expect(blockFromHashResponse.data?.block).toBeDefined();
      blockFromHash = blockFromHashResponse.data?.block!;

      const blockFromHeightResponse = await indexerClient.getBlockByOffset({
        height: blockFromHash.height,
      });
      expect(blockFromHeightResponse.data?.block).toBeDefined();
      blockFromHeight = blockFromHeightResponse.data?.block!;
    }, DUST_GENERATION_TIMEOUT + INDEXER_SYNC_WAIT);

    describe('verification by transaction hash', () => {
      /**
       * A DUST verification query by transaction hash returns the expected transaction with DUST ledger events
       *
       * @given a funded wallet transaction that generates DUST
       * @when we query the indexer for the transaction by hash
       * @then the indexer should respond with the transaction containing DUST ledger events
       */
      test(
        'should verify DUST records using transaction hash',
        async (ctx: TestContext) => {
          ctx.task!.meta.custom = {
            labels: ['DUST', 'Indexer', 'Verification', 'TransactionHash'],
          };

          log.info(`Querying indexer for transaction by hash: ${fundingResult.txHash}`);
          expect(transactionFromHash).toBeDefined();
          expect(transactionFromHash.hash).toBe(fundingResult.txHash);

          log.info(`Transaction found in indexer: ${transactionFromHash.hash}`);

          // Verify DUST ledger events are present in indexer
          expect(transactionFromHash.dustLedgerEvents).toBeDefined();
          expect(Array.isArray(transactionFromHash.dustLedgerEvents)).toBe(true);

          const dustLedgerEvents = transactionFromHash.dustLedgerEvents || [];
          log.info(`DUST ledger events found in indexer: ${dustLedgerEvents.length}`);

          // Verify DUST ledger event structure
          if (dustLedgerEvents.length > 0) {
            const dustEvent = dustLedgerEvents[0];
            expect(dustEvent.id).toBeDefined();
            expect(dustEvent.raw).toBeDefined();
            expect(dustEvent.maxId).toBeDefined();

            log.info(`DUST ledger event details:`);
            log.info(`- ID: ${dustEvent.id}`);
            log.info(`- Max ID: ${dustEvent.maxId}`);
            log.info(`- Raw: ${dustEvent.raw.substring(0, 50)}...`);
          }
        },
        TEST_TIMEOUT,
      );
    });

    describe('verification by block hash', () => {
      /**
       * A DUST verification query by block hash returns the expected block with transactions containing DUST ledger events
       *
       * @given a funded wallet transaction that generates DUST
       * @when we query the indexer for the block by hash
       * @then the indexer should respond with the block containing transactions with DUST ledger events
       */
      test(
        'should verify DUST records using block hash',
        async (ctx: TestContext) => {
          ctx.task!.meta.custom = {
            labels: ['DUST', 'Indexer', 'Verification', 'BlockHash'],
          };

          log.info(`Querying indexer for block by hash: ${fundingResult.blockHash}`);
          expect(blockFromHash).toBeDefined();
          expect(blockFromHash.hash).toBe(fundingResult.blockHash);

          log.info(
            `Block found in indexer: ${blockFromHash.hash}, Height: ${blockFromHash.height}`,
          );

          // Verify block contains transactions
          expect(blockFromHash.transactions).toBeDefined();
          expect(Array.isArray(blockFromHash.transactions)).toBe(true);

          // Find the funding transaction in the block
          const fundingTransaction = blockFromHash.transactions?.find(
            (tx) => tx.hash === fundingResult.txHash,
          );

          expect(fundingTransaction).toBeDefined();
          log.info(`Funding transaction found in block: ${fundingTransaction?.hash}`);

          // Verify DUST ledger events are present in the transaction
          expect(fundingTransaction?.dustLedgerEvents).toBeDefined();
          expect(Array.isArray(fundingTransaction?.dustLedgerEvents)).toBe(true);

          const dustLedgerEvents = fundingTransaction?.dustLedgerEvents || [];
          log.info(`DUST ledger events found in block transaction: ${dustLedgerEvents.length}`);

          if (dustLedgerEvents.length > 0) {
            const dustEvent = dustLedgerEvents[0];
            expect(dustEvent.id).toBeDefined();
            expect(dustEvent.raw).toBeDefined();
            expect(dustEvent.maxId).toBeDefined();
          }
        },
        TEST_TIMEOUT,
      );
    });

    describe('verification by block height', () => {
      /**
       * A DUST verification query by block height returns the expected block with transactions containing DUST ledger events
       *
       * @given a funded wallet transaction that generates DUST
       * @when we query the indexer for the block by height
       * @then the indexer should respond with the block containing transactions with DUST ledger events
       */
      test(
        'should verify DUST records using block height',
        async (ctx: TestContext) => {
          ctx.task!.meta.custom = {
            labels: ['DUST', 'Indexer', 'Verification', 'BlockHeight'],
          };

          expect(blockFromHeight).toBeDefined();
          const blockHeight = blockFromHeight.height;
          expect(blockHeight).toBeDefined();

          log.info(`Querying indexer for block by height: ${blockHeight}`);
          log.info(
            `Block found in indexer: ${blockFromHeight.hash}, Height: ${blockFromHeight.height}`,
          );

          // Verify block contains transactions
          expect(blockFromHeight.transactions).toBeDefined();
          expect(Array.isArray(blockFromHeight.transactions)).toBe(true);

          // Find the funding transaction in the block
          const fundingTransaction = blockFromHeight.transactions?.find(
            (tx) => tx.hash === fundingResult.txHash,
          );

          expect(fundingTransaction).toBeDefined();
          log.info(`Funding transaction found in block: ${fundingTransaction?.hash}`);

          // Verify DUST ledger events are present in the transaction
          expect(fundingTransaction?.dustLedgerEvents).toBeDefined();
          expect(Array.isArray(fundingTransaction?.dustLedgerEvents)).toBe(true);

          const dustLedgerEvents = fundingTransaction?.dustLedgerEvents || [];
          log.info(`DUST ledger events found in block transaction: ${dustLedgerEvents.length}`);

          if (dustLedgerEvents.length > 0) {
            const dustEvent = dustLedgerEvents[0];
            expect(dustEvent.id).toBeDefined();
            expect(dustEvent.raw).toBeDefined();
            expect(dustEvent.maxId).toBeDefined();
          }
        },
        TEST_TIMEOUT,
      );
    });

    describe('verification by transaction identifier', () => {
      /**
       * A DUST verification query by transaction identifier returns the expected transaction with DUST ledger events
       *
       * @given a funded wallet transaction that generates DUST
       * @when we query the indexer for the transaction by identifier
       * @then the indexer should respond with the transaction containing DUST ledger events
       */
      test(
        'should verify DUST records using transaction identifier',
        async (ctx: TestContext) => {
          ctx.task!.meta.custom = {
            labels: ['DUST', 'Indexer', 'Verification', 'TransactionIdentifier'],
          };

          // Get identifier from the transaction (if available)
          expect(transactionFromHash).toBeDefined();
          const isRegularTransaction = transactionFromHash.__typename === 'RegularTransaction';
          const regularTx = isRegularTransaction
            ? (transactionFromHash as RegularTransaction)
            : null;
          const transactionId = regularTx?.identifiers?.[0];

          if (!transactionId) {
            log.warn(
              'Transaction identifier not available, skipping identifier-based verification',
            );
            ctx.skip?.(true, 'Transaction identifier not available in this transaction');
            return;
          }

          log.info(`Querying indexer for transaction by identifier: ${transactionId}`);
          const transactionResponse = await indexerClient.getTransactionByOffset({
            identifier: transactionId,
          });

          expect(transactionResponse.data?.transactions).toBeDefined();
          expect(transactionResponse.data?.transactions.length).toBeGreaterThan(0);

          const transaction = transactionResponse.data?.transactions[0];
          expect(transaction).toBeDefined();
          const isRegularTransactionResult = transaction?.__typename === 'RegularTransaction';
          const regularTxResult = isRegularTransactionResult
            ? (transaction as RegularTransaction)
            : null;
          expect(regularTxResult?.identifiers).toContain(transactionId);

          log.info(`Transaction found in indexer by identifier: ${transaction?.hash}`);

          // Verify DUST ledger events are present in indexer
          expect(transaction?.dustLedgerEvents).toBeDefined();
          expect(Array.isArray(transaction?.dustLedgerEvents)).toBe(true);

          const dustLedgerEvents = transaction?.dustLedgerEvents || [];
          log.info(`DUST ledger events found in indexer: ${dustLedgerEvents.length}`);

          if (dustLedgerEvents.length > 0) {
            const dustEvent = dustLedgerEvents[0];
            expect(dustEvent.id).toBeDefined();
            expect(dustEvent.raw).toBeDefined();
            expect(dustEvent.maxId).toBeDefined();
          }
        },
        TEST_TIMEOUT,
      );
    });
  });
});
