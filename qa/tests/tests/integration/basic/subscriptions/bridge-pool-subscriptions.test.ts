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

// Integration tests for c2m-bridge pool observability subscription (#944).
//
// Covers: bridgePoolUpdates subscription.
//
// ── What bridgePoolUpdates does ───────────────────────────────────────────────
//
// `bridgePoolUpdates(from: Int)` pushes a `BridgePoolUpdate` frame on every
// newly indexed bridge event that affects a protocol pool (Reserve or Treasury).
// Each frame contains:
//   - `newEvent: BridgeEvent` — the event that triggered the update
//   - `pool: BridgePoolSummary` — the recomputed pool snapshot after the event
//
// The subscription emits the current summary immediately on connect (similar
// to `bridgeBalance` in #942). If `from` is provided, it also replays past
// pool-affecting events with their at-time summaries before switching to
// live mode.
//
// ── Status of tests ──────────────────────────────────────────────────────────
//
//   it.todo   → Requires bridge schema + event data in the test environment.
//               Same Q1 blocker as #941–#944 test PRs. See PR #1219.
//               Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/944
//
//   it.skip   → Blocked on specific in-flight features (noted inline).
//
// ── Types ─────────────────────────────────────────────────────────────────────
//
// Types defined inline as stubs. Consolidate with bridge-pool-queries.test.ts
// types into indexer-types.ts once #944 lands.

import log from '@utils/logging/logger';
import '@utils/logging/test-logging-hooks';
import { IndexerWsClient } from '@utils/indexer/websocket-client';
import type { TestContext } from 'vitest';

// ── Stub types (consolidate with bridge-pool-queries.test.ts once #944 lands) ─

interface BlockReference {
  blockHeight: number;
  blockHash: string;
}

interface BridgeTreasuryAggregate {
  reason: string;
  total: string;
  count: number;
}

interface BridgePoolSummary {
  reserveTotal: string;
  treasuryByReason: BridgeTreasuryAggregate[];
  subminimumTxCount: number;
  lastEventAt: BlockReference | null;
}

interface BridgeEventBase {
  id: number;
  midnightTxHash: string;
  indexedAt: BlockReference;
}

interface BridgeReserveTransfer extends BridgeEventBase {
  __typename: 'BridgeReserveTransfer';
  cardanoTxHash: string;
  amount: string;
}

interface BridgeInvalidTransfer extends BridgeEventBase {
  __typename: 'BridgeInvalidTransfer';
  cardanoTxHash: string;
  amount: string;
}

interface BridgeSubminimalFlushTransfer extends BridgeEventBase {
  __typename: 'BridgeSubminimalFlushTransfer';
  amount: string;
  count: number;
}

type PoolAffectingEvent =
  | BridgeReserveTransfer
  | BridgeInvalidTransfer
  | BridgeSubminimalFlushTransfer;

interface BridgePoolUpdate {
  newEvent: PoolAffectingEvent;
  pool: BridgePoolSummary;
}

// ── GraphQL subscription string ──────────────────────────────────────────────

const BRIDGE_POOL_UPDATES_SUBSCRIPTION = `
  subscription BridgePoolUpdates($from: Int) {
    bridgePoolUpdates(from: $from) {
      newEvent {
        ... on BridgeReserveTransfer {
          __typename id midnightTxHash cardanoTxHash amount
          indexedAt { blockHeight blockHash }
        }
        ... on BridgeInvalidTransfer {
          __typename id midnightTxHash cardanoTxHash amount
          indexedAt { blockHeight blockHash }
        }
        ... on BridgeUnapprovedTransfer {
          __typename id midnightTxHash cardanoTxHash amount recipient
          indexedAt { blockHeight blockHash }
        }
        ... on BridgeSubminimalFlushTransfer {
          __typename id midnightTxHash amount count
          indexedAt { blockHeight blockHash }
        }
      }
      pool {
        reserveTotal
        treasuryByReason { reason total count }
        subminimumTxCount
        lastEventAt { blockHeight blockHash }
      }
    }
  }
`;

// ── Tests ────────────────────────────────────────────────────────────────────

describe('bridge pool subscription — bridgePoolUpdates', () => {
  let wsClient: IndexerWsClient;

  beforeEach(async () => {
    wsClient = new IndexerWsClient();
    await wsClient.connectionInit();
  }, 30_000);

  afterEach(async () => {
    await wsClient.connectionClose();
  });

  /**
   * bridgePoolUpdates emits the current pool summary immediately on connect.
   * When no pool-affecting events exist, the initial frame has all-zero totals.
   *
   * @given no pool-affecting bridge events have been indexed
   * @when we subscribe to bridgePoolUpdates
   * @then the first frame has pool.reserveTotal=0, all treasury totals=0,
   *       subminimumTxCount=0, and pool.lastEventAt=null
   */
  it.todo('should emit current pool summary immediately on subscribe');

  /**
   * bridgePoolUpdates replays historical pool updates from the given cursor.
   *
   * @given the with-data chain has ReserveTransfer events with known ids
   * @when we subscribe with from=<firstEventId - 1>
   * @then we receive a BridgePoolUpdate frame for each pool-affecting event in id order
   * @and each frame's pool.reserveTotal is the running sum at that point in time
   */
  it.todo('should replay historical pool updates from the given cursor id');

  /**
   * bridgePoolUpdates pushes a live update when a new ReserveTransfer is indexed.
   *
   * @given we are subscribed to bridgePoolUpdates in live mode
   * @when a new ReserveTransfer event is indexed
   * @then we receive a BridgePoolUpdate where newEvent.__typename = BridgeReserveTransfer
   * @and pool.reserveTotal is updated to include the new event's amount
   */
  it.todo('should push an update when a new ReserveTransfer is indexed');

  /**
   * bridgePoolUpdates pushes a live update when an InvalidTransfer is indexed.
   *
   * @given we are subscribed to bridgePoolUpdates in live mode
   * @when a new InvalidTransfer event is indexed
   * @then we receive a BridgePoolUpdate where newEvent.__typename = BridgeInvalidTransfer
   * @and the INVALID treasury aggregate in pool.treasuryByReason is increased
   */
  it.todo('should push an update when a new InvalidTransfer is indexed');

  /**
   * bridgePoolUpdates does NOT push an update for UserTransfer events.
   * UserTransfer goes to a user address, not to a protocol pool.
   *
   * @given we are subscribed to bridgePoolUpdates in live mode
   * @when a UserTransfer event is indexed
   * @then no BridgePoolUpdate is pushed (pool is unaffected by user transfers)
   */
  it.todo('should not push a pool update for UserTransfer events (user address, not pool)');

  /**
   * bridgePoolUpdates frames carry a self-consistent pool snapshot at each point.
   * After N pool-affecting events, the Nth frame's pool totals equal the sum
   * of all previous events' amounts.
   *
   * @given the with-data chain has 3 ReserveTransfer events with known amounts A1, A2, A3
   * @when we subscribe with from=0 and receive 3 frames
   * @then frame[0].pool.reserveTotal = A1
   * @and frame[1].pool.reserveTotal = A1 + A2
   * @and frame[2].pool.reserveTotal = A1 + A2 + A3
   */
  it.todo('should carry a running pool total in each frame (cumulative consistency)');

  // Skipped: UnapprovedTransfer pool update — blocked on approval governance.
  // Tracking: https://github.com/midnightntwrk/midnight-indexer/issues/940
  test.skip('should push an update when an UnapprovedTransfer is indexed', async (_ctx: TestContext) => {
    // TODO: implement once UnapprovedTransfer can be generated in test env.
    // Expect newEvent.__typename = BridgeUnapprovedTransfer and
    // UNAPPROVED treasury aggregate to be increased.
  });

  // Skipped: SubminimalFlushTransfer pool update — blocked on threshold investigation.
  // See SubminimalFlushTransfer note in bridge-pool-queries.test.ts.
  test.skip('should push an update and increment subminimumTxCount when a flush is indexed', async (_ctx: TestContext) => {
    // TODO: implement once SubminimalFlushTransfer can be generated in test env.
    // Expect newEvent.__typename = BridgeSubminimalFlushTransfer,
    // SUBMINIMAL_FLUSH treasury aggregate to be increased,
    // and pool.subminimumTxCount to be incremented by newEvent.count.
  });
});

// Suppress unused import warnings until test bodies are implemented.
void BRIDGE_POOL_UPDATES_SUBSCRIPTION;
void log;

type _SuppressUnused = BridgePoolUpdate;
void (undefined as unknown as _SuppressUnused);
