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
// `bridgePoolUpdates` pushes a `BridgePoolUpdate` frame on every newly indexed
// bridge event that affects a protocol pool (Reserve or Treasury). Each frame
// contains:
//   - `newEvent: BridgeEvent` — the event that triggered the update; null on
//     the initial snapshot emitted on subscribe
//   - `pool: BridgePoolSummary` — the recomputed pool snapshot after the event
//
// The subscription emits the current summary immediately on connect (similar
// to `bridgeBalance` in #942), with `newEvent` null, then switches to live
// mode. It takes no arguments.
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

interface BridgeTreasuryAggregate {
  reason: string;
  total: string;
  count: number;
}

interface BridgePoolSummary {
  reserveTotal: string;
  treasuryByReason: BridgeTreasuryAggregate[];
  subminimumTxCount: number;
  lastEventBlockHeight: number | null;
}

interface BridgeEventBase {
  id: number;
  blockHeight: number;
  midnightTxHash: string;
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

type BridgeEvent = BridgeReserveTransfer | BridgeInvalidTransfer | BridgeSubminimalFlushTransfer;

interface BridgePoolUpdate {
  newEvent: BridgeEvent | null;
  pool: BridgePoolSummary;
}

// ── GraphQL subscription string ──────────────────────────────────────────────

const BRIDGE_POOL_UPDATES_SUBSCRIPTION = `
  subscription BridgePoolUpdates {
    bridgePoolUpdates {
      newEvent {
        ... on BridgeReserveTransfer {
          __typename id blockHeight midnightTxHash cardanoTxHash amount
        }
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
      pool {
        reserveTotal
        treasuryByReason { reason total count }
        subminimumTxCount
        lastEventBlockHeight
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
   *       subminimumTxCount=0, pool.lastEventBlockHeight=null, and newEvent=null
   */
  it.todo('should emit current pool summary immediately on subscribe');

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
   * @given 3 ReserveTransfer events with known amounts A1, A2, A3 are indexed while subscribed
   * @when we subscribe and receive a frame per pool-affecting event
   * @then the frame after A1 has pool.reserveTotal = A1
   * @and the frame after A2 has pool.reserveTotal = A1 + A2
   * @and the frame after A3 has pool.reserveTotal = A1 + A2 + A3
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
