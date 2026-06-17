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

// Integration tests for c2m-bridge pool observability queries (#944).
//
// Covers: bridgePoolSummary, bridgeReserveInflows, bridgeTreasuryInflows.
//
// ── What this ticket adds ─────────────────────────────────────────────────────
//
// Protocol-level observability for where NIGHT flows when bridge transactions
// are processed. Three distinct token destinations exist:
//
//   Reserve pool  — ReserveTransfer events (`bridgeReserveInflows`)
//   Treasury      — Invalid, Unapproved, SubminimalFlush events (`bridgeTreasuryInflows`)
//   User address  — UserTransfer events (covered by #941 queries)
//
// `bridgePoolSummary` returns an aggregate snapshot: cumulative totals per
// category and a count of subminimum txs aggregated into flush events.
//
// ── Status of tests ──────────────────────────────────────────────────────────
//
//   it.todo   → Requires bridge event data in the test environment.
//               Same Q1 blocker as #941/#942 test PRs. See PR #1219.
//               Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/944
//
//   it.skip   → Blocked on specific in-flight features (noted inline).
//
// ── SubminimalFlush threshold investigation ───────────────────────────────────
//
// The SubminimalFlushTransfer event fires only when accumulated subminimum
// transfers cross a configured pallet threshold. It is currently unknown
// whether this threshold can be configured to a low value (e.g. 2 txs) in
// the test environment genesis, or whether the toolkit can trigger a flush
// directly.
//
// My recommendation: investigate with the node team (Lech) whether the
// threshold is a genesis parameter or a hardcoded constant. If it is a
// genesis parameter, set it to 2 in the local undeployed env and submit
// 2 subminimum transactions to trigger a flush deterministically. If it
// cannot be configured, consider adding a toolkit command that directly
// submits a flush-triggering sequence, or defer the test until the
// production chain naturally produces flush events (post-mainnet bridge launch).
//
// Until resolved: SubminimalFlush-dependent assertions are marked it.skip.
//
// ── Types ─────────────────────────────────────────────────────────────────────
//
// Types defined inline as stubs. Move to indexer-types.ts once #944 lands.

import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import type { TestContext } from 'vitest';
import { IndexerHttpClient } from '@utils/indexer/http-client';
import type { GraphQLResponse } from '@utils/indexer/indexer-types';

// ── Stub types (move to indexer-types.ts once #944 lands) ──────────────────

export type BridgeTreasuryReason = 'INVALID' | 'UNAPPROVED' | 'SUBMINIMAL_FLUSH';

export interface BridgeTreasuryAggregate {
  reason: BridgeTreasuryReason;
  total: string;
  count: number;
}

export interface BridgePoolSummary {
  reserveTotal: string;
  treasuryByReason: BridgeTreasuryAggregate[];
  subminimumTxCount: number;
  lastEventBlockHeight: number | null;
}

export interface BridgeReserveTransfer {
  id: number;
  blockHeight: number;
  midnightTxHash: string;
  cardanoTxHash: string;
  amount: string;
}

export interface BridgeInvalidTransfer {
  id: number;
  blockHeight: number;
  midnightTxHash: string;
  cardanoTxHash: string;
  amount: string;
}

export interface BridgeUnapprovedTransfer {
  id: number;
  blockHeight: number;
  midnightTxHash: string;
  cardanoTxHash: string;
  amount: string;
  recipient: string;
}

export interface BridgeSubminimalFlushTransfer {
  id: number;
  blockHeight: number;
  midnightTxHash: string;
  amount: string;
  count: number;
}

export type BridgeTreasuryEvent =
  | BridgeInvalidTransfer
  | BridgeUnapprovedTransfer
  | BridgeSubminimalFlushTransfer;

// ── GraphQL query strings ───────────────────────────────────────────────────

const BRIDGE_POOL_SUMMARY_QUERY = `
  query BridgePoolSummary($atBlock: Int) {
    bridgePoolSummary(atBlock: $atBlock) {
      reserveTotal
      treasuryByReason {
        reason
        total
        count
      }
      subminimumTxCount
      lastEventBlockHeight
    }
  }
`;

const BRIDGE_RESERVE_INFLOWS_QUERY = `
  query BridgeReserveInflows($blockHeightFrom: Int, $blockHeightTo: Int, $offset: Int, $limit: Int) {
    bridgeReserveInflows(blockHeightFrom: $blockHeightFrom, blockHeightTo: $blockHeightTo, offset: $offset, limit: $limit) {
      ... on BridgeReserveTransfer {
        __typename id blockHeight midnightTxHash cardanoTxHash amount
      }
    }
  }
`;

const BRIDGE_TREASURY_INFLOWS_QUERY = `
  query BridgeTreasuryInflows(
    $blockHeightFrom: Int
    $blockHeightTo: Int
    $reason: BridgeTreasuryReason
    $offset: Int
    $limit: Int
  ) {
    bridgeTreasuryInflows(
      blockHeightFrom: $blockHeightFrom
      blockHeightTo: $blockHeightTo
      reason: $reason
      offset: $offset
      limit: $limit
    ) {
      ... on BridgeInvalidTransfer {
        __typename id blockHeight midnightTxHash cardanoTxHash amount
      }
      ... on BridgeUnapprovedTransfer {
        __typename id blockHeight midnightTxHash cardanoTxHash amount recipient
      }
      ... on BridgeSubminimalFlushTransfer {
        __typename id blockHeight midnightTxHash amount count
      }
    }
  }
`;

// ── Helpers ─────────────────────────────────────────────────────────────────

const httpClient = new IndexerHttpClient();

function rawRequest<T>(
  query: string,
  variables?: Record<string, unknown>,
): Promise<GraphQLResponse<T>> {
  return (
    httpClient as unknown as {
      client: {
        rawRequest: (q: string, vars?: Record<string, unknown>) => Promise<GraphQLResponse<T>>;
      };
    }
  ).client.rawRequest(query, variables);
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('bridge pool queries — bridgePoolSummary', () => {
  /**
   * bridgePoolSummary with no bridge events returns all-zero totals.
   * The zero value for HexEncoded u128 is the 16-byte big-endian representation "0".
   * This test is deterministic only if the with-data chain has no bridge events.
   * Once bridge events are present, use atBlock=<blockBeforeFirstBridgeEvent>.
   *
   * @given no bridge pallet events have been indexed (or atBlock before first bridge block)
   * @when we query bridgePoolSummary
   * @then reserveTotal is zero, all treasuryByReason totals are zero,
   *       subminimumTxCount is 0, and lastEventBlockHeight is null
   */
  it.todo('should return all-zero totals when no bridge events have been indexed');

  /**
   * bridgePoolSummary.reserveTotal reflects cumulative ReserveTransfer amounts.
   *
   * @given the with-data chain has N ReserveTransfer events with known amounts
   * @when we query bridgePoolSummary
   * @then reserveTotal equals the sum of all ReserveTransfer.amount values
   */
  it.todo('should set reserveTotal to the cumulative sum of ReserveTransfer amounts');

  /**
   * bridgePoolSummary.treasuryByReason breaks treasury inflows down by reason.
   * Each of the three treasury reasons should appear with its correct aggregate.
   *
   * @given the with-data chain has InvalidTransfer and UnapprovedTransfer events
   * @when we query bridgePoolSummary
   * @then treasuryByReason contains entries for INVALID and UNAPPROVED with correct totals
   */
  it.todo('should aggregate treasury inflows separately by INVALID and UNAPPROVED reason');

  /**
   * bridgePoolSummary.lastEventBlockHeight references the most recently indexed bridge event block.
   *
   * @given bridge events exist up to a known block height H
   * @when we query bridgePoolSummary
   * @then lastEventBlockHeight equals H
   */
  it.todo('should set lastEventBlockHeight to the most recently indexed bridge event block');

  /**
   * bridgePoolSummary respects the atBlock snapshot parameter.
   *
   * @given bridge events exist at blocks B1 and B2 (B1 < B2)
   * @when we query bridgePoolSummary(atBlock: B1)
   * @then only events up to and including B1 contribute to the totals
   * @and a second query at B2 returns a higher reserveTotal
   */
  it.todo('should return a point-in-time snapshot when atBlock is given');

  // Skipped: SubminimalFlushTransfer aggregation in bridgePoolSummary.
  // Requires crossing the subminimum accumulation threshold in the test environment.
  // Investigation required: is the threshold a configurable genesis parameter?
  // If yes, set to 2 in local undeployed env and submit 2 subminimum txs to
  // trigger a flush. If no, defer until production chain produces flush events.
  // See: https://github.com/midnightntwrk/midnight-indexer/issues/944
  test.skip('should count SubminimalFlushTransfer.count in subminimumTxCount', async (_ctx: TestContext) => {
    // TODO: implement once SubminimalFlushTransfer can be generated in test env.
    // 1. Trigger enough subminimum transfers to cross the flush threshold.
    // 2. Query bridgePoolSummary.
    // 3. Assert subminimumTxCount equals the number of individual subminimum txs aggregated.
    // 4. Assert treasuryByReason contains a SUBMINIMAL_FLUSH entry with the correct total.
  });

  // Skipped: UnapprovedTransfer treasury contribution.
  // Same blocker as other UnapprovedTransfer tests: approval governance not landed yet.
  // Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/940
  test.skip('should aggregate UnapprovedTransfer amounts under UNAPPROVED treasury reason', async (_ctx: TestContext) => {
    // TODO: implement once UnapprovedTransfer can be generated in test env.
  });
});

describe('bridge pool queries — bridgeReserveInflows', () => {
  /**
   * bridgeReserveInflows with no data returns an empty list.
   *
   * @given no ReserveTransfer events have been indexed
   * @when we query bridgeReserveInflows
   * @then the response is successful and returns an empty array
   */
  it.todo('should return an empty list when no ReserveTransfer events are indexed');

  /**
   * bridgeReserveInflows returns ReserveTransfer events in the specified block range.
   *
   * @given the with-data chain has ReserveTransfer events across multiple blocks
   * @when we query bridgeReserveInflows(blockHeightFrom: B1, blockHeightTo: B2)
   * @then only events with blockHeight in [B1, B2] are returned
   */
  it.todo('should return ReserveTransfer events within the specified block range');

  /**
   * bridgeReserveInflows returns events with the correct field shape.
   *
   * @given at least one ReserveTransfer is indexed
   * @when we query bridgeReserveInflows(limit: 1)
   * @then the event has id, blockHeight, midnightTxHash, cardanoTxHash, amount
   * @and all hex fields are non-empty strings
   */
  it.todo('should return events with correct BridgeReserveTransfer field shape');

  /**
   * bridgeReserveInflows supports offset and limit pagination.
   *
   * @given at least 3 ReserveTransfer events are indexed
   * @when we query with limit=2 offset=0 and limit=2 offset=1
   * @then results are consistent and ids are in ascending order
   */
  it.todo('should paginate ReserveTransfer events with offset and limit');
});

describe('bridge pool queries — bridgeTreasuryInflows', () => {
  /**
   * bridgeTreasuryInflows with no data returns an empty list.
   *
   * @given no treasury-redirected events (Invalid, Unapproved, SubminimalFlush) are indexed
   * @when we query bridgeTreasuryInflows
   * @then the response is successful and returns an empty array
   */
  it.todo('should return an empty list when no treasury-redirected events are indexed');

  /**
   * bridgeTreasuryInflows without a reason filter returns all treasury event types.
   *
   * @given the with-data chain has InvalidTransfer events
   * @when we query bridgeTreasuryInflows (no reason filter)
   * @then results include BridgeInvalidTransfer events
   */
  it.todo('should return all treasury event types when no reason filter is given');

  /**
   * bridgeTreasuryInflows filtered by reason=INVALID returns only InvalidTransfer events.
   *
   * @given the with-data chain has both Invalid and Reserve events
   * @when we query bridgeTreasuryInflows(reason: INVALID)
   * @then every returned event has __typename BridgeInvalidTransfer
   */
  it.todo('should return only BridgeInvalidTransfer when reason=INVALID');

  /**
   * bridgeTreasuryInflows respects the block range filter.
   *
   * @given treasury events exist at blocks B1 and B3
   * @when we query bridgeTreasuryInflows(blockHeightFrom: B1, blockHeightTo: B2) where B2 < B3
   * @then only events at B1 are returned (B3 is outside the range)
   */
  it.todo('should respect blockHeightFrom and blockHeightTo filters for treasury inflows');

  // Skipped: UNAPPROVED reason filter — blocked on approval governance logic.
  // Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/940
  test.skip('should return only BridgeUnapprovedTransfer when reason=UNAPPROVED', async (_ctx: TestContext) => {
    // TODO: implement once UnapprovedTransfer can be generated in test env.
  });

  // Skipped: SUBMINIMAL_FLUSH reason filter — blocked on threshold investigation.
  // See SubminimalFlushTransfer note at top of file.
  test.skip('should return only BridgeSubminimalFlushTransfer when reason=SUBMINIMAL_FLUSH', async (_ctx: TestContext) => {
    // TODO: implement once SubminimalFlushTransfer can be generated in test env.
  });
});

// Suppress unused import warnings until test bodies are implemented.
void BRIDGE_POOL_SUMMARY_QUERY;
void BRIDGE_RESERVE_INFLOWS_QUERY;
void BRIDGE_TREASURY_INFLOWS_QUERY;
void rawRequest;
void log;

type _SuppressUnused = BridgeTreasuryEvent | GraphQLResponse<unknown>;
void (undefined as unknown as _SuppressUnused);
