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

import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import type { GraphQLResponse } from '@utils/indexer/indexer-types';

const httpClient = new IndexerHttpClient();

/**
 * The maximum epoch span the SPO series resolvers accept, mirroring
 * MAX_EPOCH_SPAN in indexer-api/src/infra/api/v4/query.rs. The guard is
 * inclusive: a span equal to the maximum is accepted.
 */
const MAX_EPOCH_SPAN = 10_000;

/**
 * Every over-cap span used here stays modest (at most 20_000 epochs). On a build
 * that predates the guard the request is not rejected but expanded per epoch, so
 * the values are deliberately bounded rather than using the largest GraphQL Int.
 */
const OVER_CAP_TO_EPOCH = MAX_EPOCH_SPAN + 1;
const WIDE_OVER_CAP_TO_EPOCH = MAX_EPOCH_SPAN * 2;

/** The guard's rejection message, as emitted by validate_epoch_span. */
const EPOCH_SPAN_ERROR = 'epoch range too large';

function isEpochSpanRejection(response: GraphQLResponse<unknown>): boolean {
  return (response.errors ?? []).some((error) => error.message.includes(EPOCH_SPAN_ERROR));
}

/**
 * The rejection cases are deliberately never skipped: every line carrying this
 * suite also carries the guard, so a target that accepts an over-cap span is a
 * regression of the fix and must fail the run rather than pass it silently.
 */
describe('SPO epoch series queries', () => {
  describe('a registered totals series query with an epoch span within the maximum', () => {
    /**
     * A single-epoch range is the narrowest valid span and must be served.
     *
     * @given an indexer serving the SPO series surface
     * @when registered totals are requested for a range whose from and to epoch are equal
     * @then the query succeeds, because a span of 0 is within the maximum of 10000
     */
    test('should accept a range covering a single epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Spo', 'Epoch'] };

      const response = await httpClient.getRegisteredTotalsSeries(0, 0);

      expect(response).toBeSuccess();
      expect(response.data?.registeredTotalsSeries).toBeDefined();
    });

    /**
     * The cap is inclusive, so the widest permitted span must still be served.
     *
     * @given an indexer serving the SPO series surface
     * @when registered totals are requested for a span exactly equal to the maximum
     * @then the query succeeds, because a span of 10000 does not exceed the maximum of 10000
     */
    test('should accept a range whose span equals the maximum', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Spo', 'Epoch'] };

      const response = await httpClient.getRegisteredTotalsSeries(0, MAX_EPOCH_SPAN);

      expect(response).toBeSuccess();
      expect(response.data?.registeredTotalsSeries).toBeDefined();
    });

    /**
     * The span is a width, not an absolute bound, so a permitted width high up the
     * epoch axis must be served too.
     *
     * @given an indexer serving the SPO series surface
     * @when registered totals are requested for epochs 5000 to 15000
     * @then the query succeeds, because the span of 10000 is measured as a width, not from zero
     */
    test('should accept a permitted span that does not start at epoch zero', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Spo', 'Epoch'] };

      const response = await httpClient.getRegisteredTotalsSeries(5_000, 5_000 + MAX_EPOCH_SPAN);

      expect(response).toBeSuccess();
      expect(response.data?.registeredTotalsSeries).toBeDefined();
    });
  });

  describe('a registered totals series query with an epoch span exceeding the maximum', () => {
    /**
     * One epoch beyond the cap is the tightest rejection case and pins the boundary.
     *
     * @given an indexer carrying the epoch-span cap
     * @when registered totals are requested for a span of 10001 epochs
     * @then the query is rejected as a client error naming the exceeded maximum
     */
    test('should reject a range one epoch beyond the maximum span', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Spo', 'Epoch', 'Negative'] };

      const response = await httpClient.getRegisteredTotalsSeries(0, OVER_CAP_TO_EPOCH);

      expect(response).toBeError();
      expect(isEpochSpanRejection(response)).toBe(true);
    });

    /**
     * The guard measures an absolute width, so a descending range must be rejected
     * on the same basis as an ascending one.
     *
     * @given an indexer carrying the epoch-span cap
     * @when registered totals are requested with the from epoch above the to epoch, spanning 10001 epochs
     * @then the query is rejected, because the span is measured regardless of argument order
     */
    test('should reject a reversed range whose span exceeds the maximum', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Spo', 'Epoch', 'Negative'] };

      const response = await httpClient.getRegisteredTotalsSeries(OVER_CAP_TO_EPOCH, 0);

      expect(response).toBeError();
      expect(isEpochSpanRejection(response)).toBe(true);
    });

    /**
     * A span well past the cap must be refused outright rather than partially served.
     *
     * @given an indexer carrying the epoch-span cap
     * @when registered totals are requested for a span of 20000 epochs
     * @then the query is rejected and no partial series is returned, which
     *       toBeError covers by requiring a null data payload alongside the error
     */
    test('should reject a range far beyond the maximum span without returning data', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Spo', 'Epoch', 'Negative'] };

      const response = await httpClient.getRegisteredTotalsSeries(0, WIDE_OVER_CAP_TO_EPOCH);

      expect(response).toBeError();
      expect(isEpochSpanRejection(response)).toBe(true);
    });
  });

  describe('a registered SPO series query with an epoch span exceeding the maximum', () => {
    /**
     * The cap guards every series resolver, not only the totals one.
     *
     * @given an indexer carrying the epoch-span cap
     * @when registered SPO statistics are requested for a span of 10001 epochs
     * @then the query is rejected as a client error naming the exceeded maximum
     */
    test('should reject a range beyond the maximum span', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Spo', 'Epoch', 'Negative'] };

      const response = await httpClient.getRegisteredSpoSeries(0, OVER_CAP_TO_EPOCH);

      expect(response).toBeError();
      expect(isEpochSpanRejection(response)).toBe(true);
    });

    /**
     * The guard must not narrow what the resolver already served.
     *
     * @given an indexer serving the SPO series surface
     * @when registered SPO statistics are requested for a span exactly equal to the maximum
     * @then the query succeeds, because a span of 10000 is still permitted
     */
    test('should accept a range whose span equals the maximum', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Spo', 'Epoch'] };

      const response = await httpClient.getRegisteredSpoSeries(0, MAX_EPOCH_SPAN);

      expect(response).toBeSuccess();
      expect(response.data?.registeredSpoSeries).toBeDefined();
    });
  });

  describe('a registered presence query with an epoch span exceeding the maximum', () => {
    /**
     * The presence resolver expands per epoch as the series resolvers do, so it
     * carries the same cap.
     *
     * @given an indexer carrying the epoch-span cap
     * @when raw presence events are requested for a span of 10001 epochs
     * @then the query is rejected as a client error naming the exceeded maximum
     */
    test('should reject a range beyond the maximum span', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Spo', 'Epoch', 'Negative'] };

      const response = await httpClient.getRegisteredPresence(0, OVER_CAP_TO_EPOCH);

      expect(response).toBeError();
      expect(isEpochSpanRejection(response)).toBe(true);
    });

    /**
     * The guard must not narrow what the resolver already served.
     *
     * @given an indexer serving the SPO series surface
     * @when raw presence events are requested for a span exactly equal to the maximum
     * @then the query succeeds, because a span of 10000 is still permitted
     */
    test('should accept a range whose span equals the maximum', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Spo', 'Epoch'] };

      const response = await httpClient.getRegisteredPresence(0, MAX_EPOCH_SPAN);

      expect(response).toBeSuccess();
      expect(response.data?.registeredPresence).toBeDefined();
    });
  });
});
