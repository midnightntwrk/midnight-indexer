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
// (#1003) that is live on permissioned environments: governance history
// (dParameterHistory, termsAndConditionsHistory, cross-checked against
// Block.systemParameters), currentEpochInfo, committee(epoch) and the
// committee-derived registration series (registeredSpoSeries,
// registeredPresence). The registration, performance and stake surface, which
// is empty until post-mainnet registration tooling exists, lives in
// spo-registration-queries.test.ts.
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
  BlockSystemParametersSchema,
  CommitteeMemberSchema,
  DParameterChangeSchema,
  EpochInfoSchema,
  PresenceEventSchema,
  RegisteredStatSchema,
  TermsAndConditionsChangeSchema,
  VarLenghtHex,
} from '@utils/indexer/graphql/schema';
import type {
  BlockSystemParameters,
  CommitteeMember,
  EpochInfo,
} from '@utils/indexer/indexer-types';
import { fetchQueryFieldNames } from '@utils/indexer/schema-feature-probe';
import {
  MAX_GRAPHQL_INT,
  assertNoGraphqlErrors,
  epochRange,
  expectOrdered,
  expectValidList,
  findLatestCommitteeEpoch,
  skipUnlessServed,
  skipWithReason,
  surfaceAbsentReason,
} from '@utils/indexer/spo-test-support';

const httpClient = new IndexerHttpClient();

// Root Query fields every test in this file depends on
const CORE_FIELDS = ['dParameterHistory', 'currentEpochInfo', 'committee'];
const CORE_SURFACE = `SPO surface (${CORE_FIELDS.join(', ')})`;
// How many epochs below the known committee epoch the multi-epoch series test spans
const SERIES_SPAN = 5;
// How far past the current epoch the far-future committee probe reaches
const FAR_FUTURE_EPOCH_OFFSET = 1000;

let queryFields = new Set<string>();
let surfacePresent = false;
let epochInfo: EpochInfo | null = null;
let knownCommitteeEpoch: number | null = null;
let knownCommittee: CommitteeMember[] = [];

/** Fetches the latest block's governance parameters and validates their shape. */
async function fetchLatestBlockSystemParameters(): Promise<BlockSystemParameters> {
  const response = await httpClient.getBlockSystemParameters();
  expect(response).toBeSuccess();
  const block = response.data!.block;
  expect(block).not.toBeNull();

  const parsed = BlockSystemParametersSchema.safeParse(block);
  expect(
    parsed.success,
    `Block.systemParameters schema validation failed ${JSON.stringify(parsed.error, null, 2)}`,
  ).toBe(true);
  return block!;
}

/**
 * Asserts a governance history is ordered newest first: strictly decreasing
 * block heights and non-increasing timestamps.
 */
function expectNewestFirst(history: { blockHeight: number; timestamp: number }[], label: string) {
  expectOrdered(
    history,
    (previous, current) =>
      current.blockHeight < previous.blockHeight && current.timestamp <= previous.timestamp,
    label,
  );
}

function noCommitteeReason(): string {
  return `no committee data on ${env.getCurrentEnvironmentName()}`;
}

// currentEpochInfo is nullable by contract, but on an environment that ships
// spo-indexer a null means the component is not deployed or has never
// committed an epoch
function noEpochReason(): string {
  return `spo-indexer not deployed or has no epochs on ${env.getCurrentEnvironmentName()} (currentEpochInfo is null)`;
}

/**
 * The slot schedule of a committee: the fields spo-indexer writes once per
 * epoch and never touches again. The identity fields (poolIdHex, auraPubkeyHex,
 * spoSkHex) are left out because they are joined from spo_identity, which is
 * backfilled independently and can change between two reads.
 */
function committeeSchedule(members: CommitteeMember[]) {
  return members.map(({ epochNo, position, sidechainPubkeyHex, expectedSlots }) => ({
    epochNo,
    position,
    sidechainPubkeyHex,
    expectedSlots,
  }));
}

describe.skipIf(env.isUndeployedEnv())('spo queries', () => {
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

    const epochResponse = await httpClient.getCurrentEpochInfo();
    assertNoGraphqlErrors('currentEpochInfo', epochResponse);
    epochInfo = epochResponse.data!.currentEpochInfo;
    if (!epochInfo) {
      log.warn(noEpochReason());
      return;
    }

    const known = await findLatestCommitteeEpoch(httpClient, epochInfo.epochNo);
    if (known) {
      knownCommitteeEpoch = known.epoch;
      knownCommittee = known.members;
    }
  }, 60_000);

  describe('dParameterHistory', () => {
    /**
     * @given a chain whose first indexed block recorded the D-parameter
     * @when dParameterHistory is queried
     * @then at least one change is returned and every entry matches the
     *       DParameterChange schema
     */
    test('should return at least one D-parameter change set at genesis', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Governance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

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
     * @then entries are newest first with strictly decreasing block heights and
     *       non-increasing timestamps, and the oldest entry describes a
     *       non-empty committee
     */
    test('should order D-parameter changes newest first with unique block heights', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Governance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const response = await httpClient.getDParameterHistory();

      expect(response).toBeSuccess();
      const history = response.data!.dParameterHistory;
      expect(history.length).toBeGreaterThanOrEqual(1);

      expectNewestFirst(history, 'dParameterHistory');

      const genesis = history[history.length - 1];
      expect(genesis.numPermissionedCandidates + genesis.numRegisteredCandidates).toBeGreaterThan(
        0,
      );
    });

    /**
     * @given the latest block and the D-parameter history
     * @when the newest history entry at or below the block height is compared
     *       with Block.systemParameters.dParameter
     * @then both report the same candidate counts
     *
     * The block is fetched first: a D-parameter change landing between the two
     * reads then shows up in the history but not in the (older) block, and the
     * "at or below the block height" lookup still picks the matching entry.
     */
    test('should agree with the D-parameter in force at the latest block', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Governance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const block = await fetchLatestBlockSystemParameters();
      const response = await httpClient.getDParameterHistory();
      expect(response).toBeSuccess();

      const inForce = response.data!.dParameterHistory.find(
        (entry) => entry.blockHeight <= block.height,
      );
      expect(inForce, `no D-parameter entry at or below block ${block.height}`).toBeDefined();
      expect({
        numPermissionedCandidates: inForce!.numPermissionedCandidates,
        numRegisteredCandidates: inForce!.numRegisteredCandidates,
      }).toEqual(block.systemParameters.dParameter);
    });

    /**
     * @given the newest and the oldest D-parameter history entries
     * @when each entry's blockHash is resolved through block(offset: {hash})
     * @then the block has the entry's height and timestamp, and its
     *       systemParameters.dParameter equals the entry's counts
     */
    test('should reference blocks that resolve by hash to the same parameters', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Governance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const response = await httpClient.getDParameterHistory();
      expect(response).toBeSuccess();
      const history = response.data!.dParameterHistory;
      expect(history.length).toBeGreaterThanOrEqual(1);

      // Newest three plus the genesis entry, de-duplicated for short histories;
      // bounded so the test stays well inside its timeout on long histories.
      const sample = [...history.slice(0, 3), history[history.length - 1]].filter(
        (entry, index, all) => all.findIndex((e) => e.blockHash === entry.blockHash) === index,
      );

      for (const entry of sample) {
        const blockResponse = await httpClient.getBlockSystemParameters({ hash: entry.blockHash });
        expect(blockResponse, `block(hash: ${entry.blockHash})`).toBeSuccess();
        const block = blockResponse.data!.block;
        expect(block, `block ${entry.blockHash} should exist`).not.toBeNull();

        expect(block!.height).toBe(entry.blockHeight);
        expect(block!.timestamp).toBe(entry.timestamp);
        expect(block!.systemParameters.dParameter).toEqual({
          numPermissionedCandidates: entry.numPermissionedCandidates,
          numRegisteredCandidates: entry.numRegisteredCandidates,
        });
      }
    });
  });

  describe('termsAndConditionsHistory', () => {
    /**
     * @given any environment serving termsAndConditionsHistory
     * @when the history is queried
     * @then the response is a possibly empty list whose entries match the
     *       TermsAndConditionsChange schema and are ordered newest first
     */
    test('should return a well-formed history ordered newest first', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Governance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'termsAndConditionsHistory')) return;

      const response = await httpClient.getTermsAndConditionsHistory();

      expect(response).toBeSuccess();
      const history = expectValidList(
        response.data!.termsAndConditionsHistory,
        TermsAndConditionsChangeSchema,
        'termsAndConditionsHistory',
      );
      expectNewestFirst(history, 'termsAndConditionsHistory');
    });

    /**
     * @given the latest block and the Terms and Conditions history
     * @when the newest entry at or below the block height is compared with
     *       Block.systemParameters.termsAndConditions
     * @then an empty history means the block reports null, otherwise both carry
     *       the same document hash and URL
     */
    test('should agree with the Terms and Conditions in force at the latest block', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Governance'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'termsAndConditionsHistory')) return;

      const block = await fetchLatestBlockSystemParameters();
      const response = await httpClient.getTermsAndConditionsHistory();
      expect(response).toBeSuccess();

      const inForce = response.data!.termsAndConditionsHistory.find(
        (entry) => entry.blockHeight <= block.height,
      );
      if (!inForce) {
        expect(block.systemParameters.termsAndConditions).toBeNull();
        return;
      }
      expect(block.systemParameters.termsAndConditions).toEqual({
        hash: inForce.hash,
        url: inForce.url,
      });
    });
  });

  describe('currentEpochInfo', () => {
    /**
     * @given an environment where spo-indexer has recorded epochs
     * @when currentEpochInfo is queried
     * @then epoch info matches the EpochInfo schema (which already pins epochNo
     *       to a non-negative integer, epoch 0 included) and the elapsed time is
     *       within the epoch duration
     */
    test('should return well-formed current epoch info where spo-indexer data exists', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Epoch'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!epochInfo) return skipWithReason(ctx, noEpochReason());

      const response = await httpClient.getCurrentEpochInfo();

      expect(response).toBeSuccess();
      const info = response.data!.currentEpochInfo;
      expect(info).not.toBeNull();

      const parsed = EpochInfoSchema.safeParse(info);
      expect(
        parsed.success,
        `EpochInfo schema validation failed ${JSON.stringify(parsed.error, null, 2)}`,
      ).toBe(true);
      // Extrapolated from the latest stored epoch, so it may briefly overshoot
      // right at an epoch boundary; soft so a boundary race does not fail the run.
      expect.soft(info!.elapsedSeconds).toBeLessThan(info!.durationSeconds);
    });

    /**
     * @given the current epoch and the newest epoch with committee data
     * @when the two are compared
     * @then the current epoch is never behind the newest committee epoch
     *       (spo-indexer only backfills, it never runs ahead of the chain)
     */
    test('should not lag behind the newest committee epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Epoch'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!epochInfo) return skipWithReason(ctx, noEpochReason());
      if (knownCommitteeEpoch === null) return skipWithReason(ctx, noCommitteeReason());

      expect(epochInfo.epochNo).toBeGreaterThanOrEqual(knownCommitteeEpoch);
    });
  });

  describe('committee', () => {
    /**
     * @given a past epoch known to have a committee
     * @when committee(epoch) is queried
     * @then every member matches the CommitteeMember schema and echoes the
     *       requested epoch, positions are ascending and contiguous, sidechain
     *       keys are in canonical lowercase form without a prefix, expected
     *       slots are spread evenly across positions, and the committee is
     *       scheduled to produce at least one slot
     */
    test('should return committee members for a known past epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (knownCommitteeEpoch === null) return skipWithReason(ctx, noCommitteeReason());

      const response = await httpClient.getCommittee(knownCommitteeEpoch);

      expect(response).toBeSuccess();
      const members = expectValidList(response.data!.committee, CommitteeMemberSchema, 'committee');
      expect(members.length).toBeGreaterThan(0);

      for (const member of members) {
        expect(member.epochNo).toBe(knownCommitteeEpoch);
        // spo-indexer stores keys stripped of their 0x prefix and lowercased; a
        // failure here is the signal to keep SpoHex permissive (schema.ts).
        expect(
          VarLenghtHex.safeParse(member.sidechainPubkeyHex).success,
          `sidechain key ${member.sidechainPubkeyHex} should be lowercase hex without a prefix`,
        ).toBe(true);
      }

      // Strictly ascending already implies unique; the span check adds contiguity.
      const positions = members.map((member) => member.position);
      expectOrdered(positions, (previous, current) => current > previous, 'committee positions');
      expect(Math.max(...positions) - Math.min(...positions) + 1).toBe(positions.length);

      // spo-indexer hands out slots_per_epoch / positions to every position and
      // the remainder one slot at a time, so the spread is at most one.
      const expectedSlots = members.map((member) => member.expectedSlots);
      expect(Math.max(...expectedSlots) - Math.min(...expectedSlots)).toBeLessThanOrEqual(1);

      // Sidechain keys are deliberately not asserted unique: the committee is a
      // slot schedule and one validator may hold several positions (preprod).

      const totalExpectedSlots = members.reduce((sum, member) => sum + member.expectedSlots, 0);
      expect(totalExpectedSlots).toBeGreaterThan(0);
    });

    /**
     * @given a past epoch known to have a committee
     * @when committee(epoch) is queried twice in a row
     * @then both reads return the same members in the same order, and the
     *       schedule (epoch, position, sidechain key, expected slots) matches
     *       the snapshot taken in beforeAll
     *
     * Full equality is only asserted between the two back-to-back reads. The
     * beforeAll snapshot is compared on the schedule alone because poolIdHex,
     * auraPubkeyHex and spoSkHex come from a LEFT JOIN on spo_identity, which
     * spo-indexer backfills independently: a row landing between beforeAll and
     * this test turns those from null to a value on a perfectly healthy indexer.
     */
    test('should return the same member set on repeated reads', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (knownCommitteeEpoch === null) return skipWithReason(ctx, noCommitteeReason());

      const first = await httpClient.getCommittee(knownCommitteeEpoch);
      const second = await httpClient.getCommittee(knownCommitteeEpoch);

      expect(first).toBeSuccess();
      expect(second).toBeSuccess();
      expect(second.data!.committee).toEqual(first.data!.committee);
      expect(committeeSchedule(first.data!.committee)).toEqual(committeeSchedule(knownCommittee));
    });

    /**
     * @given a negative epoch number
     * @when committee(epoch) is queried
     * @then the response is successful with an empty list (the resolver does
     *       not validate the epoch, it simply finds no rows)
     */
    test('should return an empty list for a negative epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

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
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const maxIntResponse = await httpClient.getCommittee(MAX_GRAPHQL_INT);
      expect(maxIntResponse).toBeSuccess();
      expect(maxIntResponse.data!.committee).toEqual([]);

      if (epochInfo) {
        const futureResponse = await httpClient.getCommittee(
          epochInfo.epochNo + FAR_FUTURE_EPOCH_OFFSET,
        );
        expect(futureResponse).toBeSuccess();
        expect(futureResponse.data!.committee).toEqual([]);
      }
    });

    /**
     * @given an epoch just above the 32-bit GraphQL Int range
     * @when committee(epoch) is queried
     * @then the response is successful with an empty list
     *
     * The schema advertises `epoch: Int!` but the resolver binds an i64
     * (query.rs `committee`), and async-graphql parses any JSON integer into it.
     * epochUtilization binds an i32 and rejects the same value; see the
     * registration suite. If the server tightens committee to i32, flip this to
     * an error assertion.
     */
    test('should accept an epoch above the 32-bit Int range as an empty list', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const response = await httpClient.getCommittee(MAX_GRAPHQL_INT + 1);

      expect(response).toBeSuccess();
      expect(response.data!.committee).toEqual([]);
    });

    /**
     * @given epoch values that cannot be coerced to a GraphQL Int
     * @when committee(epoch) is queried with each
     * @then the indexer rejects the request with a GraphQL error and no data
     */
    test('should reject a non-integer epoch with a GraphQL error', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee', 'Negative'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));

      const invalidEpochs = ['not-an-int', 1.5] as unknown as number[];
      for (const epoch of invalidEpochs) {
        const response = await httpClient.getCommittee(epoch);
        // Variable coercion fails before any field resolves, so there is no
        // `committee` key for toBeError() to look at: async-graphql answers
        // `"data": null` today, and a server that omits `data` altogether is
        // equally correct. Assert on the error and on the absence of data.
        expect
          .soft(response.errors ?? [], `expected a GraphQL error for epoch ${String(epoch)}`)
          .not.toHaveLength(0);
        expect
          .soft(response.data ?? null, `expected no data for epoch ${String(epoch)}`)
          .toBeNull();
      }
    });
  });

  describe('registeredSpoSeries', () => {
    /**
     * @given a past epoch known to have a committee
     * @when registeredSpoSeries is queried for exactly that epoch
     * @then one row is returned whose federatedValidCount equals the number of
     *       distinct committee keys scheduled for at least one slot, with no
     *       federated invalid members
     */
    test('should report the committee size for a known epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'registeredSpoSeries')) return;
      if (knownCommitteeEpoch === null) return skipWithReason(ctx, noCommitteeReason());

      const response = await httpClient.getRegisteredSpoSeries(
        knownCommitteeEpoch,
        knownCommitteeEpoch,
      );

      expect(response).toBeSuccess();
      const stats = expectValidList(
        response.data!.registeredSpoSeries,
        RegisteredStatSchema,
        'registeredSpoSeries',
      );
      expect(stats).toHaveLength(1);

      const [stat] = stats;
      const scheduledKeys = new Set(
        knownCommittee
          .filter((member) => member.expectedSlots > 0)
          .map((member) => member.sidechainPubkeyHex),
      );
      expect(stat.epochNo).toBe(knownCommitteeEpoch);
      expect(stat.federatedValidCount).toBe(scheduledKeys.size);
      expect(stat.federatedInvalidCount).toBe(0);
    });

    /**
     * @given a past epoch known to have a committee
     * @when registeredSpoSeries is queried for a short range ending at it
     * @then exactly one row per epoch is returned, ascending and contiguous
     */
    test('should return one contiguous ascending row per epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'registeredSpoSeries')) return;
      if (knownCommitteeEpoch === null) return skipWithReason(ctx, noCommitteeReason());

      const fromEpoch = Math.max(0, knownCommitteeEpoch - SERIES_SPAN);
      const response = await httpClient.getRegisteredSpoSeries(fromEpoch, knownCommitteeEpoch);

      expect(response).toBeSuccess();
      const stats = expectValidList(
        response.data!.registeredSpoSeries,
        RegisteredStatSchema,
        'registeredSpoSeries',
      );
      expect(stats.map((stat) => stat.epochNo)).toEqual(epochRange(fromEpoch, knownCommitteeEpoch));
    });
  });

  describe('registeredPresence', () => {
    /**
     * @given a past epoch known to have a committee
     * @when registeredPresence is queried for exactly that epoch
     * @then there is one committee-source event per committee member with a
     *       null status, keyed by pool id where known and by sidechain key
     *       otherwise, and events are ordered by epoch, source and key
     *
     * The committee is re-read here instead of using the beforeAll snapshot:
     * the event key falls back from poolIdHex to the sidechain key, and
     * poolIdHex is joined from spo_identity, which spo-indexer backfills
     * independently of the committee rows.
     */
    test('should contain one committee event per member for a known epoch', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'SPO', 'Committee', 'Registration'] };
      if (!surfacePresent) return skipWithReason(ctx, surfaceAbsentReason(CORE_SURFACE));
      if (!skipUnlessServed(ctx, queryFields, 'registeredPresence')) return;
      if (knownCommitteeEpoch === null) return skipWithReason(ctx, noCommitteeReason());

      const [committeeResponse, response] = await Promise.all([
        httpClient.getCommittee(knownCommitteeEpoch),
        httpClient.getRegisteredPresence(knownCommitteeEpoch, knownCommitteeEpoch),
      ]);

      expect(committeeResponse).toBeSuccess();
      const committee = committeeResponse.data!.committee;
      expect(committee.length).toBe(knownCommittee.length);

      expect(response).toBeSuccess();
      const events = expectValidList(
        response.data!.registeredPresence,
        PresenceEventSchema,
        'registeredPresence',
      );

      for (const event of events) {
        expect(event.epochNo).toBe(knownCommitteeEpoch);
      }

      const committeeEvents = events.filter((event) => event.source === 'committee');
      expect(committeeEvents).toHaveLength(committee.length);
      for (const event of committeeEvents) {
        expect(event.status).toBeNull();
      }

      const expectedKeys = committee.map((member) => member.poolIdHex ?? member.sidechainPubkeyHex);
      expect(new Set(committeeEvents.map((event) => event.idKey))).toEqual(new Set(expectedKeys));

      expectOrdered(
        events,
        (previous, current) =>
          previous.epochNo < current.epochNo ||
          (previous.epochNo === current.epochNo &&
            (previous.source < current.source ||
              (previous.source === current.source && previous.idKey <= current.idKey))),
        'registeredPresence events',
      );
    });
  });
});
