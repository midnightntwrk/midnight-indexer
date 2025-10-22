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
import { BlockSchema } from '@utils/indexer/graphql/schema';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import type {
  Block,
  BlockResponse,
  RegularTransaction,
  Transaction,
  UnshieldedUtxo,
} from '@utils/indexer/indexer-types';
import dataProvider from '@utils/testdata-provider';
import { TestContext } from 'vitest';

const indexerHttpClient = new IndexerHttpClient();

// Utility function to get a block by hash given we extract
// the hash from the latest block. This function has been
// created to avoid code duplication and to make the tests more readable.
async function getLatestBlockByHash(): Promise<Block> {
  log.debug('Requesting latest block from indexer');
  const response: BlockResponse = await indexerHttpClient.getLatestBlock();
  expect(response).toBeSuccess();
  expect(response.data?.block).toBeDefined();
  expect(response.data?.block?.hash).toBeDefined();

  const latestBlockHash = response.data?.block?.hash;
  log.debug(`Requesting block by hash = ${latestBlockHash}`);
  const blockByHashResponse: BlockResponse = await indexerHttpClient.getBlockByOffset({
    hash: latestBlockHash,
  });
  expect(blockByHashResponse).toBeSuccess();
  expect(blockByHashResponse.data?.block).toBeDefined();
  expect(blockByHashResponse.data?.block?.hash).toBeDefined();
  expect(blockByHashResponse.data?.block?.hash).toBe(latestBlockHash);

  return blockByHashResponse.data?.block as Block;
}

// Utility function to get a block by height given we extract
// the height from the latest block. This function has been
// created to avoid code duplication and to make the tests more readable.
async function getLatestBlockByHeight(): Promise<Block> {
  log.debug('Requesting latest block from indexer');
  const response: BlockResponse = await indexerHttpClient.getLatestBlock();
  expect(response).toBeSuccess();
  expect(response.data?.block).toBeDefined();
  expect(response.data?.block?.hash).toBeDefined();

  const latestBlockHeight = response.data?.block?.height;
  log.debug(`Requesting block by height = ${latestBlockHeight}`);
  const blockByHashResponse: BlockResponse = await indexerHttpClient.getBlockByOffset({
    height: latestBlockHeight,
  });
  expect(blockByHashResponse).toBeSuccess();
  expect(blockByHashResponse.data?.block).toBeDefined();
  expect(blockByHashResponse.data?.block?.height).toBeDefined();
  expect(blockByHashResponse.data?.block?.height).toBe(latestBlockHeight);

  return blockByHashResponse.data?.block as Block;
}

describe('block queries', () => {
  describe('a block query without parameters', () => {
    /**
     * A block query without parameters returns the latest block
     *
     * @when we send a block query without parameters
     * @then Indexer should return the latest known block
     */
    test('should return the latest known block', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'Latest'],
        testKey: 'PM-17677',
      };

      log.debug('Requesting latest block from indexer');
      const response: BlockResponse = await indexerHttpClient.getLatestBlock();

      expect(response).toBeSuccess();
      expect(response.data?.block).toBeDefined();
      // TODO: How do we actually test that the block is the latest known block?
      // Should we try and request a block with a height that is greater than the
      // latest block height +1 and that will give an empty response? Will that be enough?
      // The ideal solution would be to query node as well and check that the block is the latest known block.
    });

    /**
     * A block query without parameters responds with the expected schema
     *
     * @when we send a block query without parameters
     * @then Indexer should respond with a block according to the requested schema
     */
    test('should respond with a block according to the requested schema', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'Latest', 'SchemaValidation'],
        testKey: 'PM-17678',
      };

      log.debug('Requesting latest block from indexer');
      const response: BlockResponse = await indexerHttpClient.getLatestBlock();

      log.debug('Checking if we actually received a block');
      expect(response).toBeSuccess();
      expect(response.data?.block).toBeDefined();

      log.debug('Validating block schema');
      const block = BlockSchema.safeParse(response.data?.block);
      expect(
        block.success,
        `Block schema validation failed ${JSON.stringify(block.error, null, 2)}`,
      ).toBe(true);
    });
  });

  describe('a block query by hash', () => {
    /**
     * A block query by hash returns the expected block if that hash exists
     *
     * @given we get the latest block hash
     * @when we send a block query by hash using that hash
     * @then Indexer should respond with the block with that hash
     */
    test('should return the block with that hash, given that block exists', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash'],
        testKey: 'PM-17679',
      };

      // Everything is already checked in getLatestBlockByHash function
      // If the promise resolves, we know that the block exists and the test passes
      const blockByHash = await getLatestBlockByHash();
    });

    /**
     * A block query by hash responds with the expected schema
     *
     * @when we send a block query by hash
     * @then Indexer should respond with a block according to the requested schema
     */
    test('should return blocks according to the requested schema', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash', 'SchemaValidation'],
        testKey: 'PM-17680',
      };

      const blockByHash = await getLatestBlockByHash();

      log.debug('Validating block schema');
      const parsedBlock = BlockSchema.safeParse(blockByHash);
      expect(
        parsedBlock.success,
        `Block schema validation failed ${JSON.stringify(parsedBlock.error, null, 2)}`,
      ).toBe(true);
    });

    /**
     * A block query by hash returns data with a null block if a block with that hash doesn't exist
     *
     * @given we use a hash that doesn't exist on the chain
     * @when we send a block query by hash using that hash
     * @then Indexer should respond with a null block section
     */
    test("should return a null block, given a block with that hash doesn't exist", async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash', 'Negative'],
        testKey: 'PM-17681',
      };

      const allZeroHash = '0000000000000000000000000000000000000000000000000000000000000000';
      log.debug(`Requesting a block with hash ${allZeroHash}`);

      const blockByHashResponse = await indexerHttpClient.getBlockByOffset({ hash: allZeroHash });

      expect(blockByHashResponse).toBeSuccess();
      expect(blockByHashResponse.data?.block).toBeNull();
      // TODO: Soft assert the error returned in terms of error message
    });

    /**
     * A block query by hash with invalid hashreturns an error
     *
     * @given we fabricate invalid hashes (malformed)
     * @when we send a block query by hash using them
     * @then Indexer should respond with an error
     */
    test('should return an error, when the hash is invalid (malformed)', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHash', 'Negative'],
        testKey: 'PM-17683',
      };

      const fabricatedMalformedHashes = dataProvider.getFabricatedMalformedHashes();

      for (const targetHash of fabricatedMalformedHashes) {
        log.debug(`Requesting a block with malformed hash: ${targetHash}`);

        const blockByHashResponse = await indexerHttpClient.getBlockByOffset({ hash: targetHash });

        expect.soft(blockByHashResponse).toBeError();
      }
    });
  });

  describe('a block query by height', () => {
    /**
     * A block query by height returns the expected block if that height exists
     *
     * @given we use the height of the latest block
     * @when we send a block query by height using that height
     * @then Indexer should respond with the block with that height
     */
    test('should return the block with that height, given a valid height', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHeight'],
        testKey: 'PM-17339',
      };

      // Everything is already checked in getLatestBlockByHeight function
      // If the promise resolves, we know that the block exists and the test passes
      const blockByHeight = await getLatestBlockByHeight();
    });

    /**
     * A block query by height responds with the expected schema
     *
     * @when we send a block query by height
     * @then Indexer should respond with a block according to the requested schema
     */
    test('should return a blocks according to the requested schema', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHeight', 'SchemaValidation'],
        testKey: 'PM-17684',
      };

      // Everything is already checked in getLatestBlockByHeight function
      // If the promise resolves, we know that the block exists and the test passes
      const blockByHeight = await getLatestBlockByHeight();

      log.debug('Validating block schema');
      const parsedBlock = BlockSchema.safeParse(blockByHeight);
      expect(
        parsedBlock.success,
        `Block schema validation failed ${JSON.stringify(parsedBlock.error, null, 2)}`,
      ).toBe(true);
    });

    /**
     * A block query by height = 0 returns genesis block
     *
     * @given we use a height = 0
     * @when we send a block query by height using that height
     * @then Indexer should respond with the genesis block
     */
    test('should return the genesis block, given height=0 is requested', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHeight', 'Genesis'],
        testKey: 'PM-17685',
      };

      log.debug(`Requesting genesis block (height = 0)`);

      const queryResponse = await indexerHttpClient.getBlockByOffset({ height: 0 });

      expect(
        queryResponse.errors,
        `Received unexpected error ${JSON.stringify(queryResponse.errors, null, 2)}`,
      ).toBeUndefined();
      expect(queryResponse).toBeSuccess();
      expect(queryResponse.data?.block).toBeDefined();
      expect(queryResponse.data?.block.height).toBe(0);
      expect(queryResponse.data?.block.parent).toBeNull();
    });

    /**
     * A block query by height with a height that doesn't exist (but it's within the reange of possible values)
     * returns a null block. Note that this is different from a block query by height with an invalid
     * height, which returns an error. We will use the maximum allowed height for this test.
     *
     * @given we use a height that doesn't exist
     * @when we send a block query by height using that height
     * @then Indexer should respond with an empty (null)block
     */
    test('should return an empty body answer, given that block height requested is the maximum available height', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHeight', 'Negative'],
        testKey: 'PM-17686',
      };

      const maxAllowedBlockHeight = 2 ** 32 - 1; // Note this is the maximum allowed height and will take 800+ years to reach
      log.debug(`Requesting block with max height = ${maxAllowedBlockHeight}`);

      const queryResponse = await indexerHttpClient.getBlockByOffset({
        height: maxAllowedBlockHeight,
      });

      expect(queryResponse).toBeSuccess();
      expect(queryResponse.data?.block).toBeDefined();
      expect(queryResponse.data?.block).toBeNull();
    });

    /**
     * A block query by height with an invalid height returns an error
     *
     * @given we fabricate invalid heights
     * @when we send a block query by height using them
     * @then Indexer should respond with an error
     */
    test('should return an error, given an invalid height', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHeight', 'Negative'],
        testKey: 'PM-17687',
      };

      const invalidHeights = dataProvider.getFabricatedMalformedHeights();

      for (const targetHeight of invalidHeights) {
        log.debug(`Requesting block with height = ${targetHeight}`);

        const queryResponse = await indexerHttpClient.getBlockByOffset({
          height: targetHeight,
        });

        expect.soft(queryResponse).toBeError();
      }
    });
  });

  describe('a block query by height and hash', () => {
    /**
     * A block query by height and hash returns an error as the indexer only supports one parameter at a time
     * regardless of the validity of the parameters
     *
     * @given we use both height and hash
     * @when we send a block query with both parameters
     * @then Indexer should respond with an error
     */
    test('should return an error, as only one parameter at a time can be used', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHeightAndHash', 'Negative'],
        testKey: 'PM-17688',
      };

      // Here we cover the 4 combinations of valid and invalid parameters (hash and height)
      const hashes = [dataProvider.getKnownBlockHash(), 'invalid-hash'];
      const heights = [1, 2 ** 32];

      // Generate cartesian product of hashes and heights
      const inputParameters = hashes.flatMap((hash) => heights.map((height) => ({ hash, height })));

      for (const inputParameter of inputParameters) {
        const queryResponse = await indexerHttpClient.getBlockByOffset(inputParameter);
        expect.soft(queryResponse).toBeError();
      }
    });
  });
});

/**
 * Extracts and returns all the transactions from the genesis block.
 *
 * @param block - The genesis block object to extract the transactions from.
 * @returns The array of Transaction objects contained in the genesis block.
 */
async function extractGenesisTransactions(block: Block): Promise<Transaction[]> {
  expect(block.transactions).toBeDefined();
  expect(block.transactions).not.toBeNull();
  expect(block.transactions.length).toBeGreaterThanOrEqual(1);

  return block.transactions as Transaction[];
}

describe(`genesis block`, () => {
  let genesisBlock: Block;

  beforeEach(async () => {
    const blockQueryResponse: BlockResponse = await indexerHttpClient.getBlockByOffset({
      height: 0,
    });
    expect(blockQueryResponse).toBeSuccess();
    expect(blockQueryResponse.data?.block).toBeDefined();

    genesisBlock = blockQueryResponse.data?.block as Block;
  });

  describe(`a block query to the genesis block`, async () => {
    /**
     * Genesis block contains transactions with pre-fund wallet utxos
     *
     * @given the genesis block is queried
     * @when we inspect its transactions
     * @then it should contain transactions with pre-fund wallet utxos
     */
    test('should contain transactions with pre-fund wallet utxos', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHeight', 'Genesis', 'PreFundWallets', 'UnshieldedTokens'],
        testKey: 'PM-17689',
      };

      const genesisTransactions = await extractGenesisTransactions(genesisBlock);
      expect(genesisTransactions).toBeDefined();
      expect(genesisTransactions.length).toBeGreaterThanOrEqual(1);

      for (const transaction of genesisTransactions) {
        if (transaction.__typename === 'RegularTransaction') {
          const regularTransaction = transaction as RegularTransaction;
          if (regularTransaction.identifiers?.length === 1) {
            expect(regularTransaction.unshieldedCreatedOutputs).toBeDefined();
            expect(regularTransaction.unshieldedCreatedOutputs?.length).toBeGreaterThanOrEqual(1);
          } else {
            expect(regularTransaction.raw).toBeDefined();
            expect(regularTransaction.raw).not.toBeNull();
          }
        }
      }
    });

    /**
     * Genesis block contains utxos related to exactly 4 pre-fund wallets
     *
     * @given the genesis block is queried
     * @when we inspect the utxos in its transaction
     * @then there should be utxos related to exactly 4 pre-fund wallets
     */
    test('should contain utxos related to exactly 4 pre-fund wallets', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHeight', 'Genesis', 'PreFundWallets', 'UnshieldedTokens'],
        testKey: 'PM-17690',
      };

      const expectedPreFundWallets = 4;
      const genesisTransactions = await extractGenesisTransactions(genesisBlock);

      // Loop through all the utxos in the transactions that have them and gather all
      // the pre-fund wallet addresses
      const preFundWallets: Set<string> = new Set();
      for (const transaction of genesisTransactions) {
        if (transaction.__typename === 'RegularTransaction') {
          const regularTransaction = transaction as RegularTransaction;
          const utxos = regularTransaction.unshieldedCreatedOutputs;
          if (utxos!.length > 0) {
            for (const utxo of utxos!) {
              preFundWallets.add(utxo.owner);
              log.debug(`pre-fund wallet found: ${utxo.owner}`);
            }
          }
        }
      }

      expect(preFundWallets).toHaveLength(expectedPreFundWallets);
    });

    /**
     * Genesis block contains utxos with exactly 1 token type
     *
     * @given the genesis block is queried
     * @when we inspect the utxos in its transaction
     * @then there should be utxos with exactly 1 token type
     */
    test('should contain utxos with exactly 1 token type', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHeight', 'Genesis', 'PreFundWallets', 'UnshieldedTokens'],
        testKey: 'PM-17691',
      };

      const expectedTokenTypes = 1;
      const genesisTransactions = await extractGenesisTransactions(genesisBlock);

      // Loop through all the utxos in the transactions that have them and gather all
      // the token types
      const tokenTypes: Set<string> = new Set();
      for (const transaction of genesisTransactions) {
        if (transaction.__typename === 'RegularTransaction') {
          const regularTransaction = transaction as RegularTransaction;
          const utxos = regularTransaction.unshieldedCreatedOutputs;
          if (utxos!.length > 0) {
            for (const utxo of utxos!) {
              tokenTypes.add(utxo.tokenType);
              log.debug(`tokenType found: ${utxo.tokenType}`);
            }
          }
        }
      }

      expect(tokenTypes).toHaveLength(expectedTokenTypes);
    });

    /**
     * Genesis block contains utxos sorted by outputIndex in ascending order
     *
     * @given the genesis block is queried
     * @when we inspect the utxos in its transaction
     * @then the utxos should be sorted by outputIndex in ascending order
     */
    // https://shielded.atlassian.net/browse/PM-17665
    test('should contain utxos sorted by outputIndex in ascending order', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Block', 'ByHeight', 'Genesis', 'PreFundWallets', 'UnshieldedTokens'],
        testKey: 'PM-17692',
      };

      const genesisTransactions = await extractGenesisTransactions(genesisBlock);

      // Loop through all the utxos in the transactions that have them and gather all
      // the output indexes
      const outputIndexes: Set<number> = new Set();
      for (const transaction of genesisTransactions) {
        if (transaction.__typename === 'RegularTransaction') {
          const regularTransaction = transaction as RegularTransaction;
          const utxos = regularTransaction.unshieldedCreatedOutputs;
          if (utxos!.length > 0) {
            for (const utxo of utxos!) {
              outputIndexes.add(utxo.outputIndex);
              log.debug(`outputIndex found: ${utxo.outputIndex}`);
            }
          }
        }
      }

      expect(outputIndexes).toBeDefined();
      expect(outputIndexes).not.toBeNull();
      expect(outputIndexes.size).toBeGreaterThanOrEqual(1);
      const utxos = Array.from(outputIndexes) as number[];

      // Loop through all the utxos in the genesis transaction and check whether the
      // they are sorted by outputIndex in ascending order
      let previousOutputIndex = utxos[0];
      let currentOutputIndex: number;
      for (let i = 1; i < utxos.length; i++) {
        currentOutputIndex = utxos[i];

        // NOTE: We don't need to check that outputIndex values are strictly sequential (e.g., 0, 1, 2, ... N);
        // we only need to verify that they are sorted in ascending order.
        log.debug(
          `previousOutputIndex = ${previousOutputIndex} currentOutputIndex = ${currentOutputIndex}`,
        );
        expect.soft(currentOutputIndex).toBeGreaterThan(previousOutputIndex);
      }
    });
  });
});
