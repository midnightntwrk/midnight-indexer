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

// Integration tests for c2m-bridge pool observability queries: bridgePoolSummary,
// bridgeReserveInflows, bridgeTreasuryInflows (#944).
//
// Pool observability tracks where NIGHT flows when bridge transactions are
// processed: the Reserve pool (ReserveTransfer) and the Treasury (Invalid,
// Unapproved, SubminimalFlush). Per-address balance (UserTransfer/claims) is
// covered by #941.
//
// Test-data reality (2026-07): treasury-side pool data now exists on a running
// midnight-node local-environment stack (InvalidTransfer, UnapprovedTransfer and
// SubminimalFlushTransfer events are produced), so the treasury aggregation,
// inflow-listing and point-in-time cases are real, executable tests here. On
// environments without that data (e.g. stagenet) the same cases self-skip via a
// data probe, exactly like the merged #941 bridge-queries suite. The Reserve leg
// is still not driven anywhere (no ReserveTransfer events), so reserveTotal /
// bridgeReserveInflows cases that need non-zero reserve data stay test.todo.
//   probe + ctx.skip → runs where the data exists, skips where it does not.
//   test.todo        → needs reserve event data not produced in any env yet.
//
// Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/944

import log from '@utils/logging/logger';
import { env } from 'environment/model';
import type { TestContext } from 'vitest';
import '@utils/logging/test-logging-hooks';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import { BRIDGE_TREASURY_REASONS } from '@utils/indexer/indexer-types';
import type { BridgeTreasuryReason } from '@utils/indexer/indexer-types';

const httpClient = new IndexerHttpClient();

const ZERO_U128 = '0'.repeat(32);
// A block that predates any bridge event, for the atBlock snapshot. Block 0 is
// before the genesis block height at which the earliest event can be indexed, so
// it is empty on every environment (including local-env, whose earliest treasury
// event sits at block 1).
const EARLY_BLOCK = 0;

// The __typename each treasury reason discriminates to on the inflow lists.
const REASON_TYPENAME: Record<BridgeTreasuryReason, string> = {
  INVALID: 'BridgeInvalidTransfer',
  UNAPPROVED: 'BridgeUnapprovedTransfer',
  SUBMINIMAL_FLUSH: 'BridgeSubminimalFlushTransfer',
};
const TREASURY_TYPENAMES = Object.values(REASON_TYPENAME);

// bridgePoolSummary with the per-reason `count` field. `count` is in the schema
// but not in the default GET_BRIDGE_POOL_SUMMARY document / shared type, so it is
// requested here via a local override to assert per-reason event counts.
const POOL_SUMMARY_WITH_COUNT = `
query BridgePoolSummary($AT_BLOCK: Int) {
  bridgePoolSummary(atBlock: $AT_BLOCK) {
    reserveTotal
    treasuryByReason { reason total count }
    subminimumTxCount
    lastEventBlockHeight
  }
}`;

// bridgeTreasuryInflows expanded with the per-variant fields (the default
// document selects only __typename). Requested via override so amounts, flush
// counts and block heights can be asserted from the test file alone.
const TREASURY_INFLOWS_DETAILED = `
query BridgeTreasuryInflows($REASON: BridgeTreasuryReason, $BLOCK_HEIGHT_FROM: Int, $BLOCK_HEIGHT_TO: Int, $OFFSET: Int, $LIMIT: Int) {
  bridgeTreasuryInflows(reason: $REASON, blockHeightFrom: $BLOCK_HEIGHT_FROM, blockHeightTo: $BLOCK_HEIGHT_TO, offset: $OFFSET, limit: $LIMIT) {
    __typename
    ... on BridgeInvalidTransfer { id blockHeight amount }
    ... on BridgeUnapprovedTransfer { id blockHeight amount recipient }
    ... on BridgeSubminimalFlushTransfer { id blockHeight amount count }
  }
}`;

interface PoolAggregate {
  reason: BridgeTreasuryReason;
  total: string;
  count: number;
}

interface PoolSummary {
  reserveTotal: string;
  treasuryByReason: PoolAggregate[];
  subminimumTxCount: number;
  lastEventBlockHeight: number | null;
}

interface TreasuryInflow {
  __typename: string;
  id?: number;
  blockHeight?: number;
  amount?: string;
  recipient?: string;
  count?: number;
}

const hexToBigInt = (hex: string): bigint => BigInt(`0x${hex || '0'}`);

// Cumulative NIGHT held across the reserve pool and every treasury reason for a
// given point-in-time summary.
const poolGrandTotal = (summary: PoolSummary): bigint =>
  summary.treasuryByReason.reduce(
    (acc, t) => acc + hexToBigInt(t.total),
    hexToBigInt(summary.reserveTotal),
  );

// Probed once against the target environment in beforeAll.
let surfacePresent = false;
// Any non-zero pool state at all (treasury or reserve) — gates the pure
// zero-state cases, which are only valid on an environment with no pool events.
let poolDataPresent = false;
// At least one treasury-redirected event is indexed.
let treasuryDataPresent = false;
let poolAggregates: PoolAggregate[] = [];
let treasuryInflows: TreasuryInflow[] = [];
// Sorted, de-duplicated block heights at which treasury events are indexed.
let treasuryBlocks: number[] = [];

const aggregateFor = (reason: BridgeTreasuryReason): PoolAggregate | undefined =>
  poolAggregates.find((a) => a.reason === reason);

describe.skipIf(env.isUndeployedEnv())('bridge pool queries', () => {
  beforeAll(async () => {
    const probe = await httpClient.getBridgePoolSummary(undefined, POOL_SUMMARY_WITH_COUNT);
    if (probe.errors || !probe.data) {
      log.warn(`Bridge pool surface not present on ${env.getCurrentEnvironmentName()}; skipping`);
      return;
    }
    surfacePresent = true;

    const summary = probe.data.bridgePoolSummary as unknown as PoolSummary;
    poolAggregates = summary.treasuryByReason;
    poolDataPresent =
      summary.subminimumTxCount > 0 ||
      hexToBigInt(summary.reserveTotal) > 0n ||
      poolAggregates.some((a) => hexToBigInt(a.total) > 0n);

    const inflows = await httpClient.getBridgeTreasuryInflows({}, TREASURY_INFLOWS_DETAILED);
    treasuryInflows = (inflows.data?.bridgeTreasuryInflows ?? []) as unknown as TreasuryInflow[];
    treasuryDataPresent = treasuryInflows.length > 0;
    treasuryBlocks = [
      ...new Set(
        treasuryInflows.map((e) => e.blockHeight).filter((b): b is number => typeof b === 'number'),
      ),
    ].sort((a, b) => a - b);
  }, 30_000);

  describe('bridgePoolSummary', () => {
    /**
     * @given an environment with no reserve or treasury bridge events indexed
     * @when bridgePoolSummary is queried
     * @then reserveTotal and every treasuryByReason total are the zero-value hex
     *       string, subminimumTxCount is 0, and the three treasury reasons
     *       (INVALID, UNAPPROVED, SUBMINIMAL_FLUSH) are each present
     *
     * Zero-state only holds where no pool events exist; on an environment with
     * pool data (e.g. local-env) the case is skipped.
     */
    test('should return zero pool totals when no reserve or treasury events are indexed', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool'] };
      if (!surfacePresent) return ctx.skip();
      if (poolDataPresent)
        return ctx.skip(true, 'pool data present on this env — zero-state case N/A');

      const response = await httpClient.getBridgePoolSummary();

      expect(response).toBeSuccess();
      const pool = response.data!.bridgePoolSummary;
      expect(pool.reserveTotal).toBe(ZERO_U128);
      expect(pool.subminimumTxCount).toBe(0);

      const reasons = pool.treasuryByReason.map((t) => t.reason).sort();
      expect(reasons).toEqual([...BRIDGE_TREASURY_REASONS].sort());
      for (const aggregate of pool.treasuryByReason) {
        expect(aggregate.total).toBe(ZERO_U128);
      }
    });

    /**
     * @given an environment where at least one bridge event has been indexed
     * @when bridgePoolSummary is queried
     * @then lastEventBlockHeight is a positive integer
     *
     * On environments with no bridge events at all, lastEventBlockHeight is null
     * and the case is skipped.
     */
    test('should expose lastEventBlockHeight as a positive integer where bridge events exist', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getBridgePoolSummary();
      expect(response).toBeSuccess();
      const lastEventBlockHeight = response.data!.bridgePoolSummary.lastEventBlockHeight;

      if (lastEventBlockHeight === null) {
        return ctx.skip(true, 'no bridge events on this env — lastEventBlockHeight is null');
      }
      expect(Number.isInteger(lastEventBlockHeight)).toBe(true);
      expect(lastEventBlockHeight).toBeGreaterThan(0);
    });

    /**
     * @given a block that predates any bridge event (block 0)
     * @when bridgePoolSummary(atBlock: 0) is queried
     * @then the snapshot has null lastEventBlockHeight and zero totals, confirming
     *       the point-in-time snapshot excludes later events
     */
    test('should return an empty snapshot when atBlock predates all bridge events', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool', 'ByHeight'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getBridgePoolSummary(EARLY_BLOCK);

      expect(response).toBeSuccess();
      const pool = response.data!.bridgePoolSummary;
      expect(pool.lastEventBlockHeight).toBeNull();
      expect(pool.reserveTotal).toBe(ZERO_U128);
      expect(pool.subminimumTxCount).toBe(0);
    });

    /**
     * @given a chain with N ReserveTransfer events of known amounts
     * @when bridgePoolSummary is queried
     * @then reserveTotal equals the sum of all ReserveTransfer.amount values
     */
    test.todo('should set reserveTotal to the cumulative sum of ReserveTransfer amounts');

    /**
     * @given a chain with InvalidTransfer and UnapprovedTransfer events
     * @when bridgePoolSummary is queried
     * @then treasuryByReason contains INVALID and UNAPPROVED entries whose totals
     *       equal the summed inflow amounts and whose counts equal the number of
     *       inflows of that reason
     */
    test('should aggregate treasury inflows separately by INVALID and UNAPPROVED reason', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool'] };
      if (!surfacePresent) return ctx.skip();
      if (!treasuryDataPresent) return ctx.skip(true, 'no treasury data on this env');

      const invalid = aggregateFor('INVALID');
      const unapproved = aggregateFor('UNAPPROVED');
      expect(invalid).toBeDefined();
      expect(unapproved).toBeDefined();

      // At least one of the two reasons must carry data on a treasury-bearing env;
      // assert each reason that does against its own inflow list, keeping the two
      // reasons independent.
      let asserted = 0;
      for (const [reason, aggregate] of [
        ['INVALID', invalid!] as const,
        ['UNAPPROVED', unapproved!] as const,
      ]) {
        if (aggregate.count === 0) continue;
        asserted += 1;

        const list = await httpClient.getBridgeTreasuryInflows(
          { reason },
          TREASURY_INFLOWS_DETAILED,
        );
        expect(list).toBeSuccess();
        const events = (list.data!.bridgeTreasuryInflows ?? []) as unknown as TreasuryInflow[];

        // count on the aggregate matches the number of inflows of that reason.
        expect(events.length).toBe(aggregate.count);
        // total on the aggregate equals the summed inflow amounts of that reason.
        const summed = events.reduce((acc, e) => acc + hexToBigInt(e.amount ?? '0'), 0n);
        expect(hexToBigInt(aggregate.total)).toBe(summed);
        expect(hexToBigInt(aggregate.total)).toBeGreaterThan(0n);
      }
      expect(asserted).toBeGreaterThan(0);
    });

    /**
     * @given reserve/treasury events exist at blocks B1 and B2 (B1 < B2)
     * @when bridgePoolSummary(atBlock: B1) and (atBlock: B2) are queried
     * @then only events up to and including the given block contribute, and B2
     *       totals exceed B1
     */
    test('should accumulate more pool inflow at a later atBlock than an earlier one', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool', 'ByHeight'] };
      if (!surfacePresent) return ctx.skip();
      if (!treasuryDataPresent) return ctx.skip(true, 'no treasury data on this env');
      if (treasuryBlocks.length < 2)
        return ctx.skip(true, 'treasury events span a single block — no block spread to compare');

      const earlierBlock = treasuryBlocks[0];
      const laterBlock = treasuryBlocks[treasuryBlocks.length - 1];

      const earlier = await httpClient.getBridgePoolSummary(earlierBlock, POOL_SUMMARY_WITH_COUNT);
      const later = await httpClient.getBridgePoolSummary(laterBlock, POOL_SUMMARY_WITH_COUNT);
      expect(earlier).toBeSuccess();
      expect(later).toBeSuccess();

      const earlierSummary = earlier.data!.bridgePoolSummary as unknown as PoolSummary;
      const laterSummary = later.data!.bridgePoolSummary as unknown as PoolSummary;

      expect(poolGrandTotal(laterSummary)).toBeGreaterThan(poolGrandTotal(earlierSummary));
      expect(laterSummary.lastEventBlockHeight).toBeGreaterThanOrEqual(
        earlierSummary.lastEventBlockHeight!,
      );
    });

    /**
     * @given a chain with SubminimalFlushTransfer events, each carrying a `count`
     *        of aggregated subminimum Cardano txs
     * @when bridgePoolSummary is queried
     * @then subminimumTxCount equals the sum of `count` over those flush events
     */
    test('should count SubminimalFlushTransfer.count in subminimumTxCount', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool'] };
      if (!surfacePresent) return ctx.skip();
      const flush = aggregateFor('SUBMINIMAL_FLUSH');
      if (!flush || flush.count === 0)
        return ctx.skip(true, 'no SubminimalFlushTransfer data on this env');

      const list = await httpClient.getBridgeTreasuryInflows(
        { reason: 'SUBMINIMAL_FLUSH' },
        TREASURY_INFLOWS_DETAILED,
      );
      expect(list).toBeSuccess();
      const flushes = (list.data!.bridgeTreasuryInflows ?? []) as unknown as TreasuryInflow[];
      const expectedTxCount = flushes.reduce((acc, e) => acc + (e.count ?? 0), 0);

      const summary = await httpClient.getBridgePoolSummary();
      expect(summary).toBeSuccess();
      expect(summary.data!.bridgePoolSummary.subminimumTxCount).toBe(expectedTxCount);
      expect(expectedTxCount).toBeGreaterThan(0);
    });

    /**
     * @given a chain with UnapprovedTransfer events
     * @when bridgePoolSummary is queried
     * @then the UNAPPROVED treasuryByReason total equals the summed
     *       UnapprovedTransfer amounts
     */
    test('should aggregate UnapprovedTransfer amounts under UNAPPROVED treasury reason', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool'] };
      if (!surfacePresent) return ctx.skip();
      const unapproved = aggregateFor('UNAPPROVED');
      if (!unapproved || unapproved.count === 0)
        return ctx.skip(true, 'no UnapprovedTransfer data on this env');

      const list = await httpClient.getBridgeTreasuryInflows(
        { reason: 'UNAPPROVED' },
        TREASURY_INFLOWS_DETAILED,
      );
      expect(list).toBeSuccess();
      const events = (list.data!.bridgeTreasuryInflows ?? []) as unknown as TreasuryInflow[];

      expect(events.length).toBe(unapproved.count);
      const summed = events.reduce((acc, e) => acc + hexToBigInt(e.amount ?? '0'), 0n);
      expect(hexToBigInt(unapproved.total)).toBe(summed);
      expect(hexToBigInt(unapproved.total)).toBeGreaterThan(0n);
    });
  });

  describe('bridgeReserveInflows', () => {
    /**
     * @given an environment with no ReserveTransfer events indexed
     * @when bridgeReserveInflows is queried
     * @then the response is successful and returns an empty array
     */
    test('should return an empty list when no ReserveTransfer events are indexed', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool', 'Negative'] };
      if (!surfacePresent) return ctx.skip();

      const response = await httpClient.getBridgeReserveInflows();

      expect(response).toBeSuccess();
      expect(response.data?.bridgeReserveInflows).toEqual([]);
    });

    /**
     * @given a chain with ReserveTransfer events across multiple blocks
     * @when bridgeReserveInflows(blockHeightFrom: B1, blockHeightTo: B2) is queried
     * @then only events with blockHeight in [B1, B2] are returned
     */
    test.todo('should return ReserveTransfer events within the specified block range');

    /**
     * @given at least one ReserveTransfer is indexed
     * @when bridgeReserveInflows(limit: 1) is queried
     * @then the event exposes id, blockHeight, midnightTxHash, cardanoTxHash, amount
     */
    test.todo('should return events with correct BridgeReserveTransfer field shape');

    /**
     * @given at least 3 ReserveTransfer events are indexed
     * @when queried with limit=2 offset=0 and limit=2 offset=1
     * @then results are consistent and ids are in ascending order
     */
    test.todo('should paginate ReserveTransfer events with offset and limit');
  });

  describe('bridgeTreasuryInflows', () => {
    /**
     * @given an environment with no treasury-redirected events indexed
     * @when bridgeTreasuryInflows is queried
     * @then the response is successful and returns an empty array
     *
     * Skipped on environments that do have treasury data (e.g. local-env).
     */
    test('should return an empty list when no treasury-redirected events are indexed', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool', 'Negative'] };
      if (!surfacePresent) return ctx.skip();
      if (treasuryDataPresent)
        return ctx.skip(true, 'treasury data present on this env — empty-list case N/A');

      const response = await httpClient.getBridgeTreasuryInflows();

      expect(response).toBeSuccess();
      expect(response.data?.bridgeTreasuryInflows).toEqual([]);
    });

    /**
     * @given a chain with treasury-redirected events (Invalid / Unapproved /
     *        SubminimalFlush)
     * @when bridgeTreasuryInflows is queried with no reason filter
     * @then every returned event is a treasury variant and the list size matches
     *       the summed per-reason counts from bridgePoolSummary
     */
    test('should return all treasury event types when no reason filter is given', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool'] };
      if (!surfacePresent) return ctx.skip();
      if (!treasuryDataPresent) return ctx.skip(true, 'no treasury data on this env');

      const response = await httpClient.getBridgeTreasuryInflows();
      expect(response).toBeSuccess();
      const events = response.data!.bridgeTreasuryInflows;

      expect(events.length).toBeGreaterThan(0);
      for (const event of events) {
        expect(TREASURY_TYPENAMES).toContain(event.__typename);
      }
      const summedCount = poolAggregates.reduce((acc, a) => acc + a.count, 0);
      expect(events.length).toBe(summedCount);
    });

    /**
     * @given a chain with both Invalid and other treasury events
     * @when bridgeTreasuryInflows(reason: INVALID) is queried
     * @then every returned event has __typename BridgeInvalidTransfer and the
     *       count matches the INVALID aggregate
     */
    test('should return only BridgeInvalidTransfer when reason=INVALID', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool'] };
      if (!surfacePresent) return ctx.skip();
      const invalid = aggregateFor('INVALID');
      if (!invalid || invalid.count === 0)
        return ctx.skip(true, 'no InvalidTransfer data on this env');

      const response = await httpClient.getBridgeTreasuryInflows({ reason: 'INVALID' });
      expect(response).toBeSuccess();
      const events = response.data!.bridgeTreasuryInflows;

      expect(events.length).toBe(invalid.count);
      for (const event of events) {
        expect(event.__typename).toBe(REASON_TYPENAME.INVALID);
      }
    });

    /**
     * @given treasury events exist across blocks B1..Bn
     * @when bridgeTreasuryInflows(blockHeightFrom: B1, blockHeightTo: Bk) with
     *       Bk < Bn is queried
     * @then only events with blockHeight in [B1, Bk] are returned (later events
     *       are excluded)
     */
    test('should respect blockHeightFrom and blockHeightTo filters for treasury inflows', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool', 'ByHeight'] };
      if (!surfacePresent) return ctx.skip();
      if (!treasuryDataPresent) return ctx.skip(true, 'no treasury data on this env');
      if (treasuryBlocks.length < 2)
        return ctx.skip(true, 'treasury events span a single block — no range to bound');

      const from = treasuryBlocks[0];
      // Exclude the final block so the range is a strict subset.
      const to = treasuryBlocks[treasuryBlocks.length - 2];
      const expected = treasuryInflows.filter(
        (e) => typeof e.blockHeight === 'number' && e.blockHeight >= from && e.blockHeight <= to,
      );

      const response = await httpClient.getBridgeTreasuryInflows(
        { blockHeightFrom: from, blockHeightTo: to },
        TREASURY_INFLOWS_DETAILED,
      );
      expect(response).toBeSuccess();
      const events = (response.data!.bridgeTreasuryInflows ?? []) as unknown as TreasuryInflow[];

      expect(events.length).toBe(expected.length);
      for (const event of events) {
        expect(event.blockHeight).toBeGreaterThanOrEqual(from);
        expect(event.blockHeight).toBeLessThanOrEqual(to);
      }
    });

    /**
     * @given a chain with UnapprovedTransfer events
     * @when bridgeTreasuryInflows(reason: UNAPPROVED) is queried
     * @then every returned event has __typename BridgeUnapprovedTransfer and the
     *       count matches the UNAPPROVED aggregate
     */
    test('should return only BridgeUnapprovedTransfer when reason=UNAPPROVED', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool'] };
      if (!surfacePresent) return ctx.skip();
      const unapproved = aggregateFor('UNAPPROVED');
      if (!unapproved || unapproved.count === 0)
        return ctx.skip(true, 'no UnapprovedTransfer data on this env');

      const response = await httpClient.getBridgeTreasuryInflows({ reason: 'UNAPPROVED' });
      expect(response).toBeSuccess();
      const events = response.data!.bridgeTreasuryInflows;

      expect(events.length).toBe(unapproved.count);
      for (const event of events) {
        expect(event.__typename).toBe(REASON_TYPENAME.UNAPPROVED);
      }
    });

    /**
     * @given a chain with SubminimalFlushTransfer events
     * @when bridgeTreasuryInflows(reason: SUBMINIMAL_FLUSH) is queried
     * @then every returned event has __typename BridgeSubminimalFlushTransfer and
     *       the count matches the SUBMINIMAL_FLUSH aggregate
     */
    test('should return only BridgeSubminimalFlushTransfer when reason=SUBMINIMAL_FLUSH', async (ctx: TestContext) => {
      ctx.task!.meta.custom = { labels: ['Query', 'Bridge', 'Pool'] };
      if (!surfacePresent) return ctx.skip();
      const flush = aggregateFor('SUBMINIMAL_FLUSH');
      if (!flush || flush.count === 0)
        return ctx.skip(true, 'no SubminimalFlushTransfer data on this env');

      const response = await httpClient.getBridgeTreasuryInflows({ reason: 'SUBMINIMAL_FLUSH' });
      expect(response).toBeSuccess();
      const events = response.data!.bridgeTreasuryInflows;

      expect(events.length).toBe(flush.count);
      for (const event of events) {
        expect(event.__typename).toBe(REASON_TYPENAME.SUBMINIMAL_FLUSH);
      }
    });
  });
});
