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
import { Buffer } from 'node:buffer';
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import dataProvider from '@utils/testdata-provider';
import { bech32 } from 'bech32';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { ToolkitWrapper } from '@utils/toolkit/toolkit-wrapper';
import type { DustGenerationStatusResponse } from '@utils/indexer/indexer-types';
import { DustGenerationStatusSchema } from '@utils/indexer/graphql/schema';

const indexerHttpClient = new IndexerHttpClient();

const createTestRewardAddress = (byteValue: number) => {
  const payload = Buffer.alloc(29, byteValue);
  return bech32.encode('stake_test', bech32.toWords(payload));
};

const DEFAULT_REWARD_ADDRESS = createTestRewardAddress(0);

const TOOLKIT_STARTUP_TIMEOUT = 60_000;

describe('dust generation status queries', () => {
  let toolkit: ToolkitWrapper;

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  }, TOOLKIT_STARTUP_TIMEOUT);

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('a dust generation status query with a valid reward address', () => {
    /**
     * A dust generation status query that uses a valid reward address responds with the expected schema
     *
     * NOTE: here we are not really interested in the status per se, but rather that we get an object back
     * that describes the dust generation status of the requested reward address
     *
     * NOTE2: the generation status is an array so if we request the status for N keys, we will get an array
     * of N statuses.. in this case N=1
     *
     * @given we have a valid Cardano reward address
     * @when we send a dust generation status query with that key
     * @then Indexer should respond with a dust generation status according to the requested schema
     */
    test('should respond with a dust generation status according to the requested schema', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD', 'SchemaValidation'],
        testKey: 'PM-18407',
      };

      const response: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus([DEFAULT_REWARD_ADDRESS]);

      log.debug('Checking if we actually received a dust generation status');
      expect(response).toBeSuccess();
      const dustGenerationStatus = response.data?.dustGenerationStatus;
      expect(dustGenerationStatus).toBeDefined();
      expect(Array.isArray(dustGenerationStatus)).toBe(true);
      expect(dustGenerationStatus).toHaveLength(1);

      log.debug('Validating dust generation status schema');
      const status = DustGenerationStatusSchema.safeParse(response.data?.dustGenerationStatus[0]);
      expect(
        status.success,
        `DUST generation status schema validation failed ${JSON.stringify(status.error, null, 2)}`,
      ).toBe(true);
    });

    /**
     * A dust generation status query validates registered field correctly for a registered reward address
     *
     * @given we have both registered and non-registered reward addresses
     * @when we query their status
     * @then registered keys should have registered=true, non-registered should have registered=false
     */
    test('should correctly indicate registration status for a registered key', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18408',
      };

      let registeredRewardAddress: string;
      try {
        registeredRewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      // Query registered key
      const registeredResponse: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus([registeredRewardAddress!]);

      expect(registeredResponse).toBeSuccess();
      const dustGenerationStatus = registeredResponse.data?.dustGenerationStatus;
      expect(dustGenerationStatus).toBeDefined();
      expect(Array.isArray(dustGenerationStatus)).toBe(true);
      expect(dustGenerationStatus?.length).toBe(1);
      const registeredStatus = dustGenerationStatus![0];
      expect(registeredStatus?.registered).toBe(true);
      expect(registeredStatus?.dustAddress).toBeDefined();
    });

    /**
     * A dust generation status query validates registered field correctly for a non-registered reward address
     *
     * @given we have both registered and non-registered reward addresses
     * @when we query their status
     * @then registered keys should have registered=true, non-registered should have registered=false
     */
    test('should correctly indicate registration status for a non-registered key', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18409',
      };

      let nonRegisteredRewardAddress: string;
      try {
        nonRegisteredRewardAddress = dataProvider.getCardanoRewardAddress('non-registered');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      // Query non-registered key
      const nonRegisteredResponse: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus([nonRegisteredRewardAddress!]);

      expect(nonRegisteredResponse).toBeSuccess();
      const registeredStatus = nonRegisteredResponse.data?.dustGenerationStatus[0];
      expect(registeredStatus?.registered).toBe(false);
      expect(registeredStatus?.dustAddress).toBeDefined();
    });
  });

  describe('a dust generation status query with multiple valid reward addresses', () => {
    /**
     * A dust generation status query with multiple reward addresses returns multiple statuses
     * given we send the request for 10 addresses (which is the limit after which the indexer returns an error)
     *
     * @given we have 10 Cardano reward addresses
     * @when we send a dust generation status query with those addresses
     * @then Indexer should return status for each address in the same order
     */
    test('should return statuses for multiple reward addresses in order, given the number of addresses is less than 10', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18410',
      };

      let registeredRewardAddress: string;
      let nonRegisteredRewardAddress: string;
      try {
        registeredRewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust');
        nonRegisteredRewardAddress = dataProvider.getCardanoRewardAddress('non-registered');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      const rewardAddresses = [registeredRewardAddress!, nonRegisteredRewardAddress!];
      const response: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus(rewardAddresses);

      expect(response).toBeSuccess();
      expect(response.data?.dustGenerationStatus).toBeDefined();
      expect(response.data?.dustGenerationStatus).toHaveLength(2);

      // Verify order is preserved (normalize case for string comparison)
      expect(response.data?.dustGenerationStatus[0].cardanoRewardAddress.toLowerCase()).toBe(
        registeredRewardAddress!.toLowerCase(),
      );
      expect(response.data?.dustGenerationStatus[1].cardanoRewardAddress.toLowerCase()).toBe(
        nonRegisteredRewardAddress!.toLowerCase(),
      );
    });

    /**
     * A dust generation status query with multiple reward addresses responds with the expected schema
     *
     * @given we have multiple valid Cardano reward addresses
     * @when we send a dust generation status query with those addresses
     * @then Indexer should respond with dust generation statuses according to the requested schema
     */
    test('should respond with dust generation statuses according to the requested schema for multiple addresses', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18411',
      };

      let registeredRewardAddress: string;
      let nonRegisteredRewardAddress: string;
      try {
        registeredRewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust');
        nonRegisteredRewardAddress = dataProvider.getCardanoRewardAddress('non-registered');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      const rewardAddresses = [registeredRewardAddress!, nonRegisteredRewardAddress!];
      const response: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus(rewardAddresses);

      expect(response).toBeSuccess();
      expect(response.data!.dustGenerationStatus).toBeDefined();
      expect(Array.isArray(response.data!.dustGenerationStatus)).toBe(true);
      expect(response.data!.dustGenerationStatus).toHaveLength(2);

      // Validate each status against the schema
      for (const status of response.data!.dustGenerationStatus) {
        log.debug(`Validating schema for reward address: ${status.cardanoRewardAddress}`);
        const validationResult = DustGenerationStatusSchema.safeParse(status);
        expect(
          validationResult.success,
          `DUST generation status schema validation failed for ${status.cardanoRewardAddress}: ${JSON.stringify(validationResult.error, null, 2)}`,
        ).toBe(true);
      }
    });

    /**
     * A dust generation status query with multiple reward addresses returns an error when the number of addresses
     * is greater than 10
     *
     * @given we have 11 Cardano reward addresses
     * @when we send a dust generation status query with those addresses
     * @then Indexer should return status for each address in the same order
     */
    test('should return an error given the number of addresses is greater than 10', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18412',
      };

      const rewardAddresses: string[] = [];
      for (let i = 0; i < 11; i++) {
        rewardAddresses.push(createTestRewardAddress(i + 1));
      }

      const response: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus(rewardAddresses);

      expect(response).toBeError();
    });
  });

  describe('a dust generation status query with malformed reward addresses', () => {
    /**
     * A dust generation status query with hex string of wrong length returns an error
     *
     * @given we provide a hex string that is too short or too long
     * @when we send a dust generation status query
     * @then Indexer should return an error
     */
    test('should return an error for reward addresses with wrong format', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18980',
      };

      const validRewardAddress = createTestRewardAddress(3);
      const tooShort = validRewardAddress.slice(0, -1);
      const tooLong = `${validRewardAddress}q`;

      const shortResponse: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus([tooShort]);
      expect(shortResponse).toBeError();

      const longResponse: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus([tooLong]);
      expect(longResponse).toBeError();
    });
  });

  describe('a dust generation status query with empty list of reward addresses', () => {
    /**
     * A dust generation status query with empty array returns empty result
     *
     * @given we provide an empty array of reward addresses
     * @when we send a dust generation status query
     * @then Indexer should return an empty array or an error
     */
    test('should return an empty list of dust generation statuses', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18981',
      };

      const response: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus([]);

      // The API should either return an empty array or an error
      // We check for both possibilities
      if (response.errors) {
        expect(response).toBeError();
      } else {
        expect(response).toBeSuccess();
        expect(response.data?.dustGenerationStatus).toBeDefined();
        expect(response.data?.dustGenerationStatus).toHaveLength(0);
      }
    });
  });

  describe('a dust generation status query with duplicate reward addresses', () => {
    /**
     * A dust generation status query with duplicate reward addresses returns status for each occurrence
     *
     * @given we provide duplicate reward addresses in the array
     * @when we send a dust generation status query
     * @then Indexer should return status for each occurrence in the same order
     */
    test('should handle duplicate reward addresses appropriately', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18982',
      };

      let registeredRewardAddress: string;
      try {
        registeredRewardAddress = dataProvider.getCardanoRewardAddress('registered-with-dust');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      // Send the same key twice
      const duplicateRewardAddresses = [registeredRewardAddress!, registeredRewardAddress!];
      const response: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus(duplicateRewardAddresses);

      expect(response).toBeSuccess();
      expect(response.data?.dustGenerationStatus).toBeDefined();
      expect(response.data?.dustGenerationStatus).toHaveLength(2);

      // Both results should have the same reward address
      expect(response.data?.dustGenerationStatus[0].cardanoRewardAddress.toLocaleLowerCase()).toBe(
        registeredRewardAddress!.toLowerCase(),
      );
      expect(response.data?.dustGenerationStatus[1].cardanoRewardAddress.toLocaleLowerCase()).toBe(
        registeredRewardAddress!.toLowerCase(),
      );
    });
  });
});
