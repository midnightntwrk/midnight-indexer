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
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import dataProvider from '@utils/testdata-provider';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { ToolkitWrapper } from '@utils/toolkit/toolkit-wrapper';
import type { DustGenerationStatusResponse } from '@utils/indexer/indexer-types';
import { DustGenerationStatusSchema } from '@utils/indexer/graphql/schema';

const indexerHttpClient = new IndexerHttpClient();

describe('dust generation status queries', () => {
  let toolkit: ToolkitWrapper;

  beforeAll(async () => {
    toolkit = new ToolkitWrapper({});
    await toolkit.start();
  });

  afterAll(async () => {
    await toolkit.stop();
  });

  describe('a dust generation status query with a valid stake key', () => {
    /**
     * A dust generation status query that uses a valid stake key responds with the expected schema
     *
     * NOTE: here we are not really interested in the status per se, but rather that we get an object back
     * that describes the dust generationstatus of the requested stake key
     *
     * NOTE2: the generation status is an array so if we request the status for N keys, we will get an array
     * of N statuses.. in this case N=1
     *
     * @given we have a valid Cardano stake key
     * @when we send a dust generation status query with that key
     * @then Indexer should respond with a dust generation status according to the requested schema
     */
    test('should respond with a dust generation status according to the requested schema', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD', 'SchemaValidation'],
        testKey: 'PM-18407',
      };

      let registeredStakeKey: string;
      try {
        log.debug('Getting registered stake key for schema validation');
        registeredStakeKey = '0'.repeat(64);
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      const response: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus([registeredStakeKey!]);

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
     * A dust generation status query validates registered field correctly for a registered key
     *
     * @given we have both registered and non-registered stake keys
     * @when we query their status
     * @then registered keys should have registered=true, non-registered should have registered=false
     */
    test('should correctly indicate registration status for a registered key', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18408',
      };

      let registeredStakeKey: string;
      try {
        registeredStakeKey = dataProvider.getCardanoStakeKey('registered-with-dust');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      // Query registered key
      const registeredResponse: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus([registeredStakeKey!]);

      expect(registeredResponse).toBeSuccess();
      const registeredStatus = registeredResponse.data?.dustGenerationStatus[0];
      expect(registeredStatus?.registered).toBe(true);
      expect(registeredStatus?.dustAddress).toBeDefined();
    });

    /**
     * A dust generation status query validates registered field correctly for a non-registered key
     *
     * @given we have both registered and non-registered stake keys
     * @when we query their status
     * @then registered keys should have registered=true, non-registered should have registered=false
     */
    test('should correctly indicate registration status for a non-registered key', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18409',
      };

      let nonRegisteredStakeKey: string;
      try {
        nonRegisteredStakeKey = dataProvider.getCardanoStakeKey('non-registered');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      // Query non-registered key
      const nonRegisteredResponse: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus([nonRegisteredStakeKey!]);

      expect(nonRegisteredResponse).toBeSuccess();
      const registeredStatus = nonRegisteredResponse.data?.dustGenerationStatus[0];
      expect(registeredStatus?.registered).toBe(false);
      expect(registeredStatus?.dustAddress).toBeDefined();
    });
  });

  describe('a dust generation status query with multiple valid stake keys', () => {
    /**
     * A dust generation status query with multiple stake keys returns multiple statuses
     * given we send the request for 10 keys (which is the limit after which the indexer returns an error)
     *
     * @given we have 10 Cardano stake keys
     * @when we send a dust generation status query with those keys
     * @then Indexer should return status for each key in the same order
     */
    test('should return statuses for multiple stake keys in order, given the number of keys is less than 10', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18410',
      };

      let registeredStakeKey: string;
      let nonRegisteredStakeKey: string;
      try {
        registeredStakeKey = dataProvider.getCardanoStakeKey('registered-with-dust');
        nonRegisteredStakeKey = dataProvider.getCardanoStakeKey('non-registered');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      const stakeKeys = [registeredStakeKey!, nonRegisteredStakeKey!];
      const response: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus(stakeKeys);

      expect(response).toBeSuccess();
      expect(response.data?.dustGenerationStatus).toBeDefined();
      expect(response.data?.dustGenerationStatus).toHaveLength(2);

      // Verify order is preserved (normalize case for hex comparison)
      expect(response.data?.dustGenerationStatus[0].cardanoStakeKey.toLowerCase()).toBe(
        registeredStakeKey!.toLowerCase(),
      );
      expect(response.data?.dustGenerationStatus[1].cardanoStakeKey.toLowerCase()).toBe(
        nonRegisteredStakeKey!.toLowerCase(),
      );
    });

    /**
     * A dust generation status query with multiple stake keys responds with the expected schema
     *
     * @given we have multiple valid Cardano stake keys
     * @when we send a dust generation status query with those keys
     * @then Indexer should respond with dust generation statuses according to the requested schema
     */
    test('should respond with dust generation statuses according to the requested schema for multiple keys', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18411',
      };

      let registeredStakeKey: string;
      let nonRegisteredStakeKey: string;
      try {
        registeredStakeKey = dataProvider.getCardanoStakeKey('registered-with-dust');
        nonRegisteredStakeKey = dataProvider.getCardanoStakeKey('non-registered');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      const stakeKeys = [registeredStakeKey!, nonRegisteredStakeKey!];
      const response: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus(stakeKeys);

      expect(response).toBeSuccess();
      expect(response.data?.dustGenerationStatus).toBeDefined();
      expect(response.data?.dustGenerationStatus).toHaveLength(2);

      // Validate each status against the schema
      for (const status of response.data?.dustGenerationStatus!) {
        log.debug(`Validating schema for stake key: ${status.cardanoStakeKey}`);
        const validationResult = DustGenerationStatusSchema.safeParse(status);
        expect(
          validationResult.success,
          `DUST generation status schema validation failed for ${status.cardanoStakeKey}: ${JSON.stringify(validationResult.error, null, 2)}`,
        ).toBe(true);
      }
    });

    /**
     * A dust generation status query with multiple stake keys returns an error when the number of keys
     * is greater than 10
     *
     * @given we have 11 Cardano stake keys
     * @when we send a dust generation status query with those keys
     * @then Indexer should return status for each key in the same order
     */
    test('should return an error given the number of keys is greater than 10', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18412',
      };

      const stakeKeys: string[] = [];
      for (let i = 0; i < 11; i++) {
        stakeKeys.push(i.toString().repeat(64));
      }

      const response: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus(stakeKeys);

      expect(response).toBeError();
    });
  });

  describe('a dust generation status query with malformed stake keys', () => {
    /**
     * A dust generation status query with malformed stake keys returns an error
     *
     * @given we fabricate malformed Cardano stake keys
     * @when we send a dust generation status query with them
     * @then Indexer should return an error for each malformed key
     */
    test('should return an error not accepting those keys', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18911',
      };

      const malformedKeys = dataProvider.getFabricatedMalformedCardanoStakeKeys();

      for (const malformedKey of malformedKeys) {
        log.debug(`Testing malformed stake key: ${malformedKey}`);

        const response: DustGenerationStatusResponse =
          await indexerHttpClient.getDustGenerationStatus([malformedKey]);

        expect.soft(response).toBeError();
      }
    });

    /**
     * A dust generation status query with hex string of wrong length returns an error
     *
     * @given we provide a hex string that is too short or too long
     * @when we send a dust generation status query
     * @then Indexer should return an error
     */
    test('should return an error for stake keys with wrong length', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18980',
      };

      const tooShort = '0'.repeat(63);
      const tooLong = '0'.repeat(65);

      const shortResponse: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus([tooShort]);
      expect(shortResponse).toBeError();

      const longResponse: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus([tooLong]);
      expect(longResponse).toBeError();
    });
  });

  describe('a dust generation status query with empty list of stake keys', () => {
    /**
     * A dust generation status query with empty array returns empty result
     *
     * @given we provide an empty array of stake keys
     * @when we send a dust generation status query
     * @then Indexer should return an empty array or an error
     */
    test('should return an emtpy list of dust generation statues', async (ctx: TestContext) => {
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

  describe('a dust generation status query with duplicate stake keys', () => {
    /**
     * A dust generation status query with duplicate stake keys returns status for each occurrence
     *
     * @given we provide duplicate stake keys in the array
     * @when we send a dust generation status query
     * @then Indexer should return status for each occurrence in the same order
     */
    test('should handle duplicate stake keys appropriately', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Dust', 'Tokenomics', 'cNgD'],
        testKey: 'PM-18982',
      };

      let registeredStakeKey: string;
      try {
        registeredStakeKey = dataProvider.getCardanoStakeKey('registered-with-dust');
      } catch (error) {
        log.warn(error);
        ctx.skip?.(true, (error as Error).message);
      }

      // Send the same key twice
      const duplicateKeys = [registeredStakeKey!, registeredStakeKey!];
      const response: DustGenerationStatusResponse =
        await indexerHttpClient.getDustGenerationStatus(duplicateKeys);

      expect(response).toBeSuccess();
      expect(response.data?.dustGenerationStatus).toBeDefined();
      expect(response.data?.dustGenerationStatus).toHaveLength(2);

      // Both results should have the same stake key
      expect(response.data?.dustGenerationStatus[0].cardanoStakeKey.toLocaleLowerCase()).toBe(
        registeredStakeKey!.toLowerCase(),
      );
      expect(response.data?.dustGenerationStatus[1].cardanoStakeKey.toLocaleLowerCase()).toBe(
        registeredStakeKey!.toLowerCase(),
      );
    });
  });
});
