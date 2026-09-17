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

// Integration tests for the SPO (stake pool operator) indexer GraphQL surface
// (#1003): dParameterHistory, currentEpochInfo, committee(epoch), spoCount,
// spoList, spoIdentities, stakePoolOperators, plus the pool-id lookups used for
// the non-existent-pool negative case and the epoch-span guard on
// registeredTotalsSeries.
//
// Data reality (2026-09):
//   - dParameterHistory is written by chain-indexer: the first indexed block
//     always records the D-parameter, so it is non-empty on every environment,
//     including a fresh undeployed one.
//   - currentEpochInfo and committee are written by spo-indexer, which runs on
//     all deployed environments (permissioned committees). currentEpochInfo is
//     extrapolated to the chain's real current epoch, but committee membership
//     trails it by a few epochs (spo-indexer backfills one epoch per poll), so
//     the committee tests scan downwards for the newest epoch that has data.
//     Where there is no spo-indexer data (undeployed, and qanet as of 2026-09)
//     currentEpochInfo is null and committee is [] for every epoch; those
//     cases skip with a reason.
//   - The registeredTotalsSeries epoch-span guard (#1455) shipped in 4.4.0-rc.4
//     and 4.3.800-rc.1; environments on older builds (qanet as of 2026-09)
//     return an empty list instead of an error, so its negative case gates on
//     a runtime probe.
//   - SPO registration data (spoCount, spoList, spoIdentities,
//     stakePoolOperators) is empty on every environment until post-mainnet
//     registration tooling exists. Per-endpoint tests are shape-only (success,
//     array, schema on every item, limit bounds) so they stay valid once data
//     appears. The single test that asserts emptiness is
//     'should report no SPO registrations on permissioned environments' and is
//     the one to flip when registrations exist.
//   test.todo → needs registered-SPO data not producible on any env yet.
//
// Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/1003

import log from '@utils/logging/logger';
import { env } from 'environment/model';
import type { TestContext } from 'vitest';
import type { z } from 'zod';
import '@utils/logging/test-logging-hooks';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import {
  CommitteeMemberSchema,
  DParameterChangeSchema,
  EpochInfoSchema,
  RegisteredTotalsSchema,
  SpoHex,
  SpoIdentitySchema,
  SpoSchema,
} from '@utils/indexer/graphql/schema';
import type { CommitteeMember, EpochInfo } from '@utils/indexer/indexer-types';
import dataProvider from '@utils/testdata-provider';

const httpClient = new IndexerHttpClient();

// A well-formed (56 hex chars, 28-byte) pool id that is not registered anywhere.
const FABRICATED_POOL_ID = 'deadbeef'.repeat(7);
const MALFORMED_POOL_IDS = ['', 'not-a-pool-id', 'abc', 'deadbeef'.repeat(7) + 'ff'];
const MAX_GRAPHQL_INT = 2_147_483_647;
// Server-side cap on fromEpoch..toEpoch spans (GHSA-6746-qxvv-3hwg).
const EPOCH_SPAN_LIMIT = 10_000;
// How far below the current epoch to look for committee data. Epochs are 30
// minutes on current environments, so 48 covers a day of spo-indexer lag.
const COMMITTEE_LOOKBACK_EPOCHS = 48;
// Resolver defaults / clamps, see indexer-api/src/infra/api/v4/query.rs.
const SPO_LIST_DEFAULT_LIMIT = 20;
const SPO_LIST_MAX_LIMIT = 200;
const SPO_IDENTITIES_DEFAULT_LIMIT = 50;
const STAKE_POOL_OPERATORS_DEFAULT_LIMIT = 20;

let surfacePresent = false;
// Whether the deployed indexer rejects over-wide epoch spans (#1455, shipped in
// 4.4.0-rc.4 and 4.3.800-rc.1). Older builds return an empty list instead.
let spanGuardPresent = false;
let epochInfo: EpochInfo | null = null;
let knownCommitteeEpoch: number | null = null;
let knownCommittee: CommitteeMember[] = [];

function expectValidList<T>(items: unknown, schema: z.ZodType<T>, label: string): T[] {
  expect(Array.isArray(items), `${label} should be an array`).toBe(true);
  for (const item of items as unknown[]) {
    const parsed = schema.safeParse(item);
    expect(
      parsed.success,
      `${label} item failed schema validation ${JSON.stringify(parsed.error, null, 2)}`,
    ).toBe(true);
  }
  return items as T[];
}

describe('spo queries', () => {
  beforeAll(async () => {
    // dParameterHistory is served on every environment that has the SPO
    // surface at all, so a healthy response means the surface is present.
    const probe = await httpClient.getDParameterHistory();
    if (probe.errors || !probe.data) {
      log.warn(`SPO surface not present on ${env.getCurrentEnvironmentName()}; skipping`);
      return;
    }
    surfacePresent = true;

    const spanProbe = await httpClient.getRegisteredTotalsSeries(0, EPOCH_SPAN_LIMIT + 1);
    spanGuardPresent = (spanProbe.errors ?? []).length > 0;
    if (!spanGuardPresent) {
      log.warn(
        `Epoch-span guard (#1455) not deployed on ${env.getCurrentEnvironmentName()}; skipping its negative case`,
      );
    }

    const epochResponse = await httpClient.getCurrentEpochInfo();
    epochInfo = epochResponse.data?.currentEpochInfo ?? null;
    if (!epochInfo) {
      log.warn(`No spo-indexer epoch data on ${env.getCurrentEnvironmentName()}`);
      return;
    }

    // Resolve the most recent epoch that has a committee. currentEpochInfo is
    // extrapolated to the chain's real current epoch, while spo-indexer
    // backfills committee membership one epoch per poll cycle and has been
    // observed trailing by ~4 epochs on devnet, so scan downwards from the
    // current epoch within a bounded window.
    const oldestCandidate = Math.max(0, epochInfo.epochNo - COMMITTEE_LOOKBACK_EPOCHS);
    for (let epoch = epochInfo.epochNo; epoch >= oldestCandidate; epoch--) {
      const response = await httpClient.getCommittee(epoch);
      const members = response.data?.committee ?? [];
      if (!response.errors && members.length > 0) {
        knownCommitteeEpoch = epoch;
        knownCommittee = members;
        log.info(
          `Using epoch ${epoch} (${epochInfo.epochNo - epoch} behind current) with ${members.length} committee members`,
        );
        break;
      }
    }
    if (knownCommitteeEpoch === null) {
      log.warn(
        `No committee data within epochs ${oldestCandidate}..${epochInfo.epochNo} on ${env.getCurrentEnvironmentName()}`,
      );
    }
  }, 30_000);

  describe('dParameterHistory', () => {
    /**
     * @given a chain whose first indexed block recorded the D-parameter
     * @when dParameterHistory is queried
     * @then at least one change is returned and every entry matches the
     *       DParameterChange schema
     */
    test('should return at least one D-parameter change set at genesis', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Governance'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getDParameterHistory();

      expect(response).toBeSuccess();
      const history = expectValidList(
        response.data!.dParameterHistory,
        DParameterChangeSchema,
        'dParameterHistory',
      );
      expect(history.length).toBeGreaterThanOrEqual(1);
    });

    /**
     * @given the D-parameter history
     * @when its ordering is inspected
     * @then entries are newest first with strictly decreasing block heights, and
     *       the oldest entry describes a non-empty committee
     */
    test('should order D-parameter changes newest first with unique block heights', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Governance'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getDParameterHistory();

      expect(response).toBeSuccess();
      const history = response.data!.dParameterHistory;
      expect(history.length).toBeGreaterThanOrEqual(1);

      const heights = history.map((entry) => entry.blockHeight);
      for (let i = 1; i < heights.length; i++) {
        expect(heights[i], `entry ${i} should be older than entry ${i - 1}`).toBeLessThan(
          heights[i - 1],
        );
      }

      const genesis = history[history.length - 1];
      expect(genesis.numPermissionedCandidates + genesis.numRegisteredCandidates).toBeGreaterThan(
        0,
      );
    });
  });

  describe('currentEpochInfo', () => {
    /**
     * @given an environment where spo-indexer has recorded epochs
     * @when currentEpochInfo is queried
     * @then epoch info matches the EpochInfo schema, epochNo is positive and the
     *       elapsed time is within the epoch duration
     */
    test('should return well-formed current epoch info where spo-indexer data exists', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Epoch'] };
      if (!surfacePresent) return ctx.skip();
      if (!epochInfo) {
        return ctx.skip(
          true,
          `no spo-indexer epoch data on ${env.getCurrentEnvironmentName()} — currentEpochInfo is null`,
        );
      }

      const response = await httpClient.getCurrentEpochInfo();

      expect(response).toBeSuccess();
      const info = response.data!.currentEpochInfo;
      expect(info).not.toBeNull();

      const parsed = EpochInfoSchema.safeParse(info);
      expect(
        parsed.success,
        `EpochInfo schema validation failed ${JSON.stringify(parsed.error, null, 2)}`,
      ).toBe(true);
      expect(info!.epochNo).toBeGreaterThan(0);
      // Extrapolated from the latest stored epoch, so it may briefly overshoot
      // right at an epoch boundary; soft so a boundary race does not fail the run.
      expect.soft(info!.elapsedSeconds).toBeLessThan(info!.durationSeconds);
    });

    /**
     * @given an environment where spo-indexer has recorded no epochs
     * @when currentEpochInfo is queried
     * @then the response is successful and currentEpochInfo is null (nullable
     *       contract, not an error)
     */
    test('should return a successful null where spo-indexer epoch data is absent', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Epoch', 'Negative'] };
      if (!surfacePresent) return ctx.skip();
      if (epochInfo) {
        return ctx.skip(true, `epoch data present on ${env.getCurrentEnvironmentName()}`);
      }

      const response = await httpClient.getCurrentEpochInfo();

      expect(response).toBeSuccess();
      expect(response.data!.currentEpochInfo).toBeNull();
    });
  });

  describe('committee', () => {
    /**
     * @given a past epoch known to have a committee
     * @when committee(epoch) is queried
     * @then every member matches the CommitteeMember schema and echoes the
     *       requested epoch, positions are ascending and contiguous, and the
     *       committee is scheduled to produce at least one slot
     */
    test('should return committee members for a known past epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee'] };
      if (!surfacePresent) return ctx.skip();
      if (knownCommitteeEpoch === null) {
        return ctx.skip(true, `no committee data on ${env.getCurrentEnvironmentName()}`);
      }

      const response = await httpClient.getCommittee(knownCommitteeEpoch);

      expect(response).toBeSuccess();
      const members = expectValidList(response.data!.committee, CommitteeMemberSchema, 'committee');
      expect(members.length).toBeGreaterThan(0);

      for (const member of members) {
        expect(member.epochNo).toBe(knownCommitteeEpoch);
      }

      const positions = members.map((member) => member.position);
      for (let i = 1; i < positions.length; i++) {
        expect(positions[i]).toBeGreaterThan(positions[i - 1]);
      }
      expect(new Set(positions).size).toBe(positions.length);
      expect(Math.max(...positions) - Math.min(...positions) + 1).toBe(positions.length);

      const totalExpectedSlots = members.reduce((sum, member) => sum + member.expectedSlots, 0);
      expect(totalExpectedSlots).toBeGreaterThan(0);
    });

    /**
     * @given a past epoch known to have a committee
     * @when committee(epoch) is queried twice
     * @then both reads return the same members in the same order
     */
    test('should return the same member set on repeated reads', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee'] };
      if (!surfacePresent) return ctx.skip();
      if (knownCommitteeEpoch === null) {
        return ctx.skip(true, `no committee data on ${env.getCurrentEnvironmentName()}`);
      }

      const response = await httpClient.getCommittee(knownCommitteeEpoch);

      expect(response).toBeSuccess();
      expect(response.data!.committee).toEqual(knownCommittee);
    });

    /**
     * @given a negative epoch number
     * @when committee(epoch) is queried
     * @then the response is successful with an empty list (the resolver does
     *       not validate the epoch, it simply finds no rows)
     */
    test('should return an empty list for a negative epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee', 'Negative'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getCommittee(-1);

      expect(response).toBeSuccess();
      expect(response.data!.committee).toEqual([]);
    });

    /**
     * @given an epoch far in the future
     * @when committee(epoch) is queried
     * @then the response is successful with an empty list
     */
    test('should return an empty list for a far-future epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee', 'Negative'] };
      if (!surfacePresent) return ctx.skip();

      const maxIntResponse = await httpClient.getCommittee(MAX_GRAPHQL_INT);
      expect(maxIntResponse).toBeSuccess();
      expect(maxIntResponse.data!.committee).toEqual([]);

      if (epochInfo) {
        const futureResponse = await httpClient.getCommittee(epochInfo.epochNo + 1000);
        expect(futureResponse).toBeSuccess();
        expect(futureResponse.data!.committee).toEqual([]);
      }
    });

    /**
     * @given epoch values that cannot be coerced to a GraphQL Int
     * @when committee(epoch) is queried with each
     * @then the indexer rejects the request with a GraphQL error and no data
     */
    test('should reject a non-integer epoch with a GraphQL error', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee', 'Negative'] };
      if (!surfacePresent) return ctx.skip();

      const invalidEpochs = ['not-an-int', 1.5] as unknown as number[];
      for (const epoch of invalidEpochs) {
        const response = await httpClient.getCommittee(epoch);
        // Variable-coercion errors carry no data at all, so assert on the
        // presence of an error rather than the strict toBeError() shape.
        expect
          .soft(response.errors ?? [], `expected a GraphQL error for epoch ${String(epoch)}`)
          .not.toHaveLength(0);
        expect.soft(response.data).toBeNull();
      }
    });
  });

  describe('SPO registration surface', () => {
    /**
     * @given any environment
     * @when spoCount is queried
     * @then a non-negative integer is returned (the resolver never returns null
     *       even though the field is nullable in the schema)
     */
    test('spoCount should return a non-negative integer', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getSpoCount();

      expect(response).toBeSuccess();
      const count = response.data!.spoCount;
      expect(count).not.toBeNull();
      expect(Number.isInteger(count)).toBe(true);
      expect(count!).toBeGreaterThanOrEqual(0);
    });

    /**
     * @given any environment
     * @when spoList is queried without arguments
     * @then a list bounded by the default limit is returned and every item
     *       matches the Spo schema
     */
    test('spoList should return a well-formed list with default pagination', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getSpoList();

      expect(response).toBeSuccess();
      const spos = expectValidList(response.data!.spoList, SpoSchema, 'spoList');
      expect(spos.length).toBeLessThanOrEqual(SPO_LIST_DEFAULT_LIMIT);
    });

    /**
     * @given limit, offset and search arguments, including out-of-range limits
     * @when spoList is queried with each combination
     * @then every request succeeds and honours the (silently clamped) limit
     */
    test('spoList should honour limit, offset and search arguments without error', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return ctx.skip();

      const filtered = await httpClient.getSpoList({ limit: 5, offset: 0, search: 'dead' });
      expect(filtered).toBeSuccess();
      expectValidList(filtered.data!.spoList, SpoSchema, 'spoList(search)');
      expect(filtered.data!.spoList.length).toBeLessThanOrEqual(5);

      // limit is clamped to [1, 200] server-side, never rejected.
      for (const limit of [0, 10_000]) {
        const response = await httpClient.getSpoList({ limit });
        expect(response, `spoList(limit: ${limit}) should succeed`).toBeSuccess();
        expect(response.data!.spoList.length).toBeLessThanOrEqual(SPO_LIST_MAX_LIMIT);
      }
    });

    /**
     * @given any environment
     * @when spoIdentities is queried with and without pagination
     * @then every request succeeds, respects its limit and every item matches
     *       the SpoIdentity schema
     */
    test('spoIdentities should return a well-formed list', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return ctx.skip();

      const defaults = await httpClient.getSpoIdentities();
      expect(defaults).toBeSuccess();
      const identities = expectValidList(
        defaults.data!.spoIdentities,
        SpoIdentitySchema,
        'spoIdentities',
      );
      expect(identities.length).toBeLessThanOrEqual(SPO_IDENTITIES_DEFAULT_LIMIT);

      const paged = await httpClient.getSpoIdentities({ limit: 3, offset: 1 });
      expect(paged).toBeSuccess();
      expectValidList(paged.data!.spoIdentities, SpoIdentitySchema, 'spoIdentities(paged)');
      expect(paged.data!.spoIdentities.length).toBeLessThanOrEqual(3);
    });

    /**
     * @given any environment
     * @when stakePoolOperators is queried with and without a limit
     * @then every request succeeds, respects its limit and every item is a hex
     *       SPO key
     */
    test('stakePoolOperators should return a well-formed list of SPO keys', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return ctx.skip();

      const defaults = await httpClient.getStakePoolOperators();
      expect(defaults).toBeSuccess();
      const operators = expectValidList(
        defaults.data!.stakePoolOperators,
        SpoHex,
        'stakePoolOperators',
      );
      expect(operators.length).toBeLessThanOrEqual(STAKE_POOL_OPERATORS_DEFAULT_LIMIT);

      const limited = await httpClient.getStakePoolOperators(5);
      expect(limited).toBeSuccess();
      expectValidList(limited.data!.stakePoolOperators, SpoHex, 'stakePoolOperators(5)');
      expect(limited.data!.stakePoolOperators.length).toBeLessThanOrEqual(5);
    });

    /**
     * @given a permissioned environment with no SPO registrations
     * @when spoCount, spoList, spoIdentities and stakePoolOperators are queried
     * @then each returns a successful empty result
     *
     * This is the single data-reality assertion for the registration surface.
     * Once post-mainnet registration tooling produces real SPOs, replace it with
     * non-empty assertions (the per-endpoint shape tests above already validate
     * populated data).
     */
    test('should report no SPO registrations on permissioned environments', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration', 'Negative'] };
      if (!surfacePresent) return ctx.skip();

      const [count, list, identities, operators] = await Promise.all([
        httpClient.getSpoCount(),
        httpClient.getSpoList(),
        httpClient.getSpoIdentities(),
        httpClient.getStakePoolOperators(),
      ]);

      expect(count).toBeSuccess();
      expect(count.data!.spoCount).toBe(0);
      expect(list).toBeSuccess();
      expect(list.data!.spoList).toEqual([]);
      expect(identities).toBeSuccess();
      expect(identities.data!.spoIdentities).toEqual([]);
      expect(operators).toBeSuccess();
      expect(operators.data!.stakePoolOperators).toEqual([]);
    });

    /**
     * @given a registered SPO
     * @when spoList(search) is queried with a prefix of its pool id
     * @then only matching SPOs are returned
     */
    test.todo('spoList search should filter by pool id prefix');
  });

  describe('pool-id lookups', () => {
    /**
     * @given a well-formed pool id that is not registered
     * @when spoByPoolId and spoIdentityByPoolId are queried
     * @then both resolve successfully to null
     */
    test('should return null for a fabricated well-formed pool id', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Negative'] };
      if (!surfacePresent) return ctx.skip();

      const spo = await httpClient.getSpoByPoolId(FABRICATED_POOL_ID);
      expect(spo).toBeSuccess();
      expect(spo.data!.spoByPoolId).toBeNull();

      const identity = await httpClient.getSpoIdentityByPoolId(FABRICATED_POOL_ID);
      expect(identity).toBeSuccess();
      expect(identity.data!.spoIdentityByPoolId).toBeNull();
    });

    /**
     * @given malformed pool ids (empty, non-hex, odd length, wrong length)
     * @when spoByPoolId and spoIdentityByPoolId are queried with each
     * @then the indexer resolves to null rather than raising an error
     *
     * Potential server-side finding: the pool-id normaliser only lowercases the
     * input and performs no hex validation, so garbage silently yields null.
     * If validation is added later, switch these assertions to toBeError().
     */
    test('should return null rather than an error for malformed pool ids', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Negative'] };
      if (!surfacePresent) return ctx.skip();

      const malformedIds = [...MALFORMED_POOL_IDS, ...dataProvider.getFabricatedMalformedHashes()];
      for (const poolId of malformedIds) {
        const spo = await httpClient.getSpoByPoolId(poolId);
        expect.soft(spo, `spoByPoolId(${JSON.stringify(poolId)})`).toBeSuccess();
        expect.soft(spo.data?.spoByPoolId).toBeNull();

        const identity = await httpClient.getSpoIdentityByPoolId(poolId);
        expect.soft(identity, `spoIdentityByPoolId(${JSON.stringify(poolId)})`).toBeSuccess();
        expect.soft(identity.data?.spoIdentityByPoolId).toBeNull();
      }
    });

    /**
     * @given a registered SPO
     * @when spoByPoolId is queried with its pool id
     * @then the Spo is returned with identity and metadata fields populated
     */
    test.todo('spoByPoolId should return the SPO for a registered pool id');

    /**
     * @given a registered SPO
     * @when spoIdentityByPoolId is queried with its pool id
     * @then the identity is returned with mainchain, sidechain and aura keys
     */
    test.todo('spoIdentityByPoolId should return identity fields for a registered pool');
  });

  describe('registeredTotalsSeries range validation', () => {
    /**
     * @given an epoch range wider than the server-side span cap
     * @when registeredTotalsSeries is queried
     * @then the indexer rejects it with an "epoch range too large" client error
     */
    test('should reject an epoch span larger than the maximum', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Negative'] };
      if (!surfacePresent) return ctx.skip();
      if (!spanGuardPresent) {
        return ctx.skip(
          true,
          `epoch-span guard (#1455, indexer >= 4.4.0-rc.4) not deployed on ${env.getCurrentEnvironmentName()}`,
        );
      }

      const response = await httpClient.getRegisteredTotalsSeries(0, EPOCH_SPAN_LIMIT + 1);

      expect(response).toBeError();
      expect(response.errors![0].message).toMatch(/epoch range too large/);
    });

    /**
     * @given an epoch range exactly at the server-side span cap
     * @when registeredTotalsSeries is queried
     * @then the request succeeds (the bound is inclusive) and every item matches
     *       the RegisteredTotals schema
     */
    test('should accept the maximum allowed epoch span', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Registration'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getRegisteredTotalsSeries(0, EPOCH_SPAN_LIMIT);

      expect(response).toBeSuccess();
      expectValidList(
        response.data!.registeredTotalsSeries,
        RegisteredTotalsSchema,
        'registeredTotalsSeries',
      );
    });
  });
});
