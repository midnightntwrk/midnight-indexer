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
import dataProvider from '@utils/testdata-provider';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import type {
  BlockResponse,
  RegularTransaction,
  Transaction,
  TransactionOffset,
  TransactionResponse,
  UnshieldedUtxo,
} from '@utils/indexer/indexer-types';

const indexerHttpClient = new IndexerHttpClient();

describe('transaction queries', () => {
  describe('a transaction query by hash', () => {
    /**
     * A transaction query by hash with a valid & existing hash returns the expected transaction
     *
     * @given an hash for an existing transaction
     * @when we send a transaction query with that hash
     * @then Indexer should return the transaction with that hash
     */
    test(`should return the transaction with that hash, given that transaction exists`, async () => {
      const blockResponse = await indexerHttpClient.getBlockByOffset({
        height: 0,
      });
      expect(blockResponse).toBeSuccess();
      expect(blockResponse.data?.block.transactions.length).toBeGreaterThanOrEqual(1);

      const transactionHashes = blockResponse.data?.block.transactions.map(
        (transaction) => transaction.hash,
      );

      const transactionQueryResponses: TransactionResponse[] = [];
      for (const transactionHash of transactionHashes!) {
        const transactionQueryResponse = await indexerHttpClient.getTransactionByOffset({
          hash: transactionHash,
        });
        expect(transactionQueryResponse).toBeSuccess();
        expect(transactionQueryResponse.data?.transactions).toHaveLength(1);
        expect(transactionQueryResponse.data?.transactions[0].hash).toBe(transactionHash);
      }
    });

    /**
     * A transaction query by hash with a valid & non-existing hash returns an empty list
     *
     * @given an hash for a non-existent transaction
     * @when we send a transaction query with that hash
     * @then Indexer should return an empty transaction list
     */
    test(`should return an empty transaction list, given a transaction with that hash doesn't exist`, async () => {
      const transactionOffset = {
        hash: '0000000000000000000000000000000000000000000000000000000000000000',
      };

      const response = await indexerHttpClient.getTransactionByOffset(transactionOffset);

      expect(response).toBeSuccess();
      expect(response.data?.transactions).toBeDefined();
      expect(response.data?.transactions).toHaveLength(0);
    });

    /**
     * A transaction query by hash returns an error if hash is invalid (malformed)
     *
     * @given we fabricate an invalid hashes (malformed)
     * @when we send a transaction query by hash using them
     * @then Indexer should return an error
     */
    test('should return an error, given a hash is invalid (malformed)', async () => {
      const fabricatedMalformedHashes = dataProvider.getFabricatedMalformedHashes();

      for (const targetHash of fabricatedMalformedHashes) {
        const offset: TransactionOffset = {
          hash: targetHash,
        };

        log.info(`Send a transaction query with an hash longer than expected: ${targetHash}`);
        const response: TransactionResponse =
          await indexerHttpClient.getTransactionByOffset(offset);

        expect.soft(response).toBeError();
      }
    });
  });

  describe('a transaction query by identifier', () => {
    /**
     * A transaction query by identifier with a valid & existing identifier returns the expected transaction
     * We use one of the identifiers from the regular transactions in the genesis block.
     *
     * @given a valid identifier for an existing transaction
     * @when we send a transaction query with that identifier
     * @then Indexer should return the transaction with that identifier
     */
    test('should return the transaction with that identifier, given that transaction exists', async () => {
      const blockResponse = await indexerHttpClient.getBlockByOffset({
        height: 0,
      });
      expect(blockResponse).toBeSuccess();
      expect(blockResponse.data?.block.transactions.length).toBeGreaterThan(0);

      const regularTransactions = blockResponse.data?.block.transactions.filter(
        (transaction) => transaction.__typename === 'RegularTransaction',
      );

      const identifiers = (regularTransactions as RegularTransaction[])?.map(
        (transaction) => transaction.identifiers![0],
      );

      for (const identifier of identifiers) {
        const transactionQueryResponse = await indexerHttpClient.getTransactionByOffset({
          identifier: identifier,
        });
        expect(transactionQueryResponse).toBeSuccess();
        expect(transactionQueryResponse.data?.transactions).toHaveLength(1);
        expect(transactionQueryResponse.data?.transactions[0].__typename).toBe(
          'RegularTransaction',
        );
        const regularTransaction = transactionQueryResponse.data
          ?.transactions[0] as RegularTransaction;
        expect(regularTransaction.identifiers).toBeDefined();
        expect(regularTransaction.identifiers?.length).toBeGreaterThanOrEqual(1);
        expect(regularTransaction.identifiers).toContain(identifier);
      }
    });

    /**
     * A transaction query by indentifier with a valid & non-existent identifier returns an empty list
     *
     * @given a valid identifier for a non-existent transaction
     * @when we send a transaction query with that identifier
     * @then Indexer should return an empty list of transactions
     */
    test(`should return an empty list of transactions, given a transaction with that identifier doesn't exist`, async () => {
      const transactionOffset = {
        identifier: '0000000000000000000000000000000000000000000000000000000000000000',
      };

      const response: TransactionResponse =
        await indexerHttpClient.getTransactionByOffset(transactionOffset);

      expect(response).toBeSuccess();
      expect(response.data!.transactions).toBeDefined();
      expect(response.data!.transactions).toHaveLength(0);
    });

    /**
     * Transaction queries by indentifier with invalid identifiers return an error
     *
     * @given an invalid identifier
     * @when we send a transaction query with that identifier
     * @then Indexer should return an error
     */
    test(`should return an error, given an invalid identifier`, async () => {
      const invalidIdentifiers = dataProvider.getFabricatedMalformedIdentifiers();

      for (const invalidIdentifier of invalidIdentifiers) {
        const transactionOffset = {
          identifier: invalidIdentifier,
        };

        const response = await indexerHttpClient.getTransactionByOffset(transactionOffset);

        expect.soft(response).toBeError();
      }
    });
  });

  describe('a transaction query by hash and identifier', () => {
    /**
     * A transaction query with both hash and identifier returns an error
     *
     * @given both hash and identifier are specified in the offset
     * @when we send a transaction query with both parameters
     * @then Indexer should return an error
     */
    test('should return an error, as only one parameter at a time can be used', async () => {
      // Note here we are building an offset object with random validly formed hash and identifier
      // The fact these are random doesn't matter because the indexer should reject the query with an
      // error before trying to see if a transaction with that hash and identifier exists.
      const offset: TransactionOffset = {
        hash: '77171f02184423c06e743439273af9e4557c5edf39cdf4125282dba2191e2ad4',
        identifier: '00000000246b12dc2c378d42c8a463db0501b85d93645c4e3fa0e2862590667be36c8b48',
      };

      log.info(
        "Send a transaction query with offset containing both hash and identifier: this shouldn't be allowed",
      );
      let response: TransactionResponse = await indexerHttpClient.getTransactionByOffset(offset);

      expect(response).toBeError();
    });
  });
});

async function getGenesisTransactions(): Promise<Transaction[]> {
  const blockQueryResponse: BlockResponse = await indexerHttpClient.getBlockByOffset({
    height: 0,
  });
  expect(blockQueryResponse).toBeSuccess();
  expect(blockQueryResponse.data?.block.transactions.length).toBeGreaterThanOrEqual(1);

  const transactionHashes = blockQueryResponse.data?.block.transactions.map(
    (transaction) => transaction.hash,
  );

  const transactionQueryResponses: TransactionResponse[] = [];
  for (const transactionHash of transactionHashes!) {
    const transactionQueryResponse = await indexerHttpClient.getTransactionByOffset({
      hash: transactionHash,
    });
    expect(transactionQueryResponse).toBeSuccess();
    expect(transactionQueryResponse.data?.transactions).toHaveLength(1);
    transactionQueryResponses.push(transactionQueryResponse);
  }

  const genesisTransactions: Transaction[] = [];
  for (const transactionQueryResponse of transactionQueryResponses) {
    const transaction = transactionQueryResponse.data?.transactions[0];
    if (transaction) {
      genesisTransactions.push(transaction);
    }
  }
  expect(genesisTransactions.length).toBeGreaterThanOrEqual(1);

  return genesisTransactions;
}

describe(`genesis transactions`, () => {
  describe(`transaction queries to the genesis block transactions`, async () => {
    let genesisTransactions: Transaction[];

    beforeEach(async () => {
      genesisTransactions = await getGenesisTransactions();
    });

    /**
     * Genesis regular transactions contain utxos related to 4 pre-fund wallets
     *
     * @given the genesis transactions are collected
     * @when we inspect their utxos
     * @then some of the regular transactions should contain utxos related to 4 pre-fund wallets
     */
    test('should return utxos related to 4 pre-fund wallets', async () => {
      const expectedPreFundWallets = 4;

      // Loop through all the utxos in the genesis transaction and gather all
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
     * Genesis transactions contain utxos with 1 token type
     *
     * @given the genesis transactions are queried
     * @when we inspect their utxos
     * @then some of the regular transactions should contain utxos with 1 token type
     */
    test('should return utxos with 1 token type', async () => {
      const expectedTokenTypes = 1;

      // Loop through all the utxos in the genesis transactions and gather all
      // available token types
      const tokenTypes: Set<string> = new Set();
      for (const transaction of genesisTransactions) {
        if (transaction.__typename === 'RegularTransaction') {
          const regularTransaction = transaction as RegularTransaction;
          const utxos = regularTransaction.unshieldedCreatedOutputs;
          if (utxos!.length > 0) {
            for (const utxo of utxos!) {
              tokenTypes.add(utxo.tokenType);
            }
          }
        }
      }

      expect(tokenTypes).toHaveLength(expectedTokenTypes);
    });

    /**
     * Genesis transactions contain utxos sorted by outputIndex in ascending order
     *
     * @given the genesis transactions are queried
     * @when we inspect their utxos
     * @then the utxos should be sorted by outputIndex in ascending order
     */
    test('should return utxos sorted by outputIndex in ascending order', async () => {
      // Loop through all the utxos in each of the genesis transactions and check whether the
      // utxos are sorted by outputIndex in ascending order

      for (const transaction of genesisTransactions) {
        if (transaction.__typename === 'RegularTransaction') {
          const regularTransaction = transaction as RegularTransaction;
          const utxos = regularTransaction.unshieldedCreatedOutputs;
          if (utxos!.length > 0) {
            for (let i = 1; i < utxos!.length; i++) {
              expect(utxos![i].outputIndex).toBeGreaterThan(utxos![i - 1].outputIndex);
            }
          }
        }
      }
    });
  });
});
