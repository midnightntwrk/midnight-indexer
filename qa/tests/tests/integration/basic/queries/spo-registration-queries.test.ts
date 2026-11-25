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
// beforeAll fails the suite, never skips it, when the schema lacks an SPO
// Query field or when an environment that runs spo-indexer
// (SPO_DATA_ENVIRONMENTS) serves no epoch data: both come from the system
// under test. Elsewhere the few tests anchored at the current epoch skip with
// a reason and everything else runs, the undeployed stack included. The
// epoch-span cap (#1455) is covered by spo-epoch-series-queries.test.ts.
//
// Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/1003

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
import {
  CLAMP_PROBE_LIMITS,
  EPOCH_PERFORMANCE_DEFAULT_LIMIT,
  FABRICATED_POOL_ID,
  FABRICATED_SPO_SK,
  MALFORMED_POOL_IDS,
  MAX_EPOCH_SPAN,
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
  type OrderTie,
  assertNoGraphqlErrors,
  assertSpoQueryFieldsServed,
  clampedLimit,
  epochRange,
  expectOrdered,
  expectSamePage,
  expectValidList,
  hexLowercasePrefixVariants,
  hexNormalisedVariants,
  reportMissingSpoData,
  sampleRows,
  skipWithReason,
} from '@utils/indexer/spo-test-support';
import dataProvider from '@utils/testdata-provider';

const httpClient = new IndexerHttpClient();

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
// Rows schema-checked from a maximum-span series (up to 10,001 rows): first,
// last and evenly spaced ones between, instead of parsing every row.
const SERIES_SAMPLE_SIZE = 12;

let epochInfo: EpochInfo | null = null;

function noEpochReason(): string {
  return `no spo-indexer epochs on ${env.getCurrentEnvironmentName()} to anchor a range (currentEpochInfo is null)`;
}

/**
 * Probes a list endpoint with out-of-range limits and asserts each request
 * succeeds and returns exactly the row count the server-side clamp implies:
 * zero clamps up to one row, an over-large value down to `maxLimit`.
 *
 * The exact count needs to know how many rows the endpoint can serve, so a
 * reference page at `maxLimit` is fetched alongside the probes and each
 * probe's expectation is `min(clamp, available)`. An upper bound alone would
 * be unfalsifiable: zero rows satisfy every bound, so a server that rejected
 * `limit: 0` with an empty list, or ignored the clamp entirely on an empty
 * surface, would pass. On an empty surface the expectation is zero for every
 * probe, which still proves the limits are accepted rather than rejected; the
 * clamp itself is exercised wherever the endpoint has data.
 */
async function expectLimitClamped<R extends GraphQLResponse<unknown>>(
  label: string,
  maxLimit: number,
  query: (limit: number) => Promise<R>,
  items: (response: R) => unknown[],
): Promise<void> {
  const [reference, ...probes] = await Promise.all(
    [maxLimit, ...CLAMP_PROBE_LIMITS].map((limit) => query(limit)),
  );
  expect(reference, `${label}(limit: ${maxLimit}) should succeed`).toBeSuccess();
  const available = items(reference).length;
  expect(
    available,
    `${label}(limit: ${maxLimit}) should not exceed its maximum`,
  ).toBeLessThanOrEqual(maxLimit);

  CLAMP_PROBE_LIMITS.forEach((limit, index) => {
    const response = probes[index];
    const bound = clampedLimit(limit, maxLimit);
    const expected = Math.min(bound, available);
    expect(response, `${label}(limit: ${limit}) should succeed`).toBeSuccess();
    expect(
      items(response).length,
      `${label}(limit: ${limit}) should clamp to ${bound} and return ${expected} of ${available} available rows`,
    ).toBe(expected);
  });
}

// A root Query field that returns a list, paired with the request that reads it.
type ListProbe = {
  field: string;
  query: () => Promise<GraphQLResponse<Record<string, unknown>>>;
};

// spoList's ORDER BY (COALESCE(si.mainchain_pubkey, s.pool_id) over LEFT JOINs)
// has no unique tiebreaker, so full pages of it are compared through this tie
// descriptor. poolIdHex stands in for the real key; the negative-offset test
// explains why that is sound.
const spoListTie = <T extends { poolIdHex?: unknown }>(): OrderTie<T> => ({
  limit: SPO_LIST_DEFAULT_LIMIT,
  orderKey: (row) => String(row.poolIdHex),
});

// Every pool-id lookup on the surface.
const POOL_ID_LOOKUPS: {
  field: string;
  query: (poolId: string) => Promise<GraphQLResponse<Record<string, unknown>>>;
}[] = [
  { field: 'spoByPoolId', query: (id) => httpClient.getSpoByPoolId(id) },
  { field: 'spoIdentityByPoolId', query: (id) => httpClient.getSpoIdentityByPoolId(id) },
  { field: 'poolMetadata', query: (id) => httpClient.getPoolMetadata(id) },
  { field: 'spoCompositeByPoolId', query: (id) => httpClient.getSpoCompositeByPoolId(id) },
];

/** Skip reason for an emptiness check whose source table has started to fill. */
function populatedReason(rows: number, source: string): string {
  return `${rows} ${source} on ${env.getCurrentEnvironmentName()} — replace this emptiness check with populated assertions`;
}

/**
 * Gates an emptiness check on the list an endpoint itself returns. A populated
 * list skips the test with the row count in its reason; an empty one passes
 * the "succeeds with an empty list" assertion. Returns whether the caller
 * should go on to check the endpoints that read the same table.
 */
function gateOnEmpty(
  ctx: TestContext,
  field: string,
  response: GraphQLResponse<Record<string, unknown>>,
): boolean {
  expect(response, field).toBeSuccess();
  const rows = response.data![field];
  expect(Array.isArray(rows), `${field} should be a list`).toBe(true);
  if ((rows as unknown[]).length > 0) {
    skipWithReason(ctx, populatedReason((rows as unknown[]).length, `${field} rows`));
    return false;
  }
  return true;
}

/**
 * Asserts every probe succeeds with an empty list. The probes go out in one
 * round trip, and the expectations are soft so a populated table reports all
 * of its endpoints, not just the first one.
 */
async function expectEmptyLists(probes: ListProbe[]): Promise<void> {
  const responses = await Promise.all(probes.map(({ query }) => query()));
  probes.forEach(({ field }, index) => {
    expect.soft(responses[index], field).toBeSuccess();
    expect.soft(responses[index].data?.[field], field).toEqual([]);
  });
}

describe('SPO registration queries', () => {
  beforeAll(async () => {
    // Throws on an unreachable indexer, a broken introspection or a missing
    // SPO field, failing the suite instead of skipping it.
    await assertSpoQueryFieldsServed();

    const epochResponse = await httpClient.getCurrentEpochInfo();
    assertNoGraphqlErrors('currentEpochInfo', epochResponse);
    epochInfo = epochResponse.data!.currentEpochInfo;
    if (!epochInfo) reportMissingSpoData(noEpochReason());
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
     * @then the result holds the same rows as the unfiltered list (the
     *       resolver trims the search and treats an empty one as absent)
     */
    test('should treat a whitespace-only search as unfiltered', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };

      const [unfiltered, blankSearch] = await Promise.all([
        httpClient.getSpoList(),
        httpClient.getSpoList({ search: '   ' }),
      ]);

      expect(unfiltered).toBeSuccess();
      expect(blankSearch).toBeSuccess();
      // spoList has no unique tiebreaker, so a full page is also trimmed of its
      // trailing tie group before the comparison (see spoListTie).
      expectSamePage(
        ctx,
        { label: 'spoList(search: blank)', rows: blankSearch.data!.spoList },
        { label: 'spoList()', rows: unfiltered.data!.spoList },
        spoListTie(),
      );
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

      await expectLimitClamped(
        'stakePoolOperators',
        STAKE_POOL_OPERATORS_MAX_LIMIT,
        (limit) => httpClient.getStakePoolOperators(limit),
        (response) => response.data!.stakePoolOperators,
      );
    });
  });

  describe('registration surface', () => {
    type Row = Record<string, unknown>;
    // Every paginated list endpoint, one test each below so that a page that
    // cannot be compared skips that endpoint alone.
    const paginatedEndpoints: {
      field: string;
      query: (offset: number) => Promise<GraphQLResponse<Record<string, unknown>>>;
      tie?: OrderTie<Row>;
    }[] = [
      {
        field: 'spoList',
        query: (offset) => httpClient.getSpoList({ offset }),
        tie: spoListTie(),
      },
      { field: 'spoIdentities', query: (offset) => httpClient.getSpoIdentities({ offset }) },
      {
        field: 'poolMetadataList',
        query: (offset) => httpClient.getPoolMetadataList({ offset }),
      },
      {
        field: 'spoPerformanceLatest',
        query: (offset) => httpClient.getSpoPerformanceLatest({ offset }),
        tie: {
          limit: SPO_PERFORMANCE_LATEST_DEFAULT_LIMIT,
          orderKey: (row) => `${String(row.epochNo)}:${String(row.produced)}`,
        },
      },
      {
        field: 'stakeDistribution',
        query: (offset) => httpClient.getStakeDistribution({ offset }),
      },
    ];

    /**
     * @given a negative offset
     * @when a paginated list endpoint is queried with it
     * @then the request succeeds and returns the same rows as offset 0 (the
     *       resolvers floor the offset rather than rejecting it)
     *
     * The two pages are compared order-insensitively via expectSamePage. Two
     * endpoints need a tie descriptor because their ORDER BY has no unique
     * tiebreaker, so a full page may hold different tied rows between the two
     * calls (storage/spo.rs): spoPerformanceLatest orders by (epoch_no DESC,
     * produced_blocks DESC), and spoList by COALESCE(si.mainchain_pubkey,
     * s.pool_id) over a LEFT JOIN from the snapshot to spo_identity, whose
     * pool_id is not unique. The Spo type does not expose mainchain_pubkey, so
     * spoList's key is approximated by poolIdHex. That is sound: rows can tie on
     * the real key only when COALESCE falls through to pool_id, and such rows
     * share poolIdHex; when the key is a (unique) mainchain_pubkey the trailing
     * group is a single row and dropping every row with its poolIdHex removes
     * a superset. Either way every surviving row sorts strictly before the
     * boundary. A full page that is one tie group (every performance row of an
     * epoch with zero produced blocks, say) has nothing comparable on it and
     * skips with a reason. The other three order by a unique key (spoIdentities
     * on mainchain_pubkey, poolMetadataList on its primary key,
     * stakeDistribution on live stake then the unique pm.pool_id), so their
     * pages are deterministic.
     */
    for (const { field, query, tie } of paginatedEndpoints) {
      test(`should floor a negative offset to zero on ${field}`, async (ctx: TestContext) => {
        ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration', 'Negative'] };

        const [floored, negative] = await Promise.all([query(0), query(NEGATIVE_OFFSET)]);
        expect(floored, `${field}(offset: 0)`).toBeSuccess();
        expect(negative, `${field}(offset: ${NEGATIVE_OFFSET})`).toBeSuccess();

        const rows = (response: GraphQLResponse<Record<string, unknown>>) =>
          response.data![field] as Row[];
        expectSamePage(
          ctx,
          { label: `${field}(offset: ${NEGATIVE_OFFSET})`, rows: rows(negative) },
          { label: `${field}(offset: 0)`, rows: rows(floored) },
          tie,
        );
      });
    }
  });

  // Data-reality checks for a permissioned environment with no SPO
  // registrations: every registration-backed list is expected to be empty.
  //
  // The spo-indexer fills these tables from different sources on different
  // cadences (the registered-candidates RPC once per epoch, the Blockfrost
  // stake refresher on its own schedule), so one endpoint being empty says
  // nothing about another endpoint's table. Each test therefore gates on the
  // very table it asserts: once that table fills it skips with the row count
  // in its reason instead of failing, so a legitimate data change never reads
  // as an indexer regression. Such a skip is the cue to replace the check with
  // populated assertions (the per-endpoint shape tests already validate
  // populated data). Where several endpoints read one table the gate is one of
  // them and the assertions cover the rest, which is a real cross-endpoint
  // consistency check; a table with a single endpoint can only be checked for
  // "succeeds with an empty list".
  describe('registration data', () => {
    /**
     * @given a permissioned environment with an empty stake snapshot
     * @when spoCount reports zero
     * @then spoList, stakeDistribution and registeredTotalsSeries, which all
     *       derive from the same snapshot table, return empty lists
     */
    test('should report no stake-snapshot registrations on permissioned environments', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration', 'Negative'] };

      const count = await httpClient.getSpoCount();
      expect(count).toBeSuccess();
      const registered = count.data!.spoCount ?? 0;
      if (registered > 0) {
        return skipWithReason(ctx, populatedReason(registered, 'spoCount registrations'));
      }

      const snapshotProbes: ListProbe[] = [
        { field: 'spoList', query: () => httpClient.getSpoList() },
        { field: 'stakeDistribution', query: () => httpClient.getStakeDistribution() },
      ];
      const currentEpoch = epochInfo?.epochNo;
      await expectEmptyLists([
        ...snapshotProbes,
        ...(currentEpoch === undefined
          ? []
          : [
              {
                field: 'registeredTotalsSeries',
                query: () =>
                  httpClient.getRegisteredTotalsSeries(
                    Math.max(0, currentEpoch - RANGE_SPAN),
                    currentEpoch,
                  ),
              },
            ]),
      ]);
    });

    /**
     * @given a permissioned environment with no epoch performance rows
     * @when stakePoolOperators returns no keys
     * @then spoPerformanceLatest, which reads the same performance table,
     *       returns an empty list
     */
    test('should report no epoch performance on permissioned environments', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration', 'Negative'] };

      const operators = await httpClient.getStakePoolOperators();
      if (!gateOnEmpty(ctx, 'stakePoolOperators', operators)) return;

      await expectEmptyLists([
        { field: 'spoPerformanceLatest', query: () => httpClient.getSpoPerformanceLatest() },
      ]);
    });

    /**
     * @given a permissioned environment with no SPO identities
     * @when spoIdentities is queried
     * @then it succeeds with an empty list
     */
    test('should report no SPO identities on permissioned environments', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration', 'Negative'] };

      gateOnEmpty(ctx, 'spoIdentities', await httpClient.getSpoIdentities());
    });

    /**
     * @given a permissioned environment with no cached pool metadata
     * @when poolMetadataList is queried
     * @then it succeeds with an empty list
     */
    test('should report no pool metadata on permissioned environments', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration', 'Negative'] };

      gateOnEmpty(ctx, 'poolMetadataList', await httpClient.getPoolMetadataList());
    });

    /**
     * @given a permissioned environment with no registration history
     * @when registeredFirstValidEpochs is queried
     * @then it succeeds with an empty list
     */
    test('should report no registration history on permissioned environments', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration', 'Negative'] };

      gateOnEmpty(
        ctx,
        'registeredFirstValidEpochs',
        await httpClient.getRegisteredFirstValidEpochs(),
      );
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

      const responses = await Promise.all(
        POOL_ID_LOOKUPS.map(({ query }) => query(FABRICATED_POOL_ID)),
      );
      POOL_ID_LOOKUPS.forEach(({ field }, index) => {
        const label = `${field}(${FABRICATED_POOL_ID})`;
        expect.soft(responses[index], label).toBeSuccess();
        expect.soft(responses[index].data?.[field], label).toBeNull();
      });
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

      const malformedIds = [...MALFORMED_POOL_IDS, ...dataProvider.getFabricatedMalformedHashes()];
      // Every id x lookup goes out in a single round trip. A few dozen concurrent
      // lookups are fine for the indexer, whereas one round per id would stack
      // nine sequential 20-30s qanet queries against the 60s test budget.
      const results = await Promise.all(
        malformedIds.flatMap((poolId) =>
          POOL_ID_LOOKUPS.map(async ({ field, query }) => ({
            field,
            poolId,
            response: await query(poolId),
          })),
        ),
      );

      for (const { field, poolId, response } of results) {
        const label = `${field}(${JSON.stringify(poolId)})`;
        expect.soft(response, label).toBeSuccess();
        expect.soft(response.data?.[field], label).toBeNull();
      }
    });

    /**
     * @given the 0X-prefixed and upper-case spellings of a pool id, plus its
     *        lowercase 0x-prefixed spellings
     * @when every pool-id lookup is queried with each spelling
     * @then the 0X and upper-case spellings resolve to the same result as the
     *       canonical lowercase form (null today, the registered SPO once data
     *       exists), while the lowercase 0x spellings resolve to null
     *
     * The lowercase 0x branch asserts a known indexer bug rather than the
     * intended behaviour: `normalize_hex` leaves a lowercase 0x prefix in
     * place, so those lookups run against the literal "0x..." string and never
     * match a stored id (midnightntwrk/midnight-indexer#1712, see
     * hexLowercasePrefixVariants). Once #1712 is fixed and real SPO data exists
     * this branch goes red against a non-null canonical; move those spellings
     * into the toEqual branch then.
     */
    test('should normalise 0X-prefixed and upper-case pool ids across every lookup', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Negative'] };

      const normalised = hexNormalisedVariants(FABRICATED_POOL_ID);
      const lowercasePrefixed = hexLowercasePrefixVariants(FABRICATED_POOL_ID);
      const spellings = [FABRICATED_POOL_ID, ...normalised, ...lowercasePrefixed];
      // Every lookup x spelling goes out in a single round trip, for the same
      // reason as in the malformed-id test above.
      const results = await Promise.all(
        POOL_ID_LOOKUPS.map(async ({ field, query }) => ({
          field,
          responses: await Promise.all(spellings.map((poolId) => query(poolId))),
        })),
      );

      for (const { field, responses } of results) {
        const [canonical, ...spelled] = responses;
        expect(canonical, `${field}(${FABRICATED_POOL_ID})`).toBeSuccess();

        normalised.forEach((variant, index) => {
          const response = spelled[index];
          expect.soft(response, `${field}(${variant})`).toBeSuccess();
          expect
            .soft(response.data?.[field], `${field}(${variant})`)
            .toEqual(canonical.data?.[field]);
        });
        // Known bug #1712: the lowercase 0x prefix survives normalisation, so
        // these never match. Asserted explicitly so the test says what it means.
        lowercasePrefixed.forEach((variant, index) => {
          const response = spelled[normalised.length + index];
          expect.soft(response, `${field}(${variant})`).toBeSuccess();
          expect.soft(response.data?.[field], `${field}(${variant}) known bug #1712`).toBeNull();
        });
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

      // Both the normalised and the lowercase-0x spellings are expected empty
      // here: the fabricated key is unregistered, and the lowercase-0x ones
      // never match anyway (#1712, see hexLowercasePrefixVariants).
      const keys = [
        FABRICATED_SPO_SK,
        ...hexNormalisedVariants(FABRICATED_SPO_SK),
        ...hexLowercasePrefixVariants(FABRICATED_SPO_SK),
      ];
      // All spellings go out in one round trip rather than five sequential ones.
      const responses = await Promise.all(
        keys.map((key) => httpClient.getSpoPerformanceBySpoSk(key)),
      );
      keys.forEach((key, index) => {
        const label = `spoPerformanceBySpoSk(${key})`;
        expect.soft(responses[index], label).toBeSuccess();
        expect.soft(responses[index].data?.spoPerformanceBySpoSk, label).toEqual([]);
      });
    });

    /**
     * @given a well-formed SPO key that is not registered
     * @when spoPerformanceBySpoSk is queried with default and out-of-range limits
     * @then every request succeeds and returns no more rows than the clamped limit
     */
    test('should honour the default limit and clamp out-of-range limits', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Performance'] };

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

      const epochs = [...(epochInfo ? [epochInfo.epochNo] : []), -1, MAX_GRAPHQL_INT];
      const responses = await Promise.all(
        epochs.map((epoch) => httpClient.getEpochPerformance(epoch)),
      );
      epochs.forEach((epoch, index) => {
        const response = responses[index];
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
      });
    });

    /**
     * @given out-of-range limits
     * @when epochPerformance is queried with each for the current epoch (or a
     *       negative one when no epoch data exists)
     * @then every request succeeds and returns no more rows than the clamped limit
     */
    test('should clamp out-of-range limits', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Performance'] };

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

      const epochs = [...(epochInfo ? [epochInfo.epochNo] : []), 0, -1, MAX_GRAPHQL_INT];
      const responses = await Promise.all(
        epochs.map((epoch) => httpClient.getEpochUtilization(epoch)),
      );
      epochs.forEach((epoch, index) => {
        const response = responses[index];
        expect(response, `epochUtilization(${epoch})`).toBeSuccess();
        const utilization = response.data!.epochUtilization;
        expect(utilization, `epochUtilization(${epoch}) should not be null`).not.toBeNull();
        expect(Number.isFinite(utilization)).toBe(true);
        expect(utilization!).toBeGreaterThanOrEqual(0);
      });
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

      const response = await httpClient.getEpochUtilization(MAX_GRAPHQL_INT + 1);

      // Depending on where coercion fails the body carries either no data at all
      // or a null field next to the error, so assert on both loosely.
      expect.soft(response.errors ?? [], 'expected a GraphQL error').not.toHaveLength(0);
      expect.soft(response.data?.epochUtilization ?? null).toBeNull();
    });
  });

  describe('registeredTotalsSeries', () => {
    /**
     * @given a range of recent epochs ending at the current one
     * @when registeredTotalsSeries is queried
     * @then every row lies within the range, rows ascend by epoch, the running
     *       total never decreases and newly registered never exceeds it
     */
    test('should be well-formed over a real range', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
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
     * @then the request succeeds with exactly one row per epoch, in epoch order
     *       with no gaps (the series is generated for every epoch, with or
     *       without data), and a sample of rows matches the RegisteredStat schema
     *
     * This is always a 10,001-row response, so the schema check runs on a
     * bounded sample rather than parsing every row; the contiguity check is a
     * single pass of integer comparisons and covers the whole series. The
     * from == to test validates every row of a short series exhaustively.
     */
    test('should accept the maximum span and return one row per epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };

      const response = await httpClient.getRegisteredSpoSeries(0, MAX_EPOCH_SPAN);

      expect(response).toBeSuccess();
      const stats = response.data!.registeredSpoSeries;
      expect(stats).toHaveLength(MAX_EPOCH_SPAN + 1);
      const firstGap = stats.findIndex((row, epoch) => row.epochNo !== epoch);
      expect(firstGap, 'index of the first row whose epochNo is not its position').toBe(-1);
      expectValidList(
        sampleRows(stats, SERIES_SAMPLE_SIZE),
        RegisteredStatSchema,
        'registeredSpoSeries(sample)',
      );
    });
  });

  describe('range endpoints', () => {
    /**
     * @given a short epoch range passed with its bounds reversed
     * @when each range endpoint is queried both ways round
     * @then both orderings succeed and return the same rows (the span check and
     *       the storage queries are orientation independent)
     */
    test('should accept reversed bounds', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
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

      // Both orientations of every endpoint go out in one round trip.
      const pairs = await Promise.all(
        endpoints.map(({ query }) => Promise.all([query(lo, hi), query(hi, lo)])),
      );
      endpoints.forEach(({ field }, index) => {
        const [forward, reversed] = pairs[index];
        expect.soft(forward, `${field}(${lo}, ${hi})`).toBeSuccess();
        expect.soft(reversed, `${field}(${hi}, ${lo})`).toBeSuccess();
        expect.soft(reversed.data?.[field], `${field} reversed`).toEqual(forward.data?.[field]);
      });
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

      const single = epochInfo?.epochNo ?? FALLBACK_EPOCH;
      const ranges: [number, number][] = [
        [single, single],
        [-5, -1],
      ];

      // Every range on every endpoint goes out in one round trip.
      const results = await Promise.all(
        ranges.map(([from, to]) =>
          Promise.all([
            httpClient.getRegisteredTotalsSeries(from, to),
            httpClient.getRegisteredSpoSeries(from, to),
            httpClient.getRegisteredPresence(from, to),
          ]),
        ),
      );
      ranges.forEach(([from, to], index) => {
        const width = to - from + 1;
        const [totals, stats, presence] = results[index];

        expect(totals, `registeredTotalsSeries(${from}, ${to})`).toBeSuccess();
        expect(totals.data!.registeredTotalsSeries.length).toBeLessThanOrEqual(width);

        expect(stats, `registeredSpoSeries(${from}, ${to})`).toBeSuccess();
        const rows = expectValidList(
          stats.data!.registeredSpoSeries,
          RegisteredStatSchema,
          `registeredSpoSeries(${from}, ${to})`,
        );
        expect(rows.map((row) => row.epochNo)).toEqual(epochRange(from, to));

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
      });
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

      const bounds = [undefined, -1, ...(epochInfo ? [epochInfo.epochNo] : [])];
      const responses = await Promise.all(
        bounds.map((uptoEpoch) => httpClient.getRegisteredFirstValidEpochs(uptoEpoch)),
      );
      bounds.forEach((uptoEpoch, index) => {
        const response = responses[index];
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
      });
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
     * @then the result holds the same rows as the unfiltered list (the
     *       resolver trims the search and treats an empty one as absent)
     */
    test('should treat a whitespace-only search as unfiltered', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };

      const [unfiltered, blankSearch] = await Promise.all([
        httpClient.getStakeDistribution(),
        httpClient.getStakeDistribution({ search: '   ' }),
      ]);

      expect(unfiltered).toBeSuccess();
      expect(blankSearch).toBeSuccess();
      // stakeDistribution ends its ORDER BY on the joined primary key pool_id,
      // so the order is total and only the row order is normalised.
      expectSamePage(
        ctx,
        { label: 'stakeDistribution(search: blank)', rows: blankSearch.data!.stakeDistribution },
        { label: 'stakeDistribution()', rows: unfiltered.data!.stakeDistribution },
      );
    });

    /**
     * @given a search term and ascending ordering
     * @when stakeDistribution is queried with each
     * @then both requests succeed and every item matches the StakeShare schema
     */
    test('should accept search and ascending ordering arguments', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };

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

      await expectLimitClamped(
        'stakeDistribution',
        WIDE_LIST_MAX_LIMIT,
        (limit) => httpClient.getStakeDistribution({ limit }),
        (response) => response.data!.stakeDistribution,
      );
    });
  });
});
