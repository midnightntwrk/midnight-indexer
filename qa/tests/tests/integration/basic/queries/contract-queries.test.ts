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
import {
  ContractDeployActionSchema,
  ContractCallActionSchema,
  ContractUpdateActionSchema,
} from '@utils/indexer/graphql/schema';
import dataProvider from '@utils/testdata-provider';
import { IndexerHttpClient } from '@utils/indexer/http-client';

const indexerHttpClient = new IndexerHttpClient();

describe('contract queries', () => {
  describe('a contract query by address', () => {
    /**
     * A contract query with boundary contract addresses returns success
     *
     * @given we have boundary contract addresses
     * @when we send a contract query using each boundary address
     * @then Indexer should respond with success for each address
     */
    test('should return success as they are valid contract addresses', async () => {
      const malformedAddress: string[] = dataProvider.getBoundaryContractAddresses();

      for (const address of malformedAddress) {
        const response = await indexerHttpClient.getContractAction(address);
        expect(response).toBeSuccess();
        expect(response.data?.contractAction).toBeDefined();
      }
    });

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
    test('should return null when contract with that address does not exist', async (context: TestContext) => {
      let nonExistentContractAddress: string;
      try {
        nonExistentContractAddress = dataProvider.getNonExistingContractAddress();
      } catch (error) {
        log.warn(error);
        context.skip?.(true, (error as Error).message);
      }

      const response = await indexerHttpClient.getContractAction(nonExistentContractAddress!);
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });

    /**
     * A contract query with malformed addresses returns an error
     *
     * @given we have fabricated malformed contract addresses
     * @when we send a contract query using each malformed address
     * @then Indexer should respond with an error for each address
     */
    test('should return an error for malformed addresses', async () => {
      const fabricatedMalformedAddresses = dataProvider.getFabricatedMalformedContractAddresses();

      for (const malformedAddress of fabricatedMalformedAddresses) {
        const response = await indexerHttpClient.getContractAction(malformedAddress);

        expect.soft(response).toBeError();
      }
    });
  });

  describe('a contract query by address and offset', () => {
    const validAddress = dataProvider.getNonExistingContractAddress();

    /**
     * A contract query by address and offset returns the correct action using exact block hash
     *
     * @given we have an existing contract address and the exact block hash where it was included
     * @when we send a contract query using that address and block hash
     * @then Indexer should respond with successful response and non-null contractAction with correct data
     */
    test('should return the correct action using exact block hash where it was included', async (context: TestContext) => {
      let existingContractAddress: string;
      let contractDeployBlockHash: string;
      try {
        existingContractAddress = dataProvider.getKnownContractAddress();
        contractDeployBlockHash = await dataProvider.getContractDeployBlockHash();
      } catch (error) {
        log.warn(error);
        context.skip?.(true, (error as Error).message);
      }
      const response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { hash: contractDeployBlockHash! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
    });

    /**
     * A contract query by address and offset returns the latest state using a future block hash
     *
     * @given we have an existing contract address and a valid block hash from a future block
     * @when we send a contract query using that address and future block hash
     * @then Indexer should respond with successful response and non-null contractAction reflecting latest state
     */
    test('should return the latest state using a future block hash', async (context: TestContext) => {
      let existingContractAddress: string;
      let contractUpdateBlockHash: string;
      try {
        existingContractAddress = dataProvider.getKnownContractAddress();
        contractUpdateBlockHash = await dataProvider.getContractUpdateBlockHash();
      } catch (error) {
        log.warn(error);
        context.skip?.(true, (error as Error).message);
      }

      const response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { hash: contractUpdateBlockHash! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
    });

    /**
     * A contract query by address returns a contract action that conforms to the correct schema
     *
     * @given we have an existing contract address
     * @when we send a contract query using that address
     * @then Indexer should respond with successful response and contractAction that conforms to the correct schema
     */
    test('should respond with a contract action according to the expected schema', async (context: TestContext) => {
      const contractAddress = dataProvider.getKnownContractAddress();
      const response = await indexerHttpClient.getContractAction(contractAddress);
      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeDefined();

      const contractAction = response.data!.contractAction!;
      const typename = contractAction.__typename;

      log.debug(`Validating contract action schema for type: ${typename}`);

      const schemaMap = {
        ContractDeploy: ContractDeployActionSchema,
        ContractCall: ContractCallActionSchema,
        ContractUpdate: ContractUpdateActionSchema,
      } as const;

      expect(
        Object.keys(schemaMap).includes(typename),
        `Unexpected contract action type: ${typename}`,
      ).toBe(true);

      const schema = schemaMap[typename as keyof typeof schemaMap];
      log.debug(`Validating with schema: ${typename}`);

      const parsed = schema.safeParse(contractAction);

      if (!parsed.success) {
        log.debug('Schema validation failed');
        log.debug(JSON.stringify(parsed.error, null, 2));
      } else {
        log.debug(`Schema validation succeeded for ${typename}`);
      }
      expect(
        parsed.success,
        `Contract action schema validation failed: ${JSON.stringify(parsed.error, null, 2)}`,
      ).toBe(true);
    });

    /**
     * A contract query by address and offset returns the correct action using exact block height
     *
     * @given we have an existing contract address and the exact block height where it was included
     * @when we send a contract query using that address and block height
     * @then Indexer should respond with successful response and non-null contractAction with correct data
     */
    test('should return the correct action using exact block height where it was included', async (context: TestContext) => {
      let existingContractAddress: string;
      let contractDeployHeight: number;
      try {
        existingContractAddress = dataProvider.getKnownContractAddress();
        contractDeployHeight = await dataProvider.getContractDeployBlockHeight();
      } catch (error) {
        log.warn(error);
        context.skip?.(true, (error as Error).message);
      }
      const response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { height: contractDeployHeight! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
    });

    /**
     * A contract query by address and offset returns the latest state using a future block height
     *
     * @given we have an existing contract address and a valid block height from a future block
     * @when we send a contract query using that address and future block height
     * @then Indexer should respond with successful response and non-null contractAction reflecting latest state
     */
    test('should return the latest state using a future block height', async (context: TestContext) => {
      let existingContractAddress: string;
      let contractUpdateHeight: number;
      try {
        existingContractAddress = dataProvider.getKnownContractAddress();
        contractUpdateHeight = await dataProvider.getContractUpdateBlockHeight();
      } catch (error) {
        log.warn(error);
        context.skip?.(true, (error as Error).message);
      }
      const response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { height: contractUpdateHeight! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
    });

    /**
     * A contract query by address and block offset by height returns the most recent contract action for that address before the specified block
     *
     * @given we have multiple contract actions in different blocks (example: ContractDeploy block 49, ContractUpdate block 59)
     * @when we send a contract query using that address and a past block height (example: block 49)
     * @then Indexer should return the most recent action for the address before the specified block height (so ContractDeploy block 49)
     */
    test('should return the most recent contract action for that address before the specified block', async (context: TestContext) => {
      let existingContractAddress: string;
      let contractDeployHeight: number;
      let contractCallHeight: number;

      try {
        existingContractAddress = dataProvider.getKnownContractAddress();
        contractDeployHeight = await dataProvider.getContractDeployBlockHeight();
        contractCallHeight = await dataProvider.getContractCallBlockHeight();
      } catch (error) {
        log.warn(error);
        context.skip?.(true, (error as Error).message);
      }
      let response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { height: contractDeployHeight! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
      expect(response.data?.contractAction?.__typename).toBe('ContractDeploy');

      response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { height: contractCallHeight! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).not.toBeNull();
      expect(response.data?.contractAction?.address).toBe(existingContractAddress!);
      expect(response.data?.contractAction?.__typename).toBe('ContractCall');
    });

    /**
     * A contract query by address and offset returns null when contract does not exist
     *
     * @given we have a valid address and valid block offset
     * @when we send a contract query using that address and offset
     * @then Indexer should respond with null contract action
     */
    test('should return null when contract with valid address and valid offset does not exist', async (context: TestContext) => {
      let knownBlockHash: string;
      try {
        knownBlockHash = await dataProvider.getKnownBlockHash();
      } catch (error) {
        log.warn(error);
        context.skip?.(true, (error as Error).message);
      }
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { hash: knownBlockHash! },
      });
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
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { hash: nonExistingHash },
      });
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
    test('should return error when contract with invalid address and valid hash', async (context: TestContext) => {
      let knownBlockHash: string;
      try {
        knownBlockHash = await dataProvider.getKnownBlockHash();
      } catch (error) {
        log.warn(error);
        context.skip?.(true, (error as Error).message);
      }
      const invalidAddress = dataProvider.getFabricatedMalformedContractAddresses()[10];
      const response = await indexerHttpClient.getContractAction(invalidAddress, {
        blockOffset: { hash: knownBlockHash! },
      });
      expect(response).toBeError();
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
      const response = await indexerHttpClient.getContractAction(invalidAddress, {
        blockOffset: { hash: nonExistingHash },
      });
      expect(response).toBeError();
    });

    /**
     * A contract query with valid address and invalid hash returns an error
     *
     * @given we have a valid address and invalid block hash
     * @when we send a contract query using that address and hash
     * @then Indexer should respond with an error
     */
    test('should return error when contract with valid address and invalid hash', async (context: TestContext) => {
      let knownBlockHash: string;
      try {
        knownBlockHash = await dataProvider.getKnownBlockHash();
      } catch (error) {
        log.warn(error);
        context.skip?.(true, (error as Error).message);
      }
      const malformedHashes = dataProvider.getFabricatedMalformedHashes();
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { hash: malformedHashes[0] },
      });
      expect(response).toBeError();
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
        const response = await indexerHttpClient.getContractAction(invalidAddress, {
          blockOffset: { hash: malformedHash },
        });
        expect(response).toBeError();
      }
    });

    /**
     * A contract query with valid address and valid height returns null when contract does not exist
     *
     * @given we have a valid address and valid block height
     * @when we send a contract query using that address and height
     * @then Indexer should respond with null contract action
     */
    test('should return null when contract with valid address and valid height does not exist', async () => {
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { height: 0 },
      });
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
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { height: 999999 },
      });
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
      const response = await indexerHttpClient.getContractAction(invalidAddress, {
        blockOffset: { height: 0 },
      });
      expect(response).toBeError();
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
        const response = await indexerHttpClient.getContractAction(invalidAddress, {
          blockOffset: { height: malformedHeight },
        });
        expect(response).toBeError();
      }
    });

    /**
     * A contract query with negative height returns an error
     *
     * @given we have a valid address and negative block height
     * @when we send a contract query using that address and height
     * @then Indexer should respond with an error
     */
    test('should return error for negative height', async () => {
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { height: -1 },
      });
      expect(response).toBeError();
    });

    /**
     * A contract query with non-integer height returns an error
     *
     * @given we have a valid address and non-integer block height
     * @when we send a contract query using that address and height
     * @then Indexer should respond with an error
     */
    test('should return error for non-integer height', async () => {
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { height: 0.5 },
      });
      expect(response).toBeError();
    });

    /**
     * A contract query with extremely large height returns an error
     *
     * @given we have a valid address and extremely large block height
     * @when we send a contract query using that address and height
     * @then Indexer should respond with an error
     */
    test('should return error for extremely large height', async () => {
      const response = await indexerHttpClient.getContractAction(validAddress, {
        blockOffset: { height: 2 ** 32 },
      });
      expect(response).toBeError();
    });

    /**
     * A contract query by address and offset returns null when using a block hash from before the action existed
     *
     * @given we have an existing contract address and a valid block hash from before the contract existed
     * @when we send a contract query using that address and past block hash
     * @then Indexer should respond with successful response and null contractAction
     */
    test('should return null when using a block hash from before the action existed', async (context: TestContext) => {
      let existingContractAddress: string;
      const genesisBlockHash = (await indexerHttpClient.getBlockByOffset({ height: 0 })).data?.block
        .hash;
      try {
        existingContractAddress = dataProvider.getKnownContractAddress();
      } catch (error) {
        log.warn(error);
        context.skip?.(true, (error as Error).message);
      }
      const response = await indexerHttpClient.getContractAction(existingContractAddress!, {
        blockOffset: { hash: genesisBlockHash! },
      });

      expect(response).toBeSuccess();
      expect(response.data?.contractAction).toBeNull();
    });
  });
});
