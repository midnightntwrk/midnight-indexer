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

// Integration tests for the SPO (stake pool operator) registration surface of
// the indexer GraphQL API (#1003): spoCount, spoList, spoIdentities,
// stakePoolOperators, poolMetadata, poolMetadataList, the pool-id lookups
// (spoByPoolId, spoIdentityByPoolId, spoCompositeByPoolId), the performance
// queries (spoPerformanceLatest, spoPerformanceBySpoSk, epochPerformance,
// epochUtilization), the epoch-range series (registeredTotalsSeries,
// registeredSpoSeries, registeredPresence, registeredFirstValidEpochs) and
// stakeDistribution. Governance, epoch and committee data live in
// spo-queries.test.ts.
//
// Surface presence is decided from schema introspection, never from a domain
// query's success: a probe that fails for any other reason (outage, 5xx) fails
// the suite in beforeAll instead of turning every test into a silent skip.
//
// Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/1003

import log from '@utils/logging/logger';
import { env } from 'environment/model';
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import {
  EpochPerfSchema,
  FirstValidEpochSchema,
  PoolMetadataSchema,
  PresenceEventSchema,
  RegisteredStatSchema,
  RegisteredTotalsSchema,
  SpoHex,
  SpoIdentitySchema,
  SpoSchema,
  StakeShareSchema,
} from '@utils/indexer/graphql/schema';
import type { EpochInfo, GraphQLResponse } from '@utils/indexer/indexer-types';
import { fetchQueryFieldNames } from '@utils/indexer/schema-feature-probe';
import {
  CLAMP_PROBE_LIMITS,
  EPOCH_PERFORMANCE_DEFAULT_LIMIT,
  EPOCH_SPAN_LIMIT,
  FABRICATED_POOL_ID,
  FABRICATED_SPO_SK,
  MALFORMED_POOL_IDS,
  MAX_GRAPHQL_INT,
  POOL_METADATA_LIST_DEFAULT_LIMIT,
  SPO_IDENTITIES_DEFAULT_LIMIT,
  SPO_IDENTITIES_MAX_LIMIT,
  SPO_LIST_DEFAULT_LIMIT,
  SPO_LIST_MAX_LIMIT,
  SPO_PERFORMANCE_BY_SK_DEFAULT_LIMIT,
  SPO_PERFORMANCE_LATEST_DEFAULT_LIMIT,
  STAKE_DISTRIBUTION_DEFAULT_LIMIT,
  STAKE_POOL_OPERATORS_DEFAULT_LIMIT,
  STAKE_POOL_OPERATORS_MAX_LIMIT,
  WIDE_LIST_MAX_LIMIT,
  assertNoGraphqlErrors,
  clampedLimit,
  epochRange,
  expectOrdered,
  expectSpanGuardError,
  expectValidList,
  hasSpanGuardError,
  hexCaseAndPrefixVariants,
  skipUnlessServed,
  skipWithReason,
  surfaceAbsentReason,
} from '@utils/indexer/spo-test-support';
import dataProvider from '@utils/testdata-provider';

const httpClient = new IndexerHttpClient();

// Root Query fields every test in this file depends on.
const CORE_FIELDS = [
  'spoCount',
  'spoList',
  'spoIdentities',
  'stakePoolOperators',
  'registeredTotalsSeries',
];
const CORE_SURFACE = `SPO registration surface (${CORE_FIELDS.join(', ')})`;
// Range width used for the "real range" series tests, anchored at the current epoch.
const RANGE_SPAN = 10;
// Range width for the reversed-bounds test; kept short so both orderings are cheap.
const REVERSED_RANGE_SPAN = 3;
// Epoch used for the single-epoch range when no spo-indexer epoch data exists.
const FALLBACK_EPOCH = 1;
// A negative offset every list endpoint must floor to zero.
const NEGATIVE_OFFSET = -5;
// An in-range explicit limit, and an explicit page, for the pagination checks.
const EXPLICIT_LIMIT = 5;
const EXPLICIT_PAGE = { limit: 3, offset: 1 };
// Pool-id prefix for the search filters; matches nothing while the surface is empty.
const SEARCH_TERM = FABRICATED_POOL_ID.slice(0, 4);

let queryFields = new Set<string>();
let surfacePresent = false;
let spanGuardPresent = false;
// The over-span probe response, kept so the negative test asserts on the very
// request that decided `spanGuardPresent` instead of issuing it a second time.
let spanProbe: Awaited<ReturnType<IndexerHttpClient['getRegisteredTotalsSeries']>> | null = null;
let epochInfo: EpochInfo | null = null;

function spanGuardReason(): string {
  return `epoch-span guard (#1455, indexer >= 4.4.0-rc.4) not deployed on ${env.getCurrentEnvironmentName()}`;
}

function noEpochReason(): string {
  return `no spo-indexer epoch data on ${env.getCurrentEnvironmentName()} to anchor a range`;
}

/**
 * Probes a list endpoint with out-of-range limits and asserts each request
 * succeeds and returns no more rows than the server-side clamp of that limit
 * allows (zero clamps up to one row, an over-large value down to `maxLimit`).
 */
async function expectLimitClamped<R extends GraphQLResponse<unknown>>(
  label: string,
  maxLimit: number,
  query: (limit: number) => Promise<R>,
  items: (response: R) => unknown[],
): Promise<void> {
  for (const limit of CLAMP_PROBE_LIMITS) {
    const bound = clampedLimit(limit, maxLimit);
    const response = await query(limit);
    expect(response, `${label}(limit: ${limit}) should succeed`).toBeSuccess();
    expect(
      items(response).length,
      `${label}(limit: ${limit}) should clamp to ${bound}`,
    ).toBeLessThanOrEqual(bound);
  }
}

describe.skipIf(env.isUndeployedEnv())('spo registration queries', () => {
  beforeAll(async () => {
    // Throws on an unreachable indexer or a broken introspection, failing the
    // suite instead of skipping it.
    queryFields = await fetchQueryFieldNames();

    const missing = CORE_FIELDS.filter((field) => !queryFields.has(field));
    if (missing.length > 0) {
      log.warn(
        `${CORE_SURFACE} not served on ${env.getCurrentEnvironmentName()} (missing ${missing.join(', ')}); skipping`,
      );
      return;
    }
    surfacePresent = true;

    spanProbe = await httpClient.getRegisteredTotalsSeries(0, EPOCH_SPAN_LIMIT + 1);
    spanGuardPresent = hasSpanGuardError(spanProbe);
    if (!spanGuardPresent && spanProbe.errors?.length) {
      // Any other error is not "guard absent", it is a broken endpoint.
      throw new Error(
        `registeredTotalsSeries span probe failed: ${JSON.stringify(spanProbe.errors)}`,
      );
    }
    if (!spanGuardPresent) log.warn(spanGuardReason());

    if (queryFields.has('currentEpochInfo')) {
      const epochResponse = await httpClient.getCurrentEpochInfo();
      assertNoGraphqlErrors('currentEpochInfo', epochResponse);
      epochInfo = epochResponse.data!.currentEpochInfo;
      if (!epochInfo) log.warn(noEpochReason());
    }
  }, 60_000);

  describe('spoCount', () => {
    /**
     * @given any environment
     * @when spoCount is queried
     * @then a non-negative integer is returned (the resolver never returns null
     *       even though the field is nullable in the schema)
     */
    test('should return a non-negative integer', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const response = await httpClient.getSpoCount();

      expect(response).toBeSuccess();
      const count = response.data!.spoCount;
      expect(count).not.toBeNull();
      expect(Number.isInteger(count)).toBe(true);
      expect(count!).toBeGreaterThanOrEqual(0);
    });
  });

  describe('spoList', () => {
    /**
     * @given any environment
     * @when spoList is queried without arguments
     * @then a list bounded by the default limit is returned and every item
     *       matches the Spo schema
     */
    test('should return a well-formed list with default pagination', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const response = await httpClient.getSpoList();

      expect(response).toBeSuccess();
      const spos = expectValidList(response.data!.spoList, SpoSchema, 'spoList');
      expect(spos.length).toBeLessThanOrEqual(SPO_LIST_DEFAULT_LIMIT);
    });

    /**
     * @given explicit limit, offset and search arguments
     * @when spoList is queried with them
     * @then the request succeeds, honours the limit and every item matches the
     *       Spo schema
     */
    test('should honour explicit limit, offset and search arguments', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const filtered = await httpClient.getSpoList({
        limit: EXPLICIT_LIMIT,
        offset: 0,
        search: SEARCH_TERM,
      });

      expect(filtered).toBeSuccess();
      expectValidList(filtered.data!.spoList, SpoSchema, 'spoList(search)');
      expect(filtered.data!.spoList.length).toBeLessThanOrEqual(EXPLICIT_LIMIT);
    });

    /**
     * @given out-of-range limits
     * @when spoList is queried with each
     * @then every request succeeds and returns no more rows than the clamped limit
     */
    test('should clamp out-of-range limits', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      await expectLimitClamped(
        'spoList',
        SPO_LIST_MAX_LIMIT,
        (limit) => httpClient.getSpoList({ limit }),
        (response) => response.data!.spoList,
      );
    });

    /**
     * @given a whitespace-only search string
     * @when spoList is queried with it
     * @then the result equals the unfiltered list (the resolver trims the
     *       search and treats an empty one as absent)
     */
    test('should treat a whitespace-only search as unfiltered', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const [unfiltered, blankSearch] = await Promise.all([
        httpClient.getSpoList(),
        httpClient.getSpoList({ search: '   ' }),
      ]);

      expect(unfiltered).toBeSuccess();
      expect(blankSearch).toBeSuccess();
      expect(blankSearch.data!.spoList).toEqual(unfiltered.data!.spoList);
    });
  });

  describe('spoIdentities', () => {
    /**
     * @given any environment
     * @when spoIdentities is queried without arguments
     * @then a list bounded by the default limit is returned and every item
     *       matches the SpoIdentity schema
     */
    test('should return a well-formed list with default pagination', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const response = await httpClient.getSpoIdentities();

      expect(response).toBeSuccess();
      const identities = expectValidList(
        response.data!.spoIdentities,
        SpoIdentitySchema,
        'spoIdentities',
      );
      expect(identities.length).toBeLessThanOrEqual(SPO_IDENTITIES_DEFAULT_LIMIT);
    });

    /**
     * @given an explicit limit and offset
     * @when spoIdentities is queried with them
     * @then the request succeeds, honours the limit and every item matches the
     *       SpoIdentity schema
     */
    test('should honour an explicit limit and offset', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const response = await httpClient.getSpoIdentities(EXPLICIT_PAGE);

      expect(response).toBeSuccess();
      const identities = expectValidList(
        response.data!.spoIdentities,
        SpoIdentitySchema,
        'spoIdentities(paged)',
      );
      expect(identities.length).toBeLessThanOrEqual(EXPLICIT_PAGE.limit);
    });

    /**
     * @given out-of-range limits
     * @when spoIdentities is queried with each
     * @then every request succeeds and returns no more rows than the clamped limit
     */
    test('should clamp out-of-range limits', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      await expectLimitClamped(
        'spoIdentities',
        SPO_IDENTITIES_MAX_LIMIT,
        (limit) => httpClient.getSpoIdentities({ limit }),
        (response) => response.data!.spoIdentities,
      );
    });
  });

  describe('stakePoolOperators', () => {
    /**
     * @given any environment
     * @when stakePoolOperators is queried without arguments
     * @then a list bounded by the default limit is returned and every item is a
     *       hex SPO key
     */
    test('should return a well-formed list of SPO keys with the default limit', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const response = await httpClient.getStakePoolOperators();

      expect(response).toBeSuccess();
      const operators = expectValidList(
        response.data!.stakePoolOperators,
        SpoHex,
        'stakePoolOperators',
      );
      expect(operators.length).toBeLessThanOrEqual(STAKE_POOL_OPERATORS_DEFAULT_LIMIT);
    });

    /**
     * @given an explicit in-range limit
     * @when stakePoolOperators is queried with it
     * @then the request succeeds, honours the limit and every item is a hex SPO key
     */
    test('should honour an explicit limit', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const response = await httpClient.getStakePoolOperators(EXPLICIT_LIMIT);

      expect(response).toBeSuccess();
      const operators = expectValidList(
        response.data!.stakePoolOperators,
        SpoHex,
        `stakePoolOperators(${EXPLICIT_LIMIT})`,
      );
      expect(operators.length).toBeLessThanOrEqual(EXPLICIT_LIMIT);
    });

    /**
     * @given out-of-range limits
     * @when stakePoolOperators is queried with each
     * @then every request succeeds and returns no more rows than the clamped limit
     */
    test('should clamp out-of-range limits', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      await expectLimitClamped(
        'stakePoolOperators',
        STAKE_POOL_OPERATORS_MAX_LIMIT,
        (limit) => httpClient.getStakePoolOperators(limit),
        (response) => response.data!.stakePoolOperators,
      );
    });
  });

  describe('registration surface', () => {
    /**
     * @given a negative offset
     * @when each paginated list endpoint is queried with it
     * @then the request succeeds and returns the same page as offset 0 (the
     *       resolvers floor the offset rather than rejecting it)
     */
    test('should floor a negative offset to zero on every list endpoint', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const endpoints: {
        field: string;
        query: (offset: number) => Promise<GraphQLResponse<Record<string, unknown>>>;
      }[] = [
        { field: 'spoList', query: (offset) => httpClient.getSpoList({ offset }) },
        { field: 'spoIdentities', query: (offset) => httpClient.getSpoIdentities({ offset }) },
        {
          field: 'poolMetadataList',
          query: (offset) => httpClient.getPoolMetadataList({ offset }),
        },
        {
          field: 'spoPerformanceLatest',
          query: (offset) => httpClient.getSpoPerformanceLatest({ offset }),
        },
        {
          field: 'stakeDistribution',
          query: (offset) => httpClient.getStakeDistribution({ offset }),
        },
      ];

      for (const { field, query } of endpoints.filter((e) => queryFields.has(e.field))) {
        const [floored, negative] = await Promise.all([query(0), query(NEGATIVE_OFFSET)]);
        expect.soft(floored, `${field}(offset: 0)`).toBeSuccess();
        expect.soft(negative, `${field}(offset: ${NEGATIVE_OFFSET})`).toBeSuccess();
        expect
          .soft(negative.data?.[field], `${field}(offset: ${NEGATIVE_OFFSET})`)
          .toEqual(floored.data?.[field]);
      }
    });

    /**
     * @given a permissioned environment with no SPO registrations
     * @when every registration-backed list and count is queried
     * @then each returns a successful empty result
     *
     * This is the single data-reality assertion for the registration surface.
     * It gates on spoCount: once post-mainnet registration tooling produces
     * real SPOs it skips with the count in its reason instead of failing, so a
     * legitimate data change never reads as an indexer regression. That skip
     * is the cue to replace it with non-empty assertions (the per-endpoint
     * shape tests already validate populated data). spoCount counts the stake
     * snapshot while spoIdentities reads the identity table, so split the
     * assertion per source when the two start to diverge.
     */
    test('should report no SPO registrations on permissioned environments', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const count = await httpClient.getSpoCount();
      expect(count).toBeSuccess();
      const registered = count.data!.spoCount ?? 0;
      if (registered > 0) {
        return skipWithReason(
          ctx,
          `${registered} SPO registrations on ${env.getCurrentEnvironmentName()} — replace this emptiness check with populated assertions`,
        );
      }

      const [list, identities, operators] = await Promise.all([
        httpClient.getSpoList(),
        httpClient.getSpoIdentities(),
        httpClient.getStakePoolOperators(),
      ]);

      expect(list).toBeSuccess();
      expect(list.data!.spoList).toEqual([]);
      expect(identities).toBeSuccess();
      expect(identities.data!.spoIdentities).toEqual([]);
      expect(operators).toBeSuccess();
      expect(operators.data!.stakePoolOperators).toEqual([]);

      if (queryFields.has('poolMetadataList')) {
        const metadata = await httpClient.getPoolMetadataList();
        expect(metadata).toBeSuccess();
        expect(metadata.data!.poolMetadataList).toEqual([]);
      }
      if (queryFields.has('spoPerformanceLatest')) {
        const performance = await httpClient.getSpoPerformanceLatest();
        expect(performance).toBeSuccess();
        expect(performance.data!.spoPerformanceLatest).toEqual([]);
      }
      if (queryFields.has('registeredFirstValidEpochs')) {
        const firstValid = await httpClient.getRegisteredFirstValidEpochs();
        expect(firstValid).toBeSuccess();
        expect(firstValid.data!.registeredFirstValidEpochs).toEqual([]);
      }
      if (queryFields.has('stakeDistribution')) {
        const stake = await httpClient.getStakeDistribution();
        expect(stake).toBeSuccess();
        expect(stake.data!.stakeDistribution).toEqual([]);
      }
      if (epochInfo) {
        const totals = await httpClient.getRegisteredTotalsSeries(
          Math.max(0, epochInfo.epochNo - RANGE_SPAN),
          epochInfo.epochNo,
        );
        expect(totals).toBeSuccess();
        expect(totals.data!.registeredTotalsSeries).toEqual([]);
      }
    });
  });

  describe('poolMetadataList', () => {
    /**
     * @given any environment
     * @when poolMetadataList is queried without arguments
     * @then a list bounded by the default limit is returned and every item
     *       matches the PoolMetadata schema
     */
    test('should return a well-formed list with the default limit', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'poolMetadataList')) return;

      const response = await httpClient.getPoolMetadataList();

      expect(response).toBeSuccess();
      const metadata = expectValidList(
        response.data!.poolMetadataList,
        PoolMetadataSchema,
        'poolMetadataList',
      );
      expect(metadata.length).toBeLessThanOrEqual(POOL_METADATA_LIST_DEFAULT_LIMIT);
    });

    /**
     * @given the withNameOnly filter
     * @when poolMetadataList is queried with it
     * @then every item matches the PoolMetadata schema and carries a name or ticker
     */
    test('should return only named pools with withNameOnly', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'poolMetadataList')) return;

      const response = await httpClient.getPoolMetadataList({ withNameOnly: true });

      expect(response).toBeSuccess();
      const named = expectValidList(
        response.data!.poolMetadataList,
        PoolMetadataSchema,
        'poolMetadataList(withNameOnly)',
      );
      for (const item of named) {
        expect(item.name ?? item.ticker, `${item.poolIdHex} should carry a name`).not.toBeNull();
      }
    });

    /**
     * @given out-of-range limits
     * @when poolMetadataList is queried with each
     * @then every request succeeds and returns no more rows than the clamped limit
     */
    test('should clamp out-of-range limits', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'poolMetadataList')) return;

      await expectLimitClamped(
        'poolMetadataList',
        WIDE_LIST_MAX_LIMIT,
        (limit) => httpClient.getPoolMetadataList({ limit }),
        (response) => response.data!.poolMetadataList,
      );
    });
  });

  describe('pool-id lookups', () => {
    /**
     * @given a well-formed pool id that is not registered
     * @when every pool-id lookup is queried with it
     * @then each resolves successfully to null
     */
    test('should return null for a fabricated well-formed pool id', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const spo = await httpClient.getSpoByPoolId(FABRICATED_POOL_ID);
      expect(spo).toBeSuccess();
      expect(spo.data!.spoByPoolId).toBeNull();

      const identity = await httpClient.getSpoIdentityByPoolId(FABRICATED_POOL_ID);
      expect(identity).toBeSuccess();
      expect(identity.data!.spoIdentityByPoolId).toBeNull();

      if (queryFields.has('poolMetadata')) {
        const metadata = await httpClient.getPoolMetadata(FABRICATED_POOL_ID);
        expect(metadata).toBeSuccess();
        expect(metadata.data!.poolMetadata).toBeNull();
      }
      if (queryFields.has('spoCompositeByPoolId')) {
        const composite = await httpClient.getSpoCompositeByPoolId(FABRICATED_POOL_ID);
        expect(composite).toBeSuccess();
        expect(composite.data!.spoCompositeByPoolId).toBeNull();
      }
    });

    /**
     * @given malformed pool ids (empty, non-hex, odd length, wrong length)
     * @when every pool-id lookup is queried with each
     * @then the indexer resolves to null rather than raising an error
     *
     * Potential server-side finding: the pool-id normaliser only lowercases the
     * input and performs no hex validation, so garbage silently yields null.
     * If validation is added later, switch these assertions to toBeError().
     */
    test('should return null rather than an error for malformed pool ids', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const malformedIds = [...MALFORMED_POOL_IDS, ...dataProvider.getFabricatedMalformedHashes()];
      for (const poolId of malformedIds) {
        const label = JSON.stringify(poolId);

        const spo = await httpClient.getSpoByPoolId(poolId);
        expect.soft(spo, `spoByPoolId(${label})`).toBeSuccess();
        expect.soft(spo.data?.spoByPoolId, `spoByPoolId(${label})`).toBeNull();

        const identity = await httpClient.getSpoIdentityByPoolId(poolId);
        expect.soft(identity, `spoIdentityByPoolId(${label})`).toBeSuccess();
        expect.soft(identity.data?.spoIdentityByPoolId, `spoIdentityByPoolId(${label})`).toBeNull();

        if (queryFields.has('poolMetadata')) {
          const metadata = await httpClient.getPoolMetadata(poolId);
          expect.soft(metadata, `poolMetadata(${label})`).toBeSuccess();
          expect.soft(metadata.data?.poolMetadata, `poolMetadata(${label})`).toBeNull();
        }
        if (queryFields.has('spoCompositeByPoolId')) {
          const composite = await httpClient.getSpoCompositeByPoolId(poolId);
          expect.soft(composite, `spoCompositeByPoolId(${label})`).toBeSuccess();
          expect
            .soft(composite.data?.spoCompositeByPoolId, `spoCompositeByPoolId(${label})`)
            .toBeNull();
        }
      }
    });

    /**
     * @given the 0x-prefixed and upper-case spellings of a pool id
     * @when every pool-id lookup is queried with each spelling
     * @then each spelling resolves to the same result as the canonical
     *       lowercase form (null today, the registered SPO once data exists)
     */
    test('should normalise 0x-prefixed and upper-case pool ids across every lookup', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const lookups: {
        field: string;
        query: (poolId: string) => Promise<GraphQLResponse<Record<string, unknown>>>;
      }[] = [
        { field: 'spoByPoolId', query: (id) => httpClient.getSpoByPoolId(id) },
        { field: 'spoIdentityByPoolId', query: (id) => httpClient.getSpoIdentityByPoolId(id) },
        { field: 'poolMetadata', query: (id) => httpClient.getPoolMetadata(id) },
        { field: 'spoCompositeByPoolId', query: (id) => httpClient.getSpoCompositeByPoolId(id) },
      ];

      for (const { field, query } of lookups.filter((l) => queryFields.has(l.field))) {
        const canonical = await query(FABRICATED_POOL_ID);
        expect(canonical, `${field}(${FABRICATED_POOL_ID})`).toBeSuccess();

        for (const variant of hexCaseAndPrefixVariants(FABRICATED_POOL_ID)) {
          const response = await query(variant);
          expect.soft(response, `${field}(${variant})`).toBeSuccess();
          expect
            .soft(response.data?.[field], `${field}(${variant})`)
            .toEqual(canonical.data?.[field]);
        }
      }
    });
  });

  describe('spoPerformanceLatest', () => {
    /**
     * @given any environment
     * @when spoPerformanceLatest is queried without arguments
     * @then a list bounded by the default limit is returned, every item matches
     *       the EpochPerf schema and items are ordered newest epoch first
     */
    test('should return a well-formed list ordered newest first', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Performance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'spoPerformanceLatest')) return;

      const response = await httpClient.getSpoPerformanceLatest();

      expect(response).toBeSuccess();
      const perfs = expectValidList(
        response.data!.spoPerformanceLatest,
        EpochPerfSchema,
        'spoPerformanceLatest',
      );
      expect(perfs.length).toBeLessThanOrEqual(SPO_PERFORMANCE_LATEST_DEFAULT_LIMIT);
      expectOrdered(
        perfs,
        (previous, current) => current.epochNo <= previous.epochNo,
        'spoPerformanceLatest',
      );
    });

    /**
     * @given out-of-range limits
     * @when spoPerformanceLatest is queried with each
     * @then every request succeeds and returns no more rows than the clamped limit
     */
    test('should clamp out-of-range limits', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Performance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'spoPerformanceLatest')) return;

      await expectLimitClamped(
        'spoPerformanceLatest',
        WIDE_LIST_MAX_LIMIT,
        (limit) => httpClient.getSpoPerformanceLatest({ limit }),
        (response) => response.data!.spoPerformanceLatest,
      );
    });
  });

  describe('spoPerformanceBySpoSk', () => {
    /**
     * @given a well-formed SPO key that is not registered, in every hex spelling
     * @when spoPerformanceBySpoSk is queried with each
     * @then every request succeeds with an empty list
     */
    test('should return an empty list for a fabricated key in every hex spelling', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Performance', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'spoPerformanceBySpoSk')) return;

      for (const key of [FABRICATED_SPO_SK, ...hexCaseAndPrefixVariants(FABRICATED_SPO_SK)]) {
        const response = await httpClient.getSpoPerformanceBySpoSk(key);
        expect.soft(response, `spoPerformanceBySpoSk(${key})`).toBeSuccess();
        expect
          .soft(response.data?.spoPerformanceBySpoSk, `spoPerformanceBySpoSk(${key})`)
          .toEqual([]);
      }
    });

    /**
     * @given a well-formed SPO key that is not registered
     * @when spoPerformanceBySpoSk is queried with default and out-of-range limits
     * @then every request succeeds and returns no more rows than the clamped limit
     */
    test('should honour the default limit and clamp out-of-range limits', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Performance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'spoPerformanceBySpoSk')) return;

      const defaults = await httpClient.getSpoPerformanceBySpoSk(FABRICATED_SPO_SK);
      expect(defaults).toBeSuccess();
      expect(defaults.data!.spoPerformanceBySpoSk.length).toBeLessThanOrEqual(
        SPO_PERFORMANCE_BY_SK_DEFAULT_LIMIT,
      );

      await expectLimitClamped(
        'spoPerformanceBySpoSk',
        WIDE_LIST_MAX_LIMIT,
        (limit) => httpClient.getSpoPerformanceBySpoSk(FABRICATED_SPO_SK, { limit }),
        (response) => response.data!.spoPerformanceBySpoSk,
      );
    });
  });

  describe('epochPerformance', () => {
    /**
     * @given the current, a negative and a far-future epoch
     * @when epochPerformance is queried for each
     * @then every request succeeds, every item echoes the requested epoch and
     *       matches the EpochPerf schema, and the default limit is honoured
     */
    test('should return a well-formed list for current, negative and far-future epochs', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Performance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'epochPerformance')) return;

      const epochs = [...(epochInfo ? [epochInfo.epochNo] : []), -1, MAX_GRAPHQL_INT];
      for (const epoch of epochs) {
        const response = await httpClient.getEpochPerformance(epoch);
        expect(response, `epochPerformance(${epoch})`).toBeSuccess();
        const perfs = expectValidList(
          response.data!.epochPerformance,
          EpochPerfSchema,
          `epochPerformance(${epoch})`,
        );
        expect(perfs.length).toBeLessThanOrEqual(EPOCH_PERFORMANCE_DEFAULT_LIMIT);
        for (const perf of perfs) {
          expect(perf.epochNo).toBe(epoch);
        }
      }
    });

    /**
     * @given out-of-range limits
     * @when epochPerformance is queried with each for the current epoch (or a
     *       negative one when no epoch data exists)
     * @then every request succeeds and returns no more rows than the clamped limit
     */
    test('should clamp out-of-range limits', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Performance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'epochPerformance')) return;

      const anchorEpoch = epochInfo?.epochNo ?? -1;
      await expectLimitClamped(
        `epochPerformance(${anchorEpoch})`,
        WIDE_LIST_MAX_LIMIT,
        (limit) => httpClient.getEpochPerformance(anchorEpoch, { limit }),
        (response) => response.data!.epochPerformance,
      );
    });
  });

  describe('epochUtilization', () => {
    /**
     * @given the current, zero, a negative and a far-future epoch
     * @when epochUtilization is queried for each
     * @then a finite non-negative number is returned, never null (the storage
     *       query coalesces a no-data epoch to 0)
     */
    test('should return a non-negative number, never null, for any epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Performance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'epochUtilization')) return;

      const epochs = [...(epochInfo ? [epochInfo.epochNo] : []), 0, -1, MAX_GRAPHQL_INT];
      for (const epoch of epochs) {
        const response = await httpClient.getEpochUtilization(epoch);
        expect(response, `epochUtilization(${epoch})`).toBeSuccess();
        const utilization = response.data!.epochUtilization;
        expect(utilization, `epochUtilization(${epoch}) should not be null`).not.toBeNull();
        expect(Number.isFinite(utilization)).toBe(true);
        expect(utilization!).toBeGreaterThanOrEqual(0);
      }
    });

    /**
     * @given an epoch just above the 32-bit GraphQL Int range
     * @when epochUtilization is queried
     * @then the indexer rejects it with a GraphQL error and no utilization
     *
     * epochUtilization binds an i32 (query.rs), unlike committee and
     * epochPerformance which bind i64 and accept the same value; see the
     * committee suite for the accepting side of that inconsistency.
     */
    test('should reject an epoch outside the 32-bit Int range', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Performance', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'epochUtilization')) return;

      const response = await httpClient.getEpochUtilization(MAX_GRAPHQL_INT + 1);

      // Depending on where coercion fails the body carries either no data at all
      // or a null field next to the error, so assert on both loosely.
      expect.soft(response.errors ?? [], 'expected a GraphQL error').not.toHaveLength(0);
      expect.soft(response.data?.epochUtilization ?? null).toBeNull();
    });
  });

  describe('registeredTotalsSeries', () => {
    /**
     * @given an epoch range wider than the server-side span cap
     * @when registeredTotalsSeries is queried (the beforeAll probe request)
     * @then the indexer rejects it with an "epoch range too large" client error
     *
     * Asserts on the probe response itself rather than repeating the request:
     * that halves the over-span calls and rules out the probe and the test
     * disagreeing when one of them hits a transient error.
     */
    test('should reject an epoch span larger than the maximum', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!spanGuardPresent) return skipWithReason(ctx, spanGuardReason());

      expectSpanGuardError(spanProbe!, `registeredTotalsSeries(0, ${EPOCH_SPAN_LIMIT + 1})`);
    });

    /**
     * @given an epoch range exactly at the server-side span cap
     * @when registeredTotalsSeries is queried
     * @then the request succeeds (the bound is inclusive) and every item matches
     *       the RegisteredTotals schema
     */
    test('should accept the maximum allowed epoch span', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const response = await httpClient.getRegisteredTotalsSeries(0, EPOCH_SPAN_LIMIT);

      expect(response).toBeSuccess();
      expectValidList(
        response.data!.registeredTotalsSeries,
        RegisteredTotalsSchema,
        'registeredTotalsSeries',
      );
    });

    /**
     * @given a range of recent epochs ending at the current one
     * @when registeredTotalsSeries is queried
     * @then every row lies within the range, rows ascend by epoch, the running
     *       total never decreases and newly registered never exceeds it
     */
    test('should be well-formed over a real range', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!epochInfo) return skipWithReason(ctx, noEpochReason());

      const to = epochInfo.epochNo;
      const from = Math.max(0, to - RANGE_SPAN);
      const response = await httpClient.getRegisteredTotalsSeries(from, to);

      expect(response).toBeSuccess();
      const totals = expectValidList(
        response.data!.registeredTotalsSeries,
        RegisteredTotalsSchema,
        'registeredTotalsSeries',
      );
      for (const row of totals) {
        expect(row.epochNo).toBeGreaterThanOrEqual(from);
        expect(row.epochNo).toBeLessThanOrEqual(to);
        expect(row.newlyRegistered).toBeLessThanOrEqual(row.totalRegistered);
      }
      expectOrdered(
        totals,
        (previous, current) =>
          current.epochNo > previous.epochNo && current.totalRegistered >= previous.totalRegistered,
        'registeredTotalsSeries',
      );
    });
  });

  describe('registeredSpoSeries', () => {
    /**
     * @given an epoch range exactly at the server-side span cap
     * @when registeredSpoSeries is queried
     * @then the request succeeds with exactly one row per epoch in the range
     *       (the series is generated for every epoch, with or without data)
     */
    test('should accept the maximum span and return one row per epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'registeredSpoSeries')) return;

      const response = await httpClient.getRegisteredSpoSeries(0, EPOCH_SPAN_LIMIT);

      expect(response).toBeSuccess();
      const stats = expectValidList(
        response.data!.registeredSpoSeries,
        RegisteredStatSchema,
        'registeredSpoSeries',
      );
      expect(stats).toHaveLength(EPOCH_SPAN_LIMIT + 1);
      expect(stats[0].epochNo).toBe(0);
      expect(stats[stats.length - 1].epochNo).toBe(EPOCH_SPAN_LIMIT);
    });
  });

  describe('range endpoints', () => {
    /**
     * @given an epoch range wider than the server-side span cap
     * @when registeredSpoSeries and registeredPresence are queried
     * @then both reject it with the same "epoch range too large" client error
     */
    test('should reject an over-wide span on registeredSpoSeries and registeredPresence', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!spanGuardPresent) return skipWithReason(ctx, spanGuardReason());

      const overSpan = EPOCH_SPAN_LIMIT + 1;
      if (queryFields.has('registeredSpoSeries')) {
        const stats = await httpClient.getRegisteredSpoSeries(0, overSpan);
        expectSpanGuardError(stats, `registeredSpoSeries(0, ${overSpan})`);
      }
      if (queryFields.has('registeredPresence')) {
        const presence = await httpClient.getRegisteredPresence(0, overSpan);
        expectSpanGuardError(presence, `registeredPresence(0, ${overSpan})`);
      }
    });

    /**
     * @given a short epoch range passed with its bounds reversed
     * @when each range endpoint is queried both ways round
     * @then both orderings succeed and return the same rows (the span check and
     *       the storage queries are orientation independent)
     */
    test('should accept reversed bounds', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!epochInfo) return skipWithReason(ctx, noEpochReason());

      const hi = epochInfo.epochNo;
      const lo = Math.max(0, hi - REVERSED_RANGE_SPAN);
      const endpoints: {
        field: string;
        query: (from: number, to: number) => Promise<GraphQLResponse<Record<string, unknown>>>;
      }[] = [
        {
          field: 'registeredTotalsSeries',
          query: (from, to) => httpClient.getRegisteredTotalsSeries(from, to),
        },
        {
          field: 'registeredSpoSeries',
          query: (from, to) => httpClient.getRegisteredSpoSeries(from, to),
        },
        {
          field: 'registeredPresence',
          query: (from, to) => httpClient.getRegisteredPresence(from, to),
        },
      ];

      for (const { field, query } of endpoints.filter((e) => queryFields.has(e.field))) {
        const [forward, reversed] = await Promise.all([query(lo, hi), query(hi, lo)]);
        expect.soft(forward, `${field}(${lo}, ${hi})`).toBeSuccess();
        expect.soft(reversed, `${field}(${hi}, ${lo})`).toBeSuccess();
        expect.soft(reversed.data?.[field], `${field} reversed`).toEqual(forward.data?.[field]);
      }
    });

    /**
     * @given a single-epoch range and an all-negative range
     * @when each range endpoint is queried with them
     * @then every request succeeds; registeredSpoSeries returns exactly one row
     *       per epoch (zero counts for epochs without data) and the others
     *       return at most one row per epoch
     */
    test('should accept from == to and negative epochs', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const single = epochInfo?.epochNo ?? FALLBACK_EPOCH;
      const ranges: [number, number][] = [
        [single, single],
        [-5, -1],
      ];

      for (const [from, to] of ranges) {
        const width = to - from + 1;

        const totals = await httpClient.getRegisteredTotalsSeries(from, to);
        expect(totals, `registeredTotalsSeries(${from}, ${to})`).toBeSuccess();
        expect(totals.data!.registeredTotalsSeries.length).toBeLessThanOrEqual(width);

        if (queryFields.has('registeredSpoSeries')) {
          const stats = await httpClient.getRegisteredSpoSeries(from, to);
          expect(stats, `registeredSpoSeries(${from}, ${to})`).toBeSuccess();
          const rows = expectValidList(
            stats.data!.registeredSpoSeries,
            RegisteredStatSchema,
            `registeredSpoSeries(${from}, ${to})`,
          );
          expect(rows.map((row) => row.epochNo)).toEqual(epochRange(from, to));
        }
        if (queryFields.has('registeredPresence')) {
          const presence = await httpClient.getRegisteredPresence(from, to);
          expect(presence, `registeredPresence(${from}, ${to})`).toBeSuccess();
          const events = expectValidList(
            presence.data!.registeredPresence,
            PresenceEventSchema,
            `registeredPresence(${from}, ${to})`,
          );
          for (const event of events) {
            expect(event.epochNo).toBeGreaterThanOrEqual(from);
            expect(event.epochNo).toBeLessThanOrEqual(to);
          }
        }
      }
    });
  });

  describe('registeredFirstValidEpochs', () => {
    /**
     * @given no uptoEpoch, the current epoch and a negative epoch
     * @when registeredFirstValidEpochs is queried with each
     * @then every request succeeds, every item matches the FirstValidEpoch
     *       schema and no item lies above the requested bound
     */
    test('should return a well-formed list with and without uptoEpoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'registeredFirstValidEpochs')) return;

      const bounds = [undefined, -1, ...(epochInfo ? [epochInfo.epochNo] : [])];
      for (const uptoEpoch of bounds) {
        const response = await httpClient.getRegisteredFirstValidEpochs(uptoEpoch);
        expect(response, `registeredFirstValidEpochs(${uptoEpoch})`).toBeSuccess();
        const items = expectValidList(
          response.data!.registeredFirstValidEpochs,
          FirstValidEpochSchema,
          `registeredFirstValidEpochs(${uptoEpoch})`,
        );
        if (uptoEpoch !== undefined) {
          for (const item of items) {
            expect(item.firstValidEpoch).toBeLessThanOrEqual(uptoEpoch);
          }
        }
      }
    });
  });

  describe('stakeDistribution', () => {
    /**
     * @given any environment
     * @when stakeDistribution is queried without arguments
     * @then a list bounded by the default limit is returned, every item matches
     *       the StakeShare schema and rows are ordered by live stake descending
     */
    test('should return a well-formed list ordered by live stake descending', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'stakeDistribution')) return;

      const response = await httpClient.getStakeDistribution();

      expect(response).toBeSuccess();
      const shares = expectValidList(
        response.data!.stakeDistribution,
        StakeShareSchema,
        'stakeDistribution',
      );
      expect(shares.length).toBeLessThanOrEqual(STAKE_DISTRIBUTION_DEFAULT_LIMIT);
      // Rows with an unknown live stake are not comparable, so only both-known pairs are checked.
      expectOrdered(
        shares,
        (previous, current) =>
          previous.liveStake === null ||
          current.liveStake === null ||
          BigInt(current.liveStake) <= BigInt(previous.liveStake),
        'stakeDistribution',
      );
    });

    /**
     * @given a whitespace-only search string
     * @when stakeDistribution is queried with it
     * @then the result equals the unfiltered list (the resolver trims the
     *       search and treats an empty one as absent)
     */
    test('should treat a whitespace-only search as unfiltered', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'stakeDistribution')) return;

      const [unfiltered, blankSearch] = await Promise.all([
        httpClient.getStakeDistribution(),
        httpClient.getStakeDistribution({ search: '   ' }),
      ]);

      expect(unfiltered).toBeSuccess();
      expect(blankSearch).toBeSuccess();
      expect(blankSearch.data!.stakeDistribution).toEqual(unfiltered.data!.stakeDistribution);
    });

    /**
     * @given a search term and ascending ordering
     * @when stakeDistribution is queried with each
     * @then both requests succeed and every item matches the StakeShare schema
     */
    test('should accept search and ascending ordering arguments', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'stakeDistribution')) return;

      const [searched, ascending] = await Promise.all([
        httpClient.getStakeDistribution({ search: SEARCH_TERM }),
        httpClient.getStakeDistribution({ orderByStakeDesc: false }),
      ]);

      expect(searched).toBeSuccess();
      expectValidList(
        searched.data!.stakeDistribution,
        StakeShareSchema,
        'stakeDistribution(search)',
      );
      expect(ascending).toBeSuccess();
      expectValidList(
        ascending.data!.stakeDistribution,
        StakeShareSchema,
        'stakeDistribution(orderByStakeDesc: false)',
      );
    });

    /**
     * @given out-of-range limits
     * @when stakeDistribution is queried with each
     * @then every request succeeds and returns no more rows than the clamped limit
     */
    test('should clamp out-of-range limits', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'stakeDistribution')) return;

      await expectLimitClamped(
        'stakeDistribution',
        WIDE_LIST_MAX_LIMIT,
        (limit) => httpClient.getStakeDistribution({ limit }),
        (response) => response.data!.stakeDistribution,
      );
    });
  });
});
