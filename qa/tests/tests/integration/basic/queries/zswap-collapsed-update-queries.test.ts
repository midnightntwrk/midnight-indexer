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

import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import { MerkleTreeCollapsedUpdateSchema } from '@utils/indexer/graphql/schema';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { TestContext } from 'vitest';

const indexerHttpClient = new IndexerHttpClient();

describe('zswap merkle tree collapsed update queries', () => {
  describe('a collapsed update query with valid index range', () => {
    /**
     * A collapsed update query with a valid index range returns the expected result
     *
     * @given the chain has indexed blocks with zswap state
     * @when we query for a collapsed update with startIndex=0 and endIndex=1
     * @then Indexer should return a valid collapsed update
     */
    test('should return a collapsed update for a valid index range', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Zswap', 'CollapsedUpdate'],
      };

      log.debug('Requesting zswap merkle tree collapsed update with startIndex=0, endIndex=1');
      const response = await indexerHttpClient.getZswapMerkleTreeCollapsedUpdate(0, 1);

      expect(response).toBeSuccess();
      expect(response.data?.zswapMerkleTreeCollapsedUpdate).toBeDefined();

      const collapsedUpdate = response.data!.zswapMerkleTreeCollapsedUpdate;
      expect(collapsedUpdate.startIndex).toBe(0);
      expect(collapsedUpdate.endIndex).toBe(1);
      expect(collapsedUpdate.update).toBeDefined();
      expect(collapsedUpdate.protocolVersion).toBeDefined();
    });

    /**
     * A collapsed update query responds with the expected schema
     *
     * @given the chain has indexed blocks with zswap state
     * @when we query for a collapsed update with a valid index range
     * @then the response should match the expected schema
     */
    test('should respond with a collapsed update according to the expected schema', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Zswap', 'CollapsedUpdate', 'SchemaValidation'],
      };

      log.debug('Requesting zswap merkle tree collapsed update with startIndex=0, endIndex=1');
      const response = await indexerHttpClient.getZswapMerkleTreeCollapsedUpdate(0, 1);

      expect(response).toBeSuccess();
      expect(response.data?.zswapMerkleTreeCollapsedUpdate).toBeDefined();

      log.debug('Validating collapsed update schema');
      const parsed = MerkleTreeCollapsedUpdateSchema.safeParse(
        response.data!.zswapMerkleTreeCollapsedUpdate,
      );
      expect(
        parsed.success,
        `Collapsed update schema validation failed ${JSON.stringify(parsed.error, null, 2)}`,
      ).toBe(true);
    });
  });

  describe('a collapsed update query with invalid index range', () => {
    /**
     * A collapsed update query where startIndex > endIndex should return an error
     *
     * @given we use an invalid index range where startIndex > endIndex
     * @when we query for a collapsed update
     * @then Indexer should respond with an error
     */
    test('should return an error when startIndex is greater than endIndex', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Zswap', 'CollapsedUpdate', 'Negative'],
      };

      log.debug('Requesting zswap merkle tree collapsed update with startIndex=10, endIndex=5');
      const response = await indexerHttpClient.getZswapMerkleTreeCollapsedUpdate(10, 5);

      expect(response).toBeError();
    });

    /**
     * A collapsed update query with negative indices should return an error
     *
     * @given we use negative indices
     * @when we query for a collapsed update
     * @then Indexer should respond with an error
     */
    test('should return an error when indices are negative', async (ctx: TestContext) => {
      ctx.task!.meta.custom = {
        labels: ['Query', 'Zswap', 'CollapsedUpdate', 'Negative'],
      };

      log.debug('Requesting zswap merkle tree collapsed update with startIndex=-1, endIndex=1');
      const response = await indexerHttpClient.getZswapMerkleTreeCollapsedUpdate(-1, 1);

      expect(response).toBeError();
    });
  });
});
