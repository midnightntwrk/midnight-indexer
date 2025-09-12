// This file is part of midnight-indexer.
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
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import dataProvider from '@utils/testdata-provider';
import { IndexerHttpClient } from '@utils/indexer/http-client';

const indexerHttpClient = new IndexerHttpClient();

describe('contract queries', () => {
  describe('a contract query by address', () => {
    /**
     * A contract query by address returns the most recent action for a contract with multiple actions
     *
     * @given we have a contract address with multiple actions (ContractDeploy at block 49, ContractUpdate at block 59)
     * @when we send a contract query using that address without offset
     * @then Indexer should respond with successful response and return the most recent action (ContractUpdate)
     */
    test('should return the most recent action for a contract with multiple actions', async (context: TestContext) => {
      let existingContractAddress: string;
      try {
        existingContractAddress = dataProvider.getKnownContractAddress();
      } catch (error) {
        log.warn(error);
        context.skip?.(true, (error as Error).message);
      }

      const response = await indexerHttpClient.getContractAction(existingContractAddress!);

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
      expect(['ContractUpdate', 'ContractCall']).toContain(
        response.data?.contractAction?.__typename,
      );
    });

    /**
     * A contract query by address returns null when the contract does not exist
     *
     * @given we have a non-existent contract address
     * @when we send a contract query using that address
     * @then Indexer should respond with null contract action
     */
    test('should return null when contract with that address does not exist', async () => {
      const nonExistentContractAddress = dataProvider.getNonExistingContractAddress();
      const response = await indexerHttpClient.getContractAction(nonExistentContractAddress);
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });

    describe('address validation - fabricated and malformed contract addresses', () => {
      /**
       * A contract query with malformed addresses returns an error
       *
       * @given we have fabricated malformed contract addresses
       * @when we send a contract query using each malformed address
       * @then Indexer should respond with an error for each address
       */
      test.each(dataProvider.getFabricatedMalformedContractAddresses())(
        'should return an error for malformed address: %s',
        async (malformedAddress: string) => {
          const response = await indexerHttpClient.getContractAction(malformedAddress);
          expect(response).toBeError();
          expect(response.errors).toBeDefined();
          expect(response.errors?.[0]).toHaveProperty('message');
        }
      );

    });

    describe('address validation - Boundary contract address cases', () => {
      
      test.each(dataProvider.getBoundaryContractAddresses())(
        'should return success as they are valid contract addresses: %s',
        async (malformedAddress: string) => {
          const response = await indexerHttpClient.getContractAction(malformedAddress);
          expect(response).toBeSuccess();
          expect(response.data?.contractAction).toBeDefined();
        }
      );
    });
  });

  describe('a contract query by address and offset', () => {
    const validAddress = dataProvider.getNonExistingContractAddress();
    const knownBlockHash = dataProvider.getKnownBlockHash();

    /**
     * A contract query by address and offset returns null when contract does not exist
     *
     * @given we have a valid address and valid block offset
     * @when we send a contract query using that address and offset
     * @then Indexer should respond with null contract action
     */
    test('should return null when contract with valid address and valid offset does not exist', async () => {
      const response = await indexerHttpClient.getContractAction(validAddress, { blockOffset: { hash: knownBlockHash } });
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });
    
    /**
     * A contract query by address and non-existing hash returns null when contract does not exist
     *
     * @given we have a valid address and non-existing block hash
     * @when we send a contract query using that address and hash
     * @then Indexer should respond with null contract action
     */
    test('should return null when contract with valid address and non-existing hash does not exist', async () => {
      const nonExistingHash = dataProvider.getNonExistingHash();
      const response = await indexerHttpClient.getContractAction(validAddress, { blockOffset: { hash: nonExistingHash } });
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });

    /**
     * A contract query with invalid address and valid hash returns an error
     *
     * @given we have an invalid address and valid block hash
     * @when we send a contract query using that address and hash
     * @then Indexer should respond with an error
     */
    test('should return error when contract with invalid address and valid hash', async () => {
      const invalidAddress = dataProvider.getFabricatedMalformedContractAddresses()[10]; 
      const response = await indexerHttpClient.getContractAction(invalidAddress, { blockOffset: { hash: knownBlockHash } });
      expect(response).toBeError();
      expect(response.errors).toBeDefined();
      expect(response.errors?.[0]).toHaveProperty('message');
    });

    /**
     * A contract query with invalid address and non-existing hash returns an error
     *
     * @given we have an invalid address and non-existing block hash
     * @when we send a contract query using that address and hash
     * @then Indexer should respond with an error
     */
    test('should return error when contract with invalid address and non-existing hash', async () => {
      const invalidAddress = dataProvider.getFabricatedMalformedContractAddresses()[10];
      const nonExistingHash = dataProvider.getNonExistingHash();
      const response = await indexerHttpClient.getContractAction(invalidAddress, { blockOffset: { hash: nonExistingHash } });
      expect(response).toBeError();
      expect(response.errors).toBeDefined();
      expect(response.errors?.[0]).toHaveProperty('message');
    });

    /**
     * A contract query with valid address and invalid hash returns an error
     *
     * @given we have a valid address and invalid block hash
     * @when we send a contract query using that address and hash
     * @then Indexer should respond with an error
     */
    test('should return error when contract with valid address and invalid hash', async () => {
      const malformedHashes = dataProvider.getFabricatedMalformedHashes();
        const response = await indexerHttpClient.getContractAction(validAddress, { blockOffset: { hash: malformedHashes[0] } });
        expect(response).toBeError();
        expect(response.errors).toBeDefined();
        expect(response.errors?.[0]).toHaveProperty('message');
    });

    /**
     * A contract query with invalid address and invalid hash returns an error
     *
     * @given we have an invalid address and invalid block hash
     * @when we send a contract query using that address and hash
     * @then Indexer should respond with an error
     */
    test('should return error when contract with invalid address and invalid hash', async () => {
      const invalidAddress = dataProvider.getFabricatedMalformedContractAddresses()[10]; // empty string
      const malformedHashes = dataProvider.getFabricatedMalformedHashes();
      for (const malformedHash of malformedHashes) {
        const response = await indexerHttpClient.getContractAction(invalidAddress, { blockOffset: { hash: malformedHash } });
        expect(response).toBeError();
        expect(response.errors).toBeDefined();
        expect(response.errors?.[0]).toHaveProperty('message');
      }
    });

    describe('offset validation - height parameter', () => {
      /**
       * A contract query with valid address and valid height returns null when contract does not exist
       *
       * @given we have a valid address and valid block height
       * @when we send a contract query using that address and height
       * @then Indexer should respond with null contract action
       */
      test('should return null when contract with valid address and valid height does not exist', async () => {
        const response = await indexerHttpClient.getContractAction(validAddress, { blockOffset: { height: 0 } });
        expect(response).toBeSuccess();
        expect(response.data?.contractAction).toBeNull();
      });

      /**
       * A contract query with valid address and non-existing height returns null when contract does not exist
       *
       * @given we have a valid address and non-existing block height
       * @when we send a contract query using that address and height
       * @then Indexer should respond with null contract action
       */
      test('should return null when contract with valid address and non-existing height does not exist', async () => {
        const response = await indexerHttpClient.getContractAction(validAddress, { blockOffset: { height: 999999 } });
        expect(response).toBeSuccess();
        expect(response.data?.contractAction).toBeNull();
      });

      /**
       * A contract query with invalid address and valid height returns an error
       *
       * @given we have an invalid address and valid block height
       * @when we send a contract query using that address and height
       * @then Indexer should respond with an error
       */
      test('should return error when contract with invalid address and valid height', async () => {
        const invalidAddress = dataProvider.getFabricatedMalformedContractAddresses()[10];
        const response = await indexerHttpClient.getContractAction(invalidAddress, { blockOffset: { height: 0 } });
        expect(response).toBeError();
        expect(response.errors).toBeDefined();
        expect(response.errors?.[0]).toHaveProperty('message');
      });

      /**
       * A contract query with invalid address and invalid height returns an error
       *
       * @given we have an invalid address and invalid block height
       * @when we send a contract query using that address and height
       * @then Indexer should respond with an error
       */
      test('should return error when contract with invalid address and invalid height', async () => {
        const invalidAddress = dataProvider.getFabricatedMalformedContractAddresses()[10]; 
        const malformedHeights = dataProvider.getFabricatedMalformedHeights();
        for (const malformedHeight of malformedHeights) {
          const response = await indexerHttpClient.getContractAction(invalidAddress, { blockOffset: { height: malformedHeight } });
          expect(response).toBeError();
          expect(response.errors).toBeDefined();
          expect(response.errors?.[0]).toHaveProperty('message');
        }
      });
    });

    describe('offset validation - edge cases', () => {
      /**
       * A contract query with negative height returns an error
       *
       * @given we have a valid address and negative block height
       * @when we send a contract query using that address and height
       * @then Indexer should respond with an error
       */
      test('should return error for negative height', async () => {
        const response = await indexerHttpClient.getContractAction(validAddress, { blockOffset: { height: -1 } });
        expect(response).toBeError();
        expect(response.errors).toBeDefined();
        expect(response.errors?.[0]).toHaveProperty('message');
      });

      /**
       * A contract query with non-integer height returns an error
       *
       * @given we have a valid address and non-integer block height
       * @when we send a contract query using that address and height
       * @then Indexer should respond with an error
       */
      test('should return error for non-integer height', async () => {
        const response = await indexerHttpClient.getContractAction(validAddress, { blockOffset: { height: 0.5 } });
        expect(response).toBeError();
        expect(response.errors).toBeDefined();
        expect(response.errors?.[0]).toHaveProperty('message');
      });

      /**
       * A contract query with extremely large height returns an error
       *
       * @given we have a valid address and extremely large block height
       * @when we send a contract query using that address and height
       * @then Indexer should respond with an error
       */
      test('should return error for extremely large height', async () => {
        const response = await indexerHttpClient.getContractAction(validAddress, { blockOffset: { height: 2 ** 32 } });
        expect(response).toBeError();
        expect(response.errors).toBeDefined();
        expect(response.errors?.[0]).toHaveProperty('message');
      });
    });
  });
});

